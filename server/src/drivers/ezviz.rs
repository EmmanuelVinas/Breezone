//! EZVIZ's cloud (cameras, doorbells), through the linked account (`accounts::ezviz`). A doorbell
//! is a camera with a button: its rings are presses (`button@1`), for the automations.
//!
//! EZVIZ does not push to Breezone: its messages (motion, person, ring…) are read every few
//! seconds, and a ring newer than the last one seen is a press. The picture of the last event is
//! fetched when asked, and decrypted with the verification code of the cameras when EZVIZ
//! encrypted it.
//!
//!   GET {ezviz}/v3/userdevices/v1/resources/pagelist?… → {deviceInfos: [{deviceSerial, name, deviceCategory, deviceType, status}]}
//!   GET {ezviz}/v3/unifiedmsg/list?serials=…&stype=…&limit=…&date=YYYYMMDD&endTime= → {message: [{msgId, deviceSerial, time, title, subType, pic, ext: {alarmType, pics}}]}

use super::{Described, DeviceInfo, Driver, DriverError, DriverFuture, Sink, State};
use crate::crypto::Key;
use crate::db::Db;
use reqwest::Method;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// The messages asked: every alarm, and the calls of the doorbells.
const MESSAGE_TYPES: &str = "92,9904,2701";
/// A ring of a doorbell (its alarm type, or its message subtype).
const RING: i64 = 2701;
/// A motion this recent still shows.
const MOTION_FOR: i64 = 90;
/// The devices (and whether they are online) are read again this often.
const LIST_EVERY: Duration = Duration::from_secs(300);
/// The pictures start with this when EZVIZ encrypted them.
const ENCRYPTED: &[u8] = b"hikencodepicture";

#[derive(Debug, Clone, PartialEq)]
struct Cam {
    serial: String,
    name: String,
    doorbell: bool,
    online: bool,
}

impl Cam {
    fn info(&self) -> DeviceInfo {
        let (kind, traits) = if self.doorbell { ("doorbell", vec!["camera@1", "button@1"]) } else { ("camera", vec!["camera@1"]) };
        DeviceInfo { reference: self.serial.clone(), kind: kind.into(), traits: traits.into_iter().map(str::to_string).collect(), name: self.name.clone() }
    }
}

/// What a device of the account is, for Breezone: `None` for what is not a camera (a plug, a
/// sensor…).
fn cam_of(d: &Value) -> Option<Cam> {
    let category = d["deviceCategory"].as_str().unwrap_or_default();
    let doorbell = matches!(category, "BDoorBell" | "CatEye" | "DBScreen");
    if !doorbell && !matches!(category, "IPC" | "BatteryCamera" | "XVR" | "COMMON") {
        return None;
    }
    let serial = d["deviceSerial"].as_str()?.to_string();
    let name = d["name"].as_str().filter(|n| !n.trim().is_empty()).unwrap_or(&serial).to_string();
    let online = d["status"].as_i64().or_else(|| d["STATUS"]["status"].as_i64()) == Some(1);
    Some(Cam { serial, name, doorbell, online })
}

/// The last event of a device, from a message.
#[derive(Debug, Clone, PartialEq)]
struct Event {
    id: String,
    at: i64,
    label: String,
    ring: bool,
    picture: Option<String>,
}

/// A timestamp from EZVIZ: seconds or milliseconds, a number or a text.
fn seconds(v: &Value) -> Option<i64> {
    let n = v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).or_else(|| v.as_str()?.trim().parse().ok())?;
    Some(if n >= 1_000_000_000_000 { n / 1000 } else { n })
}

fn number(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str()?.trim().parse().ok())
}

fn event_of(m: &Value) -> Option<(String, Event)> {
    let serial = m["deviceSerial"].as_str()?.to_string();
    let at = seconds(&m["time"])?;
    let alarm = number(&m["ext"]["alarmType"]).filter(|c| *c != 0);
    let ring = alarm == Some(RING) || (alarm.is_none() && number(&m["subType"]) == Some(RING));
    let first_of = |s: &str| s.split(';').find(|p| !p.is_empty()).map(str::to_string);
    let picture = m["pic"].as_str().filter(|p| !p.is_empty()).map(str::to_string).or_else(|| m["ext"]["pics"].as_str().and_then(first_of));
    let label = ["title", "detail", "sampleName"].iter().find_map(|k| m[*k].as_str().filter(|t| !t.trim().is_empty())).unwrap_or(if ring { "Ring" } else { "Alarm" }).to_string();
    let id = m["msgId"].as_str().map(str::to_string).unwrap_or_else(|| format!("{serial}@{at}"));
    Some((serial, Event { id, at, label, ring, picture }))
}

