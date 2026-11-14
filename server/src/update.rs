//! Self-update (issue #40): Breezone downloads its next release and replaces itself, server and
//! web UI together, without Docker.
//!
//! * The releases come from the official repository only: its address, the hosts a download may
//!   go through and the public key that checks the signatures are compiled in. Nothing (variable,
//!   setting, API) can point the update elsewhere.
//! * Each release publishes, per architecture, `breezone-<version>-linux-<arch>.tar.gz` (the
//!   static server and the `web/` folder) and its Ed25519 signature `.sig`. The signed message
//!   names the version and the target, so that an older signed archive cannot pass for a newer one.
//! * Installed versions live on the data volume, in `versions/<version>/` (the image's own files
//!   are read-only). `versions/state.json` says which one runs; at start, the image's binary
//!   switches to it (`exec`) if it is newer than itself.
//! * A version is only kept once it has run for [`CONFIRM_AFTER_SECS`]: a version that stops
//!   before is undone at the next start (database restored from the backup taken just before the
//!   update, previous version run again).

use crate::db::Db;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// The official repository, the only source of updates.
pub const REPOSITORY: &str = "EmmanuelVinas/breezone";
/// The latest stable release: GitHub never answers a pre-release here.
const LATEST_RELEASE: &str = "https://api.github.com/repos/EmmanuelVinas/breezone/releases/latest";
/// The recent releases, pre-releases included (beta channel).
const RELEASES: &str = "https://api.github.com/repos/EmmanuelVinas/breezone/releases?per_page=30";
/// Setting (`settings` table) that subscribes this server to the beta channel: "beta" or absent.
pub const CHANNEL_SETTING: &str = "update_channel";
/// Hosts a request or a redirect may reach: GitHub's API, its pages, and its release file storage.
const ALLOWED_HOSTS: &[&str] = &["api.github.com", "github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];
/// Public key of the project's release signing key (raw Ed25519, base64). The private key only
/// exists as a secret of the repository's release workflow.
pub const RELEASE_PUBLIC_KEY: &str = "tW6sRrUoLvF0Z6g3J1KgCX38zqNgFoQ2Oee7Q9dtDco=";
/// Larger archives are refused (the release is about 10 MB).
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
/// A new version is kept once it has run this long.
pub const CONFIRM_AFTER_SECS: u64 = 30;
/// Releases are looked for once a day.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 3600);

/// The version of this binary.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is x.y.z or x.y.z-beta.n")
}

// ---------------------------------------------------------------------------------------
// Versions, targets, addresses
// ---------------------------------------------------------------------------------------

/// Stage of a pre-release, in release order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    Alpha,
    Beta,
    Rc,
}

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Stage::Alpha => "alpha",
            Stage::Beta => "beta",
            Stage::Rc => "rc",
        }
    }
}

/// `major.minor.patch`, or a pre-release of it: `-alpha.N`, `-beta.N` or `-rc.N` (the tags of
/// the beta channel). A leading `v` is accepted, nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<(Stage, u64)>,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self { major, minor, patch, pre: None }
    }

    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches('v');
        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (s, None),
        };
        let mut parts = core.split('.');
        let mut v = Version::new(parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
        if parts.next().is_some() {
            return None;
        }
        if let Some(pre) = pre {
            let (label, n) = pre.split_once('.')?;
            let stage = match label {
                "alpha" => Stage::Alpha,
                "beta" => Stage::Beta,
                "rc" => Stage::Rc,
                _ => return None,
            };
            v.pre = Some((stage, n.parse().ok()?));
        }
        Some(v)
    }

    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }
}

/// Semantic versioning order: 1.0.0-alpha.1 < 1.0.0-beta.1 < 1.0.0-beta.2 < 1.0.0-rc.1 < 1.0.0.
impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| match (self.pre, other.pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => a.cmp(&b),
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some((stage, n)) = self.pre {
            write!(f, "-{}.{n}", stage.label())?;
        }
        Ok(())
    }
}

/// The release files for this system, if the project publishes some.
pub fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux-amd64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        _ => None,
    }
}

pub fn archive_name(version: Version, target: &str) -> String {
    format!("breezone-{version}-{target}.tar.gz")
}

/// Where a release file is downloaded from: always the official repository, whatever the API says.
pub fn download_url(version: Version, file: &str) -> String {
    format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{file}")
}

/// HTTPS on one of GitHub's hosts, without credentials or another port.
pub fn allowed_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https" && url.username().is_empty() && url.password().is_none() && url.port().is_none() && url.host_str().is_some_and(|h| ALLOWED_HOSTS.contains(&h))
}

/// What the release workflow signs: the version, the target and the archive's SHA-256.
pub fn signed_message(version: Version, target: &str, archive: &[u8]) -> Vec<u8> {
    format!("breezone {version} {target}\n{}\n", hex::encode(Sha256::digest(archive))).into_bytes()
}

