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

pub use bindings::breezone::module::types::{Curve, Described, DeviceInfo, Error, HardwareRoom, Header, Picture, Scene};
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
    /// Ends the connections it keeps, before the server stops.
    fn close(&self) {}
    /// Whether this address is hardware of this module, and its name (the network scan).
    fn probe(_host: &str) -> Option<String> {
        None
    }
}

/// A [`Driver`] as the interface wants it (made by [`module!`]).
pub struct Adapter<T>(pub T);

fn parsed(json: &str) -> Result<Value> {
    serde_json::from_str(json).map_err(|e| Error::Invalid(e.to_string()))
}

impl<T: Driver> bindings::exports::breezone::module::driver::GuestSource for Adapter<T> {
    fn open(config: String, secret: Option<String>) -> Result<bindings::exports::breezone::module::driver::Source> {
        let secret = secret.map(|s| parsed(&s)).transpose()?;
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

    fn close(&self) {
        self.0.close()
    }
}

/// Makes the WebAssembly component of a module from its [`Driver`].
#[macro_export]
macro_rules! module {
    ($driver:ty) => {
        struct __BreezoneModule;

        impl $crate::bindings::exports::breezone::module::driver::Guest for __BreezoneModule {
            type Source = $crate::Adapter<$driver>;

            fn probe(host: String) -> Option<String> {
                <$driver as $crate::Driver>::probe(&host)
            }
        }

        // Only a WebAssembly component has these exports; a native build (the tests) has none.
        #[cfg(target_arch = "wasm32")]
        $crate::bindings::export_bindings!(__BreezoneModule with_types_in $crate::bindings);
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
        imp::request(method, url, headers, body, timeout_ms).map_err(Error::Unreachable)
    }

    /// `GET url`, its answer as JSON.
    pub fn get_json(url: &str, timeout_ms: u32) -> Result<(u16, serde_json::Value)> {
        let a = request("GET", url, &[], None, timeout_ms)?;
        Ok((a.status, a.json()?))
    }

    /// A WebSocket (`ws://`) or TCP stream (`tcp://host:port`) kept open by Breezone: each message
    /// comes to [`super::Driver::on_message`] with the number returned.
    pub fn subscribe(url: &str) -> Result<u32> {
        imp::subscribe(url).map_err(Error::Unreachable)
    }

    pub fn send(subscription: u32, data: &[u8]) -> Result<()> {
        imp::send(subscription, data).map_err(Error::Unreachable)
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

        pub fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>, timeout_ms: u32) -> Result<super::Answer, String> {
            let headers = headers.iter().map(|(n, v)| super::Header { name: n.to_string(), value: v.to_string() }).collect();
            let r = h::http(&Request { method: method.into(), url: url.into(), headers, body: body.map(<[u8]>::to_vec), timeout_ms })?;
            Ok(super::Answer { status: r.status, headers: r.headers, body: r.body })
        }

        pub fn subscribe(url: &str) -> Result<u32, String> {
            h::subscribe(url)
        }

        pub fn send(s: u32, data: &[u8]) -> Result<(), String> {
            h::send(s, data)
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

    #[derive(Default)]
    struct Bench {
        answers: Vec<(String, String, u16, Vec<u8>)>,
        requests: Vec<String>,
        sent: Vec<(u32, Vec<u8>)>,
        presses: Vec<(String, u32, String)>,
        changed: u32,
        store: HashMap<String, Vec<u8>>,
        accounts: HashMap<String, String>,
        logs: Vec<(String, String)>,
        subscriptions: u32,
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

        pub fn request(method: &str, url: &str, _headers: &[(&str, &str)], _body: Option<&[u8]>, _timeout_ms: u32) -> Result<crate::host::Answer, String> {
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

        pub fn subscribe(_url: &str) -> Result<u32, String> {
            Ok(with(|b| {
                b.subscriptions += 1;
                b.subscriptions
            }))
        }

        pub fn send(s: u32, data: &[u8]) -> Result<(), String> {
            with(|b| b.sent.push((s, data.to_vec())));
            Ok(())
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

        pub fn pressed(reference: &str, button: u32, press: &str) {
            with(|b| b.presses.push((reference.to_string(), button, press.to_string())));
        }

        pub fn changed() {
            with(|b| b.changed += 1);
        }
    }
}