/// AES-128-CBC with the verification code (the IV « 01234567 » then zeros), after a header and the
/// MD5 of the MD5 of the code; an image may hold several such blocks.
pub fn decrypt(data: &[u8], code: &str) -> Result<Vec<u8>, String> {
    use aes::cipher::{BlockModeDecrypt, KeyIvInit};
    type Decryptor = cbc::Decryptor<aes::Aes128>;
    if !data.starts_with(ENCRYPTED) {
        return Ok(data.to_vec());
    }
    let hash = {
        use md5::{Digest, Md5};
        hex::encode(Md5::digest(hex::encode(Md5::digest(code.as_bytes())).as_bytes()))
    };
    let mut starts: Vec<usize> = data.windows(ENCRYPTED.len()).enumerate().filter(|(_, w)| *w == ENCRYPTED).map(|(i, _)| i).collect();
    starts.push(data.len());
    let mut key = [0u8; 16];
    let bytes = code.as_bytes();
    key[..bytes.len().min(16)].copy_from_slice(&bytes[..bytes.len().min(16)]);
    let iv: [u8; 16] = *b"01234567\0\0\0\0\0\0\0\0";
    let mut out = vec![];
    for pair in starts.windows(2) {
        let block = &data[pair[0]..pair[1]];
        let head = ENCRYPTED.len() + 32;
        if block.len() <= head {
            return Err("image too short".into());
        }
        if &block[ENCRYPTED.len()..head] != hash.as_bytes() {
            return Err("wrong verification code".into());
        }
        let mut cipher = block[head..].to_vec();
        cipher.truncate(cipher.len() - cipher.len() % 16);
        let plain = Decryptor::new(&key.into(), &iv.into()).decrypt_padded::<cbc::cipher::block_padding::Pkcs7>(&mut cipher).map_err(|_| "wrong padding".to_string())?;
        out.extend_from_slice(plain);
    }
    Ok(out)
}

fn kind_of_image(data: &[u8]) -> &'static str {
    if data.starts_with(b"\x89PNG") {
        "image/png"
    } else {
        "image/jpeg"
    }
}

pub struct Ezviz {
    http: reqwest::Client,
    db: Db,
    key: Key,
    url: String,
    cams: Mutex<(Vec<Cam>, Option<Instant>)>,
    /// The last event of each device.
    last: Mutex<HashMap<String, Event>>,
    /// The newest ring seen of each doorbell (none yet: the first reading only notes it).
    rung: Mutex<Option<HashMap<String, i64>>>,
    sink: std::sync::Mutex<Option<Sink>>,
}

fn unreachable(e: impl std::fmt::Display) -> DriverError {
    DriverError::Unreachable(format!("EZVIZ: {e}"))
}

impl Ezviz {
    pub fn new(db: Db, key: Key, url: String) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().unwrap_or_default();
        Self {
            http,
            db,
            key,
            url: url.trim_end_matches('/').to_string(),
            cams: Mutex::new((vec![], None)),
            last: Mutex::new(HashMap::new()),
            rung: Mutex::new(None),
            sink: std::sync::Mutex::new(None),
        }
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value, DriverError> {
        match crate::accounts::ezviz::call_in(&self.db, &self.key, &self.url, Method::GET, path, query).await {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(DriverError::Unreachable("aucun compte EZVIZ : Réglages › Comptes et services".into())),
            Err(e) => Err(unreachable(e.message.en)),
        }
    }

    /// The cameras and doorbells of the account, read again now.
    async fn load(&self) -> Result<Vec<Cam>, DriverError> {
        let devices = crate::accounts::ezviz::devices_with(|offset| async move {
            let query = [("groupId", "-1".to_string()), ("limit", "30".into()), ("offset", offset.to_string()), ("filter", crate::accounts::ezviz::PAGE_FILTER.into())];
            self.get("/v3/userdevices/v1/resources/pagelist", &query).await.map_err(|e| crate::error::ApiError::bad_gateway(e.to_string()))
        })
        .await
        .map_err(|e| unreachable(e.message.en))?;
        let cams: Vec<Cam> = devices.iter().filter_map(cam_of).collect();
        *self.cams.lock().await = (cams.clone(), Some(Instant::now()));
        Ok(cams)
    }

