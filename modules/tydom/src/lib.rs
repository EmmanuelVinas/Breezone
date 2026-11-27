//! Delta Dore Tydom gateways (1.0, Home, 2.0), through their local WebSocket, as `tydom2mqtt`
//! reaches them: no Delta Dore cloud once the password of the gateway is known. A gateway whose
//! password is not on its label (Tydom 2.0, Home) gets it from the Delta Dore account linked in
//! Breezone (`deltadore`, see `cloud`), once, when the source opens.
//!
//!   GET  https://<ip>/mediation/client?mac=<MAC>&appli=1 → 401, a Digest challenge (realm
//!        « protected area »): user the MAC, the password of the gateway
//!   wss://<ip>/mediation/client?mac=<MAC>&appli=1, with the `Authorization` computed from it
//!        (interface 1.3: `host::subscribe_with`), frames sent as binary
//!
//! On it, HTTP/1.1 messages each way, one a frame: `GET /info`, `GET /configs/file` (the endpoints,
//! their name and `last_usage`: shutter, light, window, alarm…), `GET /devices/meta`, `GET
//! /devices/data` (`[{id, endpoints: [{id, error, data: [{name, value, validity}]}]}]`), `POST
//! /refresh/all`, `PUT /devices/<id>/endpoints/<ep>/data` (`[{name, value}]`); the gateway pushes
//! `PUT /devices/data` when something changes, often chunked.
//!
//! Shutters (`position`, 100 open; `positionCmd` STOP) and lights (`level`; `levelCmd` ON, OFF) are
//! driven; every other endpoint is listed in the diagnostic, so that its kind may be supported next.

use breezone_module_sdk::{device, host, json, module, Described, DeviceInfo, Driver, Error, Found, Map, Result, State, Value};
use md5::{Digest, Md5};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

pub mod cloud;

/// The account of Delta Dore, a module of its own.
const ACCOUNT: &str = "deltadore";

const REALM: &str = "protected area";
const TIMEOUT_MS: u32 = 8000;
/// The gateway sends everything again when asked this often.
const REFRESH_EVERY: i64 = 300;
/// The devices are read again when nothing came for this long.
const DATA_EVERY: i64 = 120;

fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn unreachable(e: impl std::fmt::Display) -> Error {
    Error::Unreachable(format!("Tydom : {e}"))
}

// ---------------------------------------------------------------------------------------
// Digest
// ---------------------------------------------------------------------------------------

/// The fields of a `WWW-Authenticate: Digest …` challenge.
pub fn challenge(header: &str) -> HashMap<String, String> {
    let rest = header.trim().strip_prefix("Digest").unwrap_or(header).trim();
    let mut out = HashMap::new();
    let mut chars = rest.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| *c == ',' || c.is_whitespace()) {
            chars.next();
        }
        let key: String = chars.by_ref().take_while(|c| *c != '=').collect::<String>().trim().to_lowercase();
        if key.is_empty() {
            break;
        }
        let value: String = if chars.peek() == Some(&'"') {
            chars.next();
            chars.by_ref().take_while(|c| *c != '"').collect()
        } else {
            chars.by_ref().take_while(|c| *c != ',').collect::<String>().trim().to_string()
        };
        out.insert(key, value);
    }
    out
}

