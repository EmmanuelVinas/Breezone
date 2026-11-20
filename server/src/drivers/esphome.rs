//! ESPHome devices, through their native API (TCP 6053, the protocol of Home Assistant): a
//! connection kept open, the device pushing each change.
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
//! Its devices: each switch (`switch.relay@1`), light (`light@1`), cover (`cover.position@1`),
//! each sensor of power (`energy.power@1`), and the sensors of the air of the device together
//! (`environment.sensor@1`: temperature, humidity, pressure, CO₂, illuminance).

use super::{ConfigField, Described, DeviceInfo, Driver, DriverError, DriverFuture, DriverInfo, State};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex, Notify};

pub const PORT: u16 = 6053;

pub fn info() -> DriverInfo {
    DriverInfo {
        id: "esphome",
        label: "ESPHome",
        provides: vec!["switch.relay@1", "light@1", "cover.position@1", "energy.power@1", "environment.sensor@1"],
        models: vec!["ESPHome (API native)".into()],
        config: vec![
            ConfigField { name: "host", kind: "string", optional: false, label: crate::text!("Adresse IP ou nom", "IP address or name", "Dirección IP o nombre") },
            ConfigField {
                name: "key",
                kind: "password",
                optional: true,
                label: crate::text!(
                    "Clé de chiffrement de l'API (api: encryption: key)",
                    "API encryption key (api: encryption: key)",
                    "Clave de cifrado de la API (api: encryption: key)"
                ),
            },
            ConfigField {
                name: "password",
                kind: "password",
                optional: true,
                label: crate::text!("Mot de passe de l'API (ancien)", "API password (old)", "Contraseña de la API (antigua)"),
            },
        ],
    }
}

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

pub struct Frames<S> {
    stream: S,
    noise: Option<snow::TransportState>,
}

fn io(e: impl std::fmt::Display) -> DriverError {
    DriverError::Unreachable(format!("ESPHome: {e}"))
}

impl<S: AsyncRead + AsyncWrite + Unpin> Frames<S> {
    /// In clear.
    pub fn plain(stream: S) -> Self {
        Self { stream, noise: None }
    }

    /// Encrypted with the API key (base64, 32 bytes).
    pub async fn noise(mut stream: S, key: &str) -> Result<Self, DriverError> {
        let psk: [u8; 32] = STANDARD
            .decode(key.trim())
            .ok()
            .and_then(|k| k.try_into().ok())
            .ok_or_else(|| DriverError::Invalid("ESPHome : la clé de chiffrement n'est pas valable (32 octets en base64)".into()))?;
        let params: snow::params::NoiseParams = NOISE_PATTERN.parse().map_err(io)?;
        let mut hs = snow::Builder::new(params).prologue(NOISE_PROLOGUE).map_err(io)?.psk(0, &psk).map_err(io)?.build_initiator().map_err(io)?;
        let mut msg = vec![0u8; 128];
        let n = hs.write_message(&[], &mut msg).map_err(io)?;
        let mut hello = vec![0x01, 0x00, 0x00];
        let len = (n + 1) as u16;
        hello.extend_from_slice(&[0x01, (len >> 8) as u8, len as u8, 0x00]);
        hello.extend_from_slice(&msg[..n]);
        stream.write_all(&hello).await.map_err(io)?;
        // The server's hello: the protocol it chose, its name and MAC.
        let server_hello = read_noise_frame(&mut stream).await?;
        if server_hello.first() != Some(&0x01) {
            return Err(io("unknown protocol"));
        }
        let answer = read_noise_frame(&mut stream).await?;
        if answer.first() != Some(&0x00) {
            let why = String::from_utf8_lossy(answer.get(1..).unwrap_or_default()).into_owned();
            return Err(if why.contains("MAC failure") { DriverError::Invalid("ESPHome : la clé de chiffrement est refusée".into()) } else { io(format!("handshake: {why}")) });
        }
        let mut out = vec![0u8; 128];
        hs.read_message(&answer[1..], &mut out).map_err(|_| DriverError::Invalid("ESPHome : la clé de chiffrement est refusée".into()))?;
        Ok(Self { stream, noise: Some(hs.into_transport_mode().map_err(io)?) })
    }

    pub async fn send(&mut self, kind: u16, msg: &[u8]) -> Result<(), DriverError> {
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
        self.stream.write_all(&frame).await.map_err(io)
    }

