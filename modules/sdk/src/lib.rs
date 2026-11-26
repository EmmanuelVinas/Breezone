//! Writing a module of Breezone (docs/roadmap/modules.md, lot L6).
//!
//! A module implements [`Driver`] for its source (one configured piece of hardware) and declares
//! it with [`module!`]: the SDK makes the WebAssembly component of it. Everything travels as
//! ordinary JSON (`serde_json`); the calls a module does not answer say « unsupported » by
//! themselves. What it asks of Breezone (HTTP, subscriptions, its store, an account, a press) goes
//! through [`host`], which in a test runs against the [`testing`] bench instead.
//!
//! ```ignore
//! use breezone_module_sdk::{host, json, module, Described, Driver, Result, State, Value};
//!
//! struct Plug { base: String }
//!
//! impl Driver for Plug {
//!     fn open(config: Value, _secret: Option<Value>) -> Result<Self> { … }
//!     fn describe(&self) -> Result<Described> { … }
//!     fn read(&self) -> Result<Vec<(String, State)>> { … }
//!     fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()> { … }
//! }
//!
//! module!(Plug);
//! ```

/// The bindings of the interface (`server/contracts/wit/breezone-module.wit`).
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../server/contracts/wit",
        world: "module",
        pub_export_macro: true,
        export_macro_name: "export_bindings",
        default_bindings_module: "breezone_module_sdk::bindings",
    });
}

pub use bindings::breezone::module::types::{
    Curve, Described, DeviceInfo, Error, HardwareRoom, Header, LengthPrefix, Paired, PairedSource, Picture, ReadUntil, Scene, Schedule, ScheduleEnd,
};
pub use serde_json;
pub use serde_json::{json, Map, Value};

/// The values of the traits of one device.
pub type State = Map<String, Value>;
pub type Result<T> = std::result::Result<T, Error>;

/// What a module does not do.
pub fn unsupported<T>(what: &str) -> Result<T> {
    Err(Error::Unsupported(what.to_string()))
}

/// A device of a source (`kind`: its main trait; `traits`: `trait@version`).
pub fn device(reference: impl Into<String>, kind: &str, traits: &[&str], name: impl Into<String>) -> DeviceInfo {
    DeviceInfo { reference: reference.into(), kind: kind.to_string(), traits: traits.iter().map(|t| t.to_string()).collect(), name: name.into() }
}

/// A source of a module: one configured piece of hardware. Only `open`, `describe`, `read` and
/// `act` are required; the calls listed in `optional_calls` of the manifest are asked too.
pub trait Driver: Sized + 'static {
    /// Opens a source from its configuration and its secret (the password fields of its form).
    /// Checks the configuration; the hardware is reached at the first `describe` or `read`.
    fn open(config: Value, secret: Option<Value>) -> Result<Self>;
    /// Identifies the hardware and lists its devices.
    fn describe(&self) -> Result<Described>;
    /// The state of each device, by reference.
    fn read(&self) -> Result<Vec<(String, State)>>;
    /// Applies an action of a trait to a device (its parameters were checked by Breezone).
    fn act(&self, reference: &str, trait_name: &str, action: &str, params: &State) -> Result<()>;
    /// A message of a subscription ([`host::subscribe`]).
    fn on_message(&self, _subscription: u32, _data: &[u8]) -> Result<()> {
        Ok(())
    }
    /// What the hardware says about itself, raw (support).
    fn diagnostic(&self) -> Result<Value> {
        Ok(Value::Null)
    }
    fn rooms(&self) -> Result<Vec<HardwareRoom>> {
        unsupported("rooms")
    }
    fn scenes(&self) -> Result<Vec<Scene>> {
        unsupported("scenes")
    }
    fn activate_scene(&self, _id: &str) -> Result<()> {
        unsupported("scenes")
    }
    fn picture(&self, _reference: &str) -> Result<Picture> {
        unsupported("picture")
    }
    fn forecast(&self, _reference: &str) -> Result<Vec<Curve>> {
        unsupported("forecast")
    }
    fn unlock(&self, _key: &str) -> Result<bool> {
        unsupported("unlock")
    }
    /// The schedules the hardware keeps and runs itself (`schedules` in `optional_calls`):
    /// Breezone checked the devices, the action and its parameters before `save_schedule`, which
    /// creates one (empty id) or changes it, and gives its id.
    fn schedules(&self) -> Result<Vec<Schedule>> {
        unsupported("schedules")
    }
    fn save_schedule(&self, _schedule: Schedule) -> Result<String> {
        unsupported("schedules")
    }
    fn delete_schedule(&self, _id: &str) -> Result<()> {
        unsupported("schedules")
    }
    /// The source is removed: the hardware forgets Breezone (`forget` in `optional_calls`), from
    /// the source's configuration and secret, in an instance of its own (the source may be
    /// unreachable or already closed).
    fn forget(_config: Value, _secret: Option<Value>) -> Result<()> {
        Ok(())
    }
    /// What the hardware kept of a device from `since` to `until` (seconds), for the history
    /// Breezone could not record (`history` in `optional_calls`): only what it measured. From the
    /// source's configuration and secret, in an instance of its own; keep each answer short and
    /// give `more` to be asked the rest.
    fn history(_config: Value, _secret: Option<Value>, _reference: &str, _since: i64, _until: i64) -> Result<History> {
        unsupported("history")
    }
    /// A device was named in Breezone: the hardware takes the name too (`rename` in
    /// `optional_calls`, interface 1.4: `"requires": {"api": "^1.4"}`). From the source's
    /// configuration and secret, in an instance of its own.
    fn rename(_config: Value, _secret: Option<Value>, _reference: &str, _name: &str) -> Result<()> {
        unsupported("rename")
    }
    /// Ends the connections it keeps, before the server stops.
    fn close(&self) {}
    /// Whether this address is hardware of this module, and its name (the network scan, by a
    /// server that does not ask `identify`).
    fn probe(_host: &str) -> Option<String> {
        None
    }
    /// What the hardware at `host` is, for the network scan (`identify` in `optional_calls`):
    /// `announced` is what it said of itself by mDNS (`service`, `name`, `port`, `txt`; nothing to
    /// ask it then), `None` when a port of `discovery.tcp` is open there. By default, its name
    /// from [`Driver::probe`].
    fn identify(host: &str, announced: Option<&Value>) -> Option<Found> {
        if announced.is_some() {
            return None;
        }
        Self::probe(host).map(|name| Found { note: Some(format!("« {name} »")), ..Found::default() })
    }
    /// A step of the appairage (`pairing` of the manifest: `button` has the step `link`, `code`
    /// has `start`, `finish` and `cancel`); `input` has the address (`host`) of the hardware and
    /// the user's language (`lang`: "fr", "en", "es").
    fn pair(_step: &str, _input: Value) -> Result<Paired> {
        unsupported("pair")
    }
}