/// The `Authorization` of a Digest (MD5, qop auth) for `GET uri`.
pub fn digest(user: &str, password: &str, uri: &str, chal: &HashMap<String, String>, cnonce: &str) -> String {
    let realm = chal.get("realm").map(String::as_str).unwrap_or(REALM);
    let nonce = chal.get("nonce").map(String::as_str).unwrap_or_default();
    let ha1 = md5_hex(&format!("{user}:{realm}:{password}"));
    let ha2 = md5_hex(&format!("GET:{uri}"));
    let nc = "00000001";
    let response = md5_hex(&format!("{ha1}:{nonce}:{nc}:{cnonce}:auth:{ha2}"));
    let mut out = format!(r#"Digest username="{user}", realm="{realm}", nonce="{nonce}", uri="{uri}", response="{response}""#);
    if let Some(o) = chal.get("opaque") {
        out += &format!(r#", opaque="{o}""#);
    }
    out += &format!(r#", algorithm="MD5", qop="auth", nc={nc}, cnonce="{cnonce}""#);
    out
}

fn cnonce() -> String {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ---------------------------------------------------------------------------------------
// HTTP in the frames
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// "HTTP/1.1 200 OK", "PUT /devices/data HTTP/1.1".
    pub first: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Message {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn status(&self) -> Option<u16> {
        self.first.strip_prefix("HTTP/1.1 ")?.get(..3)?.parse().ok()
    }

    /// The path of a request the gateway pushes ("/devices/data").
    pub fn pushed(&self) -> Option<&str> {
        let mut parts = self.first.split(' ');
        let method = parts.next()?;
        (method != "HTTP/1.1").then(|| parts.next()).flatten()
    }

    pub fn json(&self) -> Option<Value> {
        let text = String::from_utf8_lossy(&self.body);
        let t = text.trim();
        (!t.is_empty()).then(|| serde_json_from(t)).flatten()
    }
}

fn serde_json_from(s: &str) -> Option<Value> {
    breezone_module_sdk::serde_json::from_str(s).ok()
}

fn unchunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    while let Some(eol) = body.windows(2).position(|w| w == b"\r\n") {
        let size = std::str::from_utf8(&body[..eol]).ok().and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or("").trim(), 16).ok());
        let Some(size) = size else { break };
        if size == 0 {
            break;
        }
        let start = eol + 2;
        let end = (start + size).min(body.len());
        out.extend_from_slice(&body[start..end]);
        body = body.get(end + 2..).unwrap_or_default();
    }
    out
}

/// One HTTP message of a frame (a `\x02` before it, as the remote gateway sends, is skipped).
pub fn parse(frame: &[u8]) -> Option<Message> {
    let frame = frame.strip_prefix(b"\x02").unwrap_or(frame);
    let split = frame.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&frame[..split]).ok()?;
    let mut lines = head.split("\r\n");
    let first = lines.next()?.trim().to_string();
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(n, v)| (n.trim().to_string(), v.trim().to_string())).collect();
    let raw = &frame[split + 4..];
    let mut m = Message { first, headers, body: vec![] };
    m.body = if m.header("Transfer-Encoding").is_some_and(|t| t.eq_ignore_ascii_case("chunked")) {
        unchunk(raw)
    } else {
        match m.header("Content-Length").and_then(|l| l.parse::<usize>().ok()) {
            Some(n) => raw[..n.min(raw.len())].to_vec(),
            None => raw.to_vec(),
        }
    };
    Some(m)
}

fn request(method: &str, path: &str, transaction: u32, body: Option<&str>) -> Vec<u8> {
    let body = body.unwrap_or("");
    format!("{method} {path} HTTP/1.1\r\nContent-Length: {}\r\nContent-Type: application/json; charset=UTF-8\r\nTransac-Id: {transaction}\r\n\r\n{body}", body.len()).into_bytes()
}

// ---------------------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Shutter,
    Light,
}

pub fn kind_of(usage: &str) -> Option<Kind> {
    match usage {
        "shutter" | "klineShutter" | "awning" => Some(Kind::Shutter),
        "light" => Some(Kind::Light),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    pub device: i64,
    pub endpoint: i64,
    pub name: String,
    pub usage: String,
}

impl Endpoint {
    fn reference(&self) -> String {
        format!("{}_{}", self.device, self.endpoint)
    }
}

pub fn endpoints_of(config: &Value) -> Vec<Endpoint> {
    config["endpoints"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let usage = e["last_usage"].as_str().filter(|u| !u.is_empty()).or_else(|| e["first_usage"].as_str()).unwrap_or_default().to_string();
            Some(Endpoint { device: e["id_device"].as_i64()?, endpoint: e["id_endpoint"].as_i64()?, name: e["name"].as_str().unwrap_or_default().trim().to_string(), usage })
        })
        .collect()
}

fn devices_of(endpoints: &[Endpoint]) -> Vec<DeviceInfo> {
    endpoints
        .iter()
        .filter_map(|e| {
            let name = if e.name.is_empty() { format!("Tydom {}", e.reference()) } else { e.name.clone() };
            match kind_of(&e.usage)? {
                Kind::Shutter => Some(device(e.reference(), "shutter", &["cover.position@1"], name)),
                Kind::Light => Some(device(e.reference(), "light", &["light@1"], name)),
            }
        })
        .collect()
}

