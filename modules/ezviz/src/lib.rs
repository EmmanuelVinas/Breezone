//! EZVIZ's cloud (cameras, doorbells), with the account of its app, as Home Assistant does: no
//! developer key. A doorbell is a camera with a button: its rings are presses (`button@1`).
//!
//! The API is the one the EZVIZ app uses (not documented by EZVIZ): a login gives a session,
//! renewed with its refresh id, and a new login when that fails. The module keeps the MD5 of the
//! password, which is what EZVIZ asks, never the password itself, and its session in its account
//! (`host::account_update`).
//!
//! An account with two-step verification: the first login has EZVIZ send a code, the second gives
//! it; the code binds this terminal (its "feature code", the same at each login) to the account.
//!
//!   POST /v3/users/login/v5 (account, password md5, featureCode…) → {meta: {code}, loginSession, loginArea: {apiDomain}}
//!        1100: the account is in another region (`apiDomain`); 6002: a code is needed.
//!   POST /v3/sms/nologin/checkcode (from, bizType=TERMINAL_BIND) → sends the code
//!   PUT  /v3/apigateway/login (refreshSessionId, featureCode) → {sessionInfo: {sessionId, refreshSessionId}}
//!   GET  /v3/userdevices/v1/resources/pagelist?groupId=-1&limit=30&offset=…&filter=… → {deviceInfos, page: {hasNext}}
//!   GET  /v3/unifiedmsg/list?serials=…&stype=…&limit=…&date=YYYYMMDD&endTime= → {message: [{msgId, deviceSerial, time, title, subType, pic, ext}]}
//!
//! EZVIZ does not push: its messages (motion, person, ring…) are read every few seconds, and a ring
//! newer than the last one seen is a press. The picture of the last event is fetched when asked,
//! and decrypted with the verification code of the cameras when EZVIZ encrypted it.

use breezone_module_sdk::{host, json, module, Described, DeviceInfo, Driver, Error, Map, Picture, Result, Service, State, Value};
use md5::{Digest, Md5};
use std::cell::RefCell;
use std::collections::HashMap;

const EZVIZ: &str = "https://apiieu.ezvizlife.com";
/// The name of the client given to EZVIZ at login: « Breezone » in base64.
const CLIENT: &str = "QnJlZXpvbmU=";
/// The filters of the device list: the devices, their connection and their status.
const PAGE_FILTER: &str = "CLOUD,CONNECTION,STATUS,WIFI";
/// The messages asked: every alarm, and the calls of the doorbells.
const MESSAGE_TYPES: &str = "92,9904,2701";
/// A ring of a doorbell (its alarm type, or its message subtype).
const RING: i64 = 2701;
/// A motion this recent still shows.
const MOTION_FOR: i64 = 90;
/// The devices (and whether they are online) are read again this often.
const LIST_EVERY: i64 = 300;
/// The pictures start with this when EZVIZ encrypted them.
const ENCRYPTED: &[u8] = b"hikencodepicture";

fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The same for an account at each login, so that a code asked at one login fits the next.
fn feature_code(account: &str) -> String {
    md5_hex(&format!("breezone:{}", account.trim().to_lowercase()))
}

/// The address of a region as EZVIZ gives it (« apiieu.ezvizlife.com », sometimes only « eu »).
fn domain(v: &Value) -> Option<String> {
    let d = v.as_str()?.trim().trim_end_matches('/');
    if d.is_empty() {
        return None;
    }
    Some(if d.contains("://") {
        d.to_string()
    } else if !d.contains('.') {
        format!("https://apii{d}.ezvizlife.com")
    } else {
        format!("https://{d}")
    })
}

fn code_of(body: &Value) -> i64 {
    body["meta"]["code"].as_i64().or_else(|| body["resultCode"].as_str().and_then(|c| c.parse().ok())).unwrap_or(0)
}

fn unreachable(e: impl std::fmt::Display) -> Error {
    Error::Unreachable(format!("EZVIZ: {e}"))
}

