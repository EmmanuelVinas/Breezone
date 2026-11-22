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
    /// The source it comes from.
    pub repository: RepositoryRef,
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

/// The source of a module of the library, as the settings show it.
#[derive(Debug, Clone, Serialize)]
pub struct RepositoryRef {
    pub id: i64,
    pub name: String,
    pub official: bool,
}

/// The library sorted for this server: the modules that fit first, then the others with why.
pub fn view(repo: &RepositoryRef, index: &Index, host: &crate::capabilities::Host, running: &str, installed: &dyn Fn(&str) -> Option<String>) -> Vec<LibraryView> {
    let account_fits = |a: &str| installed(a).is_some() || index.entry(a).is_some();
    let mut out: Vec<LibraryView> = index
        .modules
        .iter()
        .filter_map(|e| {
            let newest = e.versions.iter().find(|v| !v.yanked)?;
            let fits = e.best(host, running, &account_fits).map(|v| v.manifest.version.clone());
            let reasons = newest.check(host, running, &account_fits).err().unwrap_or_default();
            Some(LibraryView { id: e.id.clone(), repository: repo.clone(), manifest: newest.manifest.clone(), installed: installed(&e.id), fits, reasons })
        })
        .collect();
    out.sort_by_key(|v| (v.fits.is_none(), v.manifest.name.fr.to_lowercase()));
    out
}

/// A source of modules, as the server reads it: the official library, or one an administrator
/// added (a developer's, a community's), with the key that signs its index and its packages.
#[derive(Debug, Clone, Serialize)]
pub struct Repository {
    pub id: i64,
    pub name: String,
    /// The folder of its files (`None`: the official repository, its address compiled in).
    pub url: Option<String>,
    #[serde(skip)]
    pub public_key: String,
    pub official: bool,
    pub enabled: bool,
}

/// Reads the sources of modules and their packages, keeping each index for the day.
pub struct Library {
    /// The official library: the address of its folder (`None`: the release `modules` of the
    /// official repository) and its key (the key of the releases); the tests point elsewhere.
    official_base: Option<String>,
    official_key: String,
    cache: tokio::sync::Mutex<std::collections::HashMap<i64, (std::time::Instant, Index)>>,
}

