//! The modules still compiled into the server (docs/roadmap/modules.md, lot L2): each has a
//! manifest (`native/manifests/<id>.json`) like a module of the library, and here what only
//! compiled code can give — how its sources are made, how it is found on the network, what is done
//! when a source of it is removed. This file and the modules' own files are the only ones of the
//! server that name a module; the rest goes through the manifests.

use super::manifest::Manifest;
use crate::db::Db;
use crate::discovery::Product;
use crate::drivers::{self, Driver};
use crate::error::ApiError;
use axum::http::StatusCode;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::net::Ipv4Addr;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// What a module that reaches its hardware through a linked account needs to be made.
pub struct Context {
    pub db: Db,
    pub key: [u8; 32],
    /// The address of the maker's service, by module (tests point it elsewhere).
    pub urls: HashMap<String, String>,
}

pub type MakeDriver = fn(&Value, Option<&str>) -> Result<Arc<dyn Driver>, String>;
pub type Probe = fn(String) -> BoxFuture<Vec<Product>>;
pub type Browse = fn(Vec<Ipv4Addr>) -> BoxFuture<Vec<Product>>;
pub type OnDelete = fn(Value, Option<String>) -> BoxFuture<()>;

pub enum Create {
    /// From the configuration of the source and its secret (decrypted).
    Config(MakeDriver),
    /// From its linked account: the source is made when the account is connected.
    Account(fn(&Context) -> Arc<dyn Driver>),
    /// No source: an account or a service only.
    None,
}

/// Where an appairage stands after a step.
pub enum Paired {
    /// Waiting for the next step (the code shown on the accessory…).
    Next,
    /// Done: the source to add, with its secret and the name to give it if none is given.
    Done { config: Value, secret: String, name: &'static str },
}

pub type PairStep = fn(String, Value) -> BoxFuture<Result<Paired, ApiError>>;

pub struct Native {
    pub manifest: Manifest,
    /// The steps of its appairage, for a `pairing` other than `form`: (step, its input).
    pub pair: Option<PairStep>,
    pub create: Create,
    /// Asks an address whether it is this module's hardware (one of `discovery.tcp` is open).
    pub probe: Option<Probe>,
    /// Finds this module's hardware by mDNS, among `hosts` (and on the whole link).
    pub browse: Option<Browse>,
    /// Done before a source of it is removed (its configuration, its secret).
    pub on_delete: Option<OnDelete>,
}

fn manifest(id: &str, json: &str) -> Manifest {
    Manifest::parse(json).unwrap_or_else(|e| panic!("modules/native/manifests/{id}.json: {}", e.join("; ")))
}

fn config<D: Driver + 'static>(made: Result<D, String>) -> Result<Arc<dyn Driver>, String> {
    made.map(|d| Arc::new(d) as Arc<dyn Driver>)
}

fn url(cx: &Context, id: &str) -> String {
    cx.urls.get(id).cloned().unwrap_or_default()
}

/// A product found at `host` by a probe.
fn product(driver: &'static str, brand: &'static str, host: &str, port: u16, model: String) -> Product {
    Product { brand, driver, model, host: host.to_string(), port, mac: None, device_id: None, firmware: None, note: None, supported: true, flags: 0, install: None }
}

macro_rules! native {
    ($id:literal) => {
        manifest($id, include_str!(concat!("native/manifests/", $id, ".json")))
    };
}

