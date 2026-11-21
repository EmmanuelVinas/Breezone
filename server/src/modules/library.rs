//! The library of modules (docs/roadmap/modules.md § 4.9): an index of every module published and
//! of its versions, signed with the key of the releases (`breezone-modules-index <seq>\n<sha256>\n`)
//! and published next to the packages, at an address compiled into the server. `seq` grows with
//! each publication: an index older than the last one seen is refused, so that nobody can bring
//! back a list holding a version since withdrawn.

use super::compat::{self, Incompatibility};
use super::manifest::Manifest;
use super::package::verify_signature;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The release of the official repository that holds the index and the packages.
pub const RELEASE_TAG: &str = "modules";

/// Where the library's files are downloaded from: always the official repository.
pub fn file_url(file: &str) -> String {
    format!("https://github.com/{}/releases/download/{RELEASE_TAG}/{file}", crate::update::REPOSITORY)
}

/// The setting that keeps the `seq` of the last index accepted.
pub const SEQ_SETTING: &str = "modules.index_seq";

/// Largest index accepted.
pub const MAX_INDEX: usize = 4 << 20;

pub fn signed_message(seq: u64, index: &[u8]) -> Vec<u8> {
    format!("breezone-modules-index {seq}\n{}\n", hex::encode(Sha256::digest(index))).into_bytes()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub seq: u64,
    pub modules: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// Newest first.
    pub versions: Vec<Published>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Published {
    /// The manifest of this version: what it needs and gives.
    pub manifest: Manifest,
    /// Its package (`<id>-<version>.bzm`, signature `.bzm.sig`), next to the index.
    pub file: String,
    pub size: u64,
    pub sha256: String,
    pub published_at: i64,
    /// Withdrawn (a flaw): never installed again, the next one is proposed.
    #[serde(default)]
    pub yanked: bool,
    /// Its package holds a component (else it is only data, and needs no module host).
    #[serde(default = "crate::yes")]
    pub code: bool,
}

impl Published {
    /// Why it does not fit a server hosting `host`.
    pub fn check(&self, host: &crate::capabilities::Host, running: &str, account_fits: &dyn Fn(&str) -> bool) -> Result<(), Vec<Incompatibility>> {
        let mut host = host.clone();
        if !self.code {
            host.api = vec![super::INTERFACE.to_string()];
        }
        compat::check(&self.manifest, &host, running, account_fits)
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum IndexError {
    #[error("the signature does not match this index")]
    Signature,
    #[error("the index is not readable: {0}")]
    Unreadable(String),
    #[error("index {got} is older than the last one seen ({seen})")]
    Older { got: u64, seen: u64 },
}

impl Index {
    /// Reads an index signed by `signature` with `public_key`, not older than `seen`.
    pub fn open(bytes: &[u8], signature: &str, public_key: &str, seen: Option<u64>) -> Result<Index, IndexError> {
        if bytes.len() > MAX_INDEX {
            return Err(IndexError::Unreadable("too large".into()));
        }
        let index: Index = serde_json::from_slice(bytes).map_err(|e| IndexError::Unreadable(e.to_string()))?;
        if !verify_signature(public_key, &signed_message(index.seq, bytes), signature) {
            return Err(IndexError::Signature);
        }
        if let Some(seen) = seen.filter(|s| index.seq < *s) {
            return Err(IndexError::Older { got: index.seq, seen });
        }
        // A version whose manifest is wrong is left out rather than the whole index.
        let mut index = index;
        for e in &mut index.modules {
            e.versions.retain(|v| v.manifest.validate().is_ok() && v.manifest.id == e.id);
        }
        Ok(index)
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.modules.iter().find(|e| e.id == id)
    }
}

/// A module of the library as the settings show it.
#[derive(Debug, Clone, Serialize)]
pub struct LibraryView {
    pub id: String,
    /// The manifest of its newest version.
    pub manifest: Manifest,
    /// The version installed here, if any.
    pub installed: Option<String>,
    /// The newest version that fits this server, if any.
    pub fits: Option<String>,
    /// Why the newest version does not fit, when it does not.
    pub reasons: Vec<Incompatibility>,
}

impl Entry {
    /// The newest version not withdrawn that fits.
    pub fn best(&self, host: &crate::capabilities::Host, running: &str, account_fits: &dyn Fn(&str) -> bool) -> Option<&Published> {
        self.versions.iter().filter(|v| !v.yanked).find(|v| v.check(host, running, account_fits).is_ok())
    }

    pub fn version(&self, version: &str) -> Option<&Published> {
        self.versions.iter().find(|v| v.manifest.version == version)
    }
}

/// The library sorted for this server: the modules that fit first, then the others with why.
pub fn view(index: &Index, host: &crate::capabilities::Host, running: &str, installed: &dyn Fn(&str) -> Option<String>) -> Vec<LibraryView> {
    let account_fits = |a: &str| installed(a).is_some() || index.entry(a).is_some();
    let mut out: Vec<LibraryView> = index
        .modules
        .iter()
        .filter_map(|e| {
            let newest = e.versions.iter().find(|v| !v.yanked)?;
            let fits = e.best(host, running, &account_fits).map(|v| v.manifest.version.clone());
            let reasons = newest.check(host, running, &account_fits).err().unwrap_or_default();
            Some(LibraryView { id: e.id.clone(), manifest: newest.manifest.clone(), installed: installed(&e.id), fits, reasons })
        })
        .collect();
    out.sort_by_key(|v| (v.fits.is_none(), v.manifest.name.fr.to_lowercase()));
    out
}

/// Reads the library and the packages, keeping the index of the day.
pub struct Library {
    /// Address of the folder of the files (`None`: the official repository).
    base: Option<String>,
    public_key: String,
    cache: tokio::sync::Mutex<Option<(std::time::Instant, Index)>>,
}

/// The index is read again after this long.
const KEEP_INDEX: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

impl Library {
    pub fn new(cfg: &crate::config::Config) -> Self {
        let (base, public_key) = match &cfg.modules_library {
            Some((base, key)) => (Some(base.trim_end_matches('/').to_string()), key.clone()),
            None => (None, crate::update::RELEASE_PUBLIC_KEY.to_string()),
        };
        Self { base, public_key, cache: tokio::sync::Mutex::new(None) }
    }

    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    fn url(&self, file: &str) -> String {
        match &self.base {
            Some(base) => format!("{base}/{file}"),
            None => file_url(file),
        }
    }

    /// Downloads a file of the library: from the official repository only (HTTPS on GitHub's
    /// hosts, redirections included), `max` bytes at most.
    pub async fn fetch(&self, file: &str, max: usize) -> Result<Vec<u8>, String> {
        self.fetch_url(&self.url(file), max).await
    }

    async fn fetch_url(&self, url: &str, max: usize) -> Result<Vec<u8>, String> {
        let file = url.rsplit('/').next().unwrap_or(url).to_string();
        let official = self.base.is_none();
        let policy = reqwest::redirect::Policy::custom(move |a| if !official || crate::update::allowed_url(a.url()) { a.follow() } else { a.stop() });
        let http = reqwest::Client::builder().redirect(policy).timeout(std::time::Duration::from_secs(60)).build().map_err(|e| e.to_string())?;
        let mut resp = http.get(url).send().await.map_err(|e| e.to_string())?;
        if official && !crate::update::allowed_url(resp.url()) {
            return Err("the library answered from an address that is not allowed".into());
        }
        if !resp.status().is_success() {
            return Err(format!("{file}: {}", resp.status()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            if bytes.len() + chunk.len() > max {
                return Err(format!("{file}: too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    /// A file published with a release of Breezone (`host.json`).
    pub async fn fetch_release_file(&self, version: crate::update::Version, file: &str, max: usize) -> Result<Vec<u8>, String> {
        match &self.base {
            // The tests serve it beside the library.
            Some(_) => self.fetch(&format!("{version}-{file}"), max).await,
            None => self.fetch_url(&crate::update::download_url(version, file), max).await,
        }
    }

    /// The index, read again once a day (or when `fresh`); the `seq` accepted is kept in `db`.
    pub async fn index(&self, db: &crate::db::Db, fresh: bool) -> Result<Index, String> {
        let mut cache = self.cache.lock().await;
        if let Some((at, index)) = cache.as_ref().filter(|(at, _)| !fresh && at.elapsed() < KEEP_INDEX) {
            let _ = at;
            return Ok(index.clone());
        }
        let bytes = self.fetch("index.json", MAX_INDEX).await?;
        let signature = self.fetch("index.json.sig", 1024).await?;
        let seen = crate::db::get_setting(db, SEQ_SETTING).await.map_err(|e| e.to_string())?.and_then(|s| s.parse().ok());
        let index = Index::open(&bytes, &String::from_utf8_lossy(&signature), &self.public_key, seen).map_err(|e| e.to_string())?;
        crate::db::set_setting(db, SEQ_SETTING, &index.seq.to_string()).await.map_err(|e| e.to_string())?;
        *cache = Some((std::time::Instant::now(), index.clone()));
        Ok(index)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::modules::package::tests::{keys, sign};

    pub(crate) fn published(version: &str, api: &str) -> Published {
        let mut manifest = crate::modules::manifest::example();
        manifest.version = version.into();
        manifest.requires.api = api.into();
        manifest.requires.breezone = None;
        Published { file: format!("shelly-{version}.bzm"), size: 10, sha256: String::new(), published_at: 0, yanked: false, code: true, manifest }
    }

    pub(crate) fn index(seq: u64, versions: Vec<Published>) -> Vec<u8> {
        serde_json::to_vec(&Index { seq, modules: vec![Entry { id: "shelly".into(), versions }] }).unwrap()
    }

    #[test]
    fn a_signed_index_newer_than_the_last_is_read() {
        let (key, public) = keys();
        let bytes = index(5, vec![published("1.4.0", "^1.0")]);
        let sig = sign(&key, &signed_message(5, &bytes));
        assert_eq!(Index::open(&bytes, &sig, &public, Some(5)).unwrap().seq, 5);
        assert_eq!(Index::open(&bytes, &sig, &public, Some(6)).unwrap_err(), IndexError::Older { got: 5, seen: 6 });
        // Signed for another number: refused.
        assert_eq!(Index::open(&bytes, &sign(&key, &signed_message(7, &bytes)), &public, None).unwrap_err(), IndexError::Signature);
    }

    #[test]
    fn the_newest_version_that_fits_is_proposed() {
        let mut host = crate::capabilities::Host::local();
        host.api = vec!["1.0".into()];
        let mut withdrawn = published("1.5.0", "^1.0");
        withdrawn.yanked = true;
        let i: Index = serde_json::from_slice(&index(1, vec![published("2.0.0", "^2.0"), withdrawn, published("1.4.0", "^1.0")])).unwrap();
        let v = view(&i, &host, "0.13.0", &|_| None);
        assert_eq!(v[0].fits.as_deref(), Some("1.4.0"));
        assert_eq!(v[0].manifest.version, "2.0.0");
        assert!(matches!(v[0].reasons[..], [Incompatibility::Api { .. }]));
    }
}