/// An index is read again after this long.
const KEEP_INDEX: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// Whether the address of a source of modules may be used: HTTPS, or HTTP to an address of the
/// local network (a developer's own machine), without credentials.
pub fn repository_url_ok(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    if !u.username().is_empty() || u.password().is_some() || u.host_str().is_none() {
        return false;
    }
    match u.scheme() {
        "https" => true,
        "http" => {
            let host = u.host_str().unwrap_or_default().trim_start_matches('[').trim_end_matches(']');
            match host.parse::<std::net::IpAddr>() {
                Ok(std::net::IpAddr::V4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
                Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00,
                Err(_) => host.ends_with(".local") || host == "localhost",
            }
        }
        _ => false,
    }
}

/// An Ed25519 public key in base64 (32 bytes).
pub fn public_key_ok(key: &str) -> bool {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(key.trim()).is_ok_and(|k| k.len() == 32)
}

impl Library {
    pub fn new(cfg: &crate::config::Config) -> Self {
        let (official_base, official_key) = match &cfg.modules_library {
            Some((base, key)) => (Some(base.trim_end_matches('/').to_string()), key.clone()),
            None => (None, crate::update::RELEASE_PUBLIC_KEY.to_string()),
        };
        Self { official_base, official_key, cache: tokio::sync::Mutex::new(Default::default()) }
    }

    /// The key of the official library.
    pub fn public_key(&self) -> &str {
        &self.official_key
    }

    fn of(&self, row: crate::db::RepositoryRow) -> Repository {
        if row.official {
            Repository { id: row.id, name: row.name, url: self.official_base.clone(), public_key: self.official_key.clone(), official: true, enabled: row.enabled }
        } else {
            Repository { id: row.id, name: row.name, url: row.url, public_key: row.public_key.unwrap_or_default(), official: false, enabled: row.enabled }
        }
    }

    /// Every source of modules, the official first.
    pub async fn repositories(&self, db: &crate::db::Db) -> sqlx::Result<Vec<Repository>> {
        Ok(crate::db::list_repositories(db).await?.into_iter().map(|r| self.of(r)).collect())
    }

    pub async fn repository(&self, db: &crate::db::Db, id: i64) -> sqlx::Result<Option<Repository>> {
        Ok(crate::db::repository(db, id).await?.map(|r| self.of(r)))
    }

    /// The official source.
    pub async fn official(&self, db: &crate::db::Db) -> sqlx::Result<Repository> {
        Ok(self.repositories(db).await?.into_iter().find(|r| r.official).expect("the official source of modules is written by the migrations"))
    }

    /// Forgets the index kept for a source (it was changed or removed).
    pub async fn forget(&self, id: i64) {
        self.cache.lock().await.remove(&id);
    }

    fn url(&self, repo: &Repository, file: &str) -> String {
        match &repo.url {
            Some(base) => format!("{}/{file}", base.trim_end_matches('/')),
            None => file_url(file),
        }
    }

    /// Downloads a file of a source, `max` bytes at most: from the official repository only (HTTPS on
    /// GitHub's hosts, redirections included) for the official source unless the tests set its
    /// address; for another, from its own address (HTTPS, or HTTP on the local network).
    pub async fn fetch(&self, repo: &Repository, file: &str, max: usize) -> Result<Vec<u8>, String> {
        self.fetch_url(&self.url(repo, file), repo.official && self.official_base.is_none(), max).await
    }

    async fn fetch_url(&self, url: &str, official: bool, max: usize) -> Result<Vec<u8>, String> {
        let file = url.rsplit('/').next().unwrap_or(url).to_string();
        if !official && self.official_base.is_none() && !repository_url_ok(url) {
            return Err(format!("{url}: HTTPS, or HTTP on the local network only"));
        }
        let policy = reqwest::redirect::Policy::custom(move |a| {
            let ok = if official { crate::update::allowed_url(a.url()) } else { repository_url_ok(a.url().as_str()) };
            if ok {
                a.follow()
            } else {
                a.stop()
            }
        });
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
        match &self.official_base {
            // The tests serve it beside the library.
            Some(base) => self.fetch_url(&format!("{base}/{version}-{file}"), false, max).await,
            None => self.fetch_url(&crate::update::download_url(version, file), true, max).await,
        }
    }

    /// The index of a source, read again once a day (or when `fresh`), never older than the last
    /// one accepted from it.
    pub async fn index(&self, db: &crate::db::Db, repo: &Repository, fresh: bool) -> Result<Index, String> {
        if !fresh {
            if let Some((_, index)) = self.cache.lock().await.get(&repo.id).filter(|(at, _)| at.elapsed() < KEEP_INDEX) {
                return Ok(index.clone());
            }
        }
        let bytes = self.fetch(repo, "index.json", MAX_INDEX).await?;
        let signature = self.fetch(repo, "index.json.sig", 1024).await?;
        let seen = crate::db::repository(db, repo.id).await.map_err(|e| e.to_string())?.and_then(|r| r.seq).map(|s| s as u64);
        let index = Index::open(&bytes, &String::from_utf8_lossy(&signature), &repo.public_key, seen).map_err(|e| e.to_string())?;
        crate::db::set_repository_seq(db, repo.id, index.seq as i64).await.map_err(|e| e.to_string())?;
        self.cache.lock().await.insert(repo.id, (std::time::Instant::now(), index.clone()));
        Ok(index)
    }

    /// The index of every source switched on, with its source (or why it cannot be read).
    pub async fn indexes(&self, db: &crate::db::Db, fresh: bool) -> sqlx::Result<Vec<(Repository, Result<Index, String>)>> {
        let mut out = vec![];
        for repo in self.repositories(db).await?.into_iter().filter(|r| r.enabled) {
            let index = self.index(db, &repo, fresh).await;
            out.push((repo, index));
        }
        Ok(out)
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
    fn a_source_is_reached_safely() {
        for ok in ["https://modules.example.com/breezone", "http://192.168.1.20:8000/repo", "http://nas.local/modules", "http://localhost:9000"] {
            assert!(repository_url_ok(ok), "{ok}");
        }
        for bad in ["http://modules.example.com/", "ftp://192.168.1.2/", "https://user:pw@example.com/", "file:///etc", "http://8.8.8.8/"] {
            assert!(!repository_url_ok(bad), "{bad}");
        }
        assert!(public_key_ok(&keys().1));
        assert!(!public_key_ok("not a key"));
    }

    #[test]
    fn the_newest_version_that_fits_is_proposed() {
        let mut host = crate::capabilities::Host::local();
        host.api = vec!["1.0".into()];
        let mut withdrawn = published("1.5.0", "^1.0");
        withdrawn.yanked = true;
        let i: Index = serde_json::from_slice(&index(1, vec![published("2.0.0", "^2.0"), withdrawn, published("1.4.0", "^1.0")])).unwrap();
        let v = view(&RepositoryRef { id: 1, name: "Breezone".into(), official: true }, &i, &host, "0.13.0", &|_| None);
        assert_eq!(v[0].fits.as_deref(), Some("1.4.0"));
        assert_eq!(v[0].manifest.version, "2.0.0");
        assert!(matches!(v[0].reasons[..], [Incompatibility::Api { .. }]));
    }
}