    pub async fn recv(&mut self) -> Result<(u16, Vec<u8>), DriverError> {
        match &mut self.noise {
            None => {
                let first = self.stream.read_u8().await.map_err(io)?;
                if first != 0x00 {
                    return Err(DriverError::Invalid("ESPHome : l'appareil demande le chiffrement : donnez sa clé d'API".into()));
                }
                let len = read_varint(&mut self.stream).await?;
                let kind = read_varint(&mut self.stream).await?;
                let mut msg = vec![0u8; len as usize];
                self.stream.read_exact(&mut msg).await.map_err(io)?;
                Ok((kind as u16, msg))
            }
            Some(t) => {
                let cipher = read_noise_frame(&mut self.stream).await?;
                let mut plain = vec![0u8; cipher.len()];
                let n = t.read_message(&cipher, &mut plain).map_err(io)?;
                if n < 4 {
                    return Err(io("short frame"));
                }
                let kind = u16::from_be_bytes([plain[0], plain[1]]);
                let len = u16::from_be_bytes([plain[2], plain[3]]) as usize;
                Ok((kind, plain[4..(4 + len).min(n)].to_vec()))
            }
        }
    }
}

async fn read_varint<S: AsyncRead + Unpin>(s: &mut S) -> Result<u64, DriverError> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let b = s.read_u8().await.map_err(io)?;
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(io("bad varint"))
}

async fn read_noise_frame<S: AsyncRead + Unpin>(s: &mut S) -> Result<Vec<u8>, DriverError> {
    let mut head = [0u8; 3];
    s.read_exact(&mut head).await.map_err(io)?;
    if head[0] != 0x01 {
        return Err(DriverError::Invalid("ESPHome : l'appareil n'est pas chiffré : retirez la clé".into()));
    }
    let mut frame = vec![0u8; usize::from(u16::from_be_bytes([head[1], head[2]]))];
    s.read_exact(&mut frame).await.map_err(io)?;
    Ok(frame)
}

/// A node answering on `host`: its name, or `None` as name when it asks for its key.
pub async fn probe(host: &str) -> Option<Option<String>> {
    let stream = tokio::time::timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(format!("{host}:{PORT}"))).await.ok()?.ok()?;
    let mut frames = Frames::plain(stream);
    frames.send(1, &Msg::default().string(1, "Breezone").varint(2, 1).varint(3, 10).0).await.ok()?;
    match tokio::time::timeout(Duration::from_secs(2), frames.recv()).await.ok()? {
        Ok((2, msg)) => {
            let _ = frames.send(5, &[]).await;
            Some(Some(Fields(decode(&msg)).string(4)).filter(|n| !n.is_empty()))
        }
        Err(DriverError::Invalid(_)) => Some(None),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------------------

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
            Kind::Switch => out.push(DeviceInfo { reference: format!("switch/{}", e.object_id), kind: "switch.relay".into(), traits: vec!["switch.relay@1".into()], name }),
            Kind::Light { .. } => out.push(DeviceInfo { reference: format!("light/{}", e.object_id), kind: "light".into(), traits: vec!["light@1".into()], name }),
            Kind::Cover { .. } => out.push(DeviceInfo { reference: format!("cover/{}", e.object_id), kind: "shutter".into(), traits: vec!["cover.position@1".into()], name }),
            Kind::Sensor { class, unit } if is_power(class, unit) => {
                out.push(DeviceInfo { reference: format!("power/{}", e.object_id), kind: "energy.power".into(), traits: vec!["energy.power@1".into()], name })
            }
            Kind::Sensor { class, unit } if air_field(class, unit).is_some() => air = true,
            Kind::Sensor { .. } => {}
        }
    }
    if air {
        out.push(DeviceInfo { reference: "air".into(), kind: "sensor".into(), traits: vec!["environment.sensor@1".into()], name: node.to_string() });
    }
    out
}

fn kelvin(mireds: f32) -> i64 {
    (1_000_000.0 / mireds.max(1.0)).round() as i64
}

