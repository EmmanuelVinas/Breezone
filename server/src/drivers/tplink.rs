//! TP-Link Kasa and Tapo plugs, strips, switches and bulbs, on the local network.
//!
//! Two protocols, chosen at the first contact:
//! * **Kasa (older firmwares)**: JSON over TCP 9999, "encrypted" with an XOR autokey (171), each
//!   message after its length (4 bytes). No credentials.
//!   `{"system":{"get_sysinfo":{}}}`, `{"system":{"set_relay_state":{"state":1}}}`,
//!   `{"emeter":{"get_realtime":{}}}`, `{"smartlife.iot.smartbulb.lightingservice":{…}}`.
//! * **KLAP (Tapo, recent Kasa)**: HTTP on port 80, a handshake proving that both sides know the
//!   TP-Link account (its e-mail and password, used here only to derive the key, never sent),
//!   then AES-128-CBC messages: `get_device_info`, `set_device_info`, `get_energy_usage` (Tapo),
//!   or the Kasa messages above.
//!
//!   POST /app/handshake1  local_seed (16)          → remote_seed (16) + sha256(seeds + auth)
//!   POST /app/handshake2  sha256(seeds + auth)     → 200
//!   POST /app/request?seq=N  sha256(sig + N + cipher) + cipher → the same, the answer

use super::{Described, DeviceInfo, Driver, DriverError, DriverFuture, State};
use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------------------
// Kasa: XOR over TCP 9999
// ---------------------------------------------------------------------------------------

pub const KASA_PORT: u16 = 9999;

pub fn xor_encrypt(plain: &[u8]) -> Vec<u8> {
    let mut key = 171u8;
    let mut out = (plain.len() as u32).to_be_bytes().to_vec();
    for b in plain {
        key ^= b;
        out.push(key);
    }
    out
}

pub fn xor_decrypt(cipher: &[u8]) -> Vec<u8> {
    let mut key = 171u8;
    cipher
        .iter()
        .map(|b| {
            let p = key ^ b;
            key = *b;
            p
        })
        .collect()
}

async fn kasa_call(host: &str, message: &Value) -> Result<Value, DriverError> {
    let addr = if host.contains(':') { host.to_string() } else { format!("{host}:{KASA_PORT}") };
    let io = async {
        let mut s = tokio::net::TcpStream::connect(&addr).await?;
        s.write_all(&xor_encrypt(message.to_string().as_bytes())).await?;
        let mut len = [0u8; 4];
        s.read_exact(&mut len).await?;
        let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
        s.read_exact(&mut body).await?;
        Ok::<_, std::io::Error>(body)
    };
    let body = tokio::time::timeout(Duration::from_secs(5), io)
        .await
        .map_err(|_| DriverError::Unreachable("TP-Link: no answer".into()))?
        .map_err(|e| DriverError::Unreachable(format!("TP-Link: {e}")))?;
    serde_json::from_slice(&xor_decrypt(&body)).map_err(|e| DriverError::Unreachable(format!("TP-Link: {e}")))
}

/// A Kasa at `host` (the network scanner): its model and name.
pub async fn probe(host: &str) -> Option<(String, String)> {
    let v = tokio::time::timeout(Duration::from_secs(2), kasa_call(host, &json!({"system": {"get_sysinfo": {}}}))).await.ok()?.ok()?;
    let info = &v["system"]["get_sysinfo"];
    Some((info["model"].as_str()?.to_string(), info["alias"].as_str().unwrap_or_default().to_string()))
}

// ---------------------------------------------------------------------------------------
// KLAP
// ---------------------------------------------------------------------------------------

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// What the account is reduced to: v1 (Kasa) md5(md5(user) + md5(password)), v2 (Tapo)
/// sha256(sha1(user) + sha1(password)).
pub fn auth_hash(user: &str, password: &str, v2: bool) -> Vec<u8> {
    use md5::Md5;
    if v2 {
        let (u, p) = (sha1::Sha1::digest(user.as_bytes()), sha1::Sha1::digest(password.as_bytes()));
        sha256(&[&u, &p]).to_vec()
    } else {
        let (u, p) = (Md5::digest(user.as_bytes()), Md5::digest(password.as_bytes()));
        Md5::digest([u.as_slice(), p.as_slice()].concat()).to_vec()
    }
}