/// A stretch of what the hardware measured ([`Driver::history`]): each number the mean over
/// `period` seconds from `ts`, a counter (a field in kWh) what it rose by during it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HistoryPoint {
    pub ts: i64,
    pub period: i64,
    pub values: Map<String, Value>,
}

/// What the hardware kept of a device ([`Driver::history`]), in time order; `more`: ask again
/// from there.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct History {
    pub points: Vec<HistoryPoint>,
    pub more: Option<i64>,
}

impl History {
    pub fn to_json(&self) -> Value {
        let points: Vec<Value> = self
            .points
            .iter()
            .map(|p| {
                let mut v = p.values.clone();
                v.insert("ts".into(), json!(p.ts));
                v.insert("period".into(), json!(p.period));
                Value::Object(v)
            })
            .collect();
        json!({"points": points, "more": self.more})
    }
}

/// Hardware found on the network, as the scan shows it ([`Driver::identify`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    /// The model shown ("Shelly Pro 3EM"); the module's name when empty.
    pub model: String,
    /// What to know before adding it ("appuyez sur son bouton pour l'ajouter").
    pub note: Option<String>,
    /// What tells it apart (serial number, pairing id): a source with it is « already added ».
    pub device_id: Option<String>,
    pub firmware: Option<String>,
    pub mac: Option<String>,
    /// Found, but this module cannot drive it (a firmware not supported yet…).
    pub unsupported: bool,
    /// Given to the appairage (`flags` of its input): what the hardware announced it needs.
    pub flags: u32,
    /// Its port, when not the one it was found on.
    pub port: Option<u16>,
    /// "ip:port", when the port must be kept in its address (a HomeKit accessory).
    pub host: Option<String>,
}

impl Found {
    pub fn to_json(&self) -> Value {
        let mut v = json!({"model": self.model, "supported": !self.unsupported, "flags": self.flags});
        for (k, x) in [("note", &self.note), ("device_id", &self.device_id), ("firmware", &self.firmware), ("mac", &self.mac), ("host", &self.host)] {
            if let Some(x) = x {
                v[k] = json!(x);
            }
        }
        if let Some(p) = self.port {
            v["port"] = json!(p);
        }
        v
    }
}

/// What a module of the `service` role answers (the queries of the service contracts of its
/// manifest's `provides.services`). Nothing is kept between two calls: an account's credentials
/// come from [`host::account`] with the module's own id.
pub trait Service: 'static {
    /// Checks the account before Breezone keeps it (the service must accept it).
    fn check() -> Result<()> {
        Ok(())
    }
    /// A query of a contract ("tariff.day_color@1", "colors"): its parameters and its answer as the
    /// contract describes them.
    fn query(contract: &str, query: &str, params: Value) -> Result<Value>;
}

