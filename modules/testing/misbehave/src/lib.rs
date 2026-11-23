//! Misbehaves as its configuration says (`"panic"`, `"loop"`, `"memory"`, `"http:<url>"`), at each
//! `read`; tries the host as it says (`"sub:<url>"`: a subscription whose messages it keeps, presses
//! and announces; `"store"`: a counter in the store of the source; `"account:<kind>"`: the secret
//! of an account); otherwise answers one device.

// The world without `extras`: a module that exports none of them loads.
wit_bindgen::generate!({
    path: "../../../server/contracts/wit",
    world: "core",
});

use breezone::module::host;
use breezone::module::types::{Curve, Described, DeviceInfo, Error, HardwareRoom, LengthPrefix, Paired, PairedSource, Picture, ReadUntil, Request, Scene, Schedule};
use exports::breezone::module::driver::{Guest, GuestSource, Source as SourceHandle};

struct Module;

pub struct Source {
    mode: String,
    subscription: Option<u32>,
    last: std::cell::RefCell<Option<String>>,
    /// `schedules`: a plug whose schedules are kept here, one of them its app's own.
    schedules: std::cell::RefCell<Vec<Schedule>>,
}

impl GuestSource for Source {
    fn open(config: String, _secret: Option<String>) -> Result<SourceHandle, Error> {
        let mode = config.trim_matches('"').to_string();
        let subscription = match mode.strip_prefix("sub:") {
            Some(url) => Some(host::subscribe(url).map_err(Error::Unreachable)?),
            None => None,
        };
        let schedules = vec![Schedule {
            id: "app".into(),
            name: "App".into(),
            enabled: true,
            days: vec![6, 7],
            time: "09:00".into(),
            references: vec!["plug".into()],
            trait_name: "switch.relay".into(),
            action: "set".into(),
            params: r#"{"on": true}"#.into(),
            end: None,
            editable: false,
            note: Some("in the app".into()),
        }];
        Ok(SourceHandle::new(Source { mode, subscription, last: Default::default(), schedules: std::cell::RefCell::new(schedules) }))
    }

    fn describe(&self) -> Result<Described, Error> {
        let devices = match self.mode.as_str() {
            "schedules" => vec![DeviceInfo { reference: "plug".into(), kind: "switch.relay".into(), traits: vec!["switch.relay@1".into()], name: "Plug".into() }],
            _ => vec![],
        };
        Ok(Described { model: "misbehave".into(), firmware: None, devices, identity: None })
    }