/// Checks an archive's signature (`signature`: base64, as published) with `public_key` (base64).
pub fn verify(public_key: &str, version: Version, target: &str, archive: &[u8], signature: &str) -> bool {
    let Some(key) = STANDARD.decode(public_key.trim()).ok().and_then(|k| <[u8; 32]>::try_from(k).ok()) else { return false };
    let Ok(key) = VerifyingKey::from_bytes(&key) else { return false };
    let Some(sig) = STANDARD.decode(signature.trim()).ok().and_then(|s| <[u8; 64]>::try_from(s).ok()) else { return false };
    key.verify_strict(&signed_message(version, target, archive), &Signature::from_bytes(&sig)).is_ok()
}

// ---------------------------------------------------------------------------------------
// Installed versions (the data volume)
// ---------------------------------------------------------------------------------------

/// `versions/state.json`: which installed version runs. Read by older and newer versions alike:
/// fields are only ever added.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Version to run, installed in `versions/<current>/`.
    #[serde(default)]
    pub current: Option<String>,
    /// Installed version run before it, if any (else the image's).
    #[serde(default)]
    pub previous: Option<String>,
    /// Set until `current` has run [`CONFIRM_AFTER_SECS`].
    #[serde(default)]
    pub pending: Option<Pending>,
    /// Versions that did not start: not offered again.
    #[serde(default)]
    pub failed: Vec<String>,
    /// An update was undone at the last start: reported once, then cleared.
    #[serde(default)]
    pub rolled_back: Option<RolledBack>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    /// Starts of `current` not confirmed yet.
    pub attempts: u32,
    /// Copy of the database taken just before the update.
    pub backup: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RolledBack {
    /// The version that did not start.
    pub version: String,
    /// The version run instead.
    pub to: String,
}

pub fn versions_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("versions")
}

fn state_path(data_dir: &Path) -> PathBuf {
    versions_dir(data_dir).join("state.json")
}

pub fn read_state(data_dir: &Path) -> State {
    std::fs::read(state_path(data_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn write_state(data_dir: &Path, state: &State) -> std::io::Result<()> {
    std::fs::create_dir_all(versions_dir(data_dir))?;
    let tmp = versions_dir(data_dir).join("state.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?)?;
    std::fs::rename(tmp, state_path(data_dir))
}

/// The server binary and web UI of an installed version.
pub fn installed(data_dir: &Path, version: &str) -> Option<(PathBuf, PathBuf)> {
    let dir = versions_dir(data_dir).join(version);
    let binary = dir.join("breezone-server");
    let web = dir.join("web");
    (binary.is_file() && web.join("index.html").is_file()).then_some((binary, web))
}

/// What to do when the server starts.
#[derive(Debug, PartialEq)]
pub enum Launch {
    /// Run this binary: no newer version is installed.
    Here,
    /// Switch to the installed version.
    Switch { version: String, binary: PathBuf, web: PathBuf },
}

/// Chooses the version to run at start, and records the choice. An unconfirmed version that is
/// started again did not survive its first run: it is undone (database restored, previous
/// version) instead of being run in a loop.
pub fn choose_at_start(data_dir: &Path, own: Version) -> Launch {
    let mut state = read_state(data_dir);
    loop {
        let Some(current) = state.current.clone() else { return Launch::Here };
        // This is the installed version (switched to), or an image of the same version.
        if Version::parse(&current) == Some(own) {
            return Launch::Here;
        }
        let newer = Version::parse(&current).is_some_and(|v| v > own);
        let files = installed(data_dir, &current);
        if !newer || files.is_none() {
            // The image is as recent (it was updated) or the files are gone: the image runs.
            if newer {
                tracing::warn!("Breezone {current} is not installed completely: Breezone {own} runs");
            }
            state.current = None;
            state.previous = None;
            state.pending = None;
            remove_versions_up_to(data_dir, own);
            save(data_dir, &state);
            return Launch::Here;
        }
        if let Some(pending) = state.pending.clone().filter(|p| p.attempts >= 1) {
            tracing::error!("Breezone {current} did not start: going back");
            if let Some(backup) = pending.backup.as_deref() {
                if let Err(e) = restore_database(data_dir, backup) {
                    tracing::error!("cannot restore the database from {}: {e}", backup.display());
                }
            }
            state.failed.push(current.clone());
            state.pending = None;
            state.current = state.previous.take();
            state.rolled_back = Some(RolledBack { version: current, to: state.current.clone().unwrap_or_else(|| own.to_string()) });
            save(data_dir, &state);
            continue;
        }
        let (binary, web) = files.expect("checked above");
        return Launch::Switch { version: current, binary, web };
    }
}

fn save(data_dir: &Path, state: &State) {
    if let Err(e) = write_state(data_dir, state) {
        tracing::error!("cannot write {}: {e}", state_path(data_dir).display());
    }
}

/// An installed version cannot be started (switching failed): it is set aside, the image runs.
pub fn abandon(data_dir: &Path, version: &str) {
    let mut state = read_state(data_dir);
    if !state.failed.iter().any(|f| f == version) {
        state.failed.push(version.to_string());
    }
    state.current = None;
    state.previous = None;
    state.pending = None;
    save(data_dir, &state);
}

/// The running version has run long enough: it is kept. Returns true when it was an update.
pub fn confirm(data_dir: &Path, own: Version) -> bool {
    let mut state = read_state(data_dir);
    if state.current.as_deref() != Some(own.to_string().as_str()) || state.pending.is_none() {
        return false;
    }
    state.pending = None;
    save(data_dir, &state);
    // Only the running version and the one before are kept, and the last two backups.
    let keep: Vec<String> = [state.current.clone(), state.previous.clone()].into_iter().flatten().collect();
    if let Ok(entries) = std::fs::read_dir(versions_dir(data_dir)) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() && !keep.contains(&name) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    prune_backups(data_dir, 2);
    true
}

/// The update undone at the last start, once (then forgotten).
pub fn take_rolled_back(data_dir: &Path) -> Option<RolledBack> {
    let mut state = read_state(data_dir);
    let rolled_back = state.rolled_back.take()?;
    save(data_dir, &state);
    Some(rolled_back)
}

fn remove_versions_up_to(data_dir: &Path, own: Version) {
    let Ok(entries) = std::fs::read_dir(versions_dir(data_dir)) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if e.path().is_dir() && Version::parse(&name).is_none_or(|v| v <= own) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

fn backups_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

fn prune_backups(data_dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(backups_dir(data_dir)) else { return };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "db")).collect();
    files.sort();
    let excess = files.len().saturating_sub(keep);
    for f in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(f);
    }
}

/// Puts a backup back in place of the database (and drops the write-ahead log of the newer one).
pub fn restore_database(data_dir: &Path, backup: &Path) -> std::io::Result<()> {
    let db = data_dir.join("breezone.db");
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(data_dir.join(format!("breezone.db{suffix}")));
    }
    std::fs::copy(backup, &db)?;
    Ok(())
}

/// A consistent copy of the database, taken while it is in use.
pub async fn backup_database(db: &Db, data_dir: &Path, version: Version) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(backups_dir(data_dir))?;
    let path = backups_dir(data_dir).join(format!("breezone-{}-{version}.db", chrono::Utc::now().format("%Y%m%d-%H%M%S")));
    let _ = std::fs::remove_file(&path);
    sqlx::query("VACUUM INTO ?").bind(path.to_string_lossy().into_owned()).execute(db).await?;
    Ok(path)
}