/// The state of each device, from the last state of each entity (`kind`, its fields).
pub fn states_of(entities: &[Entity], states: &HashMap<u32, (u16, Vec<(u32, Field)>)>) -> HashMap<String, State> {
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
// The driver: a connection kept open
// ---------------------------------------------------------------------------------------

#[derive(Default)]
struct Shared {
    node: String,
    model: String,
    version: Option<String>,
    mac: Option<String>,
    entities: Vec<Entity>,
    listed: bool,
    states: HashMap<u32, (u16, Vec<(u32, Field)>)>,
    connected: bool,
    error: Option<DriverError>,
}

pub struct EspHome {
    host: String,
    key: Option<String>,
    password: Option<String>,
    shared: Arc<Mutex<Shared>>,
    listed: Arc<Notify>,
    commands: Mutex<Option<mpsc::UnboundedSender<(u16, Vec<u8>)>>>,
    task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Drop for EspHome {
    fn drop(&mut self) {
        if let Ok(mut t) = self.task.lock() {
            if let Some(t) = t.take() {
                t.abort();
            }
        }
    }
}

impl EspHome {
    pub fn new(config: &Value, secret: Option<&str>) -> Result<Self, String> {
        let host = config["host"].as_str().map(str::trim).filter(|h| !h.is_empty()).ok_or("l'adresse est requise")?.to_string();
        let secret: Value = secret.and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Value::Null);
        let key = secret["key"].as_str().map(str::to_string).filter(|k| !k.trim().is_empty());
        if let Some(k) = &key {
            if STANDARD.decode(k.trim()).map(|b| b.len()) != Ok(32) {
                return Err("la clé de chiffrement n'est pas valable (32 octets en base64, comme dans api: encryption: key)".into());
            }
        }
        let password = secret["password"].as_str().map(str::to_string).filter(|p| !p.is_empty());
        Ok(Self {
            host,
            key,
            password,
            shared: Arc::new(Mutex::new(Shared::default())),
            listed: Arc::new(Notify::new()),
            commands: Mutex::new(None),
            task: std::sync::Mutex::new(None),
        })
    }

    fn address(&self) -> String {
        if self.host.contains(':') {
            self.host.clone()
        } else {
            format!("{}:{PORT}", self.host)
        }
    }

    /// Starts the connection (once): it reconnects by itself, more and more slowly.
    async fn start(&self) {
        let mut commands = self.commands.lock().await;
        if commands.is_some() {
            return;
        }
        let (tx, rx) = mpsc::unbounded_channel();
        *commands = Some(tx);
        let (address, key, password, shared, listed) = (self.address(), self.key.clone(), self.password.clone(), self.shared.clone(), self.listed.clone());
        let task = tokio::spawn(async move {
            let mut rx = rx;
            let mut wait = 2;
            loop {
                let result = session(&address, key.as_deref(), password.as_deref(), &shared, &listed, &mut rx).await;
                let mut s = shared.lock().await;
                s.connected = false;
                if let Err(e) = result {
                    tracing::debug!("ESPHome {address}: {e}");
                    s.error = Some(e);
                }
                drop(s);
                listed.notify_waiters();
                tokio::time::sleep(Duration::from_secs(wait)).await;
                wait = (wait * 2).min(60);
            }
        });
        if let Ok(mut t) = self.task.lock() {
            *t = Some(task);
        }
    }

    /// Waits for the entities of the node (its first connection).
    async fn listed(&self) -> Result<(), DriverError> {
        self.start().await;
        for _ in 0..2 {
            {
                let s = self.shared.lock().await;
                if s.listed {
                    return Ok(());
                }
                if let Some(e) = &s.error {
                    return Err(e.clone());
                }
            }
            let _ = tokio::time::timeout(Duration::from_secs(8), self.listed.notified()).await;
        }
        let s = self.shared.lock().await;
        if s.listed {
            Ok(())
        } else {
            Err(s.error.clone().unwrap_or_else(|| io("no answer")))
        }
    }

    async fn send(&self, kind: u16, msg: Msg) -> Result<(), DriverError> {
        let tx = self.commands.lock().await.clone().ok_or_else(|| io("not connected"))?;
        tx.send((kind, msg.0)).map_err(|_| io("not connected"))
    }
}

/// One connection: hello, the node, its entities, then its states until it ends.
async fn session(
    address: &str,
    key: Option<&str>,
    password: Option<&str>,
    shared: &Mutex<Shared>,
    listed: &Notify,
    commands: &mut mpsc::UnboundedReceiver<(u16, Vec<u8>)>,
) -> Result<(), DriverError> {
    let stream = tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect(address)).await.map_err(|_| io("no answer"))?.map_err(io)?;
    let mut frames = match key {
        Some(k) => tokio::time::timeout(Duration::from_secs(10), Frames::noise(stream, k)).await.map_err(|_| io("no handshake"))??,
        None => Frames::plain(stream),
    };
    frames.send(1, &Msg::default().string(1, "Breezone").varint(2, 1).varint(3, 10).0).await?;
    let (kind, _) = frames.recv().await?;
    if kind != 2 {
        return Err(io(format!("unexpected answer {kind}")));
    }
    if let Some(p) = password {
        frames.send(3, &Msg::default().string(1, p).0).await?;
        let (kind, msg) = frames.recv().await?;
        if kind == 4 && Fields(decode(&msg)).bool(1) {
            return Err(DriverError::Invalid("ESPHome : mot de passe de l'API refusé".into()));
        }
    }
    frames.send(9, &[]).await?;
    frames.send(11, &[]).await?;
    let mut entities = vec![];
    loop {
        let (kind, msg) = tokio::time::timeout(Duration::from_secs(15), frames.recv()).await.map_err(|_| io("no answer"))??;
        let f = Fields(decode(&msg));
        match kind {
            10 => {
                let mut s = shared.lock().await;
                let friendly = f.string(13);
                s.node = if friendly.is_empty() { f.string(2) } else { friendly };
                s.model = f.string(6);
                s.version = Some(f.string(4)).filter(|v| !v.is_empty());
                s.mac = Some(f.string(3)).filter(|m| !m.is_empty());
            }
            7 => frames.send(8, &[]).await?,
            19 => break,
            k => {
                if let Some(e) = entity_of(k, &f) {
                    entities.push(e);
                }
            }
        }
    }
    {
        let mut s = shared.lock().await;
        s.entities = entities;
        s.listed = true;
        s.connected = true;
        s.error = None;
    }
    listed.notify_waiters();
    frames.send(20, &[]).await?;
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            received = frames.recv() => {
                let (kind, msg) = received?;
                match kind {
                    7 => frames.send(8, &[]).await?,
                    5 => {
                        let _ = frames.send(6, &[]).await;
                        return Ok(());
                    }
                    22 | 24 | 25 | 26 => {
                        let fields = decode(&msg);
                        let key = Fields(fields.clone()).fixed32(1);
                        shared.lock().await.states.insert(key, (kind, fields));
                    }
                    _ => {}
                }
            }
            Some((kind, msg)) = commands.recv() => frames.send(kind, &msg).await?,
            _ = ping.tick() => frames.send(7, &[]).await?,
        }
    }
}

