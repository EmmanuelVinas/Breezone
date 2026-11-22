//! ESPHome devices, through their native API (TCP 6053, the protocol of Home Assistant): a
//! connection kept open by Breezone (`host::subscribe`), the device pushing each change
//! (`on_message`).
//!
//! * Frames: in clear `0x00, varint(len), varint(type), message`; encrypted (the API key of the
//!   device, Noise_NNpsk0_25519_ChaChaPoly_SHA256, prologue "NoiseAPIInit\0\0")
//!   `0x01, u16(len), cipher(u16(type), u16(len), message)`.
//! * Messages (protobuf, `api.proto`): Hello (1/2), Authentication with the old API password
//!   (3/4), Disconnect (5/6), Ping (7/8), DeviceInfo (9/10), ListEntities (11, then one answer per
//!   entity: binary sensor 12, cover 13, light 15, sensor 16, switch 17, done 19), SubscribeStates
//!   (20, then states: cover 22, light 24, sensor 25, switch 26), commands: cover 30, light 32,
//!   switch 33.
//!
//! The handshake and the listing are questions and answers on the connection, read during the call
//! (`host::receive`); the states that come later reach `on_message`.
//!
//! Its devices: each switch (`switch.relay@1`), light (`light@1`), cover (`cover.position@1`),
//! each sensor of power (`energy.power@1`), and the sensors of the air of the device together
//! (`environment.sensor@1`: temperature, humidity, pressure, CO₂, illuminance).

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use breezone_module_sdk::{device, host, json, module, Described, DeviceInfo, Driver, Error, Found, Map, Result, State, Value};
use std::cell::RefCell;
use std::collections::HashMap;

pub const PORT: u16 = 6053;
/// How long a question waits for its answer.
const ANSWER_MS: u32 = 8_000;
/// A ping to the node this often; nothing heard for longer than `SILENT_S`: the connection is over.
const PING_S: i64 = 30;
const SILENT_S: i64 = 95;

// ---------------------------------------------------------------------------------------
// Protobuf, by hand: the few messages used
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Varint(u64),
    Fixed32(u32),
    Fixed64(u64),
    Bytes(Vec<u8>),
}

pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn get_varint(data: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *data.get(*i)?;
        *i += 1;
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// A message being written.
#[derive(Default)]
pub struct Msg(pub Vec<u8>);

impl Msg {
    pub fn varint(mut self, field: u32, v: u64) -> Self {
        put_varint(&mut self.0, u64::from(field) << 3);
        put_varint(&mut self.0, v);
        self
    }
    pub fn bool(self, field: u32, v: bool) -> Self {
        self.varint(field, u64::from(v))
    }
    pub fn fixed32(mut self, field: u32, v: u32) -> Self {
        put_varint(&mut self.0, (u64::from(field) << 3) | 5);
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn float(self, field: u32, v: f32) -> Self {
        self.fixed32(field, v.to_bits())
    }
    pub fn string(mut self, field: u32, v: &str) -> Self {
        put_varint(&mut self.0, (u64::from(field) << 3) | 2);
        put_varint(&mut self.0, v.len() as u64);
        self.0.extend_from_slice(v.as_bytes());
        self
    }
}

/// The fields of a message (a repeated one appears several times).
pub fn decode(data: &[u8]) -> Vec<(u32, Field)> {
    let mut out = vec![];
    let mut i = 0;
    while i < data.len() {
        let Some(tag) = get_varint(data, &mut i) else { break };
        let (field, wire) = ((tag >> 3) as u32, tag & 7);
        let value = match wire {
            0 => match get_varint(data, &mut i) {
                Some(v) => Field::Varint(v),
                None => break,
            },
            1 => {
                let Some(b) = data.get(i..i + 8) else { break };
                i += 8;
                Field::Fixed64(u64::from_le_bytes(b.try_into().unwrap_or_default()))
            }
            2 => {
                let Some(len) = get_varint(data, &mut i) else { break };
                let Some(b) = data.get(i..i + len as usize) else { break };
                i += len as usize;
                Field::Bytes(b.to_vec())
            }
            5 => {
                let Some(b) = data.get(i..i + 4) else { break };
                i += 4;
                Field::Fixed32(u32::from_le_bytes(b.try_into().unwrap_or_default()))
            }
            _ => break,
        };
        out.push((field, value));
    }
    out
}

/// Reading the fields of a decoded message.
pub struct Fields(pub Vec<(u32, Field)>);

impl Fields {
    fn get(&self, n: u32) -> Option<&Field> {
        self.0.iter().find(|(f, _)| *f == n).map(|(_, v)| v)
    }
    pub fn string(&self, n: u32) -> String {
        match self.get(n) {
            Some(Field::Bytes(b)) => String::from_utf8_lossy(b).into_owned(),
            _ => String::new(),
        }
    }
    pub fn uint(&self, n: u32) -> u64 {
        match self.get(n) {
            Some(Field::Varint(v)) => *v,
            _ => 0,
        }
    }
    pub fn bool(&self, n: u32) -> bool {
        self.uint(n) != 0
    }
    pub fn fixed32(&self, n: u32) -> u32 {
        match self.get(n) {
            Some(Field::Fixed32(v)) => *v,
            _ => 0,
        }
    }
    pub fn float(&self, n: u32) -> Option<f32> {
        match self.get(n) {
            Some(Field::Fixed32(v)) => Some(f32::from_bits(*v)),
            _ => None,
        }
    }
    /// A repeated varint, packed or not.
    pub fn uints(&self, n: u32) -> Vec<u64> {
        let mut out = vec![];
        for (f, v) in &self.0 {
            if *f != n {
                continue;
            }
            match v {
                Field::Varint(x) => out.push(*x),
                Field::Bytes(b) => {
                    let mut i = 0;
                    while i < b.len() {
                        match get_varint(b, &mut i) {
                            Some(x) => out.push(x),
                            None => break,
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------
// Frames: in clear, or Noise
// ---------------------------------------------------------------------------------------

const NOISE_PROLOGUE: &[u8] = b"NoiseAPIInit\x00\x00";
const NOISE_PATTERN: &str = "Noise_NNpsk0_25519_ChaChaPoly_SHA256";

fn io(e: impl std::fmt::Display) -> Error {
    Error::Unreachable(format!("ESPHome: {e}"))
}

/// A whole frame at the start of `buf`, taken out of it: its payload (the bytes after the header).
/// In clear: (type, message); encrypted: the cipher. `None`: not all of it is there yet.
pub fn take_frame(buf: &mut Vec<u8>) -> Result<Option<Frame>> {
    let Some(&first) = buf.first() else { return Ok(None) };
    match first {
        0x00 => {
            let mut i = 1;
            let Some(len) = get_varint(buf, &mut i) else { return Ok(None) };
            let Some(kind) = get_varint(buf, &mut i) else { return Ok(None) };
            let end = i + len as usize;
            if buf.len() < end {
                return Ok(None);
            }
            let msg = buf[i..end].to_vec();
            buf.drain(..end);
            Ok(Some(Frame::Plain(kind as u16, msg)))
        }
        0x01 => {
            if buf.len() < 3 {
                return Ok(None);
            }
            let end = 3 + usize::from(u16::from_be_bytes([buf[1], buf[2]]));
            if buf.len() < end {
                return Ok(None);
            }
            let payload = buf[3..end].to_vec();
            buf.drain(..end);
            Ok(Some(Frame::Noise(payload)))
        }
        _ => Err(io("not the native API")),
    }
}

#[derive(Debug, PartialEq)]
pub enum Frame {
    Plain(u16, Vec<u8>),
    Noise(Vec<u8>),
}

/// The connection to the node: its subscription, what came and is not read yet, its encryption.
struct Link {
    subscription: u32,
    buf: Vec<u8>,
    noise: Option<snow::TransportState>,
    last_heard: i64,
    last_ping: i64,
}

impl Link {
    fn send(&mut self, kind: u16, msg: &[u8]) -> Result<()> {
        let frame = match &mut self.noise {
            None => {
                let mut f = vec![0x00];
                put_varint(&mut f, msg.len() as u64);
                put_varint(&mut f, u64::from(kind));
                f.extend_from_slice(msg);
                f
            }
            Some(t) => {
                let mut plain = Vec::with_capacity(msg.len() + 4);
                plain.extend_from_slice(&kind.to_be_bytes());
                plain.extend_from_slice(&(msg.len() as u16).to_be_bytes());
                plain.extend_from_slice(msg);
                let mut cipher = vec![0u8; plain.len() + 16];
                let n = t.write_message(&plain, &mut cipher).map_err(io)?;
                let mut f = vec![0x01, (n >> 8) as u8, n as u8];
                f.extend_from_slice(&cipher[..n]);
                f
            }
        };
        host::send(self.subscription, &frame)
    }

    /// A message from a frame (decrypted when the link is encrypted).
    fn open(&mut self, frame: Frame) -> Result<(u16, Vec<u8>)> {
        match (frame, &mut self.noise) {
            (Frame::Plain(kind, msg), None) => Ok((kind, msg)),
            (Frame::Noise(cipher), Some(t)) => {
                let mut plain = vec![0u8; cipher.len()];
                let n = t.read_message(&cipher, &mut plain).map_err(io)?;
                if n < 4 {
                    return Err(io("short frame"));
                }
                let kind = u16::from_be_bytes([plain[0], plain[1]]);
                let len = usize::from(u16::from_be_bytes([plain[2], plain[3]]));
                Ok((kind, plain[4..(4 + len).min(n)].to_vec()))
            }
            (Frame::Noise(_), None) => Err(Error::Invalid("ESPHome : l'appareil demande le chiffrement : donnez sa clé d'API".into())),
            (Frame::Plain(..), Some(_)) => Err(Error::Invalid("ESPHome : l'appareil n'est pas chiffré : retirez la clé".into())),
        }
    }

    /// The next frame, waited for during the call.
    fn next_frame(&mut self) -> Result<Frame> {
        loop {
            if let Some(f) = take_frame(&mut self.buf)? {
                self.last_heard = host::now();
                return Ok(f);
            }
            let more = host::receive(self.subscription, ANSWER_MS).map_err(|_| io("no answer"))?;
            self.buf.extend_from_slice(&more);
        }
    }

    /// The next message, waited for during the call (a ping of the node answered on the way).
    fn next(&mut self) -> Result<(u16, Vec<u8>)> {
        loop {
            let frame = self.next_frame()?;
            let (kind, msg) = self.open(frame)?;
            if kind == 7 {
                self.send(8, &[])?;
                continue;
            }
            return Ok((kind, msg));
        }
    }
}

/// The Noise handshake, with the API key (base64, 32 bytes).
fn handshake(link: &mut Link, key: &str) -> Result<()> {
    let psk: [u8; 32] = STANDARD
        .decode(key.trim())
        .ok()
        .and_then(|k| k.try_into().ok())
        .ok_or_else(|| Error::Invalid("ESPHome : la clé de chiffrement n'est pas valable (32 octets en base64)".into()))?;
    let params: snow::params::NoiseParams = NOISE_PATTERN.parse().map_err(io)?;
    let mut hs = snow::Builder::new(params).prologue(NOISE_PROLOGUE).map_err(io)?.psk(0, &psk).map_err(io)?.build_initiator().map_err(io)?;
    let mut msg = vec![0u8; 128];
    let n = hs.write_message(&[], &mut msg).map_err(io)?;
    let mut hello = vec![0x01, 0x00, 0x00];
    let len = (n + 1) as u16;
    hello.extend_from_slice(&[0x01, (len >> 8) as u8, len as u8, 0x00]);
    hello.extend_from_slice(&msg[..n]);
    host::send(link.subscription, &hello)?;
    // The server's hello: the protocol it chose, its name and MAC; then its answer.
    let Frame::Noise(server_hello) = link.next_frame()? else { return Err(Error::Invalid("ESPHome : l'appareil n'est pas chiffré : retirez la clé".into())) };
    if server_hello.first() != Some(&0x01) {
        return Err(io("unknown protocol"));
    }
    let Frame::Noise(answer) = link.next_frame()? else { return Err(io("handshake")) };
    if answer.first() != Some(&0x00) {
        let why = String::from_utf8_lossy(answer.get(1..).unwrap_or_default()).into_owned();
        return Err(if why.contains("MAC failure") { Error::Invalid("ESPHome : la clé de chiffrement est refusée".into()) } else { io(format!("handshake: {why}")) });
    }
    let mut out = vec![0u8; 128];
    hs.read_message(&answer[1..], &mut out).map_err(|_| Error::Invalid("ESPHome : la clé de chiffrement est refusée".into()))?;
    link.noise = Some(hs.into_transport_mode().map_err(io)?);
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Switch,
    Light { dims: bool, rgb: bool, white: bool, mireds: (f32, f32) },
    Cover { position: bool },
    Sensor { class: String, unit: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub key: u32,
    pub object_id: String,
    pub name: String,
    pub kind: Kind,
}

/// An entity from its `ListEntities…Response`, when Breezone shows it.
pub fn entity_of(kind: u16, f: &Fields) -> Option<Entity> {
    let (key, object_id, name) = (f.fixed32(2), f.string(1), f.string(3));
    let kind = match kind {
        17 => Kind::Switch,
        15 => {
            let modes = f.uints(12);
            Kind::Light {
                dims: modes.iter().any(|m| *m >= 2),
                rgb: modes.iter().any(|m| [35, 39, 47, 51].contains(m)),
                white: modes.iter().any(|m| [11, 19, 47, 51].contains(m)),
                mireds: (f.float(9).unwrap_or(153.0), f.float(10).unwrap_or(500.0)),
            }
        }
        13 => Kind::Cover { position: f.bool(6) },
        16 => Kind::Sensor { class: f.string(9), unit: f.string(6) },
        _ => return None,
    };
    Some(Entity { key, object_id, name, kind })
}

/// The field of `environment.sensor@1` a sensor gives, from its class (or its unit).
fn air_field(class: &str, unit: &str) -> Option<&'static str> {
    match class {
        "temperature" => Some("temperature"),
        "humidity" => Some("humidity"),
        "pressure" | "atmospheric_pressure" => Some("pressure"),
        "carbon_dioxide" => Some("co2"),
        "illuminance" => Some("illuminance"),
        "battery" => Some("battery"),
        _ => match unit {
            "°C" => Some("temperature"),
            "lx" => Some("illuminance"),
            "hPa" => Some("pressure"),
            _ => None,
        },
    }
}

fn is_power(class: &str, unit: &str) -> bool {
    class == "power" || unit == "W" || unit == "kW"
}

/// The devices of a node, from its entities.
pub fn devices_of(node: &str, entities: &[Entity]) -> Vec<DeviceInfo> {
    let mut out = vec![];
    let mut air = false;
    for e in entities {
        let name = if e.name.is_empty() { node.to_string() } else { e.name.clone() };
        match &e.kind {
            Kind::Switch => out.push(device(format!("switch/{}", e.object_id), "switch.relay", &["switch.relay@1"], name)),
            Kind::Light { .. } => out.push(device(format!("light/{}", e.object_id), "light", &["light@1"], name)),
            Kind::Cover { .. } => out.push(device(format!("cover/{}", e.object_id), "shutter", &["cover.position@1"], name)),
            Kind::Sensor { class, unit } if is_power(class, unit) => out.push(device(format!("power/{}", e.object_id), "energy.power", &["energy.power@1"], name)),
            Kind::Sensor { class, unit } if air_field(class, unit).is_some() => air = true,
            Kind::Sensor { .. } => {}
        }
    }
    if air {
        out.push(device("air", "sensor", &["environment.sensor@1"], node));
    }
    out
}

fn kelvin(mireds: f32) -> i64 {
    (1_000_000.0 / mireds.max(1.0)).round() as i64
}

/// The state of each device, from the last state of each entity (`kind`, its fields).
/// The last state message of each entity, by key: its kind and its fields.
pub type States = HashMap<u32, (u16, Vec<(u32, Field)>)>;

pub fn states_of(entities: &[Entity], states: &States) -> HashMap<String, State> {
    let mut out: HashMap<String, State> = HashMap::new();
    let mut air = Map::new();
    for e in entities {
        let Some((_, fields)) = states.get(&e.key) else { continue };
        let f = Fields(fields.clone());
        match &e.kind {
            Kind::Switch => {
                out.insert(format!("switch/{}", e.object_id), Map::from_iter([("on".to_string(), json!(f.bool(2)))]));
            }
            Kind::Light { dims, rgb, white, mireds } => {
                let mut s = Map::new();
                s.insert("on".into(), json!(f.bool(2)));
                if *dims {
                    s.insert("brightness".into(), json!((f.float(3).unwrap_or(1.0) * 100.0).round().max(1.0) as i64));
                }
                if *white {
                    s.insert("color_temp_min".into(), json!(kelvin(mireds.1)));
                    s.insert("color_temp_max".into(), json!(kelvin(mireds.0)));
                    if let Some(m) = f.float(8).filter(|m| *m > 0.0) {
                        s.insert("color_temp".into(), json!(kelvin(m)));
                    }
                }
                if *rgb {
                    let c = |n| (f.float(n).unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                    s.insert("color".into(), json!(format!("#{:02x}{:02x}{:02x}", c(4), c(5), c(6))));
                    let mode = f.uint(11);
                    s.insert("mode".into(), json!(if [35, 39].contains(&mode) { "color" } else { "white" }));
                }
                s.insert("reachable".into(), json!(true));
                out.insert(format!("light/{}", e.object_id), s);
            }
            Kind::Cover { .. } => {
                let p = (f.float(3).unwrap_or(0.0) * 100.0).round() as i64;
                let moving = match f.uint(5) {
                    1 => "opening",
                    2 => "closing",
                    _ => "stopped",
                };
                out.insert(
                    format!("cover/{}", e.object_id),
                    Map::from_iter([("position".to_string(), json!(p)), ("moving".to_string(), json!(moving)), ("stoppable".to_string(), json!(true))]),
                );
            }
            Kind::Sensor { class, unit } => {
                if f.bool(3) {
                    continue;
                }
                let Some(v) = f.float(2).filter(|v| v.is_finite()) else { continue };
                let v = f64::from(v);
                if is_power(class, unit) {
                    let w = if unit == "kW" { v * 1000.0 } else { v };
                    out.insert(format!("power/{}", e.object_id), Map::from_iter([("power".to_string(), json!((w * 10.0).round() / 10.0))]));
                } else if let Some(field) = air_field(class, unit) {
                    air.entry(field).or_insert_with(|| json!((v * 10.0).round() / 10.0));
                }
            }
        }
    }
    if !air.is_empty() {
        out.insert("air".into(), air);
    }
    out
}

// ---------------------------------------------------------------------------------------
// The driver: a connection kept open by Breezone
// ---------------------------------------------------------------------------------------

#[derive(Default)]
struct Node {
    name: String,
    model: String,
    version: Option<String>,
    mac: Option<String>,
    entities: Vec<Entity>,
    states: States,
}

pub struct EspHome {
    host: String,
    key: Option<String>,
    password: Option<String>,
    link: RefCell<Option<Link>>,
    node: RefCell<Node>,
}

impl EspHome {
    fn address(&self) -> String {
        if self.host.contains(':') {
            format!("tcp://{}", self.host)
        } else {
            format!("tcp://{}:{PORT}", self.host)
        }
    }

    /// Closes the link: the next call connects again.
    fn drop_link(&self) {
        if let Some(l) = self.link.borrow_mut().take() {
            host::unsubscribe(l.subscription);
        }
    }

    /// Connects (once, or again when the link is over): hello, the node, its entities, then its
    /// states, which come to `on_message`.
    fn connect(&self) -> Result<()> {
        let now = host::now();
        let over = self.link.borrow().as_ref().is_some_and(|l| now - l.last_heard > SILENT_S);
        if over {
            self.drop_link();
        }
        if self.link.borrow().is_some() {
            return Ok(());
        }
        let subscription = host::subscribe(&self.address())?;
        let mut link = Link { subscription, buf: vec![], noise: None, last_heard: now, last_ping: now };
        let result = self.session(&mut link);
        match result {
            Ok(()) => {
                *self.link.borrow_mut() = Some(link);
                Ok(())
            }
            Err(e) => {
                host::unsubscribe(subscription);
                Err(e)
            }
        }
    }

    fn session(&self, link: &mut Link) -> Result<()> {
        if let Some(k) = &self.key {
            handshake(link, k)?;
        }
        link.send(1, &Msg::default().string(1, "Breezone").varint(2, 1).varint(3, 10).0)?;
        let (kind, _) = link.next()?;
        if kind != 2 {
            return Err(io(format!("unexpected answer {kind}")));
        }
        if let Some(p) = &self.password {
            link.send(3, &Msg::default().string(1, p).0)?;
            let (kind, msg) = link.next()?;
            if kind == 4 && Fields(decode(&msg)).bool(1) {
                return Err(Error::Invalid("ESPHome : mot de passe de l'API refusé".into()));
            }
        }
        link.send(9, &[])?;
        link.send(11, &[])?;
        let mut entities = vec![];
        let mut node = Node::default();
        loop {
            let (kind, msg) = link.next()?;
            let f = Fields(decode(&msg));
            match kind {
                10 => {
                    let friendly = f.string(13);
                    node.name = if friendly.is_empty() { f.string(2) } else { friendly };
                    node.model = f.string(6);
                    node.version = Some(f.string(4)).filter(|v| !v.is_empty());
                    node.mac = Some(f.string(3)).filter(|m| !m.is_empty());
                }
                19 => break,
                k => {
                    if let Some(e) = entity_of(k, &f) {
                        entities.push(e);
                    }
                }
            }
        }
        node.entities = entities;
        *self.node.borrow_mut() = node;
        link.send(20, &[])?;
        Ok(())
    }

    /// Handles what came: states kept, pings answered, a goodbye closes the link.
    fn absorb(&self) -> Result<bool> {
        let mut changed = false;
        let mut over = false;
        {
            let mut guard = self.link.borrow_mut();
            let Some(link) = guard.as_mut() else { return Ok(false) };
            while let Some(frame) = take_frame(&mut link.buf)? {
                link.last_heard = host::now();
                let (kind, msg) = link.open(frame)?;
                match kind {
                    7 => link.send(8, &[])?,
                    5 => {
                        let _ = link.send(6, &[]);
                        over = true;
                        break;
                    }
                    22 | 24 | 25 | 26 => {
                        let fields = decode(&msg);
                        let key = Fields(fields.clone()).fixed32(1);
                        self.node.borrow_mut().states.insert(key, (kind, fields));
                        changed = true;
                    }
                    _ => {}
                }
            }
        }
        if over {
            self.drop_link();
        }
        Ok(changed)
    }

    fn command(&self, kind: u16, msg: Msg) -> Result<()> {
        self.connect()?;
        let result = self.link.borrow_mut().as_mut().ok_or_else(|| io("not connected"))?.send(kind, &msg.0);
        if result.is_err() {
            self.drop_link();
        }
        result
    }
}

impl Driver for EspHome {
    fn open(config: Value, secret: Option<Value>) -> Result<Self> {
        let host_ = config["host"].as_str().map(str::trim).filter(|h| !h.is_empty()).ok_or_else(|| Error::Invalid("l'adresse est requise".into()))?.to_string();
        let secret = secret.unwrap_or(Value::Null);
        let key = secret["key"].as_str().map(str::to_string).filter(|k| !k.trim().is_empty());
        if let Some(k) = &key {
            if STANDARD.decode(k.trim()).map(|b| b.len()) != Ok(32) {
                return Err(Error::Invalid("la clé de chiffrement n'est pas valable (32 octets en base64, comme dans api: encryption: key)".into()));
            }
        }
        let password = secret["password"].as_str().map(str::to_string).filter(|p| !p.is_empty());
        Ok(Self { host: host_, key, password, link: RefCell::new(None), node: RefCell::new(Node::default()) })
    }

    fn describe(&self) -> Result<Described> {
        self.connect()?;
        let node = self.node.borrow();
        let name = if node.name.is_empty() { self.host.clone() } else { node.name.clone() };
        Ok(Described {
            model: if node.model.is_empty() { "ESPHome".into() } else { format!("ESPHome · {}", node.model) },
            firmware: node.version.clone(),
            devices: devices_of(&name, &node.entities),
            identity: node.mac.clone(),
        })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        self.connect()?;
        self.absorb()?;
        let now = host::now();
        let ping = self.link.borrow().as_ref().is_some_and(|l| now - l.last_ping >= PING_S);
        if ping {
            let mut guard = self.link.borrow_mut();
            if let Some(l) = guard.as_mut() {
                l.last_ping = now;
                if l.send(7, &[]).is_err() {
                    drop(guard);
                    self.drop_link();
                }
            }
        }
        let node = self.node.borrow();
        Ok(states_of(&node.entities, &node.states).into_iter().collect())
    }

    fn on_message(&self, subscription: u32, data: &[u8]) -> Result<()> {
        let ours = self.link.borrow().as_ref().is_some_and(|l| l.subscription == subscription);
        if !ours {
            return Ok(());
        }
        if let Some(l) = self.link.borrow_mut().as_mut() {
            l.buf.extend_from_slice(data);
        }
        match self.absorb() {
            Ok(true) => host::changed(),
            Ok(false) => {}
            Err(e) => {
                host::log("debug", &format!("{e:?}"));
                self.drop_link();
            }
        }
        Ok(())
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        self.connect()?;
        let (kind, object_id) = reference.split_once('/').ok_or_else(|| Error::Unsupported(format!("{reference}: nothing to drive")))?;
        let entity = self.node.borrow().entities.iter().find(|e| e.object_id == object_id && reference.starts_with(kind)).cloned();
        let entity = entity.ok_or_else(|| Error::Unsupported(format!("{reference}: unknown")))?;
        match (&entity.kind, trait_name, action) {
            (Kind::Switch, "switch.relay", "set") => {
                let on = params.get("on").and_then(Value::as_bool).ok_or_else(|| Error::Invalid("`on` is required".into()))?;
                self.command(33, Msg::default().fixed32(1, entity.key).bool(2, on))
            }
            (Kind::Light { .. }, "light", "set") => {
                let mut m = Msg::default().fixed32(1, entity.key);
                if let Some(on) = params.get("on").and_then(Value::as_bool) {
                    m = m.bool(2, true).bool(3, on);
                }
                if let Some(b) = params.get("brightness").and_then(Value::as_f64) {
                    m = m.bool(2, true).bool(3, true).bool(4, true).float(5, (b / 100.0).clamp(0.01, 1.0) as f32);
                }
                if let Some(k) = params.get("color_temp").and_then(Value::as_f64) {
                    m = m.bool(2, true).bool(3, true).bool(22, true).varint(23, 11).bool(12, true).float(13, (1_000_000.0 / k.max(1.0)) as f32);
                }
                if let Some(hex) = params.get("color").and_then(Value::as_str) {
                    let h = hex.trim_start_matches('#');
                    if h.len() == 6 {
                        let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map(|v| f32::from(v) / 255.0).unwrap_or(1.0);
                        m = m.bool(2, true).bool(3, true).bool(22, true).varint(23, 35).bool(6, true).float(7, c(0)).float(8, c(2)).float(9, c(4));
                    }
                }
                self.command(32, m)
            }
            (Kind::Cover { .. }, "cover.position", "set") => {
                let p = params.get("position").and_then(Value::as_f64).ok_or_else(|| Error::Invalid("`position` is required".into()))?;
                self.command(30, Msg::default().fixed32(1, entity.key).bool(4, true).float(5, (p / 100.0).clamp(0.0, 1.0) as f32))
            }
            (Kind::Cover { .. }, "cover.position", "stop") => self.command(30, Msg::default().fixed32(1, entity.key).bool(8, true)),
            _ => Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}"))),
        }
    }

    fn close(&self) {
        if let Some(l) = self.link.borrow_mut().as_mut() {
            let _ = l.send(5, &[]);
        }
        self.drop_link();
    }

    fn diagnostic(&self) -> Result<Value> {
        self.connect()?;
        let node = self.node.borrow();
        let entities: Vec<Value> = node.entities.iter().map(|e| json!({"object_id": e.object_id, "kind": format!("{:?}", e.kind)})).collect();
        Ok(json!({ "model": node.model, "version": node.version, "entities": entities, "states": node.states.len() }))
    }

    /// A node answering on the native API: its name, or that it asks for its key.
    fn probe(host_: &str) -> Option<String> {
        hello(host_).map(|name| name.unwrap_or_else(|| "ESPHome".into()))
    }

    /// From its mDNS announce (name, MAC, version, whether its API is encrypted), else by saying
    /// hello on its native API.
    fn identify(host_: &str, announced: Option<&Value>) -> Option<Found> {
        let (name, encrypted, mac, firmware) = match announced {
            Some(a) => {
                let txt = |k: &str| a["txt"][k].as_str().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
                let name = txt("friendly_name").or_else(|| a["name"].as_str().map(str::to_string));
                (name, txt("api_encryption").is_some(), txt("mac"), txt("version"))
            }
            None => match hello(host_)? {
                Some(name) => (Some(name), false, None, None),
                None => (None, true, None, None),
            },
        };
        let note = if encrypted { "chiffré : clé de l'API requise".to_string() } else { format!("« {} »", name.as_deref().unwrap_or("ESPHome")) };
        Some(Found { model: "ESPHome".into(), note: Some(note), mac, firmware, port: Some(PORT), ..Found::default() })
    }
}

/// Says hello on the native API: `Some(Some(name))` in clear, `Some(None)` when the node asks for
/// its key, `None` when nothing answers as ESPHome.
fn hello(host_: &str) -> Option<Option<String>> {
    let address = if host_.contains(':') { format!("tcp://{host_}") } else { format!("tcp://{host_}:{PORT}") };
    let subscription = host::subscribe(&address).ok()?;
    let mut link = Link { subscription, buf: vec![], noise: None, last_heard: 0, last_ping: 0 };
    let answer = link.send(1, &Msg::default().string(1, "Breezone").varint(2, 1).varint(3, 10).0).and_then(|_| link.next_frame());
    let name = match answer {
        Ok(Frame::Plain(2, msg)) => {
            let _ = link.send(5, &[]);
            let name = Fields(decode(&msg)).string(4);
            Some(Some(if name.is_empty() { "ESPHome".into() } else { name }))
        }
        Ok(Frame::Noise(_)) => Some(None),
        _ => None,
    };
    host::unsubscribe(subscription);
    name
}

module!(EspHome);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_as_it_announces_itself() {
        let found = EspHome::identify(
            "192.168.1.62",
            Some(&json!({"service": "_esphomelib._tcp", "name": "bureau", "port": 6053, "txt": {"friendly_name": "Bureau", "mac": "aabbccddeeff", "version": "2025.11.0"}})),
        )
        .unwrap();
        assert_eq!((found.note.as_deref(), found.mac.as_deref(), found.firmware.as_deref()), (Some("« Bureau »"), Some("aabbccddeeff"), Some("2025.11.0")));
        let locked = EspHome::identify("192.168.1.63", Some(&json!({"name": "garage", "txt": {"api_encryption": "Noise_NNpsk0_25519_ChaChaPoly_SHA256"}}))).unwrap();
        assert_eq!(locked.note.as_deref(), Some("chiffré : clé de l'API requise"));
    }
    use breezone_module_sdk::testing;

    const MANIFEST: &str = include_str!("../manifest.json");

    #[test]
    fn protobuf_and_frames_go_both_ways() {
        let m = Msg::default().fixed32(1, 0xdead_beef).bool(2, true).float(5, 0.5).string(3, "salon").varint(23, 300);
        let f = Fields(decode(&m.0));
        assert_eq!((f.fixed32(1), f.bool(2), f.float(5), f.string(3), f.uint(23)), (0xdead_beef, true, Some(0.5), "salon".to_string(), 300));
        let mut buf = vec![0x00, 3, 2, 1, 2];
        assert_eq!(take_frame(&mut buf).unwrap(), None, "not all there yet");
        buf.push(3);
        buf.extend_from_slice(&[0x01, 0, 2]);
        assert_eq!(take_frame(&mut buf).unwrap(), Some(Frame::Plain(2, vec![1, 2, 3])));
        assert_eq!(take_frame(&mut buf).unwrap(), None);
        buf.extend_from_slice(&[9, 9]);
        assert_eq!(take_frame(&mut buf).unwrap(), Some(Frame::Noise(vec![9, 9])));
    }

    fn plain(kind: u16, m: Msg) -> Vec<u8> {
        let mut f = vec![0x00];
        put_varint(&mut f, m.0.len() as u64);
        put_varint(&mut f, u64::from(kind));
        f.extend_from_slice(&m.0);
        f
    }

    /// A node in clear, answering each question as a real one, in pieces.
    fn fake_node() {
        testing::answer_sent(|_, data| {
            let mut buf = data.to_vec();
            let mut out = vec![];
            while let Ok(Some(Frame::Plain(kind, msg))) = take_frame(&mut buf) {
                match kind {
                    1 => out.push(plain(2, Msg::default().varint(1, 1).varint(2, 10).string(4, "bureau"))),
                    9 => out.push(plain(10, Msg::default().string(2, "bureau").string(3, "AA:BB:CC:DD:EE:FF").string(4, "2025.11.0").string(6, "esp32dev").string(13, "Bureau"))),
                    11 => {
                        // In two pieces, as TCP may give them.
                        let mut all = plain(17, Msg::default().string(1, "relay").fixed32(2, 1).string(3, "Prise"));
                        all.extend(plain(16, Msg::default().string(1, "t").fixed32(2, 3).string(3, "Température").string(6, "°C").string(9, "temperature")));
                        all.extend(plain(19, Msg::default()));
                        let half = all.len() / 2;
                        out.push(all[..half].to_vec());
                        out.push(all[half..].to_vec());
                    }
                    33 => {
                        let f = Fields(decode(&msg));
                        out.push(plain(26, Msg::default().fixed32(1, f.fixed32(1)).bool(2, f.bool(2))));
                    }
                    _ => {}
                }
            }
            out
        });
    }

    #[test]
    fn a_node_is_listed_during_the_call_and_its_states_come_after() {
        testing::reset();
        fake_node();
        let node: EspHome = testing::open(json!({"host": "192.168.1.60"}), None);
        let d = node.describe().unwrap();
        assert_eq!((d.model.as_str(), d.firmware.as_deref(), d.identity.as_deref()), ("ESPHome · esp32dev", Some("2025.11.0"), Some("AA:BB:CC:DD:EE:FF")));
        assert_eq!(d.devices.iter().map(|x| x.reference.as_str()).collect::<Vec<_>>(), ["switch/relay", "air"]);
        // The states the node pushes after the subscription reach `on_message`.
        node.on_message(1, &plain(26, Msg::default().fixed32(1, 1).bool(2, false))).unwrap();
        node.on_message(1, &plain(25, Msg::default().fixed32(1, 3).float(2, 19.5))).unwrap();
        assert_eq!(testing::changed_count(), 2);
        let state = testing::check(&node, MANIFEST);
        assert_eq!((state["switch/relay"]["on"].clone(), state["air"]["temperature"].clone()), (json!(false), json!(19.5)));
        node.act("switch/relay", "switch.relay", "set", &Map::from_iter([("on".to_string(), json!(true))])).unwrap();
        let answer = breezone_module_sdk::host::receive(1, 100).unwrap();
        node.on_message(1, &answer).unwrap();
        assert_eq!(node.read().unwrap().into_iter().find(|(r, _)| r == "switch/relay").unwrap().1["on"], true);
    }

    #[test]
    fn an_encrypted_node_needs_its_key() {
        testing::reset();
        // A node that asks for encryption answers a clear hello with a Noise frame.
        testing::answer_sent(|_, _| vec![vec![0x01, 0x00, 0x01, 0x00]]);
        let node: EspHome = testing::open(json!({"host": "192.168.1.61"}), None);
        assert!(matches!(node.describe(), Err(Error::Invalid(m)) if m.contains("clé")));
        assert!(EspHome::open(json!({"host": "x"}), Some(json!({"key": "court"}))).is_err());
    }
}