/// Unpacks a release archive into `versions/<version>/`: the server binary and the `web/` folder,
/// nothing outside the folder (the archive is signed, but each entry is checked anyway).
pub fn unpack(archive: &[u8], data_dir: &Path, version: Version) -> anyhow::Result<(PathBuf, PathBuf)> {
    let dir = versions_dir(data_dir).join(version.to_string());
    let partial = versions_dir(data_dir).join(format!("{version}.partial"));
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let safe = path.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir));
        let kind = entry.header().entry_type();
        if !safe || !(kind.is_file() || kind.is_dir()) {
            anyhow::bail!("unexpected entry in the archive: {}", path.display());
        }
        entry.unpack_in(&partial)?;
    }
    let binary = partial.join("breezone-server");
    if !binary.is_file() || !partial.join("web").join("index.html").is_file() {
        anyhow::bail!("the archive does not hold breezone-server and web/");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))?;
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&partial, &dir)?;
    Ok((dir.join("breezone-server"), dir.join("web")))
}

/// Replaces this process with another Breezone binary (same arguments and environment, its own
/// web UI). Only returns on failure.
#[cfg(unix)]
pub fn exec(binary: &Path, web: &Path) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(binary).args(std::env::args_os().skip(1)).env("BREEZONE_WEB_DIR", web).exec()
}

#[cfg(not(unix))]
pub fn exec(_binary: &Path, _web: &Path) -> std::io::Error {
    std::io::Error::other("switching binaries needs Unix")
}

/// The version a binary reports (`--version`): proves it runs on this system before switching.
async fn binary_version(binary: &Path) -> Option<Version> {
    let out = tokio::time::timeout(Duration::from_secs(10), tokio::process::Command::new(binary).arg("--version").output()).await.ok()?.ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace().last().and_then(Version::parse)
}

// ---------------------------------------------------------------------------------------
// The updater: checks for releases, installs one on request
// ---------------------------------------------------------------------------------------

/// Where releases come from. Only [`Source::official`] exists outside the tests.
#[derive(Clone)]
struct Source {
    latest_release: String,
    releases: String,
    public_key: String,
}