    fn read(&self) -> Result<Vec<(String, String)>, Error> {
        match self.mode.as_str() {
            "schedules" => Ok(vec![("plug".into(), r#"{"on": false}"#.into())]),
            "panic" => panic!("on purpose"),
            "loop" => {
                let mut n: u64 = 0;
                loop {
                    n = std::hint::black_box(n.wrapping_add(1));
                    if n == u64::MAX {
                        break;
                    }
                }
                Ok(vec![])
            }
            "memory" => {
                let mut v: Vec<Vec<u8>> = vec![];
                loop {
                    v.push(std::hint::black_box(vec![1u8; 1 << 20]));
                    if v.len() > 100_000 {
                        break Ok(vec![]);
                    }
                }
            }
            m if m.starts_with("sub:") => {
                if let Some(s) = self.subscription {
                    host::send(s, b"ping").map_err(Error::Unreachable)?;
                }
                let last = self.last.borrow().clone().unwrap_or_default();
                Ok(vec![("sub".into(), format!("{{\"last\":{last:?}}}"))])
            }
            "store" => {
                let n = host::store_get("n").map(|v| u32::from_le_bytes(v.try_into().unwrap_or([0; 4]))).unwrap_or(0) + 1;
                host::store_set("n", Some(&n.to_le_bytes()));
                Ok(vec![("store".into(), format!("{{\"n\":{n}}}"))])
            }
            m if m.starts_with("exchange:") => {
                // A 4-byte length first, not counting itself (as TP-Link Kasa does).
                let until = ReadUntil::LengthPrefixed(LengthPrefix { offset: 0, size: 4, big_endian: true, adds: 4 });
                let answer = host::exchange(&m[9..], b"ping", until, 3000).map_err(Error::Unreachable)?;
                Ok(vec![("exchange".into(), format!("{{\"length\":{},\"text\":{:?}}}", answer.len(), String::from_utf8_lossy(&answer[4..])))])
            }
            m if m.starts_with("account:") => {
                let secret = host::account(&m[8..]).map_err(Error::Unsupported)?;
                Ok(vec![("account".into(), format!("{{\"length\":{}}}", secret.len()))])
            }
            m if m.starts_with("http:") => {
                let r = host::http(&Request { method: "GET".into(), url: m[5..].into(), headers: vec![], body: None, timeout_ms: 2000, insecure: false })
                    .map_err(Error::Unreachable)?;
                Ok(vec![("http".into(), format!("{{\"status\":{}}}", r.status))])
            }
            _ => Ok(vec![("ok".into(), "{\"on\":true}".into())]),
        }
    }

    fn act(&self, _reference: String, _trait_name: String, _action: String, _params: String) -> Result<(), Error> {
        Err(Error::Unsupported("nothing".into()))
    }

    fn diagnostic(&self) -> Result<String, Error> {
        Ok("null".into())
    }

    fn on_message(&self, _subscription: u32, data: Vec<u8>) -> Result<(), Error> {
        *self.last.borrow_mut() = Some(String::from_utf8_lossy(&data).into_owned());
        host::pressed("remote", 1, "double");
        host::changed();
        Ok(())
    }

    fn rooms(&self) -> Result<Vec<HardwareRoom>, Error> {
        Err(Error::Unsupported("rooms".into()))
    }

    fn scenes(&self) -> Result<Vec<Scene>, Error> {
        Err(Error::Unsupported("scenes".into()))
    }

    fn activate_scene(&self, _id: String) -> Result<(), Error> {
        Err(Error::Unsupported("scenes".into()))
    }

    fn picture(&self, _reference: String) -> Result<Picture, Error> {
        Err(Error::Unsupported("picture".into()))
    }

    fn forecast(&self, _reference: String) -> Result<Vec<Curve>, Error> {
        Err(Error::Unsupported("forecast".into()))
    }

    fn unlock(&self, _key: String) -> Result<bool, Error> {
        Err(Error::Unsupported("unlock".into()))
    }

    fn schedules(&self) -> Result<Vec<Schedule>, Error> {
        Ok(self.schedules.borrow().clone())
    }

    fn save_schedule(&self, mut schedule: Schedule) -> Result<String, Error> {
        let mut all = self.schedules.borrow_mut();
        if schedule.id.is_empty() {
            schedule.id = format!("s{}", all.len());
        }
        schedule.editable = true;
        let id = schedule.id.clone();
        match all.iter_mut().find(|s| s.id == id) {
            Some(s) => *s = schedule,
            None => all.push(schedule),
        }
        Ok(id)
    }

    fn delete_schedule(&self, id: String) -> Result<(), Error> {
        self.schedules.borrow_mut().retain(|s| s.id != id);
        Ok(())
    }

    fn close(&self) {}
}

impl Guest for Module {
    type Source = Source;

    fn probe(_host: String) -> Option<String> {
        None
    }

    /// `link`: waits for its button, unless the input says it was pressed.
    fn pair(step: String, input: String) -> Result<Paired, Error> {
        match step.as_str() {
            "link" if input.contains("pressed") => Ok(Paired::Done(PairedSource { config: r#""store""#.into(), secret: "key".into(), name: "Misbehave".into() })),
            "link" => Ok(Paired::Waiting("press the button".into())),
            _ => Err(Error::Unsupported(step)),
        }
    }
}

/// `echo`: the parameters back, with the account of the module when there is one; `bad`: an answer
/// that is not what the contract says.
impl exports::breezone::module::service::Guest for Module {
    fn check() -> Result<(), Error> {
        let account = host::account("misbehave").map_err(Error::Unsupported)?;
        if account.contains("wrong") {
            return Err(Error::Invalid("refused".into()));
        }
        // An OAuth account: an access is asked (Breezone renews it).
        if account.contains("refresh_token") {
            host::access_token().map_err(Error::Unreachable)?;
        }
        Ok(())
    }

    fn query(contract: String, query: String, params: String) -> Result<String, Error> {
        match (contract.as_str(), query.as_str()) {
            // Keeps a session in its account; a field of the form is refused.
            ("tariff.day_color@1", "colors") if params.contains("update") => {
                let kept = host::account_update(r#"{"session": "s1"}"#).is_ok();
                let refused = host::account_update(r#"{"token": "changed"}"#).is_err();
                let session = host::account("misbehave").unwrap_or_default().contains("s1");
                let color = if kept && refused && session { "white" } else { "blue" };
                Ok(format!(r#"[{{"day": "2026-11-22", "color": "{color}"}}]"#))
            }
            ("tariff.day_color@1", "colors") if params.contains("bad") => Ok(r#"[{"day": "2026-11-22", "color": "green"}]"#.into()),
            ("tariff.day_color@1", "colors") => {
                let account = host::account("misbehave").unwrap_or_default();
                let color = if account.contains("red") { "red" } else { "blue" };
                Ok(format!(r#"[{{"day": "2026-11-22", "color": "{color}"}}]"#))
            }
            // What the automations may read: tomorrow's colour, and a field it does not declare.
            ("conditions@1", "read") => Ok(r#"{"tomorrow": "red", "days_left": 12, "secret": "x"}"#.into()),
            _ => Err(Error::Unsupported(format!("{contract} {query}"))),
        }
    }
}

// Only a WebAssembly component has these exports.
#[cfg(target_arch = "wasm32")]
export!(Module);