/// `a=1&b=2`, each value encoded.
fn form(pairs: &[(&str, &str)]) -> String {
    let enc = |s: &str| {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    pairs.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&")
}

/// The account: what the user typed, then the session the module keeps in it.
#[derive(Debug, Clone, Default)]
struct Session {
    account: String,
    /// MD5 of the password (hex), as EZVIZ asks it.
    password: String,
    feature_code: String,
    session_id: Option<String>,
    rf_session_id: Option<String>,
    api_url: Option<String>,
}

impl Session {
    /// From the account's fields: a password typed now is made its MD5 (a kept account has one
    /// already, with its feature code).
    fn load() -> Result<(Self, Value)> {
        let a = host::account("ezviz")?;
        let s = |k: &str| a[k].as_str().map(str::to_string);
        let account = s("account").unwrap_or_default().trim().to_string();
        let typed = s("feature_code").is_none();
        let password = s("password").unwrap_or_default();
        if account.is_empty() || password.is_empty() {
            return Err(Error::Invalid("l'identifiant et le mot de passe du compte EZVIZ sont requis".into()));
        }
        let session = Session {
            feature_code: s("feature_code").unwrap_or_else(|| feature_code(&account)),
            password: if typed { md5_hex(&password) } else { password },
            account,
            session_id: s("session_id"),
            rf_session_id: s("rf_session_id"),
            api_url: s("api_url"),
        };
        Ok((session, a))
    }

    fn keep(&self) -> Result<()> {
        host::account_update(&json!({"session_id": self.session_id, "rf_session_id": self.rf_session_id, "api_url": self.api_url}))
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("featureCode", self.feature_code.clone()),
            ("clientType", "3".into()),
            ("netType", "WIFI".into()),
            ("customno", "1000001".into()),
            ("clientNo", "web_site".into()),
            ("appId", "ys7".into()),
            ("language", "fr_FR".into()),
            ("lang", "fr".into()),
            ("sessionId", self.session_id.clone().unwrap_or_default()),
            ("User-Agent", "okhttp/3.12.1".into()),
        ]
    }

    fn send(&self, method: &str, url: &str, body: Option<&str>) -> Result<(u16, Value)> {
        let mut headers = self.headers();
        if body.is_some() {
            headers.push(("Content-Type", "application/x-www-form-urlencoded".into()));
        }
        let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let a = host::request(method, url, &refs, body.map(str::as_bytes), 20_000)?;
        let v = a.json().map_err(|_| unreachable(format!("HTTP {}", a.status)))?;
        Ok((a.status, v))
    }

    /// Logs in (following the account to its region), with the code of the two-step verification
    /// when given. Without it, an account that asks one has EZVIZ send it, and the error says so.
    fn login(&mut self, code: Option<&str>) -> Result<()> {
        let mut base = self.api_url.clone().unwrap_or_else(|| EZVIZ.to_string());
        self.session_id = None;
        let code = code.map(str::trim).filter(|c| !c.is_empty());
        for _ in 0..3 {
            let body = form(&[
                ("account", &self.account),
                ("password", &self.password),
                ("featureCode", &self.feature_code),
                ("msgType", if code.is_some() { "3" } else { "0" }),
                ("bizType", if code.is_some() { "TERMINAL_BIND" } else { "" }),
                ("cuName", CLIENT),
                ("smsCode", code.unwrap_or("")),
            ]);
            let (_, answer) = self.send("POST", &format!("{base}/v3/users/login/v5"), Some(&body))?;
            match code_of(&answer) {
                200 => {
                    self.session_id = answer["loginSession"]["sessionId"].as_str().map(str::to_string);
                    self.rf_session_id = answer["loginSession"]["rfSessionId"].as_str().map(str::to_string);
                    self.api_url = Some(domain(&answer["loginArea"]["apiDomain"]).unwrap_or(base));
                    return if self.session_id.is_some() { Ok(()) } else { Err(unreachable("no session")) };
                }
                1100 => base = domain(&answer["loginArea"]["apiDomain"]).ok_or_else(|| unreachable("unknown region"))?,
                6002 => {
                    let (_, sent) = self.send("POST", &format!("{base}/v3/sms/nologin/checkcode"), Some(&form(&[("from", &self.account), ("bizType", "TERMINAL_BIND")])))?;
                    if code_of(&sent) != 200 {
                        return Err(unreachable(format!("EZVIZ demande un code de vérification mais ne l'envoie pas ({})", code_of(&sent))));
                    }
                    return Err(Error::Invalid(
                        "EZVIZ vient d'envoyer un code de vérification (SMS ou e-mail) : saisissez-le dans « Code reçu », puis connectez à nouveau.".into(),
                    ));
                }
                1012 => return Err(Error::Invalid("le code de vérification est faux".into())),
                1013 | 1014 => return Err(Error::Invalid("EZVIZ refuse l'identifiant ou le mot de passe".into())),
                1015 => return Err(Error::Invalid("le compte EZVIZ est bloqué : ouvrez l'application EZVIZ pour le débloquer".into())),
                other => return Err(unreachable(format!("EZVIZ répond {other} : {}", answer["meta"]["message"].as_str().unwrap_or_default()))),
            }
        }
        Err(unreachable("too many regions"))
    }

    /// A new session from the refresh id.
    fn renew(&mut self) -> Result<()> {
        let (Some(base), Some(rf)) = (self.api_url.clone(), self.rf_session_id.clone()) else { return Err(unreachable("nothing to renew")) };
        let (_, body) = self.send("PUT", &format!("{base}/v3/apigateway/login"), Some(&form(&[("refreshSessionId", &rf), ("featureCode", &self.feature_code)])))?;
        match (code_of(&body), body["sessionInfo"]["sessionId"].as_str(), body["sessionInfo"]["refreshSessionId"].as_str()) {
            (200, Some(id), Some(rf)) => {
                self.session_id = Some(id.to_string());
                self.rf_session_id = Some(rf.to_string());
                Ok(())
            }
            (code, ..) => Err(unreachable(format!("EZVIZ refuse de renouveler la session ({code})"))),
        }
    }
}

