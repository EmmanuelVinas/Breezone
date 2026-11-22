//! A HomeKit controller (HAP over IP): Breezone pairs with an accessory or bridge (a Legrand
//! gateway, a VELUX gateway…) with its setup code, then reads and drives its window coverings
//! (`cover.position@1`), switches, outlets and lights (`switch.relay@1`) and hears its remotes
//! (`button@1`), on the local network, through encrypted sessions. No Apple device or account is
//! involved.
//!
//! The source keeps the accessory's address (`host`, "ip:port") and pairing id (`device_id`); the
//! pairing itself (Breezone's key) is its secret, stored encrypted. Pairing with Breezone changes
//! nothing else on the accessory, and the only pairing Breezone removes is its own (`forget`).

mod hap;
mod srp;
mod tlv;

use breezone_module_sdk::{device, host, json, module, Described, DeviceInfo, Driver, Error, Paired, PairedSource, Result, State, Value};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use hap::{hkdf, label, open, random, seal, Conn};
use std::cell::RefCell;
use std::collections::HashMap;

/// What an accessory and Breezone keep of their pairing: the accessory's identity and long-term
/// public key, Breezone's identity and long-term secret key. The same fields as the driver
/// compiled in, so that its sources keep working with the module.
#[derive(Debug, Clone, PartialEq)]
pub struct Pairing {
    pub accessory_id: String,
    /// Hex.
    pub accessory_ltpk: String,
    pub controller_id: String,
    /// Hex.
    pub controller_ltsk: String,
}

impl Pairing {
    fn from_value(v: &Value) -> Option<Self> {
        let field = |name: &str| v[name].as_str().map(str::to_string);
        Some(Self {
            accessory_id: field("accessory_id")?,
            accessory_ltpk: field("accessory_ltpk")?,
            controller_id: field("controller_id")?,
            controller_ltsk: field("controller_ltsk")?,
        })
    }

    fn to_value(&self) -> Value {
        json!({"accessory_id": self.accessory_id, "accessory_ltpk": self.accessory_ltpk, "controller_id": self.controller_id, "controller_ltsk": self.controller_ltsk})
    }

    pub fn accessory_key(&self) -> std::result::Result<VerifyingKey, HapError> {
        let bytes: [u8; 32] = hex::decode(&self.accessory_ltpk).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| HapError::Protocol("bad accessory key".into()))?;
        VerifyingKey::from_bytes(&bytes).map_err(|_| HapError::Protocol("bad accessory key".into()))
    }

    pub fn controller_key(&self) -> std::result::Result<SigningKey, HapError> {
        let bytes: [u8; 32] = hex::decode(&self.controller_ltsk).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| HapError::Protocol("bad controller key".into()))?;
        Ok(SigningKey::from_bytes(&bytes))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HapError {
    Unreachable(String),
    Protocol(String),
    /// An error code of the accessory (HAP, table 5-5).
    Accessory(u8),
    /// The accessory no longer knows Breezone's pairing (removed, or the accessory was reset): it
    /// refuses Breezone's key, or closes the connection when it is shown.
    Forgotten,
}

impl std::fmt::Display for HapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HapError::Unreachable(e) => write!(f, "unreachable: {e}"),
            HapError::Protocol(e) => write!(f, "{e}"),
            HapError::Accessory(code) => write!(f, "the accessory refuses (error {code})"),
            HapError::Forgotten => write!(f, "the accessory no longer knows Breezone's pairing: remove this module, then pair it again with its code"),
        }
    }
}

impl From<HapError> for Error {
    fn from(e: HapError) -> Self {
        match e {
            HapError::Unreachable(m) => Error::Unreachable(m),
            other => Error::Invalid(other.to_string()),
        }
    }
}

/// A text in the user's language ("fr", "en", "es").
fn say(lang: &str, fr: &str, en: &str, es: &str) -> String {
    match lang {
        "en" => en,
        "es" => es,
        _ => fr,
    }
    .to_string()
}