/// What a module without devices gives as its driver: nothing.
pub struct NoDriver;

impl Driver for NoDriver {
    fn open(_config: Value, _secret: Option<Value>) -> Result<Self> {
        unsupported("devices")
    }
    fn describe(&self) -> Result<Described> {
        unsupported("devices")
    }
    fn read(&self) -> Result<Vec<(String, State)>> {
        unsupported("devices")
    }
    fn act(&self, _reference: &str, _trait_name: &str, _action: &str, _params: &State) -> Result<()> {
        unsupported("devices")
    }
}

/// What a module without services gives as its service: nothing.
pub struct NoService;

impl Service for NoService {
    fn check() -> Result<()> {
        unsupported("services")
    }
    fn query(_contract: &str, _query: &str, _params: Value) -> Result<Value> {
        unsupported("services")
    }
}

/// A [`Driver`] as the interface wants it (made by [`module!`]).
pub struct Adapter<T>(pub T);

fn parsed(json: &str) -> Result<Value> {
    serde_json::from_str(json).map_err(|e| Error::Invalid(e.to_string()))
}

impl<T: Driver> bindings::exports::breezone::module::driver::GuestSource for Adapter<T> {
    fn open(config: String, secret: Option<String>) -> Result<bindings::exports::breezone::module::driver::Source> {
        // A secret that is not JSON (a key on its own) comes as a string.
        let secret = secret.map(|s| serde_json::from_str(&s).unwrap_or(Value::String(s)));
        Ok(bindings::exports::breezone::module::driver::Source::new(Adapter(T::open(parsed(&config)?, secret)?)))
    }

    fn describe(&self) -> Result<Described> {
        self.0.describe()
    }

    fn read(&self) -> Result<Vec<(String, String)>> {
        Ok(self.0.read()?.into_iter().map(|(r, s)| (r, Value::Object(s).to_string())).collect())
    }

    fn act(&self, reference: String, trait_name: String, action: String, params: String) -> Result<()> {
        let params = match parsed(&params)? {
            Value::Object(m) => m,
            _ => return Err(Error::Invalid("the parameters are not an object".into())),
        };
        self.0.act(&reference, &trait_name, &action, &params)
    }

    fn on_message(&self, subscription: u32, data: Vec<u8>) -> Result<()> {
        self.0.on_message(subscription, &data)
    }

    fn diagnostic(&self) -> Result<String> {
        Ok(self.0.diagnostic()?.to_string())
    }

    fn rooms(&self) -> Result<Vec<HardwareRoom>> {
        self.0.rooms()
    }

    fn scenes(&self) -> Result<Vec<Scene>> {
        self.0.scenes()
    }

    fn activate_scene(&self, id: String) -> Result<()> {
        self.0.activate_scene(&id)
    }

    fn picture(&self, reference: String) -> Result<Picture> {
        self.0.picture(&reference)
    }

    fn forecast(&self, reference: String) -> Result<Vec<Curve>> {
        self.0.forecast(&reference)
    }

    fn unlock(&self, key: String) -> Result<bool> {
        self.0.unlock(&key)
    }

    fn schedules(&self) -> Result<Vec<Schedule>> {
        self.0.schedules()
    }

    fn save_schedule(&self, schedule: Schedule) -> Result<String> {
        self.0.save_schedule(schedule)
    }

    fn delete_schedule(&self, id: String) -> Result<()> {
        self.0.delete_schedule(&id)
    }

    fn close(&self) {
        self.0.close()
    }
}