/// The session is over: EZVIZ says so by HTTP 401, or in its answer.
fn expired(status: u16, body: &Value) -> bool {
    status == 401 || matches!(code_of(body), 401 | 403 | 1003 | 2003)
}

/// A GET of the API of the account, renewing the session (or logging in again) when it is over;
/// the session kept in the account when it changed.
fn get(path_and_query: &str) -> Result<Value> {
    let (mut s, _) = Session::load()?;
    let mut changed = false;
    if s.session_id.is_none() || s.api_url.is_none() {
        s.login(None)?;
        changed = true;
    }
    let url = |s: &Session| format!("{}{path_and_query}", s.api_url.clone().unwrap_or_default());
    let (mut status, mut body) = s.send("GET", &url(&s), None)?;
    if expired(status, &body) {
        if s.renew().is_err() {
            s.login(None)?;
        }
        changed = true;
        (status, body) = s.send("GET", &url(&s), None)?;
    }
    if changed {
        s.keep()?;
    }
    let code = code_of(&body);
    if !(200..300).contains(&status) || (code != 0 && code != 200) {
        return Err(unreachable(format!("EZVIZ répond {}", if code != 0 { code } else { i64::from(status) })));
    }
    Ok(body)
}

/// Every device of the account, page by page, each with its own sections (connection, status).
fn devices() -> Result<Vec<Value>> {
    let mut out = vec![];
    let mut offset = 0;
    for _ in 0..20 {
        let page = get(&format!("/v3/userdevices/v1/resources/pagelist?groupId=-1&limit=30&offset={offset}&filter={PAGE_FILTER}"))?;
        let infos = page["deviceInfos"].as_array().cloned().unwrap_or_default();
        let more = page["page"]["hasNext"].as_bool().unwrap_or(false) && !infos.is_empty();
        offset += infos.len();
        for mut d in infos {
            if let Some(serial) = d["deviceSerial"].as_str().map(str::to_string) {
                for section in ["CONNECTION", "STATUS", "WIFI"] {
                    d[section] = page[section][&serial].clone();
                }
            }
            out.push(d);
        }
        if !more {
            break;
        }
    }
    Ok(out)
}

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

/// What a device of the account is, for Breezone: `None` for what is not a camera.
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
    Some((serial, Event { at, label, ring, picture }))
}