impl HapError {
    /// What it means for the person pairing.
    fn explain(&self, lang: &str) -> Error {
        let text = match self {
            HapError::Accessory(2) => say(
                lang,
                "Code incorrect : vérifiez le code HomeKit à 8 chiffres de l'accessoire.",
                "Wrong code: check the 8-digit HomeKit code of the accessory.",
                "Código incorrecto: compruebe el código HomeKit de 8 cifras del accesorio.",
            ),
            HapError::Accessory(3) | HapError::Accessory(5) => say(
                lang,
                "Trop d'essais : l'accessoire refuse l'appairage pour l'instant. Réessayez plus tard.",
                "Too many tries: the accessory refuses pairing for now. Try again later.",
                "Demasiados intentos: el accesorio rechaza el emparejamiento por ahora. Inténtelo más tarde.",
            ),
            HapError::Accessory(4) => {
                say(lang, "L'accessoire n'accepte plus d'autre contrôleur.", "The accessory accepts no other controller.", "El accesorio no acepta más controladores.")
            }
            HapError::Accessory(6) => say(
                lang,
                "L'accessoire est déjà appairé à un autre contrôleur HomeKit.",
                "The accessory is already paired with another HomeKit controller.",
                "El accesorio ya está emparejado con otro controlador HomeKit.",
            ),
            HapError::Accessory(7) => say(
                lang,
                "L'accessoire est occupé par un autre appairage. Réessayez dans un instant.",
                "The accessory is busy with another pairing. Try again in a moment.",
                "El accesorio está ocupado con otro emparejamiento. Inténtelo de nuevo en un momento.",
            ),
            HapError::Forgotten => say(
                lang,
                "L'accessoire ne reconnaît plus Breezone (appairage retiré, ou accessoire réinitialisé) : retirez ce module, puis appairez-le de nouveau avec son code.",
                "The accessory no longer knows Breezone (pairing removed, or accessory reset): remove this module, then pair it again with its code.",
                "El accesorio ya no reconoce Breezone (emparejamiento eliminado o accesorio restablecido): quite este módulo y vuelva a emparejarlo con su código.",
            ),
            HapError::Accessory(code) => {
                say(lang, "L'accessoire refuse (erreur {0}).", "The accessory refuses (error {0}).", "El accesorio rechaza (error {0}).").replace("{0}", &code.to_string())
            }
            HapError::Unreachable(e) => {
                return Error::Unreachable(say(lang, "Accessoire injoignable : {0}", "Accessory unreachable: {0}", "Accesorio inaccesible: {0}").replace("{0}", e))
            }
            HapError::Protocol(e) => {
                say(lang, "Réponse inattendue de l'accessoire : {0}", "Unexpected answer from the accessory: {0}", "Respuesta inesperada del accesorio: {0}").replace("{0}", e)
            }
        };
        Error::Invalid(text)
    }
}

/// "123-45-678", from the code as typed ("12345678", "123 45 678"…).
pub fn setup_code(typed: &str) -> Option<String> {
    let digits: String = typed.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 8 || typed.chars().any(|c| !(c.is_ascii_digit() || c == '-' || c == ' ')) {
        return None;
    }
    Some(format!("{}-{}-{}", &digits[..3], &digits[3..5], &digits[5..]))
}