/// Makes the WebAssembly component of a module: `module!(Plug)` for a driver, `module!(service:
/// Tempo)` for a service, `module!(driver: Plug, service: Tempo)` for both.
#[macro_export]
macro_rules! module {
    (driver: $driver:ty, service: $service:ty) => {
        struct __BreezoneModule;

        impl $crate::bindings::exports::breezone::module::driver::Guest for __BreezoneModule {
            type Source = $crate::Adapter<$driver>;

            fn probe(host: String) -> Option<String> {
                <$driver as $crate::Driver>::probe(&host)
            }

            fn pair(step: String, input: String) -> $crate::Result<$crate::Paired> {
                let input = $crate::serde_json::from_str(&input).map_err(|e| $crate::Error::Invalid(e.to_string()))?;
                <$driver as $crate::Driver>::pair(&step, input)
            }
        }

        impl $crate::bindings::exports::breezone::module::extras::Guest for __BreezoneModule {
            fn forget(config: String, secret: Option<String>) -> $crate::Result<()> {
                let config = $crate::serde_json::from_str(&config).map_err(|e| $crate::Error::Invalid(e.to_string()))?;
                let secret = secret.map(|s| $crate::serde_json::from_str(&s).unwrap_or($crate::Value::String(s)));
                <$driver as $crate::Driver>::forget(config, secret)
            }

            fn identify(host: String, announced: Option<String>) -> Option<String> {
                let announced: Option<$crate::Value> = announced.and_then(|a| $crate::serde_json::from_str(&a).ok());
                <$driver as $crate::Driver>::identify(&host, announced.as_ref()).map(|f| f.to_json().to_string())
            }

            fn history(config: String, secret: Option<String>, reference: String, since: i64, until: i64) -> $crate::Result<String> {
                let config = $crate::serde_json::from_str(&config).map_err(|e| $crate::Error::Invalid(e.to_string()))?;
                let secret = secret.map(|s| $crate::serde_json::from_str(&s).unwrap_or($crate::Value::String(s)));
                Ok(<$driver as $crate::Driver>::history(config, secret, &reference, since, until)?.to_json().to_string())
            }

            fn rename(config: String, secret: Option<String>, reference: String, name: String) -> $crate::Result<()> {
                let config = $crate::serde_json::from_str(&config).map_err(|e| $crate::Error::Invalid(e.to_string()))?;
                let secret = secret.map(|s| $crate::serde_json::from_str(&s).unwrap_or($crate::Value::String(s)));
                <$driver as $crate::Driver>::rename(config, secret, &reference, &name)
            }
        }

        impl $crate::bindings::exports::breezone::module::service::Guest for __BreezoneModule {
            fn check() -> $crate::Result<()> {
                <$service as $crate::Service>::check()
            }

            fn query(contract: String, query: String, params: String) -> $crate::Result<String> {
                let params = $crate::serde_json::from_str(&params).map_err(|e| $crate::Error::Invalid(e.to_string()))?;
                Ok(<$service as $crate::Service>::query(&contract, &query, params)?.to_string())
            }
        }

        // Only a WebAssembly component has these exports; a native build (the tests) has none.
        #[cfg(target_arch = "wasm32")]
        $crate::bindings::export_bindings!(__BreezoneModule with_types_in $crate::bindings);
    };
    (service: $service:ty) => {
        $crate::module!(driver: $crate::NoDriver, service: $service);
    };
    ($driver:ty) => {
        $crate::module!(driver: $driver, service: $crate::NoService);
    };
}

/// What a module asks of Breezone. In WebAssembly, the host; in a native test, the
/// [`testing`] bench.
pub mod host {
    use super::{Error, Header, Result};

    /// "debug" | "info" | "warn" | "error"
    pub fn log(level: &str, message: &str) {
        imp::log(level, message)
    }

    /// Unix time, in seconds.
    pub fn now() -> i64 {
        imp::now()
    }

    /// Waits without computing (a device that wants time between two requests): 10 s at most.
    pub fn wait(ms: u32) {
        imp::wait(ms)
    }

    /// An HTTP answer.
    #[derive(Debug, Clone)]
    pub struct Answer {
        pub status: u16,
        pub headers: Vec<Header>,
        pub body: Vec<u8>,
    }

    impl Answer {
        /// The body as JSON.
        pub fn json(&self) -> Result<serde_json::Value> {
            serde_json::from_slice(&self.body).map_err(|e| Error::Invalid(format!("not JSON: {e}")))
        }
    }