    async fn cams(&self) -> Result<Vec<Cam>, DriverError> {
        {
            let known = self.cams.lock().await;
            if known.1.is_some_and(|t| t.elapsed() < LIST_EVERY) {
                return Ok(known.0.clone());
            }
        }
        self.load().await
    }

    /// The latest messages of the devices, newest first; the rings not seen yet are presses.
    async fn messages(&self, cams: &[Cam]) -> Result<(), DriverError> {
        if cams.is_empty() {
            return Ok(());
        }
        let serials = cams.iter().map(|c| c.serial.as_str()).collect::<Vec<_>>().join(",");
        let today = chrono::Local::now().format("%Y%m%d").to_string();
        let query = [("serials", serials), ("stype", MESSAGE_TYPES.into()), ("limit", "20".into()), ("date", today), ("endTime", String::new())];
        let body = self.get("/v3/unifiedmsg/list", &query).await?;
        let messages = body["message"].as_array().or_else(|| body["messages"].as_array()).cloned().unwrap_or_default();
        let mut events: Vec<(String, Event)> = messages.iter().filter_map(event_of).collect();
        events.sort_by_key(|(_, e)| e.at);
        let mut last = self.last.lock().await;
        let mut rung = self.rung.lock().await;
        let first = rung.is_none();
        let rung = rung.get_or_insert_with(HashMap::new);
        for (serial, e) in events {
            if e.ring && rung.get(&serial).is_none_or(|t| e.at > *t) {
                rung.insert(serial.clone(), e.at);
                if !first {
                    if let Some(sink) = self.sink.lock().ok().and_then(|s| s.clone()) {
                        sink.press(&serial, 1, "single");
                    }
                }
            }
            if last.get(&serial).is_none_or(|l| e.at >= l.at) {
                last.insert(serial, e);
            }
        }
        Ok(())
    }

    /// The picture of the last event of a device, decrypted.
    pub async fn picture(&self, reference: &str) -> Result<(Vec<u8>, &'static str), DriverError> {
        let url = self.last.lock().await.get(reference).and_then(|e| e.picture.clone()).ok_or_else(|| DriverError::Unsupported("no picture yet".into()))?;
        let resp = self.http.get(&url).send().await.map_err(unreachable)?;
        if !resp.status().is_success() {
            return Err(unreachable(format!("picture: HTTP {}", resp.status())));
        }
        let data = resp.bytes().await.map_err(unreachable)?.to_vec();
        if !data.starts_with(ENCRYPTED) {
            return Ok((data.clone(), kind_of_image(&data)));
        }
        let Some(code) = crate::accounts::ezviz::image_code_in(&self.db, &self.key).await else {
            return Err(DriverError::Unsupported("image chiffrée : saisissez le code de vérification des caméras dans le compte EZVIZ (Réglages › Comptes et services)".into()));
        };
        let plain = decrypt(&data, &code).map_err(|e| DriverError::Unsupported(format!("image chiffrée : {e}")))?;
        let kind = kind_of_image(&plain);
        Ok((plain, kind))
    }
}

fn state_of(cam: &Cam, last: Option<&Event>, now: i64) -> State {
    let mut out = Map::new();
    out.insert("online".into(), json!(cam.online));
    out.insert("motion".into(), json!(last.is_some_and(|e| !e.ring && now - e.at < MOTION_FOR)));
    if let Some(e) = last {
        out.insert("last_event".into(), json!(e.label));
        out.insert("last_event_at".into(), json!(e.at));
        if e.picture.is_some() {
            out.insert("image_at".into(), json!(e.at));
        }
    }
    if cam.doorbell {
        out.insert("buttons".into(), json!(1));
    }
    out
}

