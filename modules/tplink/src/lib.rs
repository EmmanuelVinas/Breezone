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

use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use breezone_module_sdk::{device, host, json, module, serde_json, Described, DeviceInfo, Driver, Error, LengthPrefix, Map, ReadUntil, Result, State, Value};
use sha2::{Digest, Sha256};
use std::cell::RefCell;

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

fn unreachable(e: impl std::fmt::Display) -> Error {
    Error::Unreachable(format!("TP-Link: {e}"))
}

fn kasa_address(host_: &str) -> String {
    let h = host_.trim_start_matches("http://").trim_end_matches('/');
    if h.contains(':') {
        format!("tcp://{h}")
    } else {
        format!("tcp://{h}:{KASA_PORT}")
    }
}

/// A Kasa message and its answer: the answer is its length (4 bytes) and that many bytes.
fn kasa_call(host_: &str, message: &Value, timeout_ms: u32) -> Result<Value> {
    let until = ReadUntil::LengthPrefixed(LengthPrefix { offset: 0, size: 4, big_endian: true, adds: 4 });
    let answer = host::exchange(&kasa_address(host_), &xor_encrypt(message.to_string().as_bytes()), until, timeout_ms)?;
    if answer.len() < 4 {
        return Err(unreachable("short answer"));
    }
    serde_json::from_slice(&xor_decrypt(&answer[4..])).map_err(unreachable)
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

/// What the device answers to handshake1 for this local seed, remote seed and account (v2).
pub fn handshake1_proof(local: &[u8], remote: &[u8], auth: &[u8]) -> [u8; 32] {
    sha256(&[local, remote, auth])
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

    /// The message of this sequence: signature + cipher.
    pub fn seal(&self, seq: i32, plain: &[u8]) -> Vec<u8> {
        let mut buf = plain.to_vec();
        buf.resize(plain.len() + 16, 0);
        let cipher = cbc::Encryptor::<aes::Aes128>::new(&self.key.into(), &self.iv_for(seq).into())
            .encrypt_padded::<cbc::cipher::block_padding::Pkcs7>(&mut buf, plain.len())
            .map(|c| c.to_vec())
            .unwrap_or_default();
        let signature = sha256(&[&self.sig, &seq.to_be_bytes(), &cipher]);
        [signature.as_slice(), &cipher].concat()
    }

    /// The next message: (its sequence, signature + cipher).
    pub fn encrypt(&mut self, plain: &[u8]) -> (i32, Vec<u8>) {
        self.seq = self.seq.wrapping_add(1);
        (self.seq, self.seal(self.seq, plain))
    }

    pub fn decrypt(&self, seq: i32, body: &[u8]) -> std::result::Result<Vec<u8>, String> {
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
    host: String,
    user: String,
    password: String,
    link: RefCell<Option<Link>>,
}

fn post(url: &str, cookie: &str, body: &[u8]) -> Result<host::Answer> {
    let headers: Vec<(&str, &str)> = if cookie.is_empty() { vec![] } else { vec![("Cookie", cookie)] };
    let answer = host::request("POST", url, &headers, Some(body), 6000)?;
    if !(200..300).contains(&answer.status) {
        return Err(unreachable(format!("HTTP {}", answer.status)));
    }
    Ok(answer)
}

impl TpLink {
    fn base(&self) -> String {
        if self.host.starts_with("http") {
            self.host.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", self.host)
        }
    }

    /// Opens a KLAP session, with the account (v2 then v1), then the blank one of a device never
    /// set up.
    fn klap(&self) -> Result<(KlapSession, String)> {
        let mut candidates = vec![];
        if !self.user.is_empty() || !self.password.is_empty() {
            candidates.push(auth_hash(&self.user, &self.password, true));
            candidates.push(auth_hash(&self.user, &self.password, false));
        }
        candidates.push(auth_hash("", "", true));
        candidates.push(auth_hash("", "", false));
        let mut local = [0u8; 16];
        getrandom::fill(&mut local).map_err(unreachable)?;
        let resp = post(&format!("{}/app/handshake1", self.base()), "", &local)?;
        let cookie = resp
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("set-cookie"))
            .filter_map(|h| h.value.split(';').next())
            .filter(|c| c.starts_with("TP_SESSIONID"))
            .collect::<Vec<_>>()
            .join("; ");
        if resp.body.len() < 48 {
            return Err(unreachable("handshake: short answer"));
        }
        let (remote, server_hash) = (&resp.body[..16], &resp.body[16..48]);
        for auth in candidates {
            // v2 hashes both seeds, v1 only the local one.
            let second = if handshake1_proof(&local, remote, &auth) == server_hash {
                sha256(&[remote, &local, &auth])
            } else if sha256(&[&local, &auth]) == server_hash {
                sha256(&[remote, &auth])
            } else {
                continue;
            };
            post(&format!("{}/app/handshake2", self.base()), &cookie, &second)?;
            return Ok((KlapSession::new(&local, remote, &auth), cookie));
        }
        Err(Error::Invalid("TP-Link : l'e-mail et le mot de passe du compte TP-Link ne correspondent pas à l'appareil".into()))
    }

    /// The link to the device: Kasa when port 9999 answers, else KLAP.
    fn link(&self) -> Result<Link> {
        if let Some(l) = self.link.borrow().clone() {
            return Ok(l);
        }
        let kasa = !self.host.starts_with("http") && kasa_call(&self.host, &json!({"system": {"get_sysinfo": {}}}), 2000).is_ok();
        let link = if kasa {
            Link::Kasa
        } else {
            let (session, cookie) = self.klap()?;
            let mut link = Link::Klap { session, cookie, tapo: true };
            // A recent Kasa speaks its own messages over KLAP.
            let answer = self.send_on(&mut link, &json!({"method": "get_device_info"}));
            if !answer.as_ref().is_ok_and(|v| v["error_code"].as_i64() == Some(0)) {
                if let Link::Klap { tapo, .. } = &mut link {
                    *tapo = false;
                }
            }
            link
        };
        *self.link.borrow_mut() = Some(link.clone());
        Ok(link)
    }

    fn send_on(&self, link: &mut Link, message: &Value) -> Result<Value> {
        match link {
            Link::Kasa => kasa_call(&self.host, message, 5000),
            Link::Klap { session, cookie, .. } => {
                let (seq, body) = session.encrypt(message.to_string().as_bytes());
                let resp = post(&format!("{}/app/request?seq={seq}", self.base()), cookie, &body)?;
                let plain = session.decrypt(seq, &resp.body).map_err(unreachable)?;
                serde_json::from_slice(&plain).map_err(unreachable)
            }
        }
    }

    /// Sends a message on the link (a new session when the old one is over).
    fn send(&self, message: &Value) -> Result<Value> {
        for attempt in 0..2 {
            let mut link = self.link()?;
            match self.send_on(&mut link, message) {
                Ok(v) => {
                    // The sequence moved on: kept.
                    *self.link.borrow_mut() = Some(link);
                    return Ok(v);
                }
                Err(e) if attempt == 0 => {
                    host::log("debug", &format!("{e:?}, new session"));
                    *self.link.borrow_mut() = None;
                }
                Err(e) => return Err(e),
            }
        }
        Err(unreachable("no session"))
    }

    fn tapo(&self) -> Result<bool> {
        Ok(matches!(self.link()?, Link::Klap { tapo: true, .. }))
    }

    /// The device as it says: Kasa `get_sysinfo`, or Tapo `get_device_info` (+ its power).
    fn read_info(&self) -> Result<Value> {
        if self.tapo()? {
            let mut info = self.send(&json!({"method": "get_device_info"}))?["result"].take();
            if let Ok(e) = self.send(&json!({"method": "get_energy_usage"})) {
                if e["error_code"].as_i64() == Some(0) {
                    info["energy"] = e["result"].clone();
                }
            }
            return Ok(json!({"tapo": info}));
        }
        let mut v = self.send(&json!({"system": {"get_sysinfo": {}}}))?;
        if v["system"]["get_sysinfo"]["feature"].as_str().is_some_and(|f| f.contains("ENE")) {
            if let Ok(e) = self.send(&json!({"emeter": {"get_realtime": {}}})) {
                v["emeter"] = e["emeter"]["get_realtime"].clone();
            }
        }
        Ok(json!({"kasa": v}))
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
            traits.push("light@1");
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
            traits.push("switch.relay@1");
        }
        if let Some(mw) = t["energy"]["current_power"].as_f64() {
            traits.push("energy.power@1");
            s.insert("power".into(), json!((mw / 100.0).round() / 10.0));
        }
        out.push((device("device", kind, &traits, name), s));
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
            out.push((device(format!("child/{id}"), "switch.relay", &["switch.relay@1"], c["alias"].as_str().unwrap_or(&alias)), s));
        }
        return (format!("Kasa {model}"), out);
    }
    let mut s = Map::new();
    let mut traits = vec![];
    let kind;
    if let Some(l) = info.get("light_state") {
        kind = "light";
        traits.push("light@1");
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
        traits.push("switch.relay@1");
        s.insert("on".into(), json!(info["relay_state"].as_i64() == Some(1)));
    }
    let e = &v["kasa"]["emeter"];
    if e.is_object() {
        traits.push("energy.power@1");
        let w = e["power"].as_f64().or_else(|| e["power_mw"].as_f64().map(|mw| mw / 1000.0)).unwrap_or(0.0);
        s.insert("power".into(), json!((w * 10.0).round() / 10.0));
    }
    out.push((device("device", kind, &traits, alias), s));
    (format!("Kasa {model}"), out)
}

