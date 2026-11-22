//! Reolink cameras, doorbells and recorders (NVR), through their local API, in HTTPS (the default
//! of recent firmwares, with the camera's own certificate) or HTTP.
//!
//!   POST /cgi-bin/api.cgi?cmd=Login [{cmd: Login, param: {User: {userName, password}}}]
//!        → [{code: 0, value: {Token: {name, leaseTime}}}]
//!   POST /cgi-bin/api.cgi?cmd=…&token=… [{cmd, action: 0, param}, …] → one answer per command
//!        GetDevInfo, GetChannelstatus (a recorder's cameras), GetMdState (motion),
//!        GetAiState (person, vehicle, animal; a doorbell's visitor)
//!   GET  /cgi-bin/api.cgi?cmd=Snap&channel=…&rs=…&token=… → a JPEG of now
//!
//! A camera per channel (`camera@1`); a doorbell also has a button (`button@1`): someone ringing
//! is a press. The last event is told by a key ("motion", "person", "vehicle", "animal", "ring")
//! that the pages put in words. The camera's own certificate is accepted (Breezone allows it on
//! the local network only).

use breezone_module_sdk::{device, host, json, module, Described, Driver, Error, Map, Picture, Result, State, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
struct Channel {
    channel: i64,
    name: String,
    online: bool,
    doorbell: bool,
}

/// What a channel says now, from the answers of its commands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    pub motion: bool,
    /// "person" | "vehicle" | "animal" | "motion", the most telling one.
    pub event: Option<&'static str>,
    pub ring: bool,
}

/// What a channel sees, from `GetMdState` and `GetAiState` (or `GetEvents`).
pub fn seen(md: &Value, ai: &Value) -> Seen {
    let alarm = |v: &Value, k: &str| v[k]["alarm_state"].as_i64() == Some(1);
    let motion = md["state"].as_i64() == Some(1) || alarm(md, "md");
    let event = if alarm(ai, "people") || alarm(ai, "person") {
        Some("person")
    } else if alarm(ai, "vehicle") {
        Some("vehicle")
    } else if alarm(ai, "dog_cat") {
        Some("animal")
    } else if motion {
        Some("motion")
    } else {
        None
    };
    Seen { motion: motion || event.is_some(), event, ring: alarm(ai, "visitor") }
}

fn unreachable(e: impl std::fmt::Debug) -> Error {
    Error::Unreachable(format!("Reolink: {e:?}"))
}

pub struct Reolink {
    host: String,
    user: String,
    password: String,
    /// The base that answered ("https://…" or "http://…") and the session's token.
    session: RefCell<Option<(String, String)>>,
    channels: RefCell<Vec<Channel>>,
    /// The last event of each channel: (key, when), and whether a visitor rang at the last look.
    last: RefCell<HashMap<i64, (&'static str, i64)>>,
    ringing: RefCell<HashMap<i64, bool>>,
    /// Makes each picture asked a new one (no cache on the way).
    nonce: Cell<u32>,
}

/// A JSON request to a camera (its own certificate accepted).
fn post(url: &str, body: &Value) -> Result<Value> {
    let answer = host::request_insecure("POST", url, &[("Content-Type", "application/json")], Some(body.to_string().as_bytes()), 10_000)?;
    answer.json()
}

impl Reolink {
    /// The bases to try: the one given, else HTTPS then HTTP.
    fn bases(&self) -> Vec<String> {
        if self.host.starts_with("http") {
            vec![self.host.clone()]
        } else {
            vec![format!("https://{}", self.host), format!("http://{}", self.host)]
        }
    }