impl Driver for EspHome {
    fn describe(&self) -> DriverFuture<'_, Described> {
        Box::pin(async move {
            self.listed().await?;
            let s = self.shared.lock().await;
            let node = if s.node.is_empty() { self.host.clone() } else { s.node.clone() };
            Ok(Described {
                model: if s.model.is_empty() { "ESPHome".into() } else { format!("ESPHome · {}", s.model) },
                firmware: s.version.clone(),
                devices: devices_of(&node, &s.entities),
                identity: s.mac.clone(),
            })
        })
    }

    fn read(&self) -> DriverFuture<'_, HashMap<String, State>> {
        Box::pin(async move {
            self.start().await;
            let s = self.shared.lock().await;
            if !s.connected && s.states.is_empty() {
                return Err(s.error.clone().unwrap_or_else(|| io("connecting")));
            }
            if !s.connected {
                return Err(s.error.clone().unwrap_or_else(|| io("connection lost")));
            }
            Ok(states_of(&s.entities, &s.states))
        })
    }

    fn act<'a>(&'a self, reference: &'a str, trait_name: &'a str, action: &'a str, params: &'a State) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            let (kind, object_id) = reference.split_once('/').ok_or_else(|| DriverError::Unsupported(format!("{reference}: nothing to drive")))?;
            let entity = self.shared.lock().await.entities.iter().find(|e| e.object_id == object_id && reference.starts_with(kind)).cloned();
            let entity = entity.ok_or_else(|| DriverError::Unsupported(format!("{reference}: unknown")))?;
            match (&entity.kind, trait_name, action) {
                (Kind::Switch, "switch.relay", "set") => {
                    let on = params.get("on").and_then(Value::as_bool).ok_or_else(|| DriverError::Invalid("`on` is required".into()))?;
                    self.send(33, Msg::default().fixed32(1, entity.key).bool(2, on)).await
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
                    self.send(32, m).await
                }
                (Kind::Cover { .. }, "cover.position", "set") => {
                    let p = params.get("position").and_then(Value::as_f64).ok_or_else(|| DriverError::Invalid("`position` is required".into()))?;
                    self.send(30, Msg::default().fixed32(1, entity.key).bool(4, true).float(5, (p / 100.0).clamp(0.0, 1.0) as f32)).await
                }
                (Kind::Cover { .. }, "cover.position", "stop") => self.send(30, Msg::default().fixed32(1, entity.key).bool(8, true)).await,
                _ => Err(DriverError::Unsupported(format!("{reference}: {trait_name}.{action}"))),
            }
        })
    }

    /// The device pushes its states: they are only copied out this often.
    fn read_every(&self) -> Duration {
        Duration::from_secs(3)
    }

    fn close(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            let _ = self.send(5, Msg::default()).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Ok(mut t) = self.task.lock() {
                if let Some(t) = t.take() {
                    t.abort();
                }
            }
            Ok(())
        })
    }

    fn diagnostic(&self) -> DriverFuture<'_, Value> {
        Box::pin(async move {
            self.listed().await?;
            let s = self.shared.lock().await;
            let entities: Vec<Value> = s.entities.iter().map(|e| json!({"object_id": e.object_id, "kind": format!("{:?}", e.kind)})).collect();
            Ok(json!({ "model": s.model, "version": s.version, "entities": entities, "states": s.states.len() }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protobuf_goes_both_ways() {
        let m = Msg::default().fixed32(1, 0xdead_beef).bool(2, true).float(5, 0.5).string(3, "salon").varint(23, 300);
        let f = Fields(decode(&m.0));
        assert_eq!((f.fixed32(1), f.bool(2), f.float(5), f.string(3), f.uint(23)), (0xdead_beef, true, Some(0.5), "salon".to_string(), 300));
    }

    #[test]
    fn entities_become_devices_and_states() {
        let entity = |kind: u16, m: Msg| entity_of(kind, &Fields(decode(&m.0))).unwrap();
        let relay = entity(17, Msg::default().string(1, "relay").fixed32(2, 1).string(3, "Prise"));
        let light = entity(15, Msg::default().string(1, "lamp").fixed32(2, 2).string(3, "Lampe").varint(12, 3).varint(12, 47).float(9, 153.0).float(10, 370.0));
        let temp = entity(16, Msg::default().string(1, "t").fixed32(2, 3).string(3, "Température").string(6, "°C").string(9, "temperature"));
        let hum = entity(16, Msg::default().string(1, "h").fixed32(2, 4).string(3, "Humidité").string(6, "%").string(9, "humidity"));
        let power = entity(16, Msg::default().string(1, "p").fixed32(2, 5).string(3, "Puissance").string(6, "W").string(9, "power"));
        let cover = entity(13, Msg::default().string(1, "volet").fixed32(2, 6).string(3, "Volet").bool(6, true));
        let all = vec![relay, light, temp, hum, power, cover];
        let refs: Vec<String> = devices_of("Bureau", &all).into_iter().map(|d| d.reference).collect();
        assert_eq!(refs, ["switch/relay", "light/lamp", "power/p", "cover/volet", "air"]);
        let state = |kind: u16, m: Msg| (kind, decode(&m.0));
        let states = HashMap::from([
            (1, state(26, Msg::default().fixed32(1, 1).bool(2, true))),
            (2, state(24, Msg::default().fixed32(1, 2).bool(2, true).float(3, 0.6).float(8, 250.0).varint(11, 11))),
            (3, state(25, Msg::default().fixed32(1, 3).float(2, 21.37))),
            (4, state(25, Msg::default().fixed32(1, 4).float(2, 45.0))),
            (5, state(25, Msg::default().fixed32(1, 5).float(2, 1234.5))),
            (6, state(22, Msg::default().fixed32(1, 6).float(3, 0.3).varint(5, 2))),
        ]);
        let s = states_of(&all, &states);
        assert_eq!(s["switch/relay"]["on"], true);
        assert_eq!((s["light/lamp"]["brightness"].clone(), s["light/lamp"]["color_temp"].clone()), (json!(60), json!(4000)));
        assert_eq!((s["air"]["temperature"].clone(), s["air"]["humidity"].clone()), (json!(21.4), json!(45.0)));
        assert_eq!(s["power/p"]["power"], 1234.5);
        assert_eq!((s["cover/volet"]["position"].clone(), s["cover/volet"]["moving"].clone()), (json!(30), json!("closing")));
    }

    /// A fake node, in clear or encrypted, answering as a real one.
    async fn fake_node(listener: tokio::net::TcpListener, psk: Option<Vec<u8>>) {
        let (stream, _) = listener.accept().await.unwrap();
        let mut frames = match psk {
            None => Frames::plain(stream),
            Some(psk) => {
                let psk: [u8; 32] = psk.try_into().unwrap();
                let mut stream = stream;
                let mut hello = [0u8; 3];
                stream.read_exact(&mut hello).await.unwrap();
                let first = read_noise_frame(&mut stream).await.unwrap();
                let params: snow::params::NoiseParams = NOISE_PATTERN.parse().unwrap();
                let mut hs = snow::Builder::new(params).prologue(NOISE_PROLOGUE).unwrap().psk(0, &psk).unwrap().build_responder().unwrap();
                let mut buf = vec![0u8; 128];
                hs.read_message(&first[1..], &mut buf).unwrap();
                let server_hello = b"\x01bureau\x00AA:BB:CC:DD:EE:FF\x00";
                stream.write_all(&[0x01, 0, server_hello.len() as u8]).await.unwrap();
                stream.write_all(server_hello).await.unwrap();
                let n = hs.write_message(&[], &mut buf).unwrap();
                stream.write_all(&[0x01, 0, (n + 1) as u8, 0x00]).await.unwrap();
                stream.write_all(&buf[..n]).await.unwrap();
                Frames { stream, noise: Some(hs.into_transport_mode().unwrap()) }
            }
        };
        loop {
            let Ok((kind, msg)) = frames.recv().await else { return };
            match kind {
                1 => frames.send(2, &Msg::default().varint(1, 1).varint(2, 10).string(4, "bureau").0).await.unwrap(),
                9 => frames
                    .send(10, &Msg::default().string(2, "bureau").string(3, "AA:BB:CC:DD:EE:FF").string(4, "2025.11.0").string(6, "esp32dev").string(13, "Bureau").0)
                    .await
                    .unwrap(),
                11 => {
                    frames.send(17, &Msg::default().string(1, "relay").fixed32(2, 1).string(3, "Prise").0).await.unwrap();
                    frames.send(16, &Msg::default().string(1, "t").fixed32(2, 3).string(3, "Température").string(6, "°C").string(9, "temperature").0).await.unwrap();
                    frames.send(19, &[]).await.unwrap();
                }
                20 => {
                    frames.send(26, &Msg::default().fixed32(1, 1).bool(2, false).0).await.unwrap();
                    frames.send(25, &Msg::default().fixed32(1, 3).float(2, 19.5).0).await.unwrap();
                }
                33 => {
                    let f = Fields(decode(&msg));
                    frames.send(26, &Msg::default().fixed32(1, f.fixed32(1)).bool(2, f.bool(2)).0).await.unwrap();
                }
                _ => {}
            }
        }
    }

    async fn drive(psk: Option<Vec<u8>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(fake_node(listener, psk.clone()));
        let secret = psk.map(|k| json!({"key": STANDARD.encode(k)}).to_string());
        let node = EspHome::new(&json!({"host": format!("127.0.0.1:{port}")}), secret.as_deref()).unwrap();
        let d = node.describe().await.unwrap();
        assert_eq!((d.model.as_str(), d.firmware.as_deref(), d.identity.as_deref()), ("ESPHome · esp32dev", Some("2025.11.0"), Some("AA:BB:CC:DD:EE:FF")));
        let refs: Vec<&str> = d.devices.iter().map(|x| x.reference.as_str()).collect();
        assert_eq!(refs, ["switch/relay", "air"]);
        let mut s = HashMap::new();
        for _ in 0..50 {
            s = node.read().await.unwrap();
            if s.contains_key("air") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!((s["switch/relay"]["on"].clone(), s["air"]["temperature"].clone()), (json!(false), json!(19.5)));
        node.act("switch/relay", "switch.relay", "set", &Map::from_iter([("on".to_string(), json!(true))])).await.unwrap();
        for _ in 0..50 {
            s = node.read().await.unwrap();
            if s["switch/relay"]["on"] == true {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(s["switch/relay"]["on"], true, "the node said it switched on");
    }

    #[tokio::test]
    async fn a_node_in_clear() {
        drive(None).await;
    }

    #[tokio::test]
    async fn an_encrypted_node() {
        drive(Some((1..=32).collect())).await;
    }
}