/// In the order of the lists (sources, then accounts and services).
static NATIVE: LazyLock<Vec<Native>> = LazyLock::new(|| {
    vec![
        Native {
            pair: None,
            manifest: native!("shelly"),
            create: Create::Config(|c, _| config(drivers::shelly::Shelly::new(c))),
            probe: Some(|host| {
                Box::pin(async move {
                    let Some(s) = drivers::shelly::probe(&host).await else { return vec![] };
                    vec![Product {
                        mac: s.mac,
                        device_id: Some(s.id),
                        firmware: s.firmware,
                        note: s.note,
                        supported: s.supported,
                        ..product("shelly", "shelly", &host, 80, s.model)
                    }]
                })
            }),
            browse: None,
            on_delete: None,
        },
        Native {
            pair: None,
            manifest: native!("trydan"),
            create: Create::Config(|c, _| config(drivers::trydan::Trydan::new(c))),
            probe: Some(|host| {
                Box::pin(async move {
                    let Some(t) = drivers::trydan::probe(&host).await else { return vec![] };
                    vec![Product { device_id: (!t.id.is_empty()).then_some(t.id), firmware: t.firmware, note: t.note, ..product("trydan", "v2c", &host, 80, "V2C Trydan".into()) }]
                })
            }),
            browse: None,
            on_delete: None,
        },
        Native {
            pair: None,
            manifest: native!("hoymiles"),
            create: Create::Config(|c, _| config(drivers::hoymiles::Hoymiles::new(c))),
            probe: Some(|host| {
                Box::pin(async move {
                    let Some(h) = drivers::hoymiles::probe(&host).await else { return vec![] };
                    let note = Some(h.note());
                    vec![Product {
                        note,
                        supported: !h.encrypted,
                        device_id: Some(h.serial),
                        firmware: h.firmware,
                        ..product("hoymiles", "hoymiles", &host, drivers::hoymiles::PORT, h.model)
                    }]
                })
            }),
            browse: None,
            on_delete: None,
        },
        Native {
            pair: Some(|step, input| Box::pin(pair_homekit(step, input))),
            manifest: native!("homekit"),
            create: Create::Config(|c, s| config(drivers::homekit::HomeKit::new(c, s))),
            probe: None,
            browse: Some(|hosts| Box::pin(async move { homekit_products(crate::homekit::discovery::browse(std::time::Duration::from_secs(2), &hosts).await) })),
            // Breezone removes its own pairing from the accessory (and nothing else), so that it
            // can be paired again.
            on_delete: Some(|config, secret| {
                Box::pin(async move {
                    let pairing = secret.and_then(|s| serde_json::from_str::<crate::homekit::Pairing>(&s).ok());
                    if let (Some(pairing), Some(host)) = (pairing, config["host"].as_str()) {
                        let removed = tokio::time::timeout(std::time::Duration::from_secs(15), crate::homekit::remove_own_pairing(host, &pairing)).await;
                        if !matches!(removed, Ok(Ok(()))) {
                            tracing::warn!("cannot remove Breezone's pairing from {host}: {removed:?}; the accessory may still list it");
                        }
                    }
                })
            }),
        },
        Native {
            pair: Some(|step, input| Box::pin(pair_hue(step, input))),
            manifest: native!("hue"),
            create: Create::Config(|c, s| config(drivers::hue::Hue::new(c, s))),
            probe: Some(|host| {
                Box::pin(async move {
                    let Some(b) = drivers::hue::probe(&host).await else { return vec![] };
                    let model = format!("Philips Hue {}", if b.model.is_empty() { "Bridge".to_string() } else { format!("Bridge ({})", b.model) });
                    let note = Some(match b.api {
                        Some(api) => format!("« {} » · API {api} · appuyez sur son bouton pour l'ajouter", b.name),
                        None => format!("« {} » · appuyez sur son bouton pour l'ajouter", b.name),
                    });
                    vec![Product { device_id: Some(b.id), firmware: b.firmware, note, ..product("hue", "hue", &host, 80, model) }]
                })
            }),
            browse: None,
            on_delete: None,
        },
        Native {
            pair: None,
            manifest: native!("renson"),
            create: Create::Config(|c, _| config(drivers::renson::Renson::new(c))),
            probe: Some(|host| {
                Box::pin(async move {
                    let Some(r) = drivers::renson::probe(&host).await else { return vec![] };
                    let note = Some(format!("{} bouches · API locale", r.rooms));
                    vec![Product { note, device_id: (!r.serial.is_empty()).then_some(r.serial), ..product("renson", "renson", &host, 80, r.model) }]
                })
            }),
            browse: None,
            on_delete: None,
        },
        Native { pair: None, manifest: native!("rte"), create: Create::None, probe: None, browse: None, on_delete: None },
        Native { pair: None, manifest: native!("linky"), create: Create::None, probe: None, browse: None, on_delete: None },
        Native {
            pair: None,
            manifest: native!("netatmo"),
            create: Create::Account(|cx| Arc::new(drivers::netatmo::Netatmo::new(cx.db.clone(), cx.key, url(cx, "netatmo")))),
            probe: None,
            browse: None,
            on_delete: None,
        },
        Native {
            pair: None,
            manifest: native!("ezviz"),
            create: Create::Account(|cx| Arc::new(drivers::ezviz::Ezviz::new(cx.db.clone(), cx.key, url(cx, "ezviz")))),
            probe: None,
            browse: None,
            on_delete: None,
        },
        Native { pair: None, manifest: native!("weather"), create: Create::None, probe: None, browse: None, on_delete: None },
        Native {
            pair: None,
            manifest: native!("forecast_solar"),
            create: Create::Config(|c, s| config(drivers::forecast_solar::ForecastSolar::new(c, s))),
            probe: None,
            browse: None,
            on_delete: None,
        },
        Native {
            pair: None,
            manifest: native!("tasmota"),
            create: Create::Config(|c, s| config(drivers::tasmota::Tasmota::new(c, s))),
            probe: Some(|host| {
                Box::pin(async move {
                    let Some(name) = drivers::tasmota::probe(&host).await else { return vec![] };
                    vec![Product { note: Some(format!("« {name} »")), ..product("tasmota", "tasmota", &host, 80, "Tasmota".into()) }]
                })
            }),
            browse: None,
            on_delete: None,
        },
        Native {
            pair: None,
            manifest: native!("reolink"),
            create: Create::Config(|c, s| config(drivers::reolink::Reolink::new(c, s))),
            probe: Some(|host| {
                Box::pin(async move {
                    if !drivers::reolink::probe(&host).await {
                        return vec![];
                    }
                    vec![Product { note: Some("identifiant et mot de passe de la caméra".into()), ..product("reolink", "reolink", &host, 80, "Reolink".into()) }]
                })
            }),
            browse: None,
            on_delete: None,
        },
        Native {
            pair: None,
            manifest: native!("tplink"),
            create: Create::Config(|c, s| config(drivers::tplink::TpLink::new(c, s))),
            probe: Some(|host| {
                Box::pin(async move {
                    let Some((model, alias)) = drivers::tplink::probe(&host).await else { return vec![] };
                    let note = (!alias.is_empty()).then(|| format!("« {alias} »"));
                    vec![Product { note, ..product("tplink", "tplink", &host, drivers::tplink::KASA_PORT, format!("TP-Link Kasa {model}")) }]
                })
            }),
            browse: None,
            on_delete: None,
        },
        Native {
            pair: None,
            manifest: native!("esphome"),
            create: Create::Config(|c, s| config(drivers::esphome::EspHome::new(c, s))),
            probe: Some(|host| {
                Box::pin(async move {
                    let Some(name) = drivers::esphome::probe(&host).await else { return vec![] };
                    let note = match name {
                        Some(n) => format!("« {n} »"),
                        None => "chiffré : clé de l'API requise".into(),
                    };
                    vec![Product { note: Some(note), ..product("esphome", "esphome", &host, drivers::esphome::PORT, "ESPHome".into()) }]
                })
            }),
            browse: None,
            on_delete: None,
        },
    ]
});