/// An open KLAP session: its key, the start of its IV, its signature key and its sequence.
#[derive(Clone)]
pub struct KlapSession {
    key: [u8; 16],
    iv: [u8; 12],
    sig: [u8; 28],
    pub seq: i32,
}

impl KlapSession {
    pub fn new(local: &[u8], remote: &[u8], auth: &[u8]) -> Self {
        let lsk = sha256(&[b"lsk", local, remote, auth]);
        let iv = sha256(&[b"iv", local, remote, auth]);
        let ldk = sha256(&[b"ldk", local, remote, auth]);
        Self {
            key: lsk[..16].try_into().unwrap_or_default(),
            iv: iv[..12].try_into().unwrap_or_default(),
            sig: ldk[..28].try_into().unwrap_or([0; 28]),
            seq: i32::from_be_bytes(iv[28..32].try_into().unwrap_or_default()),
        }
    }

    fn iv_for(&self, seq: i32) -> [u8; 16] {
        let mut iv = [0u8; 16];
        iv[..12].copy_from_slice(&self.iv);
        iv[12..].copy_from_slice(&seq.to_be_bytes());
        iv
    }

    /// The next message: (its sequence, signature + cipher).
    pub fn encrypt(&mut self, plain: &[u8]) -> (i32, Vec<u8>) {
        self.seq = self.seq.wrapping_add(1);
        let mut buf = plain.to_vec();
        buf.resize(plain.len() + 16, 0);
        let cipher = cbc::Encryptor::<aes::Aes128>::new(&self.key.into(), &self.iv_for(self.seq).into())
            .encrypt_padded::<cbc::cipher::block_padding::Pkcs7>(&mut buf, plain.len())
            .map(|c| c.to_vec())
            .unwrap_or_default();
        let signature = sha256(&[&self.sig, &self.seq.to_be_bytes(), &cipher]);
        (self.seq, [signature.as_slice(), &cipher].concat())
    }

    pub fn decrypt(&self, seq: i32, body: &[u8]) -> Result<Vec<u8>, String> {
        if body.len() < 32 {
            return Err("answer too short".into());
        }
        let mut buf = body[32..].to_vec();
        cbc::Decryptor::<aes::Aes128>::new(&self.key.into(), &self.iv_for(seq).into())
            .decrypt_padded::<cbc::cipher::block_padding::Pkcs7>(&mut buf)
            .map(|p| p.to_vec())
            .map_err(|_| "cannot decrypt".to_string())
    }
}

/// How a device is spoken to.
#[derive(Clone)]
enum Link {
    Kasa,
    /// KLAP: its session and cookie; `tapo` when it speaks the Tapo messages.
    Klap {
        session: KlapSession,
        cookie: String,
        tapo: bool,
    },
}

pub struct TpLink {
    http: reqwest::Client,
    host: String,
    user: String,
    password: String,
    link: Mutex<Option<Link>>,
    /// The last description of the device.
    info: Mutex<Value>,
}

fn unreachable(e: impl std::fmt::Display) -> DriverError {
    DriverError::Unreachable(format!("TP-Link: {e}"))
}

impl TpLink {
    pub fn new(config: &Value, secret: Option<&str>) -> Result<Self, String> {
        let host = config["host"].as_str().map(str::trim).filter(|h| !h.is_empty()).ok_or("l'adresse est requise")?.to_string();
        let user = config["username"].as_str().map(str::trim).unwrap_or_default().to_string();
        let password = secret.and_then(|s| serde_json::from_str::<Value>(s).ok()).and_then(|v| v["password"].as_str().map(str::to_string)).unwrap_or_default();
        let http = reqwest::Client::builder().timeout(Duration::from_secs(6)).build().unwrap_or_default();
        Ok(Self { http, host, user, password, link: Mutex::new(None), info: Mutex::new(Value::Null) })
    }

