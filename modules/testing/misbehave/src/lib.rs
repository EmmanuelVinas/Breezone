//! Misbehaves as its configuration says (`"panic"`, `"loop"`, `"memory"`, `"http:<url>"`), at each
//! `read`; otherwise answers one device.

wit_bindgen::generate!({
    path: "../../../server/contracts/wit",
    world: "module",
});

use breezone::module::host;
use breezone::module::types::{Described, Error, Request};
use exports::breezone::module::driver::{Guest, GuestSource, Source as SourceHandle};

struct Module;

pub struct Source {
    mode: String,
}

impl GuestSource for Source {
    fn open(config: String, _secret: Option<String>) -> Result<SourceHandle, Error> {
        Ok(SourceHandle::new(Source { mode: config.trim_matches('"').to_string() }))
    }

    fn describe(&self) -> Result<Described, Error> {
        Ok(Described { model: "misbehave".into(), firmware: None, devices: vec![], identity: None })
    }

    fn read(&self) -> Result<Vec<(String, String)>, Error> {
        match self.mode.as_str() {
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
            m if m.starts_with("http:") => {
                let r = host::http(&Request { method: "GET".into(), url: m[5..].into(), headers: vec![], body: None, timeout_ms: 2000 }).map_err(Error::Unreachable)?;
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
}

impl Guest for Module {
    type Source = Source;

    fn probe(_host: String) -> Option<String> {
        None
    }
}

export!(Module);