/// The values of each endpoint, from `GET /devices/data` or a push: `{ "dev_ep": {name: value} }`.
fn absorb(values: &mut HashMap<String, Map<String, Value>>, data: &Value) {
    let devices: Vec<&Value> = match data {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![data],
        _ => vec![],
    };
    for d in devices {
        let Some(dev) = d["id"].as_i64() else { continue };
        for ep in d["endpoints"].as_array().into_iter().flatten() {
            let (Some(id), 0) = (ep["id"].as_i64(), ep["error"].as_i64().unwrap_or(0)) else { continue };
            let entry = values.entry(format!("{dev}_{id}")).or_default();
            for v in ep["data"].as_array().into_iter().flatten() {
                let (Some(name), validity) = (v["name"].as_str(), v["validity"].as_str().unwrap_or("upToDate")) else { continue };
                if validity == "upToDate" {
                    entry.insert(name.to_string(), v["value"].clone());
                }
            }
        }
    }
}

fn number(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

fn state_of(kind: Kind, v: &Map<String, Value>) -> Option<State> {
    let mut s = State::new();
    match kind {
        Kind::Shutter => {
            s.insert("position".into(), json!(number(v.get("position"))?.round().clamp(0.0, 100.0) as i64));
            s.insert("stoppable".into(), json!(true));
        }
        Kind::Light => {
            let level = number(v.get("level"))?.round().clamp(0.0, 100.0) as i64;
            s.insert("on".into(), json!(level > 0));
            if level > 0 {
                s.insert("brightness".into(), json!(level));
            }
        }
    }
    Some(s)
}

// ---------------------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------------------

fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}

/// "001A25123456" from "00:1a:25:12:34:56".
pub fn mac_of(s: &str) -> Option<String> {
    let m: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_uppercase();
    (m.len() == 12 && s.chars().all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '-' | ' '))).then_some(m)
}

pub struct Tydom {
    host: String,
    mac: String,
    password: String,
    link: Cell<Option<u32>>,
    transaction: Cell<u32>,
    endpoints: RefCell<Vec<Endpoint>>,
    values: RefCell<HashMap<String, Map<String, Value>>>,
    /// When the values were last read, and the gateway last asked to send everything.
    read_at: Cell<Option<i64>>,
    refreshed_at: Cell<Option<i64>>,
}

impl Tydom {
    fn path(&self) -> String {
        format!("/mediation/client?mac={}&appli=1", self.mac)
    }

    fn connect(&self) -> Result<u32> {
        if let Some(l) = self.link.get() {
            return Ok(l);
        }
        let path = self.path();
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let headers = [("Connection", "Upgrade"), ("Upgrade", "websocket"), ("Sec-WebSocket-Key", key), ("Sec-WebSocket-Version", "13"), ("Accept", "*/*")];
        let a = host::request_insecure("GET", &format!("https://{}:443{path}", self.host), &headers, None, TIMEOUT_MS)?;
        let header = a.headers.iter().find(|h| h.name.eq_ignore_ascii_case("www-authenticate")).map(|h| h.value.clone());
        let mut auth = vec![];
        let value;
        if let Some(h) = header {
            value = digest(&self.mac, &self.password, &path, &challenge(&h), &cnonce());
            auth.push(("Authorization", value.as_str()));
        }
        let link = host::subscribe_with(&format!("wss://{}:443{path}", self.host), &auth, true).map_err(|e| match e {
            Error::Unreachable(m) if m.contains("401") => Error::Invalid("la passerelle refuse l'adresse MAC ou le mot de passe".into()),
            e => e,
        })?;
        self.link.set(Some(link));
        Ok(link)
    }

    fn drop_link(&self) {
        if let Some(l) = self.link.take() {
            host::unsubscribe(l);
        }
    }

    /// Sends a request and waits for its answer; what the gateway pushes meanwhile is kept.
    fn ask(&self, method: &str, path: &str, body: Option<&str>) -> Result<Message> {
        let link = self.connect()?;
        let t = self.transaction.get().wrapping_add(1);
        self.transaction.set(t);
        if let Err(e) = host::send(link, &request(method, path, t, body)) {
            self.drop_link();
            return Err(e);
        }
        for _ in 0..32 {
            let frame = match host::receive(link, TIMEOUT_MS) {
                Ok(f) => f,
                Err(e) => {
                    self.drop_link();
                    return Err(unreachable(format!("{method} {path} : {e:?}")));
                }
            };
            let Some(m) = parse(&frame) else { continue };
            let ours = m.header("Transac-Id").is_none_or(|id| id == t.to_string()) && m.header("Uri-Origin").is_none_or(|o| o == path);
            match m.status() {
                Some(200..=299) if ours => return Ok(m),
                Some(s) if ours => return Err(unreachable(format!("{method} {path} : HTTP {s}"))),
                _ => {}
            }
            self.take(&m);
        }
        Err(unreachable(format!("{method} {path} : no answer")))
    }