    fn base(&self) -> String {
        if self.host.starts_with("http") {
            self.host.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", self.host)
        }
    }

    /// Opens a KLAP session, with the account (v2 then v1), then the blank one of a device never
    /// set up.
    async fn klap(&self) -> Result<(KlapSession, String), DriverError> {
        let mut candidates = vec![];
        if !self.user.is_empty() || !self.password.is_empty() {
            candidates.push(auth_hash(&self.user, &self.password, true));
            candidates.push(auth_hash(&self.user, &self.password, false));
        }
        candidates.push(auth_hash("", "", true));
        candidates.push(auth_hash("", "", false));
        let mut local = [0u8; 16];
        rand::fill(&mut local[..]);
        let resp = self.http.post(format!("{}/app/handshake1", self.base())).body(local.to_vec()).send().await.map_err(unreachable)?;
        if !resp.status().is_success() {
            return Err(unreachable(format!("handshake: HTTP {}", resp.status())));
        }
        let cookie = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|c| c.split(';').next())
            .filter(|c| c.starts_with("TP_SESSIONID"))
            .collect::<Vec<_>>()
            .join("; ");
        let body = resp.bytes().await.map_err(unreachable)?;
        if body.len() < 48 {
            return Err(unreachable("handshake: short answer"));
        }
        let (remote, server_hash) = (&body[..16], &body[16..48]);
        for auth in candidates {
            // v2 hashes both seeds, v1 only the local one.
            let v2 = sha256(&[&local, remote, &auth]);
            let v1 = sha256(&[&local, &auth]);
            let second = if v2 == server_hash {
                sha256(&[remote, &local, &auth])
            } else if v1 == server_hash {
                sha256(&[remote, &auth])
            } else {
                continue;
            };
            let resp = self.http.post(format!("{}/app/handshake2", self.base())).header("Cookie", &cookie).body(second.to_vec()).send().await.map_err(unreachable)?;
            if !resp.status().is_success() {
                return Err(unreachable(format!("handshake: HTTP {}", resp.status())));
            }
            return Ok((KlapSession::new(&local, remote, &auth), cookie));
        }
        Err(DriverError::Invalid("TP-Link : l'e-mail et le mot de passe du compte TP-Link ne correspondent pas à l'appareil".into()))
    }

    /// The link to the device: Kasa when port 9999 answers, else KLAP.
    async fn link(&self) -> Result<Link, DriverError> {
        if let Some(l) = self.link.lock().await.clone() {
            return Ok(l);
        }
        let host = self.host.trim_start_matches("http://").trim_end_matches('/');
        let kasa = tokio::time::timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(format!("{host}:{KASA_PORT}"))).await.is_ok_and(|r| r.is_ok());
        let link = if kasa && !self.host.starts_with("http") {
            Link::Kasa
        } else {
            let (session, cookie) = self.klap().await?;
            let mut link = Link::Klap { session, cookie, tapo: true };
            // A recent Kasa speaks its own messages over KLAP.
            let answer = self.send_on(&mut link, &json!({"method": "get_device_info"})).await;
            if !answer.as_ref().is_ok_and(|v| v["error_code"].as_i64() == Some(0)) {
                if let Link::Klap { tapo, .. } = &mut link {
                    *tapo = false;
                }
            }
            link
        };
        *self.link.lock().await = Some(link.clone());
        Ok(link)
    }

    async fn send_on(&self, link: &mut Link, message: &Value) -> Result<Value, DriverError> {
        match link {
            Link::Kasa => kasa_call(self.host.trim_start_matches("http://"), message).await,
            Link::Klap { session, cookie, .. } => {
                let (seq, body) = session.encrypt(message.to_string().as_bytes());
                let resp = self.http.post(format!("{}/app/request?seq={seq}", self.base())).header("Cookie", cookie.as_str()).body(body).send().await.map_err(unreachable)?;
                if !resp.status().is_success() {
                    return Err(unreachable(format!("HTTP {}", resp.status())));
                }
                let bytes = resp.bytes().await.map_err(unreachable)?;
                let plain = session.decrypt(seq, &bytes).map_err(unreachable)?;
                serde_json::from_slice(&plain).map_err(unreachable)
            }
        }
    }

    /// Sends a message on the link (a new session when the old one is over).
    async fn send(&self, message: &Value) -> Result<Value, DriverError> {
        for attempt in 0..2 {
            let mut link = self.link().await?;
            match self.send_on(&mut link, message).await {
                Ok(v) => {
                    // The sequence moved on: kept.
                    *self.link.lock().await = Some(link);
                    return Ok(v);
                }
                Err(e) if attempt == 0 => {
                    tracing::debug!("TP-Link {}: {e}, new session", self.host);
                    *self.link.lock().await = None;
                }
                Err(e) => return Err(e),
            }
        }
        Err(unreachable("no session"))
    }

    async fn tapo(&self) -> Result<bool, DriverError> {
        Ok(matches!(self.link().await?, Link::Klap { tapo: true, .. }))
    }

    /// The device as it says: Kasa `get_sysinfo`, or Tapo `get_device_info` (+ its power).
    async fn read_info(&self) -> Result<Value, DriverError> {
        let v = if self.tapo().await? {
            let mut info = self.send(&json!({"method": "get_device_info"})).await?["result"].take();
            if let Ok(e) = self.send(&json!({"method": "get_energy_usage"})).await {
                if e["error_code"].as_i64() == Some(0) {
                    info["energy"] = e["result"].clone();
                }
            }
            json!({"tapo": info})
        } else {
            let mut v = self.send(&json!({"system": {"get_sysinfo": {}}})).await?;
            if v["system"]["get_sysinfo"]["feature"].as_str().is_some_and(|f| f.contains("ENE")) {
                if let Ok(e) = self.send(&json!({"emeter": {"get_realtime": {}}})).await {
                    v["emeter"] = e["emeter"]["get_realtime"].clone();
                }
            }
            json!({"kasa": v})
        };
        *self.info.lock().await = v.clone();
        Ok(v)
    }
}