fn uuid_v4(mut b: [u8; 16]) -> String {
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode_upper(b);
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// "ip:port" or "name:port" of the local network.
fn valid_host(host_: &str) -> bool {
    !host_.is_empty() && host_.len() <= 253 && host_.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

/// A pairing started on an accessory, waiting for its code. An accessory with a screen shows a
/// new code each time a pairing starts: the code is only asked once it is shown.
struct PairSetup {
    host: String,
    conn: Conn,
    salt: Vec<u8>,
    b_pub: Vec<u8>,
}

thread_local! {
    /// The pairing started (`start`), kept by Breezone with this instance until `finish`.
    static STARTED: RefCell<Option<PairSetup>> = const { RefCell::new(None) };
}

impl PairSetup {
    /// `with_auth`: the accessory announces Apple authentication (feature flag 1), as most
    /// certified ones do.
    fn start(host_: &str, with_auth: bool) -> std::result::Result<Self, HapError> {
        let mut conn = Conn::connect(host_)?;
        let method = [u8::from(with_auth)];
        let m2 = conn.tlv("/pair-setup", &[(tlv::STATE, &[1]), (tlv::METHOD, &method)])?;
        let salt = tlv::get(&m2, tlv::SALT).ok_or_else(|| HapError::Protocol("pair-setup: no salt".into()))?.to_vec();
        let b_pub = tlv::get(&m2, tlv::PUBLIC_KEY).ok_or_else(|| HapError::Protocol("pair-setup: no public key".into()))?.to_vec();
        Ok(Self { host: host_.to_string(), conn, salt, b_pub })
    }

    fn finish(self, code: &str) -> std::result::Result<Pairing, HapError> {
        let Self { mut conn, salt, b_pub, .. } = self;
        let client = srp::Client::new(&random::<32>());
        let agreed = client.agree(&salt, &b_pub, code).map_err(HapError::Protocol)?;
        let m4 = conn.tlv("/pair-setup", &[(tlv::STATE, &[3]), (tlv::PUBLIC_KEY, &client.public), (tlv::PROOF, &agreed.proof)])?;
        if tlv::get(&m4, tlv::PROOF) != Some(agreed.expected.as_slice()) {
            return Err(HapError::Protocol("the accessory's proof does not match".into()));
        }

        // Breezone's long-term identity for this accessory: a UUID, as Apple's controllers send
        // it (some accessories, Legrand's, refuse anything else).
        let controller_id = uuid_v4(random::<16>());
        let me = SigningKey::from_bytes(&random::<32>());
        let ltpk = me.verifying_key().to_bytes();
        let x = hkdf("Pair-Setup-Controller-Sign-Salt", &agreed.key, "Pair-Setup-Controller-Sign-Info");
        let mut info = x.to_vec();
        info.extend_from_slice(controller_id.as_bytes());
        info.extend_from_slice(&ltpk);
        let signature = me.sign(&info).to_bytes();
        let inner = tlv::encode(&[(tlv::IDENTIFIER, controller_id.as_bytes()), (tlv::PUBLIC_KEY, &ltpk), (tlv::SIGNATURE, &signature)]);
        let key = hkdf("Pair-Setup-Encrypt-Salt", &agreed.key, "Pair-Setup-Encrypt-Info");
        let sealed = seal(&key, label("PS-Msg05"), &[], &inner);
        // The code was right: a refusal now is about Breezone's identity, not the code.
        let m6 = conn.tlv("/pair-setup", &[(tlv::STATE, &[5]), (tlv::ENCRYPTED_DATA, &sealed)]).map_err(|e| match e {
            HapError::Accessory(code) => HapError::Protocol(format!("code accepted, identity refused (M6, error {code})")),
            e => e,
        })?;

        let sealed = tlv::get(&m6, tlv::ENCRYPTED_DATA).ok_or_else(|| HapError::Protocol("pair-setup: no data".into()))?;
        let inner = tlv::decode(&open(&key, label("PS-Msg06"), &[], sealed)?).map_err(HapError::Protocol)?;
        let accessory_id = tlv::get(&inner, tlv::IDENTIFIER).ok_or_else(|| HapError::Protocol("pair-setup: no identifier".into()))?;
        let accessory_ltpk = tlv::get(&inner, tlv::PUBLIC_KEY).ok_or_else(|| HapError::Protocol("pair-setup: no key".into()))?;
        let pairing = Pairing {
            accessory_id: String::from_utf8_lossy(accessory_id).into_owned(),
            accessory_ltpk: hex::encode(accessory_ltpk),
            controller_id,
            controller_ltsk: hex::encode(me.to_bytes()),
        };
        let signature =
            tlv::get(&inner, tlv::SIGNATURE).and_then(|s| ed25519_dalek::Signature::from_slice(s).ok()).ok_or_else(|| HapError::Protocol("pair-setup: no signature".into()))?;
        let x = hkdf("Pair-Setup-Accessory-Sign-Salt", &agreed.key, "Pair-Setup-Accessory-Sign-Info");
        let mut info = x.to_vec();
        info.extend_from_slice(accessory_id);
        info.extend_from_slice(accessory_ltpk);
        pairing.accessory_key()?.verify(&info, &signature).map_err(|_| HapError::Protocol("the accessory's signature does not match".into()))?;
        Ok(pairing)
    }
}

/// Short HomeKit types (the UUIDs of Apple's base: "0000008C-0000-1000-8000-0026BB765291" → "8C").
mod types {
    pub const INFORMATION: &str = "3E";
    pub const WINDOW_COVERING: &str = "8C";
    pub const WINDOW: &str = "8B";
    pub const DOOR: &str = "81";
    pub const SWITCH: &str = "49";
    pub const OUTLET: &str = "47";
    pub const LIGHTBULB: &str = "43";
    pub const NAME: &str = "23";
    pub const MANUFACTURER: &str = "20";
    pub const MODEL: &str = "21";
    pub const FIRMWARE: &str = "52";
    pub const CURRENT_POSITION: &str = "6D";
    pub const TARGET_POSITION: &str = "7C";
    pub const POSITION_STATE: &str = "72";
    pub const HOLD_POSITION: &str = "6F";
    pub const ON: &str = "25";
    pub const STATELESS_SWITCH: &str = "89";
    pub const SWITCH_EVENT: &str = "73";
    pub const LABEL_INDEX: &str = "CB";
}

/// A remote's events: the presses, HomeKit's values 0, 1, 2.
const PRESSES: [&str; 3] = ["single", "double", "long"];
/// Without any event this long, the listening session is opened again (it may be dead).
const QUIET_S: i64 = 600;

/// "0000008C-0000-1000-8000-0026BB765291" or "8C" → "8C"; any other UUID as it is.
fn short(kind: &str) -> String {
    let k = kind.to_ascii_uppercase();
    match k.strip_suffix("-0000-1000-8000-0026BB765291") {
        Some(head) => head.trim_start_matches('0').to_string(),
        None => k,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Cover {
        current: u64,
        target: u64,
        state: Option<u64>,
        hold: Option<u64>,
    },
    Switch {
        on: u64,
    },
    /// A remote: the event of each button, with its number.
    Buttons {
        events: Vec<(u64, u32)>,
    },
}

/// A device of the accessory: where its characteristics are.
#[derive(Debug, Clone, PartialEq)]
struct Tracked {
    reference: String,
    aid: u64,
    kind: Kind,
}

#[derive(Debug, Clone)]
struct Layout {
    model: String,
    firmware: Option<String>,
    devices: Vec<(DeviceInfo, Tracked)>,
}

fn characteristic<'a>(service: &'a Value, kind: &str) -> Option<&'a Value> {
    service["characteristics"].as_array()?.iter().find(|c| c["type"].as_str().map(short).as_deref() == Some(kind))
}

fn iid(service: &Value, kind: &str) -> Option<u64> {
    characteristic(service, kind)?["iid"].as_u64()
}

fn text(service: &Value, kind: &str) -> Option<String> {
    characteristic(service, kind)?["value"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The devices of an accessory database (`GET /accessories`).
fn layout(database: &Value) -> Layout {
    use types::*;
    let accessories = database["accessories"].as_array().cloned().unwrap_or_default();
    let info = |a: &Value| a["services"].as_array().and_then(|s| s.iter().find(|s| s["type"].as_str().map(short).as_deref() == Some(INFORMATION)).cloned());
    let bridge = accessories.iter().find(|a| a["aid"] == 1).and_then(info);
    let maker = bridge.as_ref().and_then(|b| text(b, MANUFACTURER));
    let model = bridge.as_ref().and_then(|b| text(b, MODEL)).or_else(|| bridge.as_ref().and_then(|b| text(b, NAME))).unwrap_or_else(|| "HomeKit".into());
    let mut devices = vec![];
    for a in &accessories {
        let Some(aid) = a["aid"].as_u64() else { continue };
        let accessory_name = info(a).and_then(|i| text(&i, NAME)).unwrap_or_else(|| format!("Accessoire {aid}"));
        let services = a["services"].as_array().cloned().unwrap_or_default();
        let several =
            services.iter().filter(|s| matches!(s["type"].as_str().map(short).as_deref(), Some(WINDOW_COVERING | WINDOW | DOOR | SWITCH | OUTLET | LIGHTBULB))).count() > 1;
        // The buttons of a remote are one device: the remote.
        let mut buttons: Vec<(u64, u32)> = services
            .iter()
            .filter(|s| s["type"].as_str().map(short).as_deref() == Some(STATELESS_SWITCH))
            .filter_map(|s| {
                let event = iid(s, SWITCH_EVENT)?;
                let index = characteristic(s, LABEL_INDEX).and_then(|c| c["value"].as_u64()).map(|i| i as u32);
                Some((event, index.unwrap_or(0)))
            })
            .collect();
        if !buttons.is_empty() {
            // Numbered as the accessory labels them, else in their order.
            if buttons.iter().any(|(_, i)| *i == 0) {
                for (n, b) in buttons.iter_mut().enumerate() {
                    b.1 = n as u32 + 1;
                }
            }
            buttons.sort_by_key(|(_, i)| *i);
            let reference = format!("{aid}.buttons");
            devices.push((device(reference.clone(), "remote", &["button@1"], accessory_name.clone()), Tracked { reference, aid, kind: Kind::Buttons { events: buttons } }));
        }
        for s in &services {
            let Some(siid) = s["iid"].as_u64() else { continue };
            let kind = s["type"].as_str().map(short).unwrap_or_default();
            // One device per accessory: its own name ("Rolling shutter switch 1"), the service's
            // is generic ("Window covering").
            let name = match (text(s, NAME), several) {
                (Some(n), true) => n,
                (None, true) => format!("{accessory_name} {siid}"),
                (_, false) => accessory_name.clone(),
            };
            let reference = format!("{aid}.{siid}");
            let (tracked, traits, device_kind) = match kind.as_str() {
                WINDOW_COVERING | WINDOW | DOOR => {
                    let (Some(current), Some(target)) = (iid(s, CURRENT_POSITION), iid(s, TARGET_POSITION)) else { continue };
                    // The kind tells a shutter or blind from a roof window (VELUX) or a door.
                    let device_kind = match kind.as_str() {
                        WINDOW => "window",
                        DOOR => "door",
                        _ => "shutter",
                    };
                    (Kind::Cover { current, target, state: iid(s, POSITION_STATE), hold: iid(s, HOLD_POSITION) }, "cover.position@1", device_kind)
                }
                SWITCH | OUTLET | LIGHTBULB => {
                    let Some(on) = iid(s, ON) else { continue };
                    (Kind::Switch { on }, "switch.relay@1", "switch.relay")
                }
                _ => continue,
            };
            devices.push((device(reference.clone(), device_kind, &[traits], name), Tracked { reference, aid, kind: tracked }));
        }
    }
    // "Legrand Ecometer" already names its maker.
    let maker = maker.filter(|m| !model.to_lowercase().starts_with(&m.to_lowercase()));
    Layout { model: [maker, Some(model)].into_iter().flatten().collect::<Vec<_>>().join(" "), firmware: bridge.as_ref().and_then(|b| text(b, FIRMWARE)), devices }
}

/// The state of each device, from the values read (`aid.iid` → value).
fn states_of(layout: &Layout, values: &HashMap<(u64, u64), Value>, presses: &HashMap<String, State>) -> Vec<(String, State)> {
    let mut out = vec![];
    for (_, t) in &layout.devices {
        let get = |iid: u64| values.get(&(t.aid, iid)).filter(|v| !v.is_null());
        let mut s = State::new();
        match &t.kind {
            Kind::Cover { current, target, state, hold } => {
                let Some(position) = get(*current).and_then(Value::as_i64) else { continue };
                s.insert("position".into(), json!(position.clamp(0, 100)));
                // Without « hold position » nothing stops it on the way (see `act`): the apps hide
                // the stop button.
                s.insert("stoppable".into(), json!(hold.is_some()));
                if let Some(v) = get(*target).and_then(Value::as_i64) {
                    s.insert("target".into(), json!(v.clamp(0, 100)));
                }
                // HomeKit: 0 decreasing (closing), 1 increasing (opening), 2 stopped.
                if let Some(v) = state.and_then(get).and_then(Value::as_i64) {
                    s.insert("moving".into(), json!(["closing", "opening", "stopped"].get(v as usize).copied().unwrap_or("stopped")));
                }
            }
            Kind::Switch { on } => {
                let Some(v) = get(*on) else { continue };
                s.insert("on".into(), json!(v.as_bool().unwrap_or_else(|| v.as_i64() == Some(1))));
            }
            // A remote has no state to read: its last press is what was heard.
            Kind::Buttons { events } => {
                s.insert("buttons".into(), json!(events.len()));
                if let Some(last) = presses.get(&t.reference) {
                    s.extend(last.clone());
                }
            }
        }
        out.push((t.reference.clone(), s));
    }
    out
}

/// The remotes' events of a database: (aid, iid) → (remote, button).
fn button_events(layout: &Layout) -> HashMap<(u64, u64), (String, u32)> {
    layout
        .devices
        .iter()
        .filter_map(|(_, t)| match &t.kind {
            Kind::Buttons { events } => Some(events.iter().map(|(iid, n)| ((t.aid, *iid), (t.reference.clone(), *n))).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// The presses an event tells: (remote, button, press).
fn presses_of(event: &Value, buttons: &HashMap<(u64, u64), (String, u32)>) -> Vec<(String, u32, &'static str)> {
    event["characteristics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let (remote, n) = buttons.get(&(c["aid"].as_u64()?, c["iid"].as_u64()?))?;
            let press = PRESSES.get(c["value"].as_u64()? as usize)?;
            Some((remote.clone(), *n, *press))
        })
        .collect()
}

/// The session that hears the remotes, apart from the one that reads and drives (as the
/// reference controllers do).
struct Listener {
    conn: Conn,
    /// The last time it was opened or heard something.
    heard: i64,
}

pub struct HomeKit {
    host: String,
    pairing: Pairing,
    /// The session that reads and drives, kept open; opened again after an error.
    conn: RefCell<Option<Conn>>,
    layout: RefCell<Option<Layout>>,
    listener: RefCell<Option<Listener>>,
    /// When the listening session may be opened again, and how long to wait after a failure.
    retry: RefCell<(i64, i64)>,
    /// The last press of each remote.
    presses: RefCell<HashMap<String, State>>,
}

impl HomeKit {
    /// A request on the session, opened (again) when needed: tried twice.
    fn on_session<T>(&self, f: impl Fn(&mut Conn) -> std::result::Result<T, HapError>) -> std::result::Result<T, HapError> {
        let mut conn = self.conn.borrow_mut();
        for attempt in 0..2 {
            if conn.is_none() {
                *conn = Some(Conn::verify(&self.host, &self.pairing)?);
            }
            match f(conn.as_mut().expect("opened")) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    *conn = None;
                    if attempt == 1 || matches!(e, HapError::Forgotten) {
                        return Err(e);
                    }
                }
            }
        }
        unreachable!()
    }

    fn layout(&self) -> std::result::Result<Layout, HapError> {
        if let Some(l) = self.layout.borrow().clone() {
            return Ok(l);
        }
        let l = layout(&self.on_session(|c| c.get_json("/accessories"))?);
        *self.layout.borrow_mut() = Some(l.clone());
        Ok(l)
    }

    fn put(&self, body: &Value) -> std::result::Result<(), HapError> {
        self.on_session(|c| c.put_json("/characteristics", body))
    }

    /// Opens the listening session when there are remotes to hear and it is not open (or quiet
    /// for long): its failures only delay the next try.
    fn listen(&self, layout: &Layout) {
        let now = host::now();
        let buttons = button_events(layout);
        if buttons.is_empty() {
            *self.listener.borrow_mut() = None;
            return;
        }
        if self.listener.borrow().as_ref().is_some_and(|l| now - l.heard < QUIET_S) || now < self.retry.borrow().0 {
            return;
        }
        *self.listener.borrow_mut() = None;
        let opened = (|| {
            let mut conn = Conn::verify(&self.host, &self.pairing)?;
            let ev: Vec<Value> = buttons.keys().map(|(aid, iid)| json!({"aid": aid, "iid": iid, "ev": true})).collect();
            conn.put_json("/characteristics", &json!({ "characteristics": ev }))?;
            Ok::<Conn, HapError>(conn)
        })();
        match opened {
            Ok(conn) => {
                host::log("info", &format!("listening to {} button(s) of the HomeKit accessory at {}", buttons.len(), self.host));
                *self.retry.borrow_mut() = (0, 0);
                let mut listener = Listener { conn, heard: now };
                // Events that came with the answer.
                let events = std::mem::take(&mut listener.conn.events);
                *self.listener.borrow_mut() = Some(listener);
                self.heard(events, &buttons);
            }
            Err(e) => {
                let wait = (self.retry.borrow().1 * 2).clamp(5, 300);
                *self.retry.borrow_mut() = (now + wait, wait);
                host::log("debug", &format!("HomeKit events at {}: {e}", self.host));
            }
        }
    }

    /// Presses heard: told to Breezone, and kept as the remote's last press.
    fn heard(&self, events: Vec<Value>, buttons: &HashMap<(u64, u64), (String, u32)>) {
        for event in events {
            for (remote, button, press) in presses_of(&event, buttons) {
                if let Value::Object(o) = json!({"last_button": button, "last_press": press, "last_at": host::now()}) {
                    self.presses.borrow_mut().insert(remote.clone(), o);
                }
                host::pressed(&remote, button, press);
            }
        }
    }
}

impl Driver for HomeKit {
    fn open(config: Value, secret: Option<Value>) -> Result<Self> {
        let host_ = config["host"].as_str().map(str::trim).unwrap_or_default();
        if !valid_host(host_) {
            return Err(Error::Invalid("invalid HomeKit accessory address".into()));
        }
        let pairing = secret.as_ref().and_then(Pairing::from_value).ok_or_else(|| Error::Invalid("a HomeKit accessory is added by pairing it with its code".into()))?;
        Ok(Self {
            host: host_.to_string(),
            pairing,
            conn: RefCell::new(None),
            layout: RefCell::new(None),
            listener: RefCell::new(None),
            retry: RefCell::new((0, 0)),
            presses: RefCell::default(),
        })
    }

    fn describe(&self) -> Result<Described> {
        // The database again: accessories may have been added in their own app.
        *self.layout.borrow_mut() = None;
        let l = self.layout()?;
        let covers = l.devices.iter().filter(|(d, _)| d.traits.iter().any(|t| t.starts_with("cover.position"))).count();
        let others = l.devices.len() - covers;
        let note = format!("{covers} volet{} · {others} autre{}", if covers > 1 { "s" } else { "" }, if others > 1 { "s" } else { "" });
        // Remotes added or removed: listened to again.
        *self.listener.borrow_mut() = None;
        Ok(Described {
            model: format!("{} ({note})", l.model),
            firmware: l.firmware.clone(),
            devices: l.devices.iter().map(|(d, _)| d.clone()).collect(),
            identity: Some(self.pairing.accessory_id.clone()),
        })
    }

    fn read(&self) -> Result<Vec<(String, State)>> {
        let l = self.layout()?;
        let ids: Vec<String> = l
            .devices
            .iter()
            .flat_map(|(_, t)| {
                let iids = match &t.kind {
                    Kind::Cover { current, target, state, .. } => [Some(*current), Some(*target), *state].into_iter().flatten().collect::<Vec<_>>(),
                    Kind::Switch { on } => vec![*on],
                    Kind::Buttons { .. } => vec![],
                };
                iids.into_iter().map(move |i| format!("{}.{i}", t.aid))
            })
            .collect();
        let values: HashMap<(u64, u64), Value> = if ids.is_empty() {
            HashMap::new()
        } else {
            let answer = self.on_session(|c| c.get_json(&format!("/characteristics?id={}", ids.join(","))))?;
            answer["characteristics"]
                .as_array()
                .map(|cs| cs.iter().filter_map(|c| Some(((c["aid"].as_u64()?, c["iid"].as_u64()?), c["value"].clone()))).collect())
                .unwrap_or_default()
        };
        // Once a reading's session is open: a starting server does not open two sessions at once
        // on an accessory that may still hold the previous ones.
        self.listen(&l);
        Ok(states_of(&l, &values, &self.presses.borrow()))
    }

    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> {
        let l = self.layout()?;
        let t = l.devices.iter().map(|(_, t)| t).find(|t| t.reference == reference).ok_or_else(|| Error::Unsupported(format!("{reference}: unknown device")))?;
        let write = |iid: u64, value: Value| json!({"characteristics": [{"aid": t.aid, "iid": iid, "value": value}]});
        match (&t.kind, trait_name, action) {
            (Kind::Cover { target, .. }, "cover.position", "set") => {
                let position = params.get("position").and_then(Value::as_i64).ok_or_else(|| Error::Invalid("`position` is required".into()))?;
                Ok(self.put(&write(*target, json!(position.clamp(0, 100))))?)
            }
            (Kind::Cover { hold: Some(hold), .. }, "cover.position", "stop") => {
                host::log("info", &format!("HomeKit {reference}: stop with « hold position »"));
                Ok(self.put(&write(*hold, json!(true)))?)
            }
            // Without « hold position », a shutter cannot be stopped through HomeKit: sending it to
            // its current position would send it back where it started when, as the Legrand
            // gateway does, the position is only updated at the end of the move.
            (Kind::Cover { .. }, "cover.position", "stop") => {
                Err(Error::Unsupported("ce volet ne peut pas être arrêté en cours de route par HomeKit : utilisez l'interrupteur ou l'application Legrand".into()))
            }
            (Kind::Switch { on }, "switch.relay", "set") => {
                let value = params.get("on").and_then(Value::as_bool).ok_or_else(|| Error::Invalid("`on` is required".into()))?;
                Ok(self.put(&write(*on, json!(value)))?)
            }
            _ => Err(Error::Unsupported(format!("{reference}: {trait_name}.{action}"))),
        }
    }

    fn on_message(&self, subscription: u32, data: &[u8]) -> Result<()> {
        let mut events = vec![];
        {
            let mut listener = self.listener.borrow_mut();
            let Some(l) = listener.as_mut().filter(|l| l.conn.subscription == subscription) else { return Ok(()) };
            l.conn.received(data);
            loop {
                match l.conn.try_message() {
                    Ok(Some((status, body))) => {
                        if status.starts_with("EVENT/") {
                            if let Ok(v) = breezone_module_sdk::serde_json::from_slice(&body) {
                                events.push(v);
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        host::log("debug", &format!("HomeKit events at {}: {e}", self.host));
                        *listener = None;
                        break;
                    }
                }
            }
            if let Some(l) = listener.as_mut() {
                l.heard = host::now();
            }
        }
        if let Some(l) = self.layout.borrow().as_ref() {
            self.heard(events, &button_events(l));
        }
        Ok(())
    }

    /// The accessory's database and the values of its characteristics now, as HomeKit gives them.
    fn diagnostic(&self) -> Result<Value> {
        let database = self.on_session(|c| c.get_json("/accessories"))?;
        Ok(json!({ "accessories": database["accessories"] }))
    }

    /// Removes Breezone's own pairing from the accessory (and nothing else): the accessory forgets
    /// Breezone, and can be paired again.
    fn forget(&self) -> Result<()> {
        *self.listener.borrow_mut() = None;
        let id = self.pairing.controller_id.clone();
        self.on_session(|c| c.tlv("/pairings", &[(tlv::STATE, &[1]), (tlv::METHOD, &[4]), (tlv::IDENTIFIER, id.as_bytes())]))?;
        *self.conn.borrow_mut() = None;
        Ok(())
    }

    fn close(&self) {
        *self.listener.borrow_mut() = None;
        *self.conn.borrow_mut() = None;
    }

    /// `start` (an accessory with a screen shows its code now), then `finish` with its setup code
    /// (8 digits), or `cancel`.
    fn pair(step: &str, input: Value) -> Result<Paired> {
        let lang = input["lang"].as_str().unwrap_or("fr").to_string();
        let host_ = input["host"].as_str().map(str::trim).filter(|h| valid_host(h)).ok_or_else(|| Error::Invalid("invalid address".into()))?.to_string();
        let apple = input["flags"].as_u64().unwrap_or(0) & 1 == 1;
        match step {
            "start" => {
                // A pairing left half-done keeps the accessory busy: the previous one is dropped first.
                STARTED.with(|s| s.borrow_mut().take());
                let setup = PairSetup::start(&host_, apple).map_err(|e| e.explain(&lang))?;
                STARTED.with(|s| *s.borrow_mut() = Some(setup));
                Ok(Paired::Next)
            }
            "cancel" => {
                STARTED.with(|s| s.borrow_mut().take());
                Ok(Paired::Next)
            }
            "finish" => {
                let code = setup_code(input["code"].as_str().unwrap_or_default()).ok_or_else(|| {
                    Error::Invalid(say(
                        &lang,
                        "Le code HomeKit a 8 chiffres (par exemple 123-45-678).",
                        "The HomeKit code has 8 digits (for example 123-45-678).",
                        "El código HomeKit tiene 8 cifras (por ejemplo 123-45-678).",
                    ))
                })?;
                let started = STARTED.with(|s| s.borrow_mut().take()).filter(|s| s.host == host_);
                let setup = match started {
                    Some(s) => s,
                    None => PairSetup::start(&host_, apple).map_err(|e| e.explain(&lang))?,
                };
                let pairing = setup.finish(&code).map_err(|e| e.explain(&lang))?;
                Ok(Paired::Done(PairedSource {
                    config: json!({"host": host_, "device_id": pairing.accessory_id}).to_string(),
                    secret: pairing.to_value().to_string(),
                    name: "HomeKit".into(),
                }))
            }
            other => Err(Error::Unsupported(format!("unknown step {other}"))),
        }
    }
}

module!(HomeKit);

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Value {
        json!({"accessories": [
            {"aid": 1, "services": [{"iid": 1, "type": "3E", "characteristics": [
                {"iid": 2, "type": "23", "value": "Module Control"}, {"iid": 3, "type": "20", "value": "Legrand"},
                {"iid": 4, "type": "21", "value": "Gateway"}, {"iid": 5, "type": "52", "value": "2.1.0"}]}]},
            {"aid": 2, "services": [
                {"iid": 1, "type": "0000003E-0000-1000-8000-0026BB765291", "characteristics": [{"iid": 2, "type": "00000023-0000-1000-8000-0026BB765291", "value": "Volet salon"}]},
                {"iid": 8, "type": "0000008C-0000-1000-8000-0026BB765291", "characteristics": [
                    {"iid": 13, "type": "00000023-0000-1000-8000-0026BB765291", "value": "Window covering"},
                    {"iid": 9, "type": "0000006D-0000-1000-8000-0026BB765291", "value": 40},
                    {"iid": 10, "type": "0000007C-0000-1000-8000-0026BB765291", "value": 40},
                    {"iid": 11, "type": "00000072-0000-1000-8000-0026BB765291", "value": 2},
                    {"iid": 12, "type": "0000006F-0000-1000-8000-0026BB765291", "value": false}]}]},
            {"aid": 3, "services": [
                {"iid": 1, "type": "3E", "characteristics": [{"iid": 2, "type": "23", "value": "Prise cuisine"}]},
                {"iid": 8, "type": "47", "characteristics": [{"iid": 9, "type": "25", "value": true}]}]},
            {"aid": 4, "services": [
                {"iid": 1, "type": "3E", "characteristics": [{"iid": 2, "type": "23", "value": "Ecometer"}]},
                {"iid": 8, "type": "E863F007-079E-48FF-8F27-9C2605A29F52", "characteristics": []}]},
            {"aid": 5, "services": [
                {"iid": 1, "type": "3E", "characteristics": [{"iid": 2, "type": "23", "value": "Télécommande"}]},
                {"iid": 10, "type": "89", "characteristics": [{"iid": 11, "type": "73", "value": null}, {"iid": 12, "type": "CB", "value": 2}]},
                {"iid": 20, "type": "89", "characteristics": [{"iid": 21, "type": "73", "value": null}, {"iid": 22, "type": "CB", "value": 1}]}]}
        ]})
    }

    #[test]
    fn covers_switches_and_remotes_of_a_bridge() {
        let l = layout(&database());
        assert_eq!(l.model, "Legrand Gateway");
        assert_eq!(l.firmware.as_deref(), Some("2.1.0"));
        let names: Vec<(&str, &str, &str)> = l.devices.iter().map(|(d, _)| (d.reference.as_str(), d.kind.as_str(), d.name.as_str())).collect();
        assert_eq!(names, vec![("2.8", "shutter", "Volet salon"), ("3.8", "switch.relay", "Prise cuisine"), ("5.buttons", "remote", "Télécommande")]);
        assert_eq!(l.devices[0].1.kind, Kind::Cover { current: 9, target: 10, state: Some(11), hold: Some(12) });
        assert_eq!(l.devices[2].1.kind, Kind::Buttons { events: vec![(21, 1), (11, 2)] }, "numbered as labelled");

        let values = HashMap::from([((2, 9), json!(35)), ((2, 10), json!(80)), ((2, 11), json!(1)), ((3, 9), json!(1))]);
        let states: HashMap<String, State> = states_of(&l, &values, &HashMap::new()).into_iter().collect();
        assert_eq!(states["2.8"]["position"], 35);
        assert_eq!(states["2.8"]["target"], 80);
        assert_eq!(states["2.8"]["moving"], "opening");
        assert_eq!(states["2.8"]["stoppable"], true);
        assert_eq!(states["3.8"]["on"], true);
        assert_eq!(states["5.buttons"]["buttons"], 2);
        assert_eq!(short("0000008C-0000-1000-8000-0026BB765291"), "8C");

        let buttons = button_events(&l);
        let event = json!({"characteristics": [{"aid": 5, "iid": 11, "value": 2}, {"aid": 5, "iid": 21, "value": 0}, {"aid": 2, "iid": 9, "value": 3}]});
        assert_eq!(presses_of(&event, &buttons), vec![("5.buttons".to_string(), 2, "long"), ("5.buttons".to_string(), 1, "single")]);
    }

    #[test]
    fn a_shutter_without_hold_position_cannot_be_stopped() {
        let mut db = database();
        db["accessories"][1]["services"][1]["characteristics"].as_array_mut().unwrap().pop();
        let l = layout(&db);
        assert_eq!(l.devices[0].1.kind, Kind::Cover { current: 9, target: 10, state: Some(11), hold: None });
        let states: HashMap<String, State> = states_of(&l, &HashMap::from([((2, 9), json!(100)), ((2, 10), json!(0)), ((2, 11), json!(0))]), &HashMap::new()).into_iter().collect();
        assert_eq!(states["2.8"]["stoppable"], false);
        assert_eq!(states["2.8"]["moving"], "closing");
    }

    #[test]
    fn setup_codes_as_typed() {
        assert_eq!(setup_code("03145154").as_deref(), Some("031-45-154"));
        assert_eq!(setup_code("031-45-154").as_deref(), Some("031-45-154"));
        assert_eq!(setup_code("031 45 154").as_deref(), Some("031-45-154"));
        assert_eq!(setup_code("0314515"), None);
        assert_eq!(setup_code("03145154a"), None);
    }

    #[test]
    fn messages_whole_or_chunked() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}EVENT/1.0 200 OK\r\n";
        let (status, body, used) = hap::parse_message(plain).unwrap().unwrap();
        assert_eq!((status.as_str(), body.as_slice()), ("HTTP/1.1 200 OK", b"{}".as_slice()));
        assert_eq!(hap::parse_message(&plain[used..]).unwrap(), None, "not whole yet");
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        let (_, body, used) = hap::parse_message(chunked).unwrap().unwrap();
        assert_eq!((body.as_slice(), used), (b"abcde".as_slice(), chunked.len()));
    }

    /// The secret of a source paired by the driver compiled in.
    #[test]
    fn the_pairing_of_the_driver_compiled_in() {
        let p = Pairing { accessory_id: "AA".into(), accessory_ltpk: "00".repeat(32), controller_id: "C".into(), controller_ltsk: "11".repeat(32) };
        let secret: Value = breezone_module_sdk::serde_json::from_str(&p.to_value().to_string()).unwrap();
        assert_eq!(Pairing::from_value(&secret), Some(p));
        assert!(HomeKit::open(json!({"host": "192.168.1.25:5001"}), None).is_err(), "needs a pairing");
    }
}