impl Source {
    fn official() -> Self {
        Self { latest_release: LATEST_RELEASE.into(), releases: RELEASES.into(), public_key: RELEASE_PUBLIC_KEY.into() }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    name: String,
}

/// What the settings page shows.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateView {
    pub current: String,
    pub latest: Option<String>,
    /// Release notes (Markdown).
    pub notes: Option<String>,
    pub published_at: Option<String>,
    pub url: Option<String>,
    /// A newer version exists.
    pub available: bool,
    /// It can be installed from here (else see `reason`).
    pub can_install: bool,
    /// Why it cannot: "disabled" (BREEZONE_SELF_UPDATE=false), "platform" (no build for this
    /// system), "development" (not a release build), "assets" (the release has no file for this
    /// system yet), "failed" (this version did not start here).
    pub reason: Option<&'static str>,
    /// "idle" | "checking" | "downloading" | "installing" | "restarting" | "failed"
    pub state: &'static str,
    /// Download progress, 0 to 1.
    pub progress: Option<f64>,
    pub error: Option<String>,
    pub checked_at: Option<i64>,
    /// The last update was undone at start: `{version, to}`.
    pub rolled_back: Option<RolledBack>,
    /// This server also receives the pre-releases (beta channel).
    pub beta: bool,
    /// `latest` is a pre-release.
    pub prerelease: bool,
}

/// The newest of the releases, drafts left out.
fn newest(releases: Vec<GithubRelease>) -> Option<GithubRelease> {
    releases.into_iter().filter(|r| !r.draft).filter_map(|r| Version::parse(&r.tag_name).map(|v| (v, r))).max_by_key(|(v, _)| *v).map(|(_, r)| r)
}

struct Status {
    latest: Option<GithubRelease>,
    checked_at: Option<i64>,
    state: &'static str,
    progress: Option<f64>,
    error: Option<String>,
    /// Binary and web UI to switch to once the server has stopped.
    switch_to: Option<(PathBuf, PathBuf)>,
}

pub struct Updater {
    data_dir: PathBuf,
    enabled: bool,
    beta: std::sync::atomic::AtomicBool,
    source: Source,
    http: reqwest::Client,
    status: Mutex<Status>,
    rolled_back: Mutex<Option<RolledBack>>,
    restart: tokio::sync::Notify,
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!("Breezone/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(120))
        .https_only(true)
        // Redirects (GitHub serves release files from another host) stay on GitHub.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else if allowed_url(attempt.url()) {
                attempt.follow()
            } else {
                let refused = format!("redirect outside GitHub refused: {}", attempt.url().host_str().unwrap_or("?"));
                attempt.error(refused)
            }
        }))
        .build()
        .expect("HTTP client")
}

impl Updater {
    pub fn new(data_dir: PathBuf, enabled: bool) -> Self {
        Self::with_source(data_dir, enabled, Source::official())
    }

    fn with_source(data_dir: PathBuf, enabled: bool, source: Source) -> Self {
        Self {
            data_dir,
            enabled,
            beta: std::sync::atomic::AtomicBool::new(false),
            source,
            http: http_client(),
            status: Mutex::new(Status { latest: None, checked_at: None, state: "idle", progress: None, error: None, switch_to: None }),
            rolled_back: Mutex::new(None),
            restart: tokio::sync::Notify::new(),
        }
    }

    /// Records the update undone at start, to show it.
    pub fn set_rolled_back(&self, r: Option<RolledBack>) {
        *self.rolled_back.lock().unwrap() = r;
    }

    pub fn beta(&self) -> bool {
        self.beta.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Joins or leaves the beta channel. The release found on the other channel is forgotten:
    /// the next check looks again. Leaving never goes back: a server on a pre-release stays on it
    /// until a newer version comes out (the database migrations cannot be undone).
    pub fn set_beta(&self, beta: bool) {
        if self.beta.swap(beta, std::sync::atomic::Ordering::Relaxed) != beta {
            let mut s = self.status.lock().unwrap();
            if matches!(s.state, "idle" | "failed") {
                s.latest = None;
                s.checked_at = None;
            }
        }
    }

    fn cannot_install(&self, latest: Option<&GithubRelease>) -> Option<&'static str> {
        if !self.enabled {
            return Some("disabled");
        }
        let Some(target) = target() else { return Some("platform") };
        if cfg!(debug_assertions) {
            return Some("development");
        }
        let latest = latest?;
        let version = Version::parse(&latest.tag_name)?;
        if read_state(&self.data_dir).failed.contains(&version.to_string()) {
            return Some("failed");
        }
        let archive = archive_name(version, target);
        let sig = format!("{archive}.sig");
        if !(latest.assets.iter().any(|a| a.name == archive) && latest.assets.iter().any(|a| a.name == sig)) {
            return Some("assets");
        }
        None
    }