impl Driver for TpLink {
    fn open(config: Value, secret: Option<Value>) -> Result<Self> {
        let host_ = config["host"].as_str().map(str::trim).filter(|h| !h.is_empty()).ok_or_else(|| Error::Invalid("l'adresse est requise".into()))?.to_string();
        let user = config["username"].as_str().map(str::trim).unwrap_or_default().to_string();
        let password = secret.as_ref().and_then(|v| v["password"].as_str()).unwrap_or_default().to_string();
        Ok(Self { host: host_, user, password, link: RefCell::new(None) })
    }

    fn describe(&self) -> Result<Described> {
        let v = self.read_info()?;
        let (model, list) = read_all(&v);
        let firmware = v["tapo"]["fw_ver"].as_str().or(v["kasa"]["system"]["get_sysinfo"]["sw_ver"].as_str()).map(str::to_string);
        let identity = v["tapo"]["mac"].as_str().or(v["kasa"]["system"]["get_sysinfo"]["mac"].as_str()).map(str::to_string);
        Ok(Described { model, firmware, devices: list.into_iter().map(|(d, _)| d).collect(), identity })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let v = self.read_info()?;
        Ok(read_all(&v).1.into_iter().map(|(d, s)| (d.reference, s)).collect())
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        if action != "set" || !matches!(trait_name, "switch.relay" | "light") {
            return Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}")));
        }
        let on = params.get("on").and_then(Value::as_bool);
        let brightness = params.get("brightness").and_then(Value::as_i64).map(|b| b.clamp(1, 100));
        let ct = params.get("color_temp").and_then(Value::as_i64);
        let color = params.get("color").and_then(Value::as_str).and_then(hex_hsv);
        if self.tapo()? {
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
            let v = self.send(&json!({"method": "set_device_info", "params": p}))?;
            return match v["error_code"].as_i64() {
                Some(0) => Ok(()),
                code => Err(Error::Invalid(format!("TP-Link : refusé ({})", code.unwrap_or(-1)))),
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
        let v = self.send(&message)?;
        let code = v.as_object().and_then(|o| o.values().next()).and_then(|m| m.as_object()).and_then(|m| m.values().next()).and_then(|r| r["err_code"].as_i64());
        match code {
            Some(0) | None => Ok(()),
            Some(c) => Err(Error::Invalid(format!("TP-Link : refusé ({c})"))),
        }
    }

    fn diagnostic(&self) -> Result<Value> {
        let mut v = self.read_info()?;
        // What tells the hardware apart, not where it is nor whose it is.
        if let Some(o) = v["tapo"].as_object_mut() {
            for k in ["ip", "ssid", "nickname", "mac", "longitude", "latitude"] {
                o.remove(k);
            }
        }
        if let Some(o) = v["kasa"]["system"]["get_sysinfo"].as_object_mut() {
            for k in ["mac", "alias", "deviceId", "hwId", "oemId", "latitude_i", "longitude_i"] {
                o.remove(k);
            }
        }
        Ok(v)
    }

    fn probe(host_: &str) -> Option<String> {
        let v = kasa_call(host_, &json!({"system": {"get_sysinfo": {}}}), 2000).ok()?;
        v["system"]["get_sysinfo"]["model"].as_str().map(|m| format!("Kasa {}", m.split('(').next().unwrap_or(m)))
    }
}

module!(TpLink);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::{testing, Header};
    use std::cell::Cell;
    use std::rc::Rc;

