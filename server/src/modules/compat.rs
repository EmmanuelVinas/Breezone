//! Whether a module fits a server (docs/roadmap/modules.md § 4.4), decided here only: the library
//! sorts and explains with it, an installation is refused with it, a module that no longer fits is
//! switched off at start with it, and an update of Breezone is checked with it beforehand.

use super::manifest::{api_version, Manifest, Needed};
use crate::capabilities::Host;
use serde::Serialize;

/// Why a module does not fit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Incompatibility {
    /// It needs another version of the module interface.
    Api {
        required: String,
        hosted: Vec<String>,
    },
    /// It needs another version of Breezone.
    Breezone {
        required: String,
        running: String,
    },
    /// A trait is missing, or only an older revision of it.
    Trait {
        name: String,
        major: u32,
        revision: u32,
        hosted: Option<u32>,
    },
    Service {
        name: String,
        major: u32,
        revision: u32,
        hosted: Option<u32>,
    },
    /// An account module it needs cannot be installed.
    Account {
        id: String,
    },
}

impl std::fmt::Display for Incompatibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Api { required, hosted } => write!(f, "needs the module interface {required} (this server has {})", hosted.join(", ")),
            Self::Breezone { required, running } => write!(f, "needs Breezone {required} (this is {running})"),
            Self::Trait { name, major, revision, hosted: Some(h) } => write!(f, "needs {name}@{major}.{revision} (this server has {name}@{major}.{h})"),
            Self::Trait { name, major, revision, hosted: None } => write!(f, "needs {name}@{major}.{revision}, unknown here"),
            Self::Service { name, major, revision, hosted: Some(h) } => write!(f, "needs the service {name}@{major}.{revision} (this server has {name}@{major}.{h})"),
            Self::Service { name, major, revision, hosted: None } => write!(f, "needs the service {name}@{major}.{revision}, unknown here"),
            Self::Account { id } => write!(f, "needs the {id} account, which cannot be installed"),
        }
    }
}

/// The version of Breezone as requirements read it: a pre-release counts as its version (a
/// 0.15.0-beta.2 fits `>=0.15.0`), so that the beta channel gets the modules of the coming release.
pub fn breezone_version(v: &str) -> Option<semver::Version> {
    let mut v = semver::Version::parse(v.trim().trim_start_matches('v')).ok()?;
    v.pre = semver::Prerelease::EMPTY;
    Some(v)
}

/// Every reason `m` does not fit a server hosting `host`, running Breezone `running`;
/// `account_fits` tells whether an account module it needs can be installed.
pub fn check(m: &Manifest, host: &Host, running: &str, account_fits: impl Fn(&str) -> bool) -> Result<(), Vec<Incompatibility>> {
    let mut out = vec![];
    match semver::VersionReq::parse(&m.requires.api) {
        Ok(req) if host.api.iter().filter_map(|v| api_version(v)).any(|v| req.matches(&v)) => {}
        _ => out.push(Incompatibility::Api { required: m.requires.api.clone(), hosted: host.api.clone() }),
    }
    if let Some(b) = &m.requires.breezone {
        let fits = semver::VersionReq::parse(b).ok().zip(breezone_version(running)).is_some_and(|(req, v)| req.matches(&v));
        if !fits {
            out.push(Incompatibility::Breezone { required: b.clone(), running: running.to_string() });
        }
    }
    for t in m.provides.traits.iter().filter_map(|t| Needed::parse(t)) {
        // An extension trait is the module's own: the server only shows it.
        if t.is_extension() {
            continue;
        }
        let hosted = host.trait_revision(&t.name, t.major);
        if hosted.is_none_or(|h| h < t.revision) {
            out.push(Incompatibility::Trait { name: t.name, major: t.major, revision: t.revision, hosted });
        }
    }
    for s in m.provides.services.iter().filter_map(|s| Needed::parse(s)) {
        let hosted = host.service_revision(&s.name, s.major);
        if hosted.is_none_or(|h| h < s.revision) {
            out.push(Incompatibility::Service { name: s.name, major: s.major, revision: s.revision, hosted });
        }
    }
    for a in &m.requires.accounts {
        if !account_fits(a) {
            out.push(Incompatibility::Account { id: a.clone() });
        }
    }
    if out.is_empty() {
        Ok(())
    } else {
        Err(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::manifest::example;

    fn host() -> Host {
        Host { api: vec!["1.0".into()], ..Host::local() }
    }

    #[test]
    fn the_example_fits_this_server() {
        assert_eq!(check(&example(), &host(), "0.13.0", |_| true), Ok(()));
        // A beta of the version asked fits; an older version does not.
        assert_eq!(check(&example(), &host(), "0.13.0-beta.1", |_| true), Ok(()));
        assert!(matches!(check(&example(), &host(), "0.12.4", |_| true).unwrap_err()[..], [Incompatibility::Breezone { .. }]));
    }

    #[test]
    fn each_reason_is_told() {
        let mut m = example();
        m.requires.api = "^2.0".into();
        m.requires.accounts = vec!["netatmo".into()];
        m.provides.traits = vec!["switch.relay@1.9".into(), "pool.filtration@1".into(), "x.shelly.thing@1".into()];
        m.provides.services = vec!["tariff.day_color@2".into()];
        let errors = check(&m, &host(), "0.13.0", |a| a != "netatmo").unwrap_err();
        let kinds: Vec<String> = errors.iter().map(|e| serde_json::to_value(e).unwrap()["kind"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, ["api", "trait", "trait", "service", "account"], "{errors:#?}");
        assert!(errors[1].to_string().contains("switch.relay@1.9"), "{}", errors[1]);
    }

    #[test]
    fn a_minor_of_the_interface_fits_a_newer_host() {
        let mut h = host();
        h.api = vec!["1.3".into()];
        let mut m = example();
        m.requires.api = "^1.1".into();
        assert_eq!(check(&m, &h, "0.13.0", |_| true), Ok(()));
        h.api = vec!["1.0".into()];
        assert!(check(&m, &h, "0.13.0", |_| true).is_err());
    }
}