    /// A push of the gateway: new values.
    fn take(&self, m: &Message) -> bool {
        let data_push = m.pushed().is_some_and(|p| p.starts_with("/devices/data")) || m.header("Uri-Origin").is_some_and(|o| o.starts_with("/devices/data") || o == "/refresh/all");
        match (data_push, m.json()) {
            (true, Some(v)) => {
                absorb(&mut self.values.borrow_mut(), &v);
                self.read_at.set(Some(host::now()));
                true
            }
            _ => false,
        }
    }

    fn load_endpoints(&self) -> Result<Vec<Endpoint>> {
        let config = self.ask("GET", "/configs/file", None)?.json().ok_or_else(|| unreachable("/configs/file : not JSON"))?;
        let e = endpoints_of(&config);
        *self.endpoints.borrow_mut() = e.clone();
        Ok(e)
    }

    fn read_data(&self) -> Result<()> {
        let now = host::now();
        if self.refreshed_at.get().is_none_or(|t| now - t >= REFRESH_EVERY) {
            self.ask("POST", "/refresh/all", None)?;
            self.refreshed_at.set(Some(now));
        }
        if self.read_at.get().is_none_or(|t| now - t >= DATA_EVERY) {
            let data = self.ask("GET", "/devices/data", None)?.json().ok_or_else(|| unreachable("/devices/data : not JSON"))?;
            absorb(&mut self.values.borrow_mut(), &data);
            self.read_at.set(Some(now));
        }
        Ok(())
    }

    fn endpoint(&self, reference: &str) -> Option<Endpoint> {
        self.endpoints.borrow().iter().find(|e| e.reference() == reference).cloned()
    }

    fn put(&self, e: &Endpoint, name: &str, value: &str) -> Result<()> {
        let body = json!([{"name": name, "value": value}]).to_string();
        self.ask("PUT", &format!("/devices/{}/endpoints/{}/data", e.device, e.endpoint), Some(&body)).map(|_| ())
    }
}

/// The password of the gateway from the linked Delta Dore account: the one it kept when it was
/// connected, else asked to Delta Dore now.
fn password_from_account(mac: &str) -> Result<String> {
    let account = host::account(ACCOUNT)
        .map_err(|_| Error::Invalid("le mot de passe de la passerelle est requis : saisis-le, ou connecte le compte Delta Dore (Réglages › Comptes et services)".into()))?;
    if let Some(p) = account["gateways"][mac].as_str() {
        return Ok(p.to_string());
    }
    cloud::gateway_password(&account, mac)
}

impl Driver for Tydom {
    fn open(config: Value, secret: Option<Value>) -> Result<Self> {
        let host = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !valid_host(host) {
            return Err(Error::Invalid("adresse de la passerelle invalide".into()));
        }
        let mac =
            mac_of(config["mac"].as_str().unwrap_or_default()).ok_or_else(|| Error::Invalid("l'adresse MAC a 12 caractères (001A25…), sur l'étiquette de la passerelle".into()))?;
        let typed = secret.and_then(|s| s["password"].as_str().map(str::to_string)).filter(|p| !p.is_empty());
        let password = match typed {
            Some(p) => p,
            None => password_from_account(&mac)?,
        };
        Ok(Tydom {
            host: host.to_string(),
            mac,
            password,
            link: Cell::new(None),
            transaction: Cell::new(0),
            endpoints: RefCell::new(vec![]),
            values: RefCell::new(HashMap::new()),
            read_at: Cell::new(None),
            refreshed_at: Cell::new(None),
        })
    }