/// An address of the local network as the scanner gives it ("ip" or "ip:port").
fn host_of(input: &Value) -> Result<String, ApiError> {
    let host = input["host"].as_str().unwrap_or_default().trim();
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')) {
        return Err(ApiError::bad_request("invalid address"));
    }
    Ok(host.to_string())
}

/// A Hue bridge gives Breezone an access key in the 30 seconds after its button is pressed (as for
/// the Hue app): one step, `link`.
async fn pair_hue(step: String, input: Value) -> Result<Paired, ApiError> {
    if step != "link" {
        return Err(ApiError::bad_request(format!("unknown step {step}")));
    }
    let host = host_of(&input)?;
    let (bridge, key) = drivers::hue::link(&host).await.map_err(|e| match e {
        drivers::hue::LinkError::ButtonNotPressed => ApiError::new(
            StatusCode::CONFLICT,
            crate::text!(
                "Appuyez sur le bouton rond du pont Hue, puis cliquez sur « Ajouter » dans les 30 secondes.",
                "Press the round button of the Hue bridge, then click « Add » within 30 seconds.",
                "Pulse el botón redondo del puente Hue y haga clic en «Añadir» en 30 segundos."
            ),
        )
        .with_detail("code", "button"),
        drivers::hue::LinkError::Other(m) => {
            ApiError::new(StatusCode::BAD_GATEWAY, crate::text!("Le pont Hue ne répond pas : {0}", "The Hue bridge does not answer: {0}", "El puente Hue no responde: {0}", m))
        }
    })?;
    Ok(Paired::Done { config: serde_json::json!({"host": host, "device_id": bridge.id}), secret: key, name: "Philips Hue" })
}

/// HomeKit pairings started, by accessory address, until their code is typed.
static STARTED: LazyLock<tokio::sync::Mutex<HashMap<String, (std::time::Instant, crate::homekit::PairSetup)>>> = LazyLock::new(Default::default);
const STARTED_FOR: std::time::Duration = std::time::Duration::from_secs(120);