/// A hue (0–360) and saturation (0–100) as a colour "#rrggbb", and back (full brightness).
pub fn hsv_hex(h: f64, s: f64) -> String {
    let (s, v) = (s / 100.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |f: f64| ((f + m) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", to(r), to(g), to(b))
}

pub fn hex_hsv(hex: &str) -> Option<(i64, i64)> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|v| v as f64 / 255.0);
    let (r, g, b) = (p(0)?, p(2)?, p(4)?);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let hue = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let sat = if max == 0.0 { 0.0 } else { d / max * 100.0 };
    Some(((hue.rem_euclid(360.0)).round() as i64, sat.round() as i64))
}

fn decode_name(v: &Value) -> Option<String> {
    let raw = v.as_str()?;
    STANDARD.decode(raw).ok().and_then(|b| String::from_utf8(b).ok()).filter(|s| !s.is_empty()).or_else(|| Some(raw.to_string()))
}

/// The devices and their state, from what the device said.
pub fn read_all(v: &Value) -> (String, Vec<(DeviceInfo, State)>) {
    let mut out = vec![];
    if let Some(t) = v.get("tapo") {
        let model = t["model"].as_str().unwrap_or("Tapo").to_string();
        let name = decode_name(&t["nickname"]).unwrap_or_else(|| model.clone());
        let bulb = t["type"].as_str().is_some_and(|k| k.contains("BULB")) || t.get("brightness").is_some() && t.get("color_temp").is_some();
        let mut s = Map::new();
        s.insert("on".into(), json!(t["device_on"].as_bool().unwrap_or(false)));
        let mut traits = vec![];
        let kind;
        if bulb || t.get("brightness").is_some() {
            kind = "light";
            traits.push("light@1".to_string());
            if let Some(b) = t["brightness"].as_i64() {
                s.insert("brightness".into(), json!(b));
            }
            let ct = t["color_temp"].as_i64().unwrap_or(0);
            if t.get("color_temp").is_some() {
                s.insert("color_temp_min".into(), json!(2500));
                s.insert("color_temp_max".into(), json!(6500));
                if ct > 0 {
                    s.insert("color_temp".into(), json!(ct));
                }
            }
            if let (Some(h), Some(sat)) = (t["hue"].as_f64(), t["saturation"].as_f64()) {
                s.insert("color".into(), json!(hsv_hex(h, sat)));
                s.insert("mode".into(), json!(if ct > 0 { "white" } else { "color" }));
            }
            s.insert("reachable".into(), json!(true));
        } else {
            kind = "switch.relay";
            traits.push("switch.relay@1".to_string());
        }
        if let Some(mw) = t["energy"]["current_power"].as_f64() {
            traits.push("energy.power@1".to_string());
            s.insert("power".into(), json!((mw / 100.0).round() / 10.0));
        }
        out.push((DeviceInfo { reference: "device".into(), kind: kind.into(), traits, name }, s));
        return (format!("Tapo {model}"), out);
    }
    let info = &v["kasa"]["system"]["get_sysinfo"];
    let model = info["model"].as_str().unwrap_or("Kasa").split('(').next().unwrap_or("Kasa").to_string();
    let alias = info["alias"].as_str().unwrap_or(&model).to_string();
    if let Some(children) = info["children"].as_array() {
        for c in children {
            let Some(id) = c["id"].as_str() else { continue };
            let mut s = Map::new();
            s.insert("on".into(), json!(c["state"].as_i64() == Some(1)));
            out.push((
                DeviceInfo {
                    reference: format!("child/{id}"),
                    kind: "switch.relay".into(),
                    traits: vec!["switch.relay@1".into()],
                    name: c["alias"].as_str().unwrap_or(&alias).to_string(),
                },
                s,
            ));
        }
        return (format!("Kasa {model}"), out);
    }
    let mut s = Map::new();
    let mut traits = vec![];
    let kind;
    if let Some(l) = info.get("light_state") {
        kind = "light";
        traits.push("light@1".to_string());
        // Off, a bulb tells its last look in `dft_on_state`.
        let look = if l["on_off"].as_i64() == Some(1) { l } else { &l["dft_on_state"] };
        s.insert("on".into(), json!(l["on_off"].as_i64() == Some(1)));
        if let Some(b) = look["brightness"].as_i64() {
            s.insert("brightness".into(), json!(b));
        }
        if info["is_variable_color_temp"].as_i64() == Some(1) {
            s.insert("color_temp_min".into(), json!(2500));
            s.insert("color_temp_max".into(), json!(6500));
            if let Some(ct) = look["color_temp"].as_i64().filter(|c| *c > 0) {
                s.insert("color_temp".into(), json!(ct));
            }
        }
        if info["is_color"].as_i64() == Some(1) {
            let (h, sat) = (look["hue"].as_f64().unwrap_or(0.0), look["saturation"].as_f64().unwrap_or(0.0));
            s.insert("color".into(), json!(hsv_hex(h, sat)));
            s.insert("mode".into(), json!(if look["color_temp"].as_i64().unwrap_or(0) > 0 { "white" } else { "color" }));
        }
        s.insert("reachable".into(), json!(true));
    } else {
        kind = "switch.relay";
        traits.push("switch.relay@1".to_string());
        s.insert("on".into(), json!(info["relay_state"].as_i64() == Some(1)));
    }
    let e = &v["kasa"]["emeter"];
    if e.is_object() {
        traits.push("energy.power@1".to_string());
        let w = e["power"].as_f64().or_else(|| e["power_mw"].as_f64().map(|mw| mw / 1000.0)).unwrap_or(0.0);
        s.insert("power".into(), json!((w * 10.0).round() / 10.0));
    }
    out.push((DeviceInfo { reference: "device".into(), kind: kind.into(), traits, name: alias }, s));
    (format!("Kasa {model}"), out)
}