    fn describe(&self) -> Result<Described> {
        let info = self.ask("GET", "/info", None)?.json().unwrap_or(Value::Null);
        let endpoints = self.load_endpoints()?;
        let model = info["productName"].as_str().map(|p| format!("Delta Dore {p}")).unwrap_or_else(|| "Delta Dore Tydom".into());
        let firmware = info["mainVersionSW"].as_str().map(str::to_string);
        Ok(Described { model, firmware, devices: devices_of(&endpoints), identity: Some(self.mac.clone()) })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        if self.endpoints.borrow().is_empty() {
            self.load_endpoints()?;
        }
        self.read_data()?;
        let values = self.values.borrow();
        Ok(self
            .endpoints
            .borrow()
            .iter()
            .filter_map(|e| {
                let kind = kind_of(&e.usage)?;
                Some((e.reference(), state_of(kind, values.get(&e.reference())?)?))
            })
            .collect())
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        if self.endpoints.borrow().is_empty() {
            self.load_endpoints()?;
        }
        let e = self.endpoint(reference).ok_or_else(|| Error::Invalid(format!("{reference} : inconnu de la passerelle")))?;
        match (kind_of(&e.usage), trait_name, action) {
            (Some(Kind::Shutter), "cover.position", "set") => {
                let p = params.get("position").and_then(Value::as_i64).ok_or_else(|| Error::Invalid("position requise".into()))?;
                self.put(&e, "position", &p.clamp(0, 100).to_string())
            }
            (Some(Kind::Shutter), "cover.position", "stop") => self.put(&e, "positionCmd", "STOP"),
            (Some(Kind::Light), "light", "set") => {
                if let Some(b) = params.get("brightness").and_then(Value::as_i64) {
                    return self.put(&e, "level", &b.clamp(0, 100).to_string());
                }
                match params.get("on").and_then(Value::as_bool) {
                    Some(on) => self.put(&e, "levelCmd", if on { "ON" } else { "OFF" }),
                    None => Err(Error::Invalid("on ou brightness requis".into())),
                }
            }
            _ => Err(Error::Unsupported(format!("{reference} ({}) : {trait_name}.{action}", e.usage))),
        }
    }

    fn on_message(&self, subscription: u32, data: &[u8]) -> Result<()> {
        if self.link.get() != Some(subscription) {
            return Ok(());
        }
        if parse(data).is_some_and(|m| self.take(&m)) {
            host::changed();
        }
        Ok(())
    }

    fn close(&self) {
        self.drop_link();
    }