    fn login(&self) -> Result<(String, String)> {
        if self.password.is_empty() {
            return Err(Error::Invalid("Reolink : le mot de passe de la caméra est requis".into()));
        }
        let body = json!([{"cmd": "Login", "param": {"User": {"Version": "0", "userName": self.user, "password": self.password}}}]);
        let mut last_error = None;
        for base in self.bases() {
            let v = match post(&format!("{base}/cgi-bin/api.cgi?cmd=Login"), &body) {
                Ok(v) => v,
                Err(e) => {
                    last_error = Some(e);
                    continue;
                }
            };
            let first = &v[0];
            if first["code"].as_i64() == Some(0) {
                let token = first["value"]["Token"]["name"].as_str().ok_or_else(|| unreachable("no token"))?.to_string();
                return Ok((base, token));
            }
            let detail = first["error"]["detail"].as_str().unwrap_or("login failed");
            return Err(Error::Invalid(format!("Reolink : identifiants refusés ({detail})")));
        }
        Err(last_error.unwrap_or_else(|| unreachable("no answer")))
    }

    fn session(&self) -> Result<(String, String)> {
        if self.session.borrow().is_none() {
            let s = self.login()?;
            *self.session.borrow_mut() = Some(s);
        }
        Ok(self.session.borrow().clone().unwrap_or_default())
    }

    /// Sends commands at once (logging in, or again when the session is over).
    fn call(&self, commands: Value) -> Result<Vec<Value>> {
        for attempt in 0..2 {
            let (base, token) = self.session()?;
            let cmd = commands[0]["cmd"].as_str().unwrap_or("GetDevInfo");
            let v = post(&format!("{base}/cgi-bin/api.cgi?cmd={cmd}&token={token}"), &commands)?;
            let list = v.as_array().cloned().unwrap_or_default();
            // -6: please log in first (the token ran out).
            let expired = list.iter().any(|r| r["code"].as_i64() == Some(1) && r["error"]["rspCode"].as_i64() == Some(-6));
            if expired && attempt == 0 {
                *self.session.borrow_mut() = None;
                continue;
            }
            return Ok(list);
        }
        Err(unreachable("session refused"))
    }

