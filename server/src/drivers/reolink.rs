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
//! that the pages put in words.

use super::{Described, DeviceInfo, Driver, DriverError, DriverFuture, Sink, State};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

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

/// A Reolink camera or NVR on `host`: its API answers, asking to log in.
pub async fn probe(host: &str) -> bool {
    let Ok(http) = reqwest::Client::builder().timeout(Duration::from_secs(2)).danger_accept_invalid_certs(true).build() else { return false };
    for base in [format!("https://{host}"), format!("http://{host}")] {
        let Ok(answer) = http.post(format!("{base}/cgi-bin/api.cgi?cmd=GetDevInfo")).json(&json!([{"cmd": "GetDevInfo", "action": 0, "param": {"channel": 0}}])).send().await
        else {
            continue;
        };
        if answer.json::<Value>().await.is_ok_and(|v| v[0]["cmd"] == "GetDevInfo") {
            return true;
        }
    }
    false
}

pub struct Reolink {
    http: reqwest::Client,
    host: String,
    user: String,
    password: String,
    /// The base that answered ("https://…" or "http://…") and the session's token.
    session: Mutex<Option<(String, String)>>,
    channels: Mutex<Vec<Channel>>,
    /// The last event of each channel: (key, when), and whether a visitor rang at the last look.
    last: Mutex<HashMap<i64, (&'static str, i64)>>,
    ringing: Mutex<HashMap<i64, bool>>,
    sink: std::sync::Mutex<Option<Sink>>,
}

fn unreachable(e: impl std::fmt::Display) -> DriverError {
    DriverError::Unreachable(format!("Reolink: {e}"))
}

impl Reolink {
    pub fn new(config: &Value, secret: Option<&str>) -> Result<Self, String> {
        let host = config["host"].as_str().map(str::trim).filter(|h| !h.is_empty()).ok_or("l'adresse est requise")?.trim_end_matches('/').to_string();
        let user = config["username"].as_str().map(str::trim).filter(|u| !u.is_empty()).unwrap_or("admin").to_string();
        let password = secret.and_then(|s| serde_json::from_str::<Value>(s).ok()).and_then(|v| v["password"].as_str().map(str::to_string)).unwrap_or_default();
        // A camera answers with its own certificate: it is not checked (the network is the home's).
        let http = reqwest::Client::builder().timeout(Duration::from_secs(10)).danger_accept_invalid_certs(true).build().map_err(|e| e.to_string())?;
        Ok(Self {
            http,
            host,
            user,
            password,
            session: Mutex::new(None),
            channels: Mutex::new(vec![]),
            last: Mutex::new(HashMap::new()),
            ringing: Mutex::new(HashMap::new()),
            sink: std::sync::Mutex::new(None),
        })
    }

    /// The bases to try: the one given, else HTTPS then HTTP.
    fn bases(&self) -> Vec<String> {
        if self.host.starts_with("http") {
            vec![self.host.clone()]
        } else {
            vec![format!("https://{}", self.host), format!("http://{}", self.host)]
        }
    }

    async fn login(&self) -> Result<(String, String), DriverError> {
        if self.password.is_empty() {
            return Err(DriverError::Invalid("Reolink : le mot de passe de la caméra est requis".into()));
        }
        let body = json!([{"cmd": "Login", "param": {"User": {"Version": "0", "userName": self.user, "password": self.password}}}]);
        let mut last_error = None;
        for base in self.bases() {
            let resp = match self.http.post(format!("{base}/cgi-bin/api.cgi?cmd=Login")).json(&body).send().await {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(unreachable(e));
                    continue;
                }
            };
            let v: Value = resp.json().await.map_err(unreachable)?;
            let first = &v[0];
            if first["code"].as_i64() == Some(0) {
                let token = first["value"]["Token"]["name"].as_str().ok_or_else(|| unreachable("no token"))?.to_string();
                return Ok((base, token));
            }
            let detail = first["error"]["detail"].as_str().unwrap_or("login failed");
            return Err(DriverError::Invalid(format!("Reolink : identifiants refusés ({detail})")));
        }
        Err(last_error.unwrap_or_else(|| unreachable("no answer")))
    }

    /// Sends commands at once (logging in, or again when the session is over).
    async fn call(&self, commands: Value) -> Result<Vec<Value>, DriverError> {
        for attempt in 0..2 {
            let (base, token) = {
                let mut session = self.session.lock().await;
                if session.is_none() {
                    *session = Some(self.login().await?);
                }
                session.clone().unwrap_or_default()
            };
            let cmd = commands[0]["cmd"].as_str().unwrap_or("GetDevInfo");
            let resp = self.http.post(format!("{base}/cgi-bin/api.cgi?cmd={cmd}&token={token}")).json(&commands).send().await.map_err(unreachable)?;
            let v: Value = resp.json().await.map_err(unreachable)?;
            let list = v.as_array().cloned().unwrap_or_default();
            // -6: please log in first (the token ran out).
            let expired = list.iter().any(|r| r["code"].as_i64() == Some(1) && r["error"]["rspCode"].as_i64() == Some(-6));
            if expired && attempt == 0 {
                *self.session.lock().await = None;
                continue;
            }
            return Ok(list);
        }
        Err(unreachable("session refused"))
    }