    const MANIFEST: &str = include_str!("../manifest.json");

    fn kasa(v: &Value) -> Vec<u8> {
        xor_encrypt(v.to_string().as_bytes())
    }

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

    #[test]
    fn a_kasa_plug_is_read_and_driven_on_port_9999() {
        testing::reset();
        let sysinfo = json!({"system": {"get_sysinfo": {"model": "HS110(EU)", "alias": "Bouilloire", "sw_ver": "1.5.4", "mac": "AA:BB", "feature": "TIM:ENE", "relay_state": 1}}});
        // The first contact, the description, its power; then the read.
        testing::queue_exchanges(vec![kasa(&sysinfo), kasa(&sysinfo), kasa(&json!({"emeter": {"get_realtime": {"power_mw": 1500400}}}))]);
        let plug: TpLink = testing::open(json!({"host": "192.168.1.30"}), None);
        let d = plug.describe().unwrap();
        assert_eq!(
            (d.model.as_str(), d.firmware.as_deref(), d.devices[0].traits.clone()),
            ("Kasa HS110", Some("1.5.4"), vec!["switch.relay@1".to_string(), "energy.power@1".to_string()])
        );
        let power = kasa(&json!({"emeter": {"get_realtime": {"power": 1500.4}}}));
        testing::queue_exchanges(vec![kasa(&sysinfo), power.clone(), kasa(&sysinfo), power]);
        let state = testing::check(&plug, MANIFEST);
        assert_eq!((state["device"]["on"].clone(), state["device"]["power"].clone()), (json!(true), json!(1500.4)));
        testing::answer_exchange("192.168.1.30:9999", kasa(&json!({"system": {"set_relay_state": {"err_code": 0}}})));
        plug.act("device", "switch.relay", "set", &json!({"on": false}).as_object().unwrap().clone()).unwrap();
        assert!(testing::requests().iter().all(|r| r.starts_with("EXCHANGE tcp://192.168.1.30:9999")));
        testing::answer_exchange("192.168.1.30:9999", kasa(&json!({"system": {"set_relay_state": {"err_code": -3}}})));
        assert!(plug.act("device", "switch.relay", "set", &json!({"on": true}).as_object().unwrap().clone()).is_err());
    }