impl Driver for TpLink {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            let v = self.read_info().await?;
            let (model, list) = read_all(&v);
            let firmware = v["tapo"]["fw_ver"].as_str().or(v["kasa"]["system"]["get_sysinfo"]["sw_ver"].as_str()).map(str::to_string);
            let identity = v["tapo"]["mac"].as_str().or(v["kasa"]["system"]["get_sysinfo"]["mac"].as_str()).map(str::to_string);
            Ok(Described { model, firmware, devices: list.into_iter().map(|(d, _)| d).collect(), identity })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            let v = self.read_info().await?;
            Ok(read_all(&v).1.into_iter().map(|(d, s)| (d.reference, s)).collect())
        })
    }

    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            if action != "set" || !matches!(trait_name, "switch.relay" | "light") {
                return Err(DriverError::Unsupported(format!("{reference}: {trait_name}.{action}")));
            }
            let on = params.get("on").and_then(Value::as_bool);
            let brightness = params.get("brightness").and_then(Value::as_i64).map(|b| b.clamp(1, 100));
            let ct = params.get("color_temp").and_then(Value::as_i64);
            let color = params.get("color").and_then(Value::as_str).and_then(hex_hsv);
            if self.tapo().await? {
                let mut p = Map::new();
                if let Some(on) = on {
                    p.insert("device_on".into(), json!(on));
                }
                if let Some(b) = brightness {
                    p.insert("brightness".into(), json!(b));
                    p.insert("device_on".into(), json!(true));
                }
                if let Some(k) = ct {
                    p.insert("color_temp".into(), json!(k));
                    p.insert("device_on".into(), json!(true));
                }
                if let Some((h, s)) = color {
                    p.insert("hue".into(), json!(h));
                    p.insert("saturation".into(), json!(s));
                    p.insert("color_temp".into(), json!(0));
                    p.insert("device_on".into(), json!(true));
                }
                let v = self.send(&json!({"method": "set_device_info", "params": p})).await?;
                return match v["error_code"].as_i64() {
                    Some(0) => Ok(()),
                    code => Err(DriverError::Invalid(format!("TP-Link : refusé ({})", code.unwrap_or(-1)))),
                };
            }
            let message = if let Some(id) = reference.strip_prefix("child/") {
                json!({"context": {"child_ids": [id]}, "system": {"set_relay_state": {"state": i32::from(on.unwrap_or(false))}}})
            } else if trait_name == "light" {
                let mut l = Map::new();
                if let Some(on) = on {
                    l.insert("on_off".into(), json!(i32::from(on)));
                }
                if let Some(b) = brightness {
                    l.insert("brightness".into(), json!(b));
                    l.insert("on_off".into(), json!(1));
                }
                if let Some(k) = ct {
                    l.insert("color_temp".into(), json!(k));
                    l.insert("on_off".into(), json!(1));
                }
                if let Some((h, s)) = color {
                    l.insert("hue".into(), json!(h));
                    l.insert("saturation".into(), json!(s));
                    l.insert("color_temp".into(), json!(0));
                    l.insert("on_off".into(), json!(1));
                }
                json!({"smartlife.iot.smartbulb.lightingservice": {"transition_light_state": l}})
            } else {
                json!({"system": {"set_relay_state": {"state": i32::from(on.unwrap_or(false))}}})
            };
            let v = self.send(&message).await?;
            let code = v.as_object().and_then(|o| o.values().next()).and_then(|m| m.as_object()).and_then(|m| m.values().next()).and_then(|r| r["err_code"].as_i64());
            match code {
                Some(0) | None => Ok(()),
                Some(c) => Err(DriverError::Invalid(format!("TP-Link : refusé ({c})"))),
            }
        })
    }

    fn read_every(&self) -> Duration {
        Duration::from_secs(10)
    }

    fn diagnostic(&self) -> DriverFuture<'_, Value> {
        Box::pin(async move {
            let mut v = self.read_info().await?;
            // What tells the hardware apart, not where it is nor whose it is.
            for path in [("tapo", "ip"), ("tapo", "ssid"), ("tapo", "nickname"), ("tapo", "mac"), ("tapo", "longitude"), ("tapo", "latitude")] {
                if let Some(o) = v[path.0].as_object_mut() {
                    o.remove(path.1);
                }
            }
            if let Some(o) = v["kasa"]["system"]["get_sysinfo"].as_object_mut() {
                for k in ["mac", "alias", "deviceId", "hwId", "oemId", "latitude_i", "longitude_i"] {
                    o.remove(k);
                }
            }
            Ok(v)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kasa_xor_goes_both_ways() {
        let msg = br#"{"system":{"get_sysinfo":{}}}"#;
        let enc = xor_encrypt(msg);
        assert_eq!(&enc[..4], &(msg.len() as u32).to_be_bytes());
        assert_eq!(xor_decrypt(&enc[4..]), msg);
    }

    #[test]
    fn a_klap_message_is_signed_and_decrypted_with_its_sequence() {
        let auth = auth_hash("moi@exemple.fr", "secret", true);
        let mut a = KlapSession::new(&[1; 16], &[2; 16], &auth);
        let b = KlapSession::new(&[1; 16], &[2; 16], &auth);
        let start = a.seq;
        let (seq, body) = a.encrypt(br#"{"method":"get_device_info"}"#);
        assert_eq!(seq, start.wrapping_add(1));
        assert_eq!(b.decrypt(seq, &body).unwrap(), br#"{"method":"get_device_info"}"#);
        assert!(b.decrypt(seq + 1, &body).is_err() || b.decrypt(seq + 1, &body).unwrap() != br#"{"method":"get_device_info"}"#);
    }

    #[test]
    fn colours_go_both_ways() {
        assert_eq!(hsv_hex(0.0, 100.0), "#ff0000");
        assert_eq!(hex_hsv("#00ff00"), Some((120, 100)));
        let (h, s) = hex_hsv("#ffb35c").unwrap();
        assert_eq!(hsv_hex(h as f64, s as f64), "#ffb35c");
    }

    #[test]
    fn a_tapo_plug_that_measures_and_a_kasa_strip() {
        let tapo = json!({"tapo": {"model": "P110", "type": "SMART.TAPOPLUG", "nickname": STANDARD.encode("Lave-linge"), "device_on": true, "energy": {"current_power": 1834500}}});
        let (model, list) = read_all(&tapo);
        assert_eq!((model.as_str(), list[0].0.name.as_str(), list[0].0.kind.as_str()), ("Tapo P110", "Lave-linge", "switch.relay"));
        assert_eq!((list[0].1["on"].clone(), list[0].1["power"].clone()), (json!(true), json!(1834.5)));
        let strip = json!({"kasa": {"system": {"get_sysinfo": {"model": "HS300(EU)", "alias": "Bureau", "children": [{"id": "8006A", "alias": "Écran", "state": 1}, {"id": "8006B", "alias": "Imprimante", "state": 0}]}}}});
        let (model, list) = read_all(&strip);
        assert_eq!(model, "Kasa HS300");
        let refs: Vec<(&str, bool)> = list.iter().map(|(d, s)| (d.reference.as_str(), s["on"] == true)).collect();
        assert_eq!(refs, [("child/8006A", true), ("child/8006B", false)]);
    }
}