    /// An HTTP request (refused by Breezone unless the module's permissions allow its address).
    pub fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>, timeout_ms: u32) -> Result<Answer> {
        imp::request(method, url, headers, body, timeout_ms, false).map_err(Error::Unreachable)
    }

    /// The same, accepting the device's own HTTPS certificate (a camera's): Breezone allows it on
    /// the local network only.
    pub fn request_insecure(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>, timeout_ms: u32) -> Result<Answer> {
        imp::request(method, url, headers, body, timeout_ms, true).map_err(Error::Unreachable)
    }

    /// A question and its answer on TCP (`tcp://host:port`): writes `data`, reads until `until`.
    pub fn exchange(url: &str, data: &[u8], until: super::ReadUntil, timeout_ms: u32) -> Result<Vec<u8>> {
        imp::exchange(url, data, until, timeout_ms).map_err(Error::Unreachable)
    }

    /// `GET url`, its answer as JSON.
    pub fn get_json(url: &str, timeout_ms: u32) -> Result<(u16, serde_json::Value)> {
        let a = request("GET", url, &[], None, timeout_ms)?;
        Ok((a.status, a.json()?))
    }

    /// A WebSocket (`ws://`, `wss://`: a device of the local network may have its own certificate) or TCP stream (`tcp://host:port`) kept open by Breezone: each message
    /// comes to [`super::Driver::on_message`] with the number returned.
    pub fn subscribe(url: &str) -> Result<u32> {
        imp::subscribe(url).map_err(Error::Unreachable)
    }

    /// The same, for a WebSocket that wants headers when it opens (an `Authorization` computed
    /// from a challenge), its frames sent as binary when `binary`. Interface 1.3:
    /// `"requires": {"api": "^1.3"}`.
    pub fn subscribe_with(url: &str, headers: &[(&str, &str)], binary: bool) -> Result<u32> {
        imp::subscribe_with(url, headers, binary).map_err(Error::Unreachable)
    }

    pub fn send(subscription: u32, data: &[u8]) -> Result<()> {
        imp::send(subscription, data).map_err(Error::Unreachable)
    }

    /// The next message of a subscription, waited for during a call (a handshake, a listing): 30 s
    /// at most. What comes between calls still goes to [`super::Driver::on_message`].
    pub fn receive(subscription: u32, timeout_ms: u32) -> Result<Vec<u8>> {
        imp::receive(subscription, timeout_ms).map_err(Error::Unreachable)
    }

    pub fn unsubscribe(subscription: u32) {
        imp::unsubscribe(subscription)
    }

    /// The store of this source (small values, kept across restarts).
    pub fn store_get(key: &str) -> Option<Vec<u8>> {
        imp::store_get(key)
    }

    pub fn store_set(key: &str, value: Option<&[u8]>) {
        imp::store_set(key, value)
    }

    /// The secret of an account the manifest requires, as JSON.
    pub fn account(kind: &str) -> Result<serde_json::Value> {
        let s = imp::account(kind).map_err(Error::Unsupported)?;
        serde_json::from_str(&s).map_err(|e| Error::Invalid(e.to_string()))
    }

    /// An access to the service of the module's own OAuth account (`oauth` of its manifest):
    /// Breezone renews it when it is about to end and keeps the new refresh token.
    pub fn access_token() -> Result<String> {
        imp::access_token().map_err(Error::Unreachable)
    }

    /// Keeps fields in the module's own account (a session; `Null` removes one). While the
    /// account is checked, it may change what was typed (a password kept as a hash); once kept,
    /// never the fields of its form.
    pub fn account_update(fields: &serde_json::Value) -> Result<()> {
        imp::account_update(&fields.to_string()).map_err(Error::Invalid)
    }

    /// A button of a remote was pressed: "single" | "double" | "long".
    pub fn pressed(reference: &str, button: u32, press: &str) {
        imp::pressed(reference, button, press)
    }

    /// Asks Breezone to read this source again now (the hardware said something changed).
    pub fn changed() {
        imp::changed()
    }

    #[cfg(target_arch = "wasm32")]
    mod imp {
        use crate::bindings::breezone::module::host as h;
        use crate::bindings::breezone::module::types::Request;

        pub fn log(level: &str, message: &str) {
            let level = match level {
                "debug" => h::Level::Debug,
                "warn" => h::Level::Warn,
                "error" => h::Level::Error,
                _ => h::Level::Info,
            };
            h::log(level, message)
        }

        pub fn now() -> i64 {
            h::now()
        }

        pub fn wait(ms: u32) {
            h::wait(ms)
        }

        pub fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>, timeout_ms: u32, insecure: bool) -> Result<super::Answer, String> {
            let headers = headers.iter().map(|(n, v)| super::Header { name: n.to_string(), value: v.to_string() }).collect();
            let r = h::http(&Request { method: method.into(), url: url.into(), headers, body: body.map(<[u8]>::to_vec), timeout_ms, insecure })?;
            Ok(super::Answer { status: r.status, headers: r.headers, body: r.body })
        }

        pub fn exchange(url: &str, data: &[u8], until: crate::ReadUntil, timeout_ms: u32) -> Result<Vec<u8>, String> {
            h::exchange(url, data, until, timeout_ms)
        }

        pub fn subscribe(url: &str) -> Result<u32, String> {
            h::subscribe(url)
        }

        pub fn subscribe_with(url: &str, headers: &[(&str, &str)], binary: bool) -> Result<u32, String> {
            let headers: Vec<super::Header> = headers.iter().map(|(n, v)| super::Header { name: n.to_string(), value: v.to_string() }).collect();
            h::subscribe_with(url, &headers, binary)
        }

        pub fn send(s: u32, data: &[u8]) -> Result<(), String> {
            h::send(s, data)
        }

        pub fn receive(s: u32, timeout_ms: u32) -> Result<Vec<u8>, String> {
            h::receive(s, timeout_ms)
        }

        pub fn unsubscribe(s: u32) {
            h::unsubscribe(s)
        }

        pub fn store_get(key: &str) -> Option<Vec<u8>> {
            h::store_get(key)
        }

        pub fn store_set(key: &str, value: Option<&[u8]>) {
            h::store_set(key, value)
        }

        pub fn account(kind: &str) -> Result<String, String> {
            h::account(kind)
        }

        pub fn access_token() -> Result<String, String> {
            h::access_token()
        }

        pub fn account_update(fields: &str) -> Result<(), String> {
            h::account_update(fields)
        }

        pub fn pressed(reference: &str, button: u32, press: &str) {
            h::pressed(reference, button, press)
        }

        pub fn changed() {
            h::changed()
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod imp {
        pub use crate::testing::host::*;
    }
}