/// AES-128-CBC with the verification code (the IV « 01234567 » then zeros), after a header and the
/// MD5 of the MD5 of the code; an image may hold several such blocks.
pub fn decrypt(data: &[u8], code: &str) -> std::result::Result<Vec<u8>, String> {
    use aes::cipher::{BlockModeDecrypt, KeyIvInit};
    if !data.starts_with(ENCRYPTED) {
        return Ok(data.to_vec());
    }
    let hash = md5_hex(&md5_hex(code));
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
        let plain = cbc::Decryptor::<aes::Aes128>::new(&key.into(), &iv.into())
            .decrypt_padded::<cbc::cipher::block_padding::Pkcs7>(&mut cipher)
            .map_err(|_| "wrong padding".to_string())?;
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

pub struct Ezviz {
    cams: RefCell<(Vec<Cam>, Option<i64>)>,
    /// The last event of each device.
    last: RefCell<HashMap<String, Event>>,
    /// The newest ring seen of each doorbell (none yet: the first reading only notes it).
    rung: RefCell<Option<HashMap<String, i64>>>,
}

impl Ezviz {
    fn load(&self) -> Result<Vec<Cam>> {
        let cams: Vec<Cam> = devices()?.iter().filter_map(cam_of).collect();
        *self.cams.borrow_mut() = (cams.clone(), Some(host::now()));
        Ok(cams)
    }

    fn cams(&self) -> Result<Vec<Cam>> {
        let fresh = self.cams.borrow().1.is_some_and(|t| host::now() - t < LIST_EVERY);
        if fresh {
            return Ok(self.cams.borrow().0.clone());
        }
        self.load()
    }

    /// The latest messages of the devices; the rings not seen yet are presses.
    fn messages(&self, cams: &[Cam]) -> Result<()> {
        if cams.is_empty() {
            return Ok(());
        }
        let serials = cams.iter().map(|c| c.serial.as_str()).collect::<Vec<_>>().join(",");
        let today = chrono::DateTime::from_timestamp(host::now(), 0).map(|t| t.format("%Y%m%d").to_string()).unwrap_or_default();
        let body = get(&format!("/v3/unifiedmsg/list?serials={serials}&stype={MESSAGE_TYPES}&limit=20&date={today}&endTime="))?;
        let messages = body["message"].as_array().or_else(|| body["messages"].as_array()).cloned().unwrap_or_default();
        let mut events: Vec<(String, Event)> = messages.iter().filter_map(event_of).collect();
        events.sort_by_key(|(_, e)| e.at);
        let mut last = self.last.borrow_mut();
        let mut rung = self.rung.borrow_mut();
        let first = rung.is_none();
        let rung = rung.get_or_insert_with(HashMap::new);
        for (serial, e) in events {
            if e.ring && rung.get(&serial).is_none_or(|t| e.at > *t) {
                rung.insert(serial.clone(), e.at);
                if !first {
                    host::pressed(&serial, 1, "single");
                }
            }
            if last.get(&serial).is_none_or(|l| e.at >= l.at) {
                last.insert(serial, e);
            }
        }
        Ok(())
    }
}

impl Driver for Ezviz {
    fn open(_config: Value, _secret: Option<Value>) -> Result<Self> {
        Ok(Self { cams: RefCell::new((vec![], None)), last: RefCell::new(HashMap::new()), rung: RefCell::new(None) })
    }

    fn describe(&self) -> Result<Described> {
        let cams = self.load()?;
        Ok(Described { model: "EZVIZ".into(), firmware: None, devices: cams.iter().map(Cam::info).collect(), identity: None })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let cams = self.cams()?;
        self.messages(&cams)?;
        let last = self.last.borrow();
        let now = host::now();
        Ok(cams.iter().map(|c| (c.serial.clone(), state_of(c, last.get(&c.serial), now))).collect())
    }

    fn act(&self, _reference: &str, _trait_name: &str, _action: &str, _params: &State) -> Result<()> {
        Err(Error::Unsupported("nothing to drive on an EZVIZ camera yet".into()))
    }

    /// The picture of the last event of a device, decrypted.
    fn picture(&self, reference: &str) -> Result<Picture> {
        let url = self.last.borrow().get(reference).and_then(|e| e.picture.clone()).ok_or_else(|| Error::Unsupported("no picture yet".into()))?;
        let a = host::request("GET", &url, &[], None, 20_000)?;
        if !(200..300).contains(&a.status) {
            return Err(unreachable(format!("picture: HTTP {}", a.status)));
        }
        if !a.body.starts_with(ENCRYPTED) {
            return Ok(Picture { media_type: kind_of_image(&a.body).into(), data: a.body });
        }
        let code = host::account("ezviz")?["image_code"]
            .as_str()
            .map(str::to_string)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| Error::Unsupported("image chiffrée : saisissez le code de vérification des caméras dans le compte EZVIZ (Réglages › Comptes et services)".into()))?;
        let plain = decrypt(&a.body, &code).map_err(|e| Error::Unsupported(format!("image chiffrée : {e}")))?;
        Ok(Picture { media_type: kind_of_image(&plain).into(), data: plain })
    }

    fn diagnostic(&self) -> Result<Value> {
        let page = get(&format!("/v3/userdevices/v1/resources/pagelist?groupId=-1&limit=30&offset=0&filter={PAGE_FILTER}"))?;
        // What tells the hardware apart, not who owns it.
        let devices: Vec<Value> = page["deviceInfos"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| json!({"deviceCategory": d["deviceCategory"], "deviceSubCategory": d["deviceSubCategory"], "deviceType": d["deviceType"], "version": d["version"], "status": d["status"]}))
            .collect();
        let last: Vec<Value> = self.last.borrow().values().map(|e| json!({"label": e.label, "at": e.at, "ring": e.ring, "picture": e.picture.is_some()})).collect();
        Ok(json!({"devices": devices, "last_events": last}))
    }
}