    fn load(&self) -> Result<(Value, Vec<Channel>)> {
        let list = self.call(json!([{"cmd": "GetDevInfo", "action": 0, "param": {}}, {"cmd": "GetChannelstatus", "action": 0, "param": {}}]))?;
        let info = list.first().map(|r| r["value"]["DevInfo"].clone()).unwrap_or(Value::Null);
        let model = info["model"].as_str().unwrap_or("Reolink").to_string();
        let doorbell = model.to_lowercase().contains("doorbell") || info["type"].as_str() == Some("BELL");
        let statuses = list.get(1).and_then(|r| r["value"]["status"].as_array().cloned()).unwrap_or_default();
        let channels = if statuses.len() > 1 || info["channelNum"].as_i64().unwrap_or(1) > 1 {
            statuses
                .iter()
                .filter_map(|s| {
                    let ch = s["channel"].as_i64()?;
                    Some(Channel {
                        channel: ch,
                        name: s["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("{model} {}", ch + 1)),
                        online: s["online"].as_i64() != Some(0),
                        doorbell: s["typeInfo"].as_str().is_some_and(|t| t.to_lowercase().contains("doorbell")),
                    })
                })
                .collect()
        } else {
            vec![Channel { channel: 0, name: info["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(&model).to_string(), online: true, doorbell }]
        };
        *self.channels.borrow_mut() = channels.clone();
        Ok((info, channels))
    }
}

impl Driver for Reolink {
    fn open(config: Value, secret: Option<Value>) -> Result<Self> {
        let host =
            config["host"].as_str().map(str::trim).filter(|h| !h.is_empty()).ok_or_else(|| Error::Invalid("l'adresse est requise".into()))?.trim_end_matches('/').to_string();
        let user = config["username"].as_str().map(str::trim).filter(|u| !u.is_empty()).unwrap_or("admin").to_string();
        let password = secret.and_then(|v| v["password"].as_str().map(str::to_string)).unwrap_or_default();
        Ok(Reolink {
            host,
            user,
            password,
            session: RefCell::new(None),
            channels: RefCell::new(vec![]),
            last: RefCell::new(HashMap::new()),
            ringing: RefCell::new(HashMap::new()),
            nonce: Cell::new(host::now() as u32),
        })
    }

    fn describe(&self) -> Result<Described> {
        let (info, channels) = self.load()?;
        let devices = channels
            .iter()
            .map(|c| {
                if c.doorbell {
                    device(c.channel.to_string(), "doorbell", &["camera@1", "button@1"], c.name.clone())
                } else {
                    device(c.channel.to_string(), "camera", &["camera@1"], c.name.clone())
                }
            })
            .collect();
        Ok(Described {
            model: info["model"].as_str().map(|m| format!("Reolink {m}")).unwrap_or_else(|| "Reolink".into()),
            firmware: info["firmVer"].as_str().map(str::to_string),
            devices,
            identity: info["serial"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
        })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let known = self.channels.borrow().clone();
        let channels = if known.is_empty() { self.load()?.1 } else { known };
        let commands: Vec<Value> = channels
            .iter()
            .flat_map(|c| [json!({"cmd": "GetMdState", "action": 0, "param": {"channel": c.channel}}), json!({"cmd": "GetAiState", "action": 0, "param": {"channel": c.channel}})])
            .collect();
        let answers = self.call(Value::Array(commands))?;
        let now = host::now();
        let mut last = self.last.borrow_mut();
        let mut ringing = self.ringing.borrow_mut();
        let mut out = vec![];
        for (i, c) in channels.iter().enumerate() {
            let md = answers.get(2 * i).map(|r| r["value"].clone()).unwrap_or(Value::Null);
            let ai = answers.get(2 * i + 1).map(|r| r["value"].clone()).unwrap_or(Value::Null);
            let s = seen(&md, &ai);
            if let Some(e) = s.event {
                // An event goes on as long as it is seen: its start is kept.
                if last.get(&c.channel).is_none_or(|(k, at)| *k != e || now - at > 60) {
                    last.insert(c.channel, (e, now));
                }
            }
            // A visitor who rings: a press, once per ring.
            let was = ringing.insert(c.channel, s.ring).unwrap_or(false);
            if s.ring && !was {
                last.insert(c.channel, ("ring", now));
                host::pressed(&c.channel.to_string(), 1, "single");
            }
            let mut st = Map::new();
            st.insert("online".into(), json!(c.online));
            st.insert("motion".into(), json!(s.motion));
            if let Some((k, at)) = last.get(&c.channel) {
                st.insert("last_event".into(), json!(k));
                st.insert("last_event_at".into(), json!(at));
            }
            // The picture is that of now: a new one every minute.
            st.insert("image_at".into(), json!(now - now % 60));
            if c.doorbell {
                st.insert("buttons".into(), json!(1));
            }
            out.push((c.channel.to_string(), st));
        }
        Ok(out)
    }

    fn act(&self, _reference: &str, _trait_name: &str, _action: &str, _params: &State) -> Result<()> {
        Err(Error::Unsupported("nothing to drive on a Reolink camera yet".into()))
    }

    fn picture(&self, reference: &str) -> Result<Picture> {
        let channel: i64 = reference.parse().map_err(|_| Error::Unsupported("unknown camera".into()))?;
        for attempt in 0..2 {
            let (base, token) = self.session()?;
            let rs = self.nonce.get().wrapping_mul(1_103_515_245).wrapping_add(12_345);
            self.nonce.set(rs);
            let answer = host::request_insecure("GET", &format!("{base}/cgi-bin/api.cgi?cmd=Snap&channel={channel}&rs={rs:x}&token={token}"), &[], None, 10_000)?;
            let image = answer.headers.iter().any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value.starts_with("image/"));
            if image || answer.body.starts_with(b"\xff\xd8") {
                return Ok(Picture { data: answer.body, media_type: "image/jpeg".into() });
            }
            // A JSON answer: the session is over, or the camera refuses.
            if attempt == 0 {
                *self.session.borrow_mut() = None;
            }
        }
        Err(unreachable("no picture"))
    }

    fn diagnostic(&self) -> Result<Value> {
        let (mut info, channels) = self.load()?;
        if let Some(o) = info.as_object_mut() {
            o.remove("serial");
            o.remove("name");
        }
        Ok(json!({ "device": info, "channels": channels.len() }))
    }

    /// A Reolink camera or NVR on `host`: its API answers, asking to log in.
    fn probe(host_: &str) -> Option<String> {
        let ask = json!([{"cmd": "GetDevInfo", "action": 0, "param": {"channel": 0}}]);
        for base in [format!("https://{host_}"), format!("http://{host_}")] {
            if post(&format!("{base}/cgi-bin/api.cgi?cmd=GetDevInfo"), &ask).is_ok_and(|v| v[0]["cmd"] == "GetDevInfo") {
                return Some("Reolink".into());
            }
        }
        None
    }
}

module!(Reolink);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    #[test]
    fn what_a_camera_sees() {
        assert_eq!(seen(&json!({"state": 0}), &json!({"people": {"alarm_state": 0}})), Seen::default());
        let s = seen(&json!({"state": 1}), &json!({"people": {"alarm_state": 1, "support": 1}, "vehicle": {"alarm_state": 0}}));
        assert_eq!((s.motion, s.event, s.ring), (true, Some("person"), false));
        assert_eq!(seen(&json!({"state": 1}), &json!({})).event, Some("motion"));
        assert!(seen(&json!({"state": 0}), &json!({"visitor": {"alarm_state": 1}})).ring);
    }

    fn doorbell(ring: bool) {
        testing::answer_json("POST", "cmd=Login", &json!([{"code": 0, "value": {"Token": {"name": "tok", "leaseTime": 3600}}}]));
        testing::answer_json(
            "POST",
            "cmd=GetDevInfo&token=tok",
            &json!([{"code": 0, "value": {"DevInfo": {"model": "Reolink Video Doorbell PoE", "type": "BELL", "firmVer": "v3.0", "serial": "S1", "name": "Entrée", "channelNum": 1}}},
                    {"code": 0, "value": {"status": []}}]),
        );
        testing::answer_json(
            "POST",
            "cmd=GetMdState&token=tok",
            &json!([{"code": 0, "value": {"state": 1}}, {"code": 0, "value": {"people": {"alarm_state": 1}, "visitor": {"alarm_state": i32::from(ring)}}}]),
        );
    }

    #[test]
    fn a_doorbell_that_rings_presses_once() {
        testing::reset();
        doorbell(false);
        let r: Reolink = testing::open(json!({"host": "192.168.1.50"}), Some(json!({"password": "pw"})));
        let s = testing::check(&r, MANIFEST);
        assert_eq!((s["0"]["motion"].clone(), s["0"]["last_event"].clone(), s["0"]["buttons"].clone()), (json!(true), json!("person"), json!(1)));
        assert!(testing::requests()[0].starts_with("POST https://192.168.1.50/cgi-bin/api.cgi?cmd=Login"));
        doorbell(true);
        r.read().unwrap();
        r.read().unwrap();
        assert_eq!(testing::presses(), [("0".to_string(), 1, "single".to_string())], "one press per ring");
    }

    #[test]
    fn a_picture_and_a_refused_password() {
        testing::reset();
        doorbell(false);
        testing::answer("GET", "cmd=Snap", 200, b"\xff\xd8\xff\xe0jpeg".to_vec());
        let r: Reolink = testing::open(json!({"host": "192.168.1.50"}), Some(json!({"password": "pw"})));
        let p = r.picture("0").unwrap();
        assert_eq!((p.media_type.as_str(), p.data.len()), ("image/jpeg", 8));
        let none: Reolink = testing::open(json!({"host": "192.168.1.50"}), None);
        assert!(matches!(none.read(), Err(Error::Invalid(m)) if m.contains("mot de passe")));
        testing::answer_json("POST", "cmd=Login", &json!([{"code": 1, "error": {"detail": "login failed", "rspCode": -7}}]));
        let wrong: Reolink = testing::open(json!({"host": "192.168.1.51"}), Some(json!({"password": "bad"})));
        assert!(matches!(wrong.describe(), Err(Error::Invalid(m)) if m.contains("refusés")));
    }
}