/// The test bench: a module's [`Driver`] runs natively (`cargo test`), its HTTP requests answered
/// by recorded answers, and what it reads checked against the catalog of traits and its manifest.
#[cfg(not(target_arch = "wasm32"))]
pub mod testing {
    use super::{Driver, State, Value};
    use std::cell::RefCell;
    use std::collections::HashMap;

    type Responder = std::rc::Rc<dyn Fn(&[(&str, &str)], &[u8]) -> crate::host::Answer>;
    /// A fake device on a subscription: the pieces it sends back for what it receives.
    type Device = std::rc::Rc<dyn Fn(u32, &[u8]) -> Vec<Vec<u8>>>;

    #[derive(Default)]
    struct Bench {
        answers: Vec<(String, String, u16, Vec<u8>)>,
        responders: Vec<(String, String, Responder)>,
        exchanges: Vec<(String, Vec<u8>)>,
        queued: std::collections::VecDeque<Vec<u8>>,
        waited: u64,
        requests: Vec<String>,
        sent: Vec<(u32, Vec<u8>)>,
        presses: Vec<(String, u32, String)>,
        changed: u32,
        store: HashMap<String, Vec<u8>>,
        accounts: HashMap<String, String>,
        access_token: Option<String>,
        logs: Vec<(String, String)>,
        subscriptions: u32,
        /// What each subscription will receive, in order (`incoming`), and an answer computed from
        /// what is sent (`answer_sent`).
        incoming: std::collections::VecDeque<(u32, Vec<u8>)>,
        on_sent: Option<Device>,
    }

    thread_local! {
        static BENCH: RefCell<Bench> = RefCell::new(Bench::default());
    }

    fn with<T>(f: impl FnOnce(&mut Bench) -> T) -> T {
        BENCH.with(|b| f(&mut b.borrow_mut()))
    }

    /// Forgets everything of the bench (answers, requests, store…).
    pub fn reset() {
        with(|b| *b = Bench::default());
    }

    /// Answers the requests `method` whose address contains `url` (the last given wins).
    pub fn answer(method: &str, url: &str, status: u16, body: impl Into<Vec<u8>>) {
        with(|b| b.answers.push((method.to_string(), url.to_string(), status, body.into())));
    }

    /// Answers the requests `method` whose address contains `url` with what `f` makes of their
    /// headers and body (a device that computes its answer), before the answers of [`answer`].
    pub fn respond(method: &str, url: &str, f: impl Fn(&[(&str, &str)], &[u8]) -> crate::host::Answer + 'static) {
        with(|b| b.responders.push((method.to_string(), url.to_string(), std::rc::Rc::new(f))));
    }

    /// How long the module asked to wait, in all (milliseconds).
    pub fn waited_ms() -> u64 {
        with(|b| b.waited)
    }

    /// Answers the TCP exchanges whose address contains `url` (the last given wins).
    pub fn answer_exchange(url: &str, answer: impl Into<Vec<u8>>) {
        with(|b| b.exchanges.push((url.to_string(), answer.into())));
    }

    /// Answers the next TCP exchanges with these, in this order (before `answer_exchange`).
    pub fn queue_exchanges(answers: Vec<Vec<u8>>) {
        with(|b| b.queued.extend(answers));
    }

    /// Answers with JSON.
    pub fn answer_json(method: &str, url: &str, body: &Value) {
        answer(method, url, 200, body.to_string());
    }

    /// The requests made, as « METHOD url ».
    pub fn requests() -> Vec<String> {
        with(|b| b.requests.clone())
    }

    /// What was sent on subscriptions.
    pub fn sent() -> Vec<(u32, Vec<u8>)> {
        with(|b| b.sent.clone())
    }

    /// The presses told: (reference, button, press).
    pub fn presses() -> Vec<(String, u32, String)> {
        with(|b| b.presses.clone())
    }

    /// How many times the module asked to be read again.
    pub fn changed_count() -> u32 {
        with(|b| b.changed)
    }

    /// The secret of an account, as the module will get it.
    pub fn account(kind: &str, secret: &Value) {
        with(|b| b.accounts.insert(kind.to_string(), secret.to_string()));
    }

    /// Bytes a subscription receives (read by `host::receive`, in order).
    pub fn incoming(subscription: u32, data: impl Into<Vec<u8>>) {
        with(|b| b.incoming.push_back((subscription, data.into())));
    }