/// The account: a login (with the code of the two-step verification when typed), then the
/// devices it reaches; what is kept is the MD5 of the password, the feature code and the session.
pub struct Account;

impl Service for Account {
    fn check() -> Result<()> {
        let (mut s, typed) = Session::load()?;
        s.login(typed["code"].as_str())?;
        host::account_update(&json!({
            "password": s.password,
            "feature_code": s.feature_code,
            "code": null,
            "session_id": s.session_id,
            "rf_session_id": s.rf_session_id,
            "api_url": s.api_url,
        }))?;
        devices().map(|_| ())
    }

    fn query(contract: &str, query: &str, _params: Value) -> Result<Value> {
        Err(Error::Unsupported(format!("{contract} {query}")))
    }
}

module!(driver: Ezviz, service: Account);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    #[test]
    fn a_region_is_an_address() {
        assert_eq!(domain(&json!("apiieu.ezvizlife.com")).as_deref(), Some("https://apiieu.ezvizlife.com"));
        assert_eq!(domain(&json!("eu")).as_deref(), Some("https://apiieu.ezvizlife.com"));
        assert_eq!(domain(&json!("")), None);
        assert_eq!(feature_code("Moi@Exemple.fr "), feature_code("moi@exemple.fr"));
        assert_eq!(form(&[("a", "b c"), ("d", "é=")]), "a=b%20c&d=%C3%A9%3D");
    }

    #[test]
    fn a_doorbell_is_a_camera_with_a_button() {
        let bell = cam_of(&json!({"deviceSerial": "BD1", "name": "Entrée", "deviceCategory": "BDoorBell", "status": 1})).unwrap();
        assert_eq!((bell.info().traits, bell.info().kind.as_str(), bell.online), (vec!["camera@1".to_string(), "button@1".to_string()], "doorbell", true));
        assert!(cam_of(&json!({"deviceSerial": "P1", "deviceCategory": "Socket"})).is_none());
        let (_, e) = event_of(&json!({"deviceSerial": "C1", "time": "1763000100", "title": "Mouvement", "ext": {"alarmType": 10000, "pics": ";https://pic/1.jpg;x"}})).unwrap();
        assert_eq!((e.ring, e.picture.as_deref()), (false, Some("https://pic/1.jpg")));
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
        let data = [ENCRYPTED, md5_hex(&md5_hex(code)).as_bytes(), &cipher].concat();
        assert_eq!(decrypt(&data, code).unwrap(), picture);
        assert!(decrypt(&data, "WRONG1").is_err());
        assert_eq!(decrypt(&picture, code).unwrap(), picture);
    }

    fn cloud() {
        testing::answer_json(
            "POST",
            "/v3/users/login/v5",
            &json!({"meta": {"code": 200}, "loginSession": {"sessionId": "s1", "rfSessionId": "r1"}, "loginArea": {"apiDomain": "apiieu.ezvizlife.com"}}),
        );
        testing::answer_json(
            "GET",
            "/v3/userdevices/v1/resources/pagelist",
            &json!({"meta": {"code": 200}, "deviceInfos": [{"deviceSerial": "BD1", "name": "Entrée", "deviceCategory": "BDoorBell", "status": 1}], "page": {"hasNext": false}}),
        );
    }

    #[test]
    fn the_account_keeps_a_hash_and_its_session_and_a_ring_is_a_press() {
        testing::reset();
        cloud();
        testing::account("ezviz", &json!({"account": "moi@exemple.fr", "password": "secret", "code": "123456"}));
        Account::check().unwrap();
        let kept = breezone_module_sdk::host::account("ezviz").unwrap();
        assert_eq!((kept["password"].as_str(), kept["session_id"].as_str(), kept.get("code")), (Some(md5_hex("secret").as_str()), Some("s1"), None));
        assert!(kept["feature_code"].is_string());

        let now = breezone_module_sdk::host::now();
        let ring = |at: i64| json!({"meta": {"code": 200}, "message": [{"deviceSerial": "BD1", "time": at, "title": "Appel", "subType": "2701"}]});
        testing::answer_json("GET", "/v3/unifiedmsg/list", &ring(now - 30));
        let cams: Ezviz = testing::open(json!({}), None);
        let state = testing::check(&cams, MANIFEST);
        assert_eq!((state["BD1"]["online"].clone(), state["BD1"]["buttons"].clone()), (json!(true), json!(1)));
        assert!(testing::presses().is_empty(), "the first reading only notes the last ring");
        testing::answer_json("GET", "/v3/unifiedmsg/list", &ring(now - 5));
        cams.read().unwrap();
        assert_eq!(testing::presses(), [("BD1".to_string(), 1, "single".to_string())]);
    }

    #[test]
    fn an_expired_session_is_renewed_and_kept() {
        testing::reset();
        cloud();
        testing::account(
            "ezviz",
            &json!({"account": "moi@exemple.fr", "password": md5_hex("secret"), "feature_code": "f", "session_id": "old", "rf_session_id": "r0", "api_url": "https://apiieu.ezvizlife.com"}),
        );
        // The first answer says the session is over; the renewal gives a new one.
        testing::respond("GET", "/v3/userdevices/v1/resources/pagelist", {
            let n = std::rc::Rc::new(std::cell::Cell::new(0));
            move |_, _| {
                n.set(n.get() + 1);
                let body = if n.get() == 1 { json!({"meta": {"code": 1003}}) } else { json!({"meta": {"code": 200}, "deviceInfos": [], "page": {"hasNext": false}}) };
                breezone_module_sdk::host::Answer { status: 200, headers: vec![], body: body.to_string().into_bytes() }
            }
        });
        testing::answer_json("PUT", "/v3/apigateway/login", &json!({"meta": {"code": 200}, "sessionInfo": {"sessionId": "new", "refreshSessionId": "r1"}}));
        assert!(devices().unwrap().is_empty());
        assert_eq!(breezone_module_sdk::host::account("ezviz").unwrap()["session_id"], "new");
        assert!(!testing::requests().iter().any(|r| r.contains("login/v5")), "renewed, not logged in again");
    }

    #[test]
    fn an_account_with_two_step_verification_is_sent_a_code() {
        testing::reset();
        testing::answer_json("POST", "/v3/users/login/v5", &json!({"meta": {"code": 6002}}));
        testing::answer_json("POST", "/v3/sms/nologin/checkcode", &json!({"meta": {"code": 200}}));
        testing::account("ezviz", &json!({"account": "moi@exemple.fr", "password": "secret"}));
        assert!(matches!(Account::check(), Err(Error::Invalid(m)) if m.contains("Code reçu")));
    }
}