    pub fn view(&self) -> UpdateView {
        let s = self.status.lock().unwrap();
        let current = current_version();
        // A release older than this build (installed from an image whose release is not
        // published yet) is not "the latest version": it is not shown.
        let release = s.latest.as_ref().filter(|r| Version::parse(&r.tag_name).is_some_and(|v| v >= current));
        let latest_version = release.and_then(|r| Version::parse(&r.tag_name));
        let available = latest_version.is_some_and(|v| v > current);
        let reason = if available { self.cannot_install(release) } else { None };
        UpdateView {
            current: current.to_string(),
            latest: latest_version.map(|v| v.to_string()),
            notes: release.and_then(|r| r.body.clone()),
            published_at: release.and_then(|r| r.published_at.clone()),
            url: latest_version.map(|v| format!("https://github.com/{REPOSITORY}/releases/tag/v{v}")),
            available,
            can_install: available && reason.is_none(),
            reason,
            state: s.state,
            progress: s.progress,
            error: s.error.clone(),
            checked_at: s.checked_at,
            rolled_back: self.rolled_back.lock().unwrap().clone(),
            beta: self.beta(),
            prerelease: latest_version.is_some_and(|v| v.is_prerelease()),
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let url = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
        if !allowed_url(&url) {
            return Err("release address refused".into());
        }
        self.http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }

    /// Reads the latest release of the official repository: the latest stable one, or on the
    /// beta channel the newest one, pre-releases included.
    pub async fn check(&self) -> Result<UpdateView, String> {
        let beta = self.beta();
        let release = if beta { newest(self.get_json(&self.source.releases).await?).ok_or("no release")? } else { self.get_json(&self.source.latest_release).await? };
        {
            let mut s = self.status.lock().unwrap();
            // The channel changed meanwhile: this answer is for the other one.
            if self.beta() == beta {
                s.latest = Some(release);
                s.checked_at = Some(crate::db::now());
            }
        }
        Ok(self.view())
    }

    /// Checks for a release now and then, once a day.
    pub async fn run_checks(self: std::sync::Arc<Self>) {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            if let Err(e) = self.check().await {
                tracing::info!("cannot check for a Breezone update: {e}");
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    }

    async fn download(&self, url: &str, total_hint: Option<u64>) -> Result<Vec<u8>, String> {
        let url = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
        if !allowed_url(&url) {
            return Err("download address refused".into());
        }
        let mut resp = self.http.get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
        let total = resp.content_length().or(total_hint);
        if total.is_some_and(|t| t > MAX_ARCHIVE_BYTES) {
            return Err("file too large".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
                return Err("file too large".into());
            }
            if let Some(t) = total.filter(|t| *t > 0) {
                self.status.lock().unwrap().progress = Some((bytes.len() as f64 / t as f64).min(1.0));
            }
        }
        Ok(bytes)
    }

    fn set(&self, state: &'static str, error: Option<String>) {
        let mut s = self.status.lock().unwrap();
        s.state = state;
        s.error = error;
        if state != "downloading" {
            s.progress = None;
        }
    }

    /// Installs the latest release and asks the server to restart on it. Returns at once: the
    /// progress shows in [`Updater::view`].
    pub fn start_install(self: &std::sync::Arc<Self>, db: Db, hub: std::sync::Arc<crate::hub::Hub>, actor: String) -> Result<(), &'static str> {
        {
            let mut s = self.status.lock().unwrap();
            if !matches!(s.state, "idle" | "failed") {
                return Err("an update is already running");
            }
            let current = current_version();
            let latest = s.latest.as_ref().and_then(|r| Version::parse(&r.tag_name));
            if latest.is_none_or(|v| v <= current) {
                return Err("no newer version");
            }
            if self.cannot_install(s.latest.as_ref()).is_some() {
                return Err("this version cannot be installed here");
            }
            s.state = "downloading";
            s.error = None;
            s.progress = Some(0.0);
        }
        let me = self.clone();
        tokio::spawn(async move {
            match me.install(&db).await {
                Ok(version) => {
                    hub.log(
                        "system",
                        "info",
                        None,
                        Some(&actor),
                        crate::text!("Mise à jour vers Breezone {0} : redémarrage", "Updating to Breezone {0}: restarting", "Actualización a Breezone {0}: reinicio", version),
                        None,
                    )
                    .await;
                    me.restart.notify_one();
                }
                Err(e) => {
                    tracing::error!("update failed: {e}");
                    hub.log(
                        "system",
                        "error",
                        None,
                        Some(&actor),
                        crate::text!("Échec de la mise à jour de Breezone : {0}", "Breezone update failed: {0}", "Error al actualizar Breezone: {0}", e),
                        None,
                    )
                    .await;
                    me.set("failed", Some(e));
                }
            }
        });
        Ok(())
    }

    async fn install(&self, db: &Db) -> Result<Version, String> {
        let target = target().ok_or("no build for this system")?;
        let current = current_version();
        let version = {
            let s = self.status.lock().unwrap();
            s.latest.as_ref().and_then(|r| Version::parse(&r.tag_name)).ok_or("no release")?
        };
        // Never back to an older (possibly vulnerable) version.
        if version <= current {
            return Err(format!("{version} is not newer than {current}"));
        }
        let archive_file = archive_name(version, target);
        let signature = self.download(&download_url(version, &format!("{archive_file}.sig")), None).await?;
        let archive = self.download(&download_url(version, &archive_file), None).await?;
        self.set("installing", None);
        let signature = String::from_utf8(signature).map_err(|_| "unreadable signature")?;
        if !verify(&self.source.public_key, version, target, &archive, &signature) {
            return Err("the signature of the archive is not the project's: update refused".into());
        }
        let data_dir = self.data_dir.clone();
        let (binary, web) = tokio::task::spawn_blocking(move || unpack(&archive, &data_dir, version)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
        if binary_version(&binary).await != Some(version) {
            let _ = std::fs::remove_dir_all(versions_dir(&self.data_dir).join(version.to_string()));
            return Err("the new version does not run on this system".into());
        }
        let backup = backup_database(db, &self.data_dir, current).await.map_err(|e| format!("database backup: {e}"))?;
        let mut state = read_state(&self.data_dir);
        state.previous = state.current.clone().filter(|c| c != &version.to_string());
        state.current = Some(version.to_string());
        state.pending = Some(Pending { attempts: 1, backup: Some(backup) });
        write_state(&self.data_dir, &state).map_err(|e| e.to_string())?;
        let mut s = self.status.lock().unwrap();
        s.state = "restarting";
        s.progress = None;
        s.switch_to = Some((binary, web));
        Ok(version)
    }

    /// Resolves once an update is installed: the server then stops and calls [`Updater::switch`].
    pub async fn restart_requested(&self) {
        self.restart.notified().await
    }

    /// The binary and web UI to switch to, once the server has stopped.
    pub fn switch_target(&self) -> Option<(PathBuf, PathBuf)> {
        self.status.lock().unwrap().switch_to.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::io::Write;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("breezone-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_are_compared_by_number() {
        assert!(v("v0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert_eq!(v("v0.8.1").to_string(), "0.8.1");
        assert_eq!(Version::parse("0.8"), None);
        assert_eq!(Version::parse("0.8.1-rc1"), None);
        assert_eq!(Version::parse("0.8.1.2"), None);
    }

    #[test]
    fn pre_releases_come_before_their_version() {
        assert!(v("1.0.0-alpha.1") < v("1.0.0-beta.1"));
        assert!(v("1.0.0-beta.1") < v("1.0.0-beta.2"));
        assert!(v("1.0.0-beta.2") < v("1.0.0-beta.10"));
        assert!(v("1.0.0-beta.10") < v("1.0.0-rc.1"));
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("0.9.1") < v("1.0.0-beta.1"));
        assert!(v("1.0.0") < v("1.0.1-beta.1"));
        assert_eq!(v("v1.0.0-beta.2").to_string(), "1.0.0-beta.2");
        assert!(v("1.0.0-beta.2").is_prerelease() && !v("1.0.0").is_prerelease());
        assert_eq!(Version::parse("1.0.0-beta"), None, "a pre-release is numbered");
        assert_eq!(Version::parse("1.0.0-dev.1"), None);
        assert_eq!(Version::parse("1.0.0-beta.1.partial"), None, "an unfinished download's folder");
    }

    #[test]
    fn the_beta_channel_takes_the_newest_release_drafts_aside() {
        let release = |tag: &str, draft: bool| GithubRelease { tag_name: tag.into(), draft, body: None, published_at: None, assets: vec![] };
        let pick = |rs: Vec<GithubRelease>| newest(rs).map(|r| r.tag_name);
        assert_eq!(pick(vec![release("v0.9.1", false), release("v1.0.0-beta.2", false), release("v1.0.0-beta.1", false)]), Some("v1.0.0-beta.2".into()));
        assert_eq!(pick(vec![release("v1.0.0-beta.2", false), release("v1.0.0", false)]), Some("v1.0.0".into()), "the stable version once out");
        assert_eq!(pick(vec![release("v0.9.1", false), release("v1.0.0-beta.1", true)]), Some("v0.9.1".into()), "a draft is not published");
        assert_eq!(pick(vec![release("nightly", false)]), None);
    }

    #[test]
    fn downloads_only_come_from_the_official_repository() {
        assert_eq!(
            download_url(v("0.8.1"), "breezone-0.8.1-linux-arm64.tar.gz"),
            "https://github.com/EmmanuelVinas/breezone/releases/download/v0.8.1/breezone-0.8.1-linux-arm64.tar.gz"
        );
        let ok = |u: &str| allowed_url(&reqwest::Url::parse(u).unwrap());
        assert!(ok("https://github.com/EmmanuelVinas/breezone/releases/download/v1.0.0/x"));
        assert!(ok("https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sig=x"));
        assert!(ok("https://api.github.com/repos/EmmanuelVinas/breezone/releases/latest"));
        assert!(!ok("http://github.com/EmmanuelVinas/breezone"), "plain HTTP");
        assert!(!ok("https://github.com.evil.example/x"), "lookalike host");
        assert!(!ok("https://evil.example/github.com"));
        assert!(!ok("https://user@github.com/x"), "credentials");
        assert!(!ok("https://github.com:8443/x"), "another port");
    }

    fn signed(key: &SigningKey, version: Version, target: &str, archive: &[u8]) -> String {
        STANDARD.encode(key.sign(&signed_message(version, target, archive)).to_bytes())
    }

    #[test]
    fn only_archives_signed_for_this_version_and_target_pass() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = STANDARD.encode(key.verifying_key().to_bytes());
        let archive = b"archive bytes".to_vec();
        let sig = signed(&key, v("0.9.0"), "linux-arm64", &archive);
        assert!(verify(&public, v("0.9.0"), "linux-arm64", &archive, &sig));
        assert!(!verify(&public, v("0.9.0"), "linux-arm64", b"archive bytes!", &sig), "other content");
        assert!(!verify(&public, v("0.9.1"), "linux-arm64", &archive, &sig), "an old archive passed off as newer");
        assert!(!verify(&public, v("0.9.0"), "linux-amd64", &archive, &sig), "another architecture");
        let other = SigningKey::from_bytes(&[8; 32]);
        assert!(!verify(&public, v("0.9.0"), "linux-arm64", &archive, &signed(&other, v("0.9.0"), "linux-arm64", &archive)), "another key");
        assert!(!verify(&public, v("0.9.0"), "linux-arm64", &archive, "not base64!"));
        assert!(!verify(RELEASE_PUBLIC_KEY, v("0.9.0"), "linux-arm64", &archive, &sig), "test key is not the project's");
    }

    /// Checks a release archive signed by the workflow (or by hand, with the same commands):
    /// `BREEZONE_ARCHIVE=… BREEZONE_VERSION=… BREEZONE_TARGET=… cargo test -- --ignored signed_by_the_release_key`
    #[test]
    #[ignore]
    fn signed_by_the_release_key() {
        let archive = std::env::var("BREEZONE_ARCHIVE").expect("BREEZONE_ARCHIVE");
        let version = v(&std::env::var("BREEZONE_VERSION").expect("BREEZONE_VERSION"));
        let target = std::env::var("BREEZONE_TARGET").expect("BREEZONE_TARGET");
        let bytes = std::fs::read(&archive).unwrap();
        let sig = std::fs::read_to_string(format!("{archive}.sig")).unwrap();
        assert!(verify(RELEASE_PUBLIC_KEY, version, &target, &bytes, &sig));
    }

    #[test]
    fn the_project_key_is_a_valid_ed25519_key() {
        let key: [u8; 32] = STANDARD.decode(RELEASE_PUBLIC_KEY).unwrap().try_into().unwrap();
        assert!(VerifyingKey::from_bytes(&key).is_ok());
    }

    fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            // Written by hand so that unsafe paths can be tested too.
            let name = header.as_old_mut().name.as_mut();
            name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_cksum();
            builder.append(&header, *data).unwrap();
        }
        let mut gz = builder.into_inner().unwrap();
        gz.flush().unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn unpacks_the_server_and_web_ui_only_inside_its_folder() {
        let dir = temp_dir("unpack");
        let archive = tar_gz(&[("breezone-server", b"#!/bin/sh\necho Breezone 0.9.0\n"), ("web/index.html", b"<html>")]);
        let (binary, web) = unpack(&archive, &dir, v("0.9.0")).unwrap();
        assert!(binary.ends_with("versions/0.9.0/breezone-server"));
        assert!(web.join("index.html").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&binary).unwrap().permissions().mode() & 0o777, 0o755);
        }

        let evil = tar_gz(&[("breezone-server", b"x"), ("web/index.html", b"x"), ("../../escaped", b"x")]);
        assert!(unpack(&evil, &dir, v("0.9.1")).is_err());
        assert!(!dir.join("escaped").exists() && !dir.parent().unwrap().join("escaped").exists());
        assert!(unpack(&tar_gz(&[("breezone-server", b"x")]), &dir, v("0.9.2")).is_err(), "no web UI");
    }