impl Driver for Ezviz {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let cams = self.load().await?;
            Ok(Described { model: "EZVIZ".into(), firmware: None, devices: cams.iter().map(Cam::info).collect(), identity: None })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            let cams = self.cams().await?;
            self.messages(&cams).await?;
            let last = self.last.lock().await;
            let now = crate::db::now();
            Ok(cams.iter().map(|c| (c.serial.clone(), state_of(c, last.get(&c.serial), now))).collect())
        })
    }

    fn act<'a>(&'a self, _reference: &'a str, _trait_name: &'a str, _action: &'a str, _params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async { Err(DriverError::Unsupported("nothing to drive on an EZVIZ camera yet".into())) })
    }

    /// A ring shows within a few seconds.
    fn read_every(&self) -> Duration {
        Duration::from_secs(8)
    }

    fn listen(&self, sink: Sink) {
        if let Ok(mut s) = self.sink.lock() {
            *s = Some(sink);
        }
    }

    fn cloud(&self) -> bool {
        true
    }

    fn picture<'a>(&'a self, reference: &'a str) -> DriverFuture<'a, (Vec<u8>, &'static str)> {
        Box::pin(Ezviz::picture(self, reference))
    }

    fn diagnostic(&self) -> DriverFuture<'_, Value> {
        Box::pin(async move {
            let query = [("groupId", "-1".to_string()), ("limit", "30".into()), ("offset", "0".into()), ("filter", crate::accounts::ezviz::PAGE_FILTER.into())];
            let page = self.get("/v3/userdevices/v1/resources/pagelist", &query).await?;
            // What tells the hardware apart, not who owns it.
            let devices: Vec<Value> = page["deviceInfos"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|d| json!({"deviceCategory": d["deviceCategory"], "deviceSubCategory": d["deviceSubCategory"], "deviceType": d["deviceType"], "version": d["version"], "status": d["status"]}))
                .collect();
            let last: Vec<Value> = self.last.lock().await.values().map(|e| json!({"label": e.label, "at": e.at, "ring": e.ring, "picture": e.picture.is_some()})).collect();
            Ok(json!({"devices": devices, "last_events": last}))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_doorbell_is_a_camera_with_a_button() {
        let bell = cam_of(&json!({"deviceSerial": "BD1", "name": "Entrée", "deviceCategory": "BDoorBell", "status": 1})).unwrap();
        assert_eq!(bell.info().traits, ["camera@1", "button@1"]);
        assert_eq!(bell.info().kind, "doorbell");
        assert!(bell.online);
        let cam = cam_of(&json!({"deviceSerial": "C1", "name": "", "deviceCategory": "IPC", "status": 2})).unwrap();
        assert_eq!((cam.name.as_str(), cam.online, cam.info().traits.len()), ("C1", false, 1));
        assert!(cam_of(&json!({"deviceSerial": "P1", "deviceCategory": "Socket"})).is_none());
    }

    #[test]
    fn a_message_is_an_event_and_a_ring_is_told_apart() {
        let (serial, e) =
            event_of(&json!({"msgId": "m1", "deviceSerial": "BD1", "time": 1_763_000_000_000_i64, "title": "Appel", "subType": "2701", "ext": {"alarmType": "0"}, "pic": ""}))
                .unwrap();
        assert_eq!((serial.as_str(), e.at, e.ring, e.picture.clone()), ("BD1", 1_763_000_000, true, None));
        let (_, e) = event_of(&json!({"deviceSerial": "C1", "time": "1763000100", "title": "Mouvement", "ext": {"alarmType": 10000, "pics": ";https://pic/1.jpg;x"}})).unwrap();
        assert_eq!((e.ring, e.picture.as_deref(), e.label.as_str()), (false, Some("https://pic/1.jpg"), "Mouvement"));
        let s = state_of(&Cam { serial: "C1".into(), name: "C".into(), doorbell: false, online: true }, Some(&e), 1_763_000_150);
        assert_eq!((s["motion"].clone(), s["image_at"].clone()), (json!(true), json!(1_763_000_100)));
    }

    #[test]
    fn an_encrypted_picture_is_decrypted_with_the_code() {
        use aes::cipher::{BlockModeEncrypt, KeyIvInit};
        let code = "ABCDEF";
        let picture = b"\xff\xd8 a picture \xff\xd9".to_vec();
        let mut key = [0u8; 16];
        key[..6].copy_from_slice(code.as_bytes());
        let mut buf = picture.clone();
        buf.resize(32, 0);
        let cipher = cbc::Encryptor::<aes::Aes128>::new(&key.into(), b"01234567\0\0\0\0\0\0\0\0".into())
            .encrypt_padded::<cbc::cipher::block_padding::Pkcs7>(&mut buf, picture.len())
            .unwrap()
            .to_vec();
        use md5::{Digest, Md5};
        let hash = hex::encode(Md5::digest(hex::encode(Md5::digest(code.as_bytes())).as_bytes()));
        let data = [ENCRYPTED, hash.as_bytes(), &cipher].concat();
        assert_eq!(decrypt(&data, code).unwrap(), picture);
        assert!(decrypt(&data, "WRONG1").is_err());
        assert_eq!(decrypt(&picture, code).unwrap(), picture, "a clear picture stays as it is");
    }
}