    async fn load(&self) -> Result<(Value, Vec<Channel>), DriverError> {
        let list = self.call(json!([{"cmd": "GetDevInfo", "action": 0, "param": {}}, {"cmd": "GetChannelstatus", "action": 0, "param": {}}])).await?;
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
        *self.channels.lock().await = channels.clone();
        Ok((info, channels))
    }
}

impl Driver for Reolink {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let (info, channels) = self.load().await?;
            let devices = channels
                .iter()
                .map(|c| {
                    let (kind, traits) = if c.doorbell { ("doorbell", vec!["camera@1", "button@1"]) } else { ("camera", vec!["camera@1"]) };
                    DeviceInfo { reference: c.channel.to_string(), kind: kind.into(), traits: traits.into_iter().map(str::to_string).collect(), name: c.name.clone() }
                })
                .collect();
            Ok(Described {
                model: info["model"].as_str().map(|m| format!("Reolink {m}")).unwrap_or_else(|| "Reolink".into()),
                firmware: info["firmVer"].as_str().map(str::to_string),
                devices,
                identity: info["serial"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
            })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            let channels = {
                let known = self.channels.lock().await.clone();
                if known.is_empty() {
                    self.load().await?.1
                } else {
                    known
                }
            };
            let commands: Vec<Value> = channels
                .iter()
                .flat_map(|c| {
                    [json!({"cmd": "GetMdState", "action": 0, "param": {"channel": c.channel}}), json!({"cmd": "GetAiState", "action": 0, "param": {"channel": c.channel}})]
                })
                .collect();
            let answers = self.call(Value::Array(commands)).await?;
            let now = crate::db::now();
            let mut last = self.last.lock().await;
            let mut ringing = self.ringing.lock().await;
            let mut out = HashMap::new();
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
                    if let Some(sink) = self.sink.lock().ok().and_then(|s| s.clone()) {
                        sink.press(&c.channel.to_string(), 1, "single");
                    }
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
                out.insert(c.channel.to_string(), st);
            }
            Ok(out)
        })
    }

    fn act<'a>(&'a self, _reference: &'a str, _trait_name: &'a str, _action: &'a str, _params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async { Err(DriverError::Unsupported("nothing to drive on a Reolink camera yet".into())) })
    }

    /// Motion and rings show within a few seconds.
    fn read_every(&self) -> Duration {
        Duration::from_secs(5)
    }

    fn listen(&self, sink: Sink) {
        if let Ok(mut s) = self.sink.lock() {
            *s = Some(sink);
        }
    }

    fn picture<'a>(&'a self, reference: &'a str) -> DriverFuture<'a, (Vec<u8>, &'static str)> {
        Box::pin(async move {
            let channel: i64 = reference.parse().map_err(|_| DriverError::Unsupported("unknown camera".into()))?;
            for attempt in 0..2 {
                let (base, token) = {
                    let mut session = self.session.lock().await;
                    if session.is_none() {
                        *session = Some(self.login().await?);
                    }
                    session.clone().unwrap_or_default()
                };
                let rs: u32 = rand::random();
                let resp = self.http.get(format!("{base}/cgi-bin/api.cgi?cmd=Snap&channel={channel}&rs={rs:x}&token={token}")).send().await.map_err(unreachable)?;
                let jpeg = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("image/"));
                let bytes = resp.bytes().await.map_err(unreachable)?.to_vec();
                if jpeg || bytes.starts_with(b"\xff\xd8") {
                    return Ok((bytes, "image/jpeg"));
                }
                // A JSON answer: the session is over, or the camera refuses.
                if attempt == 0 {
                    *self.session.lock().await = None;
                }
            }
            Err(unreachable("no picture"))
        })
    }

    fn diagnostic(&self) -> DriverFuture<'_, Value> {
        Box::pin(async move {
            let (info, channels) = self.load().await?;
            let mut info = info;
            if let Some(o) = info.as_object_mut() {
                o.remove("serial");
                o.remove("name");
            }
            Ok(json!({ "device": info, "channels": channels.len() }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_camera_sees() {
        assert_eq!(seen(&json!({"state": 0}), &json!({"people": {"alarm_state": 0}})), Seen::default());
        let s = seen(&json!({"state": 1}), &json!({"people": {"alarm_state": 1, "support": 1}, "vehicle": {"alarm_state": 0}}));
        assert_eq!((s.motion, s.event, s.ring), (true, Some("person"), false));
        assert_eq!(seen(&json!({"state": 1}), &json!({})).event, Some("motion"));
        assert!(seen(&json!({"state": 0}), &json!({"visitor": {"alarm_state": 1}})).ring);
    }
}