    fn install_fake(dir: &Path, version: &str) {
        let d = versions_dir(dir).join(version);
        std::fs::create_dir_all(d.join("web")).unwrap();
        std::fs::write(d.join("breezone-server"), b"x").unwrap();
        std::fs::write(d.join("web/index.html"), b"x").unwrap();
    }

    #[test]
    fn starts_the_newest_installed_version_unless_the_image_is_as_recent() {
        let dir = temp_dir("choose");
        assert_eq!(choose_at_start(&dir, v("0.8.0")), Launch::Here, "nothing installed");

        install_fake(&dir, "0.8.1");
        write_state(&dir, &State { current: Some("0.8.1".into()), ..Default::default() }).unwrap();
        match choose_at_start(&dir, v("0.8.0")) {
            Launch::Switch { version, binary, web } => {
                assert_eq!(version, "0.8.1");
                assert!(binary.ends_with("0.8.1/breezone-server") && web.ends_with("0.8.1/web"));
            }
            other => panic!("{other:?}"),
        }

        // A newer image (pulled since): it runs, the older installed versions go.
        assert_eq!(choose_at_start(&dir, v("0.8.2")), Launch::Here);
        assert_eq!(read_state(&dir).current, None);
        assert!(!versions_dir(&dir).join("0.8.1").exists());
    }