    /// A Tapo plug that knows the account, answering KLAP as the real one.
    fn tapo_plug(user: &str, password: &str, on: Rc<Cell<bool>>) {
        let auth = auth_hash(user, password, true);
        let remote = [7u8; 16];
        let session: Rc<RefCell<Option<KlapSession>>> = Rc::default();
        let (s1, a1) = (session.clone(), auth.clone());
        testing::respond("POST", "/app/handshake1", move |_, local| {
            *s1.borrow_mut() = Some(KlapSession::new(local, &remote, &a1));
            let body = [remote.as_slice(), &handshake1_proof(local, &remote, &a1)].concat();
            breezone_module_sdk::host::Answer { status: 200, headers: vec![Header { name: "Set-Cookie".into(), value: "TP_SESSIONID=abc;TIMEOUT=86400".into() }], body }
        });
        testing::respond("POST", "/app/handshake2", |headers, _| {
            let ok = headers.iter().any(|(n, v)| *n == "Cookie" && *v == "TP_SESSIONID=abc");
            breezone_module_sdk::host::Answer { status: if ok { 200 } else { 403 }, headers: vec![], body: vec![] }
        });
        testing::respond("POST", "/app/request", move |_, body| {
            let s = session.borrow().clone().unwrap();
            // The sequence follows the one of the session: the one the signature tells.
            let seq = (1..=20).map(|n| s.seq.wrapping_add(n)).find(|q| sha256(&[&s.sig, &q.to_be_bytes(), &body[32..]]) == body[..32]).unwrap();
            let ask: Value = serde_json::from_slice(&s.decrypt(seq, body).unwrap()).unwrap();
            let answer = match ask["method"].as_str() {
                Some("get_device_info") => {
                    json!({"error_code": 0, "result": {"model": "P110", "type": "SMART.TAPOPLUG", "nickname": STANDARD.encode("Lave-linge"), "device_on": on.get(), "fw_ver": "1.3.1", "mac": "11-22"}})
                }
                Some("get_energy_usage") => json!({"error_code": 0, "result": {"current_power": 1834500}}),
                Some("set_device_info") => {
                    on.set(ask["params"]["device_on"].as_bool().unwrap_or(on.get()));
                    json!({"error_code": 0})
                }
                _ => json!({"error_code": -1}),
            };
            breezone_module_sdk::host::Answer { status: 200, headers: vec![], body: s.seal(seq, answer.to_string().as_bytes()) }
        });
    }