/// A HomeKit accessory: `start` (one with a screen shows its code now), then `finish` with its
/// setup code (8 digits), or `cancel`. Nothing else changes on the accessory: what its own app set
/// up stays as it is.
async fn pair_homekit(step: String, input: Value) -> Result<Paired, ApiError> {
    let host = host_of(&input)?;
    let apple = input["flags"].as_u64().unwrap_or(0) & 1 == 1;
    match step.as_str() {
        "start" => {
            // A pairing left half-done keeps the accessory busy: the previous one is dropped first.
            STARTED.lock().await.retain(|h, (at, _)| *h != host && at.elapsed() < STARTED_FOR);
            let setup = crate::homekit::PairSetup::start(&host, apple).await.map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.explain()))?;
            let at = std::time::Instant::now();
            STARTED.lock().await.insert(host.clone(), (at, setup));
            // Never typed: the accessory is freed, else it refuses any other pairing.
            tokio::spawn(async move {
                tokio::time::sleep(STARTED_FOR).await;
                let mut started = STARTED.lock().await;
                if started.get(&host).is_some_and(|(t, _)| *t == at) {
                    started.remove(&host);
                }
            });
            Ok(Paired::Next)
        }
        "cancel" => {
            STARTED.lock().await.remove(&host);
            Ok(Paired::Next)
        }
        "finish" => {
            let code = crate::homekit::setup_code(input["code"].as_str().unwrap_or_default()).ok_or_else(|| {
                ApiError::bad_request(crate::text!(
                    "Le code HomeKit a 8 chiffres (par exemple 123-45-678).",
                    "The HomeKit code has 8 digits (for example 123-45-678).",
                    "El código HomeKit tiene 8 cifras (por ejemplo 123-45-678)."
                ))
            })?;
            let started = STARTED.lock().await.remove(&host).filter(|(at, _)| at.elapsed() < STARTED_FOR);
            let paired = match started {
                Some((_, setup)) => setup.finish(&code).await,
                None => crate::homekit::pair_setup(&host, &code, apple).await,
            };
            let pairing = paired.map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.explain()))?;
            let config = serde_json::json!({"host": host, "device_id": pairing.accessory_id});
            Ok(Paired::Done { config, secret: serde_json::to_string(&pairing).unwrap_or_default(), name: "HomeKit" })
        }
        other => Err(ApiError::bad_request(format!("unknown step {other}"))),
    }
}

/// What the HomeKit accessories announced are, as products to add.
fn homekit_products(announced: Vec<crate::homekit::discovery::Announced>) -> Vec<Product> {
    let port = |host: &str| host.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let mut found = vec![];
    for a in announced {
        let kind = match a.category {
            Some(2) => "pont",
            Some(14) => "volet",
            Some(15) => "fenêtre",
            Some(7) => "prise",
            Some(5) => "lumière",
            _ => "accessoire",
        };
        let model = a.model.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| a.name.clone());
        // The Legrand Ecometer gives HomeKit nothing it measures: its lines come from the cloud.
        if format!("{model} {}", a.name).to_lowercase().contains("ecometer") {
            found.push(Product {
                device_id: Some(a.id),
                note: Some("ses mesures ne passent pas par HomeKit : elles viennent du compte Netatmo".into()),
                supported: false,
                flags: a.flags,
                ..product("netatmo", "netatmo", &a.host, port(&a.host), model)
            });
            continue;
        }
        let note = Some(if a.paired {
            format!("HomeKit · {kind} « {} » · déjà appairé à un autre contrôleur", a.name)
        } else {
            format!("HomeKit · {kind} « {} » · à appairer avec son code", a.name)
        });
        found.push(Product { device_id: Some(a.id), note, supported: !a.paired, flags: a.flags, ..product("homekit", "homekit", &a.host, port(&a.host), model) });
    }
    found
}

pub fn all() -> &'static [Native] {
    &NATIVE
}

pub fn get(id: &str) -> Option<&'static Native> {
    all().iter().find(|n| n.manifest.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_manifest_file_is_a_native_module() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules/native/manifests");
        let on_disk: HashSet<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().trim_end_matches(".json").to_string()).collect();
        let native: HashSet<String> = all().iter().map(|n| n.manifest.id.clone()).collect();
        assert_eq!(on_disk, native);
    }

    #[test]
    fn native_modules_fit_this_server() {
        // Compiled in, they need no module host: only their traits and services are checked.
        let host = crate::capabilities::Host { api: vec!["1.0".into()], ..crate::capabilities::Host::local() };
        for n in all() {
            let m = &n.manifest;
            assert_eq!(super::super::compat::check(m, &host, env!("CARGO_PKG_VERSION"), |_| true), Ok(()), "{}", m.id);
            let driver = m.has(super::super::manifest::Role::Driver);
            assert_eq!(driver, !matches!(n.create, Create::None), "{}: a driver makes sources, and only a driver", m.id);
            assert_eq!(n.probe.is_some(), !m.discovery.tcp.is_empty(), "{}: a probe is asked on the ports of discovery.tcp", m.id);
            assert_eq!(n.browse.is_some(), !m.discovery.mdns.is_empty() && n.probe.is_none(), "{}: browsed by mDNS", m.id);
            assert_eq!(n.pair.is_some(), !matches!(m.pairing, super::super::manifest::Pairing::Form | super::super::manifest::Pairing::Oauth), "{}: pairing steps", m.id);
        }
    }

    #[test]
    fn the_shelly_models_are_those_of_the_driver() {
        assert_eq!(get("shelly").unwrap().manifest.models, drivers::shelly::models());
    }
}