    /// A device that answers what is sent on a subscription: each piece `f` returns is received
    /// after it (a fake device on a TCP stream).
    pub fn answer_sent(f: impl Fn(u32, &[u8]) -> Vec<Vec<u8>> + 'static) {
        with(|b| b.on_sent = Some(std::rc::Rc::new(f)));
    }

    /// The access `host::access_token` gives (none: the account is not connected).
    pub fn access_token(token: &str) {
        with(|b| b.access_token = Some(token.to_string()));
    }

    /// What the module wrote to its log: (level, message).
    pub fn logs() -> Vec<(String, String)> {
        with(|b| b.logs.clone())
    }

    /// Opens a source, as Breezone would.
    pub fn open<T: Driver>(config: Value, secret: Option<Value>) -> T {
        T::open(config, secret).unwrap_or_else(|e| panic!("open: {e:?}"))
    }

    /// The traits of the catalog (`server/contracts/traits`, or the folder `BREEZONE_TRAITS_DIR`).
    fn catalog() -> Vec<Value> {
        let dir = std::env::var("BREEZONE_TRAITS_DIR").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../server/contracts/traits").to_string());
        let mut out = vec![];
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{dir}: {e} (set BREEZONE_TRAITS_DIR)")).flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap();
            out.push(serde_json::from_str(&text).unwrap());
        }
        out
    }

    fn type_ok(kind: &str, value: &Value, field: &Value) -> bool {
        match kind {
            "bool" => value.is_boolean(),
            "integer" => value.as_i64().is_some() || value.as_f64().is_some_and(|f| f.fract() == 0.0),
            "number" => value.is_number(),
            "string" => value.is_string(),
            "enum" => value.as_str().is_some_and(|v| field["values"].as_array().is_some_and(|vs| vs.iter().any(|x| x == v))),
            _ => false,
        }
    }

    /// Checks what a source says against the catalog and the module's manifest (its
    /// `manifest.json`, as text): each device's traits are traits the module provides; each value
    /// of its state is a field of one of its traits, of the right type and within bounds; each
    /// field the traits require is there. Returns the state read, to look at it further.
    pub fn check<T: Driver>(source: &T, manifest: &str) -> HashMap<String, State> {
        let manifest: Value = serde_json::from_str(manifest).expect("manifest.json");
        // `trait@major.revision` in the manifest; `trait@major` on a device.
        let provides: Vec<String> = manifest["provides"]["traits"].as_array().into_iter().flatten().filter_map(|t| t.as_str()).map(str::to_string).collect();
        let provided =
            |t: &str| provides.iter().any(|p| p.split('@').next() == t.split('@').next() && p.split('@').nth(1).and_then(|v| v.split('.').next()) == t.split('@').nth(1));
        let catalog = catalog();
        let described = source.describe().unwrap_or_else(|e| panic!("describe: {e:?}"));
        let states: HashMap<String, State> = source.read().unwrap_or_else(|e| panic!("read: {e:?}")).into_iter().collect();
        for d in &described.devices {
            let mut fields: Vec<&Value> = vec![];
            for t in &d.traits {
                assert!(provided(t), "{}: {t} is not in provides.traits of the manifest", d.reference);
                let (name, major) = t.split_once('@').unwrap_or_else(|| panic!("{}: {t} is not trait@version", d.reference));
                let def = catalog
                    .iter()
                    .find(|c| c["trait"] == name && c["version"].as_u64().map(|v| v.to_string()).as_deref() == Some(major))
                    .unwrap_or_else(|| panic!("{}: {t} is not in the catalog of traits", d.reference));
                fields.extend(def["state"].as_array().into_iter().flatten());
            }
            let Some(state) = states.get(&d.reference) else { continue };
            for (k, v) in state {
                let field = fields.iter().find(|f| f["name"] == k.as_str()).unwrap_or_else(|| panic!("{}: {k} is a field of none of its traits", d.reference));
                if v.is_null() {
                    assert!(field["optional"] == true, "{}: {k} is null but not optional", d.reference);
                    continue;
                }
                let kind = field["type"].as_str().unwrap_or_default();
                assert!(type_ok(kind, v, field), "{}: {k} = {v} is not a {kind}", d.reference);
                if let Some(x) = v.as_f64() {
                    if let Some(min) = field["min"].as_f64() {
                        assert!(x >= min, "{}: {k} = {x} < {min}", d.reference);
                    }
                    if let Some(max) = field["max"].as_f64() {
                        assert!(x <= max, "{}: {k} = {x} > {max}", d.reference);
                    }
                }
            }
            for f in fields.iter().filter(|f| f["optional"] != true) {
                let name = f["name"].as_str().unwrap_or_default();
                assert!(state.contains_key(name), "{}: {name} is required by its traits but missing", d.reference);
            }
        }
        states
    }

    /// The host's functions, as the bench answers them.
    pub mod host {
        use super::with;

        pub fn log(level: &str, message: &str) {
            with(|b| b.logs.push((level.to_string(), message.to_string())));
        }

        pub fn now() -> i64 {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
        }

        /// The bench does not wait: it counts.
        pub fn wait(ms: u32) {
            with(|b| b.waited += u64::from(ms));
        }

        pub fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>, _timeout_ms: u32, _insecure: bool) -> Result<crate::host::Answer, String> {
            let responder = with(|b| b.responders.iter().rev().find(|(m, u, _)| m == method && url.contains(u.as_str())).map(|(.., f)| f.clone()));
            if let Some(f) = responder {
                with(|b| b.requests.push(format!("{method} {url}")));
                return Ok(f(headers, body.unwrap_or_default()));
            }
            with(|b| {
                b.requests.push(format!("{method} {url}"));
                b.answers
                    .iter()
                    .rev()
                    .find(|(m, u, ..)| m == method && url.contains(u.as_str()))
                    .map(|(_, _, status, body)| crate::host::Answer { status: *status, headers: vec![], body: body.clone() })
                    .ok_or_else(|| format!("no answer for {method} {url} on the bench"))
            })
        }

        pub fn exchange(url: &str, data: &[u8], _until: crate::ReadUntil, _timeout_ms: u32) -> Result<Vec<u8>, String> {
            with(|b| {
                b.requests.push(format!("EXCHANGE {url} {}", data.len()));
                if let Some(next) = b.queued.pop_front() {
                    return Ok(next);
                }
                b.exchanges.iter().rev().find(|(u, _)| url.contains(u.as_str())).map(|(_, answer)| answer.clone()).ok_or_else(|| format!("no answer for {url} on the bench"))
            })
        }

        pub fn subscribe(_url: &str) -> Result<u32, String> {
            Ok(with(|b| {
                b.subscriptions += 1;
                b.subscriptions
            }))
        }

        pub fn subscribe_with(url: &str, headers: &[(&str, &str)], _binary: bool) -> Result<u32, String> {
            with(|b| {
                let names: Vec<&str> = headers.iter().map(|(n, _)| *n).collect();
                b.requests.push(format!("SUBSCRIBE {url} {}", names.join(",")));
            });
            subscribe(url)
        }

        pub fn send(s: u32, data: &[u8]) -> Result<(), String> {
            let answer = with(|b| {
                b.sent.push((s, data.to_vec()));
                b.on_sent.clone()
            });
            if let Some(f) = answer {
                let pieces = f(s, data);
                with(|b| b.incoming.extend(pieces.into_iter().map(|p| (s, p))));
            }
            Ok(())
        }

        pub fn receive(s: u32, _timeout_ms: u32) -> Result<Vec<u8>, String> {
            with(|b| {
                let at = b.incoming.iter().position(|(sub, _)| *sub == s).ok_or_else(|| "timeout".to_string())?;
                Ok(b.incoming.remove(at).map(|(_, d)| d).unwrap_or_default())
            })
        }

        pub fn unsubscribe(_s: u32) {}

        pub fn store_get(key: &str) -> Option<Vec<u8>> {
            with(|b| b.store.get(key).cloned())
        }

        pub fn store_set(key: &str, value: Option<&[u8]>) {
            with(|b| match value {
                Some(v) => b.store.insert(key.to_string(), v.to_vec()),
                None => b.store.remove(key),
            });
        }

        pub fn account(kind: &str) -> Result<String, String> {
            with(|b| b.accounts.get(kind).cloned()).ok_or_else(|| format!("{kind}: no account on the bench"))
        }

        pub fn access_token() -> Result<String, String> {
            with(|b| b.access_token.clone()).ok_or_else(|| "no OAuth account on the bench".to_string())
        }

        /// Merges into the accounts of the bench (all of them: the bench does not know whose).
        pub fn account_update(fields: &str) -> Result<(), String> {
            let update: serde_json::Map<String, serde_json::Value> = serde_json::from_str(fields).map_err(|e| e.to_string())?;
            with(|b| {
                for secret in b.accounts.values_mut() {
                    let mut current: serde_json::Map<String, serde_json::Value> = serde_json::from_str(secret).unwrap_or_default();
                    for (k, v) in &update {
                        if v.is_null() {
                            current.remove(k);
                        } else {
                            current.insert(k.clone(), v.clone());
                        }
                    }
                    *secret = serde_json::Value::Object(current).to_string();
                }
            });
            Ok(())
        }

        pub fn pressed(reference: &str, button: u32, press: &str) {
            with(|b| b.presses.push((reference.to_string(), button, press.to_string())));
        }

        pub fn changed() {
            with(|b| b.changed += 1);
        }
    }
}