    /// Every endpoint of the gateway, by kind and with the names of its values (not the names its
    /// owner gave), so that what is not supported yet shows.
    /// A Tydom found by the network scan: its port 443 asks the Digest of the gateways (realm
    /// « protected area ») on the path of their WebSocket. Its MAC is not told there: the server
    /// gives the one its network knows.
    fn identify(host_: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() || !valid_host(host_) {
            return None;
        }
        let headers = [("Connection", "Upgrade"), ("Upgrade", "websocket"), ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="), ("Sec-WebSocket-Version", "13")];
        let a = host::request_insecure("GET", &format!("https://{host_}:443/mediation/client?mac=001A25000000&appli=1"), &headers, None, 3000).ok()?;
        let header = a.headers.iter().find(|h| h.name.eq_ignore_ascii_case("www-authenticate"))?;
        (a.status == 401 && challenge(&header.value).get("realm").is_some_and(|r| r == REALM)).then(|| Found {
            model: "Delta Dore Tydom".into(),
            note: Some("mot de passe : laissez-le vide si le compte Delta Dore est connecté".into()),
            port: Some(443),
            ..Found::default()
        })
    }

    fn diagnostic(&self) -> Result<Value> {
        let info = self.ask("GET", "/info", None)?.json().unwrap_or(Value::Null);
        let endpoints = self.load_endpoints()?;
        let meta = self.ask("GET", "/devices/meta", None).ok().and_then(|m| m.json()).unwrap_or(Value::Null);
        let mut names: HashMap<String, Vec<Value>> = HashMap::new();
        for d in meta.as_array().into_iter().flatten() {
            let dev = d["id"].as_i64().unwrap_or_default();
            for ep in d["endpoints"].as_array().into_iter().flatten() {
                let fields: Vec<Value> = ep["metadata"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|m| json!({"name": m["name"], "type": m["type"], "permission": m["permission"], "min": m["min"], "max": m["max"], "enum_values": m["enum_values"]}))
                    .collect();
                names.insert(format!("{dev}_{}", ep["id"].as_i64().unwrap_or_default()), fields);
            }
        }
        let list: Vec<Value> = endpoints
            .iter()
            .map(|e| json!({"usage": e.usage, "supported": kind_of(&e.usage).is_some(), "values": names.get(&e.reference()).cloned().unwrap_or_default()}))
            .collect();
        Ok(json!({"product": info["productName"], "firmware": info["mainVersionSW"], "endpoints": list}))
    }
}

module!(Tydom);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    #[test]
    fn a_digest_as_rfc_2617_computes_it() {
        // The example of RFC 2617 § 3.5.
        let chal = challenge(r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#);
        assert_eq!(chal["realm"], "testrealm@host.com");
        assert_eq!(chal["qop"], "auth,auth-int");
        let h = digest("Mufasa", "Circle Of Life", "/dir/index.html", &chal, "0a4f113b");
        assert!(h.contains(r#"response="6629fae49393a05397450978507c4ef1""#), "{h}");
        assert!(h.contains(r#"opaque="5ccc069c403ebaf9f0171e9517f40e41""#));
        let tydom = challenge(r#"Digest realm="protected area", qop="auth", nonce="abc", opaque="xyz""#);
        assert_eq!((tydom["nonce"].as_str(), tydom["opaque"].as_str()), ("abc", "xyz"));
    }

    #[test]
    fn reads_the_messages_of_the_gateway() {
        let plain = parse(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 18\r\nTransac-Id: 3\r\n\r\n{\"productName\":1}x").unwrap();
        assert_eq!((plain.status(), plain.header("transac-id")), (Some(200), Some("3")));
        assert_eq!(plain.body, b"{\"productName\":1}x".to_vec());
        let chunked = parse(b"PUT /devices/data HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n[{\"id\":\r\n5\r\n1}]  \r\n0\r\n\r\n").unwrap();
        assert_eq!(chunked.pushed(), Some("/devices/data"));
        assert_eq!(chunked.json(), Some(json!([{"id": 1}])));
        assert_eq!(parse(b"\x02HTTP/1.1 401 Unauthorized\r\n\r\n").unwrap().status(), Some(401));
        assert_eq!(parse(b"garbage"), None);
    }

    fn config() -> Value {
        json!({"endpoints": [
            {"id_endpoint": 1, "id_device": 1700000001, "name": "Volet salon", "first_usage": "shutter", "last_usage": "shutter"},
            {"id_endpoint": 1, "id_device": 1700000002, "name": "Plafonnier", "first_usage": "light", "last_usage": "light"},
            {"id_endpoint": 1, "id_device": 1700000003, "name": "Centrale", "first_usage": "alarm", "last_usage": "alarm"}
        ]})
    }

    fn data() -> Value {
        json!([
            {"id": 1700000001, "endpoints": [{"id": 1, "error": 0, "data": [{"name": "position", "validity": "upToDate", "value": 40}]}]},
            {"id": 1700000002, "endpoints": [{"id": 1, "error": 0, "data": [{"name": "level", "validity": "upToDate", "value": 60}]}]},
            {"id": 1700000003, "endpoints": [{"id": 1, "error": 0, "data": [{"name": "alarmMode", "validity": "upToDate", "value": "OFF"}]}]}
        ])
    }

    #[test]
    fn shutters_and_lights_from_the_configuration() {
        let e = endpoints_of(&config());
        assert_eq!(e.len(), 3);
        let devices = devices_of(&e);
        assert_eq!(devices.len(), 2, "the alarm is only in the diagnostic");
        assert_eq!((devices[0].kind.as_str(), devices[0].reference.as_str()), ("shutter", "1700000001_1"));
        let mut values = HashMap::new();
        absorb(&mut values, &data());
        assert_eq!(state_of(Kind::Shutter, &values["1700000001_1"]).unwrap()["position"], 40);
        let light = state_of(Kind::Light, &values["1700000002_1"]).unwrap();
        assert_eq!((light["on"].clone(), light["brightness"].clone()), (json!(true), json!(60)));
        assert_eq!(mac_of("00:1a:25:12:34:56").as_deref(), Some("001A25123456"));
        assert_eq!(mac_of("001A2512345"), None);
    }

    /// A gateway on the bench: it answers each request with its Transac-Id.
    fn gateway() {
        testing::respond("GET", "/mediation/client", |_, _| breezone_module_sdk::host::Answer {
            status: 401,
            headers: vec![breezone_module_sdk::Header { name: "WWW-Authenticate".into(), value: r#"Digest realm="protected area", qop="auth", nonce="n1""#.into() }],
            body: vec![],
        });
        testing::answer_sent(|_, sent| {
            let text = String::from_utf8_lossy(sent).to_string();
            let t = text.lines().find_map(|l| l.strip_prefix("Transac-Id: ")).unwrap_or("0").trim().to_string();
            let path = text.split(' ').nth(1).unwrap_or_default().to_string();
            let body = match path.as_str() {
                "/info" => json!({"productName": "TYDOM", "mainVersionSW": "03.02.11"}),
                "/configs/file" => config(),
                "/devices/data" => data(),
                _ => json!({}),
            }
            .to_string();
            let mut out = vec![format!("HTTP/1.1 200 OK\r\nUri-Origin: {path}\r\nTransac-Id: {t}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()];
            if path == "/refresh/all" {
                // A push comes before the next answer.
                let push = json!([{"id": 1700000001, "endpoints": [{"id": 1, "error": 0, "data": [{"name": "position", "validity": "upToDate", "value": 0}]}]}]).to_string();
                out.push(format!("PUT /devices/data HTTP/1.1\r\nContent-Length: {}\r\n\r\n{push}", push.len()).into_bytes());
            }
            out
        });
    }

    #[test]
    fn through_the_host() {
        testing::reset();
        gateway();
        let t: Tydom = testing::open(json!({"host": "192.168.1.80", "mac": "00:1A:25:12:34:56"}), Some(json!({"password": "secret"})));
        let s = testing::check(&t, MANIFEST);
        assert_eq!(s["1700000002_1"]["brightness"], 60);
        assert!(testing::requests().iter().any(|r| r.starts_with("SUBSCRIBE wss://192.168.1.80:443/mediation/client?mac=001A25123456&appli=1 Authorization")));
        let sent: Vec<String> = testing::sent().iter().map(|(_, d)| String::from_utf8_lossy(d).to_string()).collect();
        assert!(sent.iter().any(|s| s.starts_with("POST /refresh/all")));
        let mut p = State::new();
        p.insert("position".into(), json!(75));
        t.act("1700000001_1", "cover.position", "set", &p).unwrap();
        assert!(testing::sent().last().is_some_and(|(_, d)| String::from_utf8_lossy(d).contains(r#"[{"name":"position","value":"75"}]"#)));
        t.act("1700000001_1", "cover.position", "stop", &State::new()).unwrap();
        assert!(t.act("1700000003_1", "light", "set", &p).is_err(), "the alarm is not driven");
    }

    #[test]
    fn a_tydom_is_found_by_its_digest() {
        testing::reset();
        testing::respond("GET", "https://192.168.1.80:443/mediation/client", |_, _| breezone_module_sdk::host::Answer {
            status: 401,
            headers: vec![breezone_module_sdk::Header { name: "WWW-Authenticate".into(), value: r#"Digest realm="protected area", qop="auth", nonce="abc", opaque="x""#.into() }],
            body: vec![],
        });
        let found = Tydom::identify("192.168.1.80", None).expect("a Tydom");
        assert_eq!((found.model.as_str(), found.port), ("Delta Dore Tydom", Some(443)));
        // Another HTTPS server (a NAS, a box) is not one.
        testing::respond("GET", "https://192.168.1.81:443/", |_, _| breezone_module_sdk::host::Answer { status: 404, headers: vec![], body: vec![] });
        assert!(Tydom::identify("192.168.1.81", None).is_none());
    }

    #[test]
    fn a_configuration_is_checked() {
        assert!(Tydom::open(json!({"host": "192.168.1.80", "mac": "001A25"}), Some(json!({"password": "x"}))).is_err());
        assert!(Tydom::open(json!({"host": "192.168.1.80", "mac": "001A25123456"}), None).is_err());
    }

    #[test]
    fn without_its_password_the_gateway_takes_the_one_of_the_account() {
        testing::reset();
        testing::account("deltadore", &json!({"email": "guillaume@example.com", "password": "pw", "gateways": {"001A25123456": "kept"}}));
        let t = Tydom::open(json!({"host": "192.168.1.80", "mac": "00:1A:25:12:34:56"}), Some(json!({"password": ""}))).unwrap();
        assert_eq!(t.password, "kept");
        assert!(testing::requests().is_empty(), "kept by the account: Delta Dore is not asked");
        let typed = Tydom::open(json!({"host": "192.168.1.80", "mac": "001A25123456"}), Some(json!({"password": "typed"}))).unwrap();
        assert_eq!(typed.password, "typed", "what is typed comes first");
    }
}