    #[test]
    fn a_version_that_did_not_start_is_undone_with_the_database() {
        let dir = temp_dir("rollback");
        std::fs::write(dir.join("breezone.db"), b"migrated by 0.9.0").unwrap();
        std::fs::write(dir.join("breezone.db-wal"), b"wal").unwrap();
        let backup = dir.join("backup.db");
        std::fs::write(&backup, b"before the update").unwrap();
        install_fake(&dir, "0.8.1");
        install_fake(&dir, "0.9.0");
        write_state(
            &dir,
            &State { current: Some("0.9.0".into()), previous: Some("0.8.1".into()), pending: Some(Pending { attempts: 1, backup: Some(backup) }), ..Default::default() },
        )
        .unwrap();

        // 0.9.0 stopped before being confirmed: back to 0.8.1, with the database of before.
        match choose_at_start(&dir, v("0.8.0")) {
            Launch::Switch { version, .. } => assert_eq!(version, "0.8.1"),
            other => panic!("{other:?}"),
        }
        assert_eq!(std::fs::read(dir.join("breezone.db")).unwrap(), b"before the update");
        assert!(!dir.join("breezone.db-wal").exists());
        let state = read_state(&dir);
        assert_eq!(state.failed, vec!["0.9.0".to_string()]);
        assert_eq!(state.rolled_back, Some(RolledBack { version: "0.9.0".into(), to: "0.8.1".into() }));
        assert_eq!(take_rolled_back(&dir).map(|r| r.version), Some("0.9.0".into()));
        assert_eq!(take_rolled_back(&dir), None, "reported once");
    }