    #[test]
    fn a_tapo_plug_opens_a_klap_session_with_the_account() {
        testing::reset();
        let on = Rc::new(Cell::new(true));
        tapo_plug("moi@exemple.fr", "secret", on.clone());
        // No Kasa on port 9999: no answer on the bench, so KLAP.
        let plug: TpLink = testing::open(json!({"host": "192.168.1.31", "username": "moi@exemple.fr"}), Some(json!({"password": "secret"})));
        let d = plug.describe().unwrap();
        assert_eq!((d.model.as_str(), d.devices[0].name.as_str(), d.identity.as_deref()), ("Tapo P110", "Lave-linge", Some("11-22")));
        let state = testing::check(&plug, MANIFEST);
        assert_eq!(state["device"]["power"], json!(1834.5));
        plug.act("device", "switch.relay", "set", &json!({"on": false}).as_object().unwrap().clone()).unwrap();
        assert!(!on.get());
        let handshakes = testing::requests().iter().filter(|r| r.contains("handshake1")).count();
        assert_eq!(handshakes, 1, "the session is kept");
        let diagnostic = plug.diagnostic().unwrap();
        assert!(diagnostic["tapo"].get("mac").is_none() && diagnostic["tapo"].get("nickname").is_none());
    }

    #[test]
    fn a_wrong_account_is_told() {
        testing::reset();
        tapo_plug("moi@exemple.fr", "secret", Rc::new(Cell::new(false)));
        let plug: TpLink = testing::open(json!({"host": "192.168.1.31", "username": "moi@exemple.fr"}), Some(json!({"password": "autre"})));
        assert!(matches!(plug.describe(), Err(Error::Invalid(m)) if m.contains("mot de passe")));
    }
}