    #[test]
    fn a_confirmed_version_keeps_running_and_old_files_go() {
        let dir = temp_dir("confirm");
        for version in ["0.8.1", "0.8.2", "0.9.0"] {
            install_fake(&dir, version);
        }
        std::fs::create_dir_all(backups_dir(&dir)).unwrap();
        for name in ["a.db", "b.db", "c.db"] {
            std::fs::write(backups_dir(&dir).join(name), b"x").unwrap();
        }
        write_state(&dir, &State { current: Some("0.9.0".into()), previous: Some("0.8.2".into()), pending: Some(Pending { attempts: 1, backup: None }), ..Default::default() })
            .unwrap();
        assert!(!confirm(&dir, v("0.8.0")), "another version runs");
        assert!(confirm(&dir, v("0.9.0")));
        assert_eq!(read_state(&dir).pending, None);
        assert!(!versions_dir(&dir).join("0.8.1").exists());
        assert!(versions_dir(&dir).join("0.8.2").exists() && versions_dir(&dir).join("0.9.0").exists());
        assert_eq!(std::fs::read_dir(backups_dir(&dir)).unwrap().count(), 2);
        assert!(matches!(choose_at_start(&dir, v("0.8.0")), Launch::Switch { .. }), "confirmed: started again");
    }

    #[tokio::test]
    async fn backs_up_the_database_in_use() {
        let dir = temp_dir("backup");
        let db = crate::db::connect(&dir.join("breezone.db")).await.unwrap();
        crate::db::insert_user(&db, "admin", "hash", "admin").await.unwrap();
        let path = backup_database(&db, &dir, v("0.8.0")).await.unwrap();
        let copy = crate::db::connect(&path).await.unwrap();
        assert_eq!(crate::db::count_users(&copy).await.unwrap(), 1);
    }

    #[test]
    fn an_older_or_unsigned_release_cannot_be_offered_for_install() {
        let dir = temp_dir("offer");
        let updater = Updater::with_source(dir.clone(), true, Source::official());
        let release = |tag: &str, files: &[&str]| GithubRelease {
            tag_name: tag.into(),
            draft: false,
            body: None,
            published_at: None,
            assets: files.iter().map(|f| GithubAsset { name: (*f).into() }).collect(),
        };
        updater.status.lock().unwrap().latest = Some(release("v0.0.1", &[]));
        let view = updater.view();
        assert!(!view.available && !view.can_install, "older than this build");
        assert_eq!(view.latest, None, "an older release is not shown as the latest");
        updater.status.lock().unwrap().latest = Some(release("v99.0.0", &[]));
        let view = updater.view();
        assert!(view.available);
        assert!(!view.can_install, "no files, or a development build: {view:?}");
        let disabled = Updater::with_source(dir, false, Source::official());
        disabled.status.lock().unwrap().latest = Some(release("v99.0.0", &[]));
        assert_eq!(disabled.view().reason, Some("disabled"));

        updater.status.lock().unwrap().latest = Some(release("v99.0.0-beta.1", &[]));
        let view = updater.view();
        assert!(view.available && view.prerelease);
        assert_eq!(view.latest.as_deref(), Some("99.0.0-beta.1"));
        // Changing channel forgets the release found on the other one.
        updater.set_beta(true);
        assert!(updater.view().beta && updater.view().latest.is_none());
    }
}
