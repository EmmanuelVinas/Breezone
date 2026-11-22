//! Installing, updating, going back and removing a module (docs/roadmap/modules.md § 4.8), on the
//! model of the updates of Breezone: the package is checked (signature, manifest, compatibility)
//! before anything is written, written in a folder of its own, and the version before is kept to
//! go back to.

use super::package::{Package, PackageError};
use crate::db::{self, Db};
use crate::error::ApiError;
use axum::http::StatusCode;
use std::path::Path;

/// "builtin" (the archive of Breezone) | "library" | "file" (sent by an administrator).
pub type Origin = &'static str;

fn refused(e: PackageError) -> ApiError {
    let status = if matches!(e, PackageError::Signature) { StatusCode::FORBIDDEN } else { StatusCode::UNPROCESSABLE_ENTITY };
    ApiError::new(status, crate::text!("Module refusé : {0}", "Module refused: {0}", "Módulo rechazado: {0}", e.to_string()))
}

/// Installs a signed package (`public_key`: the key of the releases) and takes it into account.
/// `expected`: the module and version asked for (from the library).
pub async fn install(
    db: &Db,
    data_dir: &Path,
    bytes: &[u8],
    signature: &str,
    from: &super::library::Repository,
    origin: Origin,
    expected: Option<(&str, &str)>,
) -> Result<(String, String), ApiError> {
    let package = Package::open(bytes, signature, &from.public_key, expected).map_err(refused)?;
    install_package(db, data_dir, package, origin, Some(from)).await
}

/// Installs a package already checked (signed, or one of those that come with Breezone, whose
/// archive is signed as a whole).
/// `from`: the source it comes from (`None`: it comes with Breezone).
pub async fn install_package(db: &Db, data_dir: &Path, package: Package, origin: Origin, from: Option<&super::library::Repository>) -> Result<(String, String), ApiError> {
    let m = &package.manifest;
    let official = from.is_none_or(|r| r.official);
    // A source other than Breezone's never takes the place of a module that came from elsewhere
    // (one that came with Breezone among them): its updates come from where it was installed from.
    if let Some(installed) = db::module_row(db, &m.id).await?.filter(|r| r.origin != "native") {
        let was = db::module_repository(db, &m.id).await?;
        let now = from.map(|r| r.id);
        // The same source again, or Breezone's library taking over a module that came with Breezone.
        let allowed = was == now || (official && installed.origin == "builtin");
        if !allowed {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                crate::text!(
                    "Le module {0} vient d'une autre source : retirez-le d'abord pour le prendre ici.",
                    "The {0} module comes from another source: remove it first to take it from here.",
                    "El módulo {0} viene de otra fuente: quítelo primero para tomarlo de aquí.",
                    m.id.clone()
                ),
            ));
        }
    }
    if db::rejected_versions(db, &m.id).await?.contains(&m.version) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            crate::text!(
                "La version {0} de ce module a déjà échoué ici : elle n'est plus installée.",
                "Version {0} of this module already failed here: it is not installed again.",
                "La versión {0} de este módulo ya falló aquí: no se vuelve a instalar.",
                m.version.clone()
            ),
        ));
    }
    let host = host_for(package.wasm.is_some());
    if let Err(reasons) = super::compat::check(m, &host, env!("CARGO_PKG_VERSION"), |a| super::manifest(a).is_some()) {
        let why = reasons.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ; ");
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            crate::text!(
                "Ce module ne fonctionne pas avec ce serveur : {0}",
                "This module does not work with this server: {0}",
                "Este módulo no funciona con este servidor: {0}",
                why
            ),
        )
        .with_detail("reasons", &reasons));
    }
    let before = db::module_row(db, &m.id).await?;
    let previous = before.as_ref().map(|r| r.version.clone()).filter(|v| *v != m.version);
    let dir = super::modules_dir(data_dir);
    package.unpack(&dir).map_err(|e| ApiError::internal(format!("cannot write the module: {e}")))?;
    db::save_module(db, &m.id, &m.version, previous.as_deref(), origin).await?;
    db::set_module_repository(db, &m.id, from.map(|r| r.id)).await?;
    keep_only(&dir.join(&m.id), &[Some(m.version.as_str()), previous.as_deref()]);
    tracing::info!(module = m.id, version = m.version, origin, "module installed");
    super::register(package.manifest.clone(), dir.join(&m.id).join(&m.version));
    Ok((m.id.clone(), m.version.clone()))
}

/// What a module is checked against: a module without code needs no module host.
fn host_for(code: bool) -> crate::capabilities::Host {
    let mut host = crate::capabilities::Host::local();
    if !code {
        host.api = vec![super::INTERFACE.to_string()];
    }
    host
}

/// Removes the versions of a module but those given.
fn keep_only(dir: &Path, keep: &[Option<&str>]) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !keep.iter().flatten().any(|k| *k == name) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Goes back to the version installed before.
pub async fn rollback(db: &Db, data_dir: &Path, id: &str) -> Result<String, ApiError> {
    let row = db::module_row(db, id).await?.ok_or_else(|| ApiError::not_found("unknown module"))?;
    let previous = row.previous_version.clone().ok_or_else(|| ApiError::bad_request("no version to go back to"))?;
    let manifest = read_manifest(data_dir, id, &previous).ok_or_else(|| ApiError::bad_request("the version before is no longer here"))?;
    db::save_module(db, id, &previous, None, if row.origin == "native" { "library" } else { leak_origin(&row.origin) }).await?;
    keep_only(&super::modules_dir(data_dir).join(id), &[Some(previous.as_str())]);
    super::register(manifest, super::modules_dir(data_dir).join(id).join(&previous));
    tracing::info!(module = id, from = row.version, to = previous, "module rolled back");
    Ok(previous)
}

fn leak_origin(origin: &str) -> Origin {
    match origin {
        "builtin" => "builtin",
        "file" => "file",
        "dev" => "dev",
        _ => "library",
    }
}

/// Removes a module no source and no account uses (not one that comes with Breezone: it would be
/// installed again at the next start).
pub async fn uninstall(db: &Db, data_dir: &Path, id: &str) -> Result<(), ApiError> {
    if db::module_row(db, id).await?.is_some_and(|r| r.origin == "builtin") {
        return Err(ApiError::bad_request(crate::text!(
            "Ce module est livré avec Breezone : il ne se retire pas.",
            "This module comes with Breezone: it cannot be removed.",
            "Este módulo viene con Breezone: no se puede quitar."
        )));
    }
    let used = db::list_sources(db).await?.iter().filter(|s| s.driver == id).count();
    let account = db::list_accounts(db).await?.iter().any(|a| a.kind == id && a.secret.is_some());
    if used > 0 || account {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            crate::text!(
                "Ce module sert encore : retirez d'abord ses modules et son compte.",
                "This module is still used: remove its sources and its account first.",
                "Este módulo aún se usa: quite primero sus fuentes y su cuenta."
            ),
        ));
    }
    if !db::delete_module(db, id).await? {
        return Err(ApiError::not_found("unknown module"));
    }
    let _ = std::fs::remove_dir_all(super::modules_dir(data_dir).join(id));
    super::unregister(id);
    tracing::info!(module = id, "module removed");
    Ok(())
}

fn read_manifest(data_dir: &Path, id: &str, version: &str) -> Option<super::manifest::Manifest> {
    let text = std::fs::read_to_string(super::modules_dir(data_dir).join(id).join(version).join("manifest.json")).ok()?;
    super::manifest::Manifest::parse(&text).ok()
}

/// At start: installs the modules that come with Breezone (`<dir>/*.bzm` and their `.sig`, beside
/// the web UI) when they are missing here or newer than the one installed, without the network.
pub async fn install_builtin(db: &Db, data_dir: &Path, dir: &Path, public_key: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for path in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "bzm")) {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        // Beside Breezone itself, in the image or in its signed archive: trusted like the binary.
        // A signature, when there is one, is checked all the same.
        let opened = match std::fs::read_to_string(path.with_extension("bzm.sig")) {
            Ok(sig) => Package::open(&bytes, &sig, public_key, None),
            Err(_) => Package::trusted(&bytes),
        };
        let Ok(package) = opened else {
            tracing::warn!("{}: not a module of Breezone, left out", path.display());
            continue;
        };
        let installed = db::module_row(db, &package.manifest.id).await.ok().flatten();
        // Missing here (or its files gone), or older than this one.
        let id = package.manifest.id.clone();
        let newer = installed.as_ref().is_none_or(|r| {
            r.origin == "native" || semver::Version::parse(&r.version).ok() < Some(package.manifest.semver()) || !super::modules_dir(data_dir).join(&id).join(&r.version).is_dir()
        });
        if newer {
            if let Err(e) = install_package(db, data_dir, package, "builtin", None).await {
                tracing::warn!("{}: {}", path.display(), e.message.en);
            }
        }
    }
}

/// At start, in a development build: installs the packages of the folder of modules being
/// developed (`BREEZONE_MODULES_DEV`), without a signature, each time (a new build of the same
/// version replaces the one before). Never read by a release build.
pub async fn install_dev(db: &Db, data_dir: &Path, dir: &Path) {
    if !cfg!(debug_assertions) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        tracing::warn!("{}: no folder of modules in development", dir.display());
        return;
    };
    for path in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "bzm")) {
        let opened = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| Package::trusted(&b).map_err(|e| e.to_string()));
        match opened {
            Ok(package) => match install_package(db, data_dir, package, "dev", None).await {
                Ok((id, version)) => tracing::warn!(module = id, version, "module in development installed, not signed"),
                Err(e) => tracing::warn!("{}: {}", path.display(), e.message.en),
            },
            Err(e) => tracing::warn!("{}: {e}", path.display()),
        }
    }
}

/// At start: takes the installed modules into account, and marks those that no longer fit this
/// version of Breezone (they are kept, switched off, until a version that fits is installed).
pub async fn load_installed(db: &Db, data_dir: &Path) -> sqlx::Result<()> {
    for row in db::list_modules(db).await?.into_iter().filter(|r| r.origin != "native") {
        let host = host_for(super::modules_dir(data_dir).join(&row.id).join(&row.version).join("module.wasm").is_file());
        let Some(manifest) = read_manifest(data_dir, &row.id, &row.version) else {
            db::set_module_state(db, &row.id, "error", Some("its files are missing")).await?;
            continue;
        };
        match super::compat::check(&manifest, &host, env!("CARGO_PKG_VERSION"), |a| super::manifest(a).is_some() || a == row.id) {
            Ok(()) => db::set_module_state(db, &row.id, "installed", None).await?,
            Err(reasons) => {
                let why = reasons.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
                tracing::warn!(module = row.id, "module switched off: {why}");
                db::set_module_state(db, &row.id, "incompatible", Some(&why)).await?;
            }
        }
        super::register(manifest, super::modules_dir(data_dir).join(&row.id).join(&row.version));
    }
    Ok(())
}

/// After an update of Breezone: the modules that no longer fit are replaced by the newest version
/// of the library that does, when there is one (they stay switched off otherwise).
pub async fn replace_incompatible(db: &Db, data_dir: &Path, library: &super::library::Library) {
    let Ok(rows) = db::list_modules(db).await else { return };
    let stale: Vec<_> = rows.into_iter().filter(|r| r.state == "incompatible" && r.auto_update && r.origin != "native").collect();
    if stale.is_empty() {
        return;
    }
    let host = crate::capabilities::Host::local();
    for row in stale {
        let repo = match db::module_repository(db, &row.id).await.ok().flatten() {
            Some(id) => library.repository(db, id).await.ok().flatten(),
            None => library.official(db).await.ok(),
        };
        let Some(repo) = repo.filter(|r| r.enabled) else { continue };
        let index = match library.index(db, &repo, false).await {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(module = row.id, "not replaced: {e}");
                continue;
            }
        };
        let Some(published) = index.entry(&row.id).and_then(|e| e.best(&host, env!("CARGO_PKG_VERSION"), &|a| super::manifest(a).is_some())) else { continue };
        let fetched = async {
            let bytes = library.fetch(&repo, &published.file, super::package::MAX_PACKAGE).await?;
            let sig = library.fetch(&repo, &format!("{}.sig", published.file), 1024).await?;
            Ok::<_, String>((bytes, sig))
        };
        match fetched.await {
            Ok((bytes, sig)) => match install(db, data_dir, &bytes, &String::from_utf8_lossy(&sig), &repo, "library", Some((&row.id, &published.manifest.version))).await {
                Ok(_) => tracing::info!(module = row.id, version = published.manifest.version, "module replaced by a version that fits"),
                Err(e) => tracing::warn!(module = row.id, "not replaced: {}", e.message.en),
            },
            Err(e) => tracing::warn!(module = row.id, "not replaced: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::package::tests::{keys, sign};
    use crate::modules::package::{build, signed_message};

    fn manifest(id: &str, version: &str, api: &str) -> String {
        let mut m = crate::modules::manifest::example();
        m.id = id.into();
        m.version = version.into();
        m.requires.api = api.into();
        m.requires.breezone = None;
        serde_json::to_string(&m).unwrap()
    }

    #[tokio::test]
    async fn installed_updated_rolled_back_and_removed() {
        let dir = std::env::temp_dir().join(format!("breezone-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = db::connect(&dir.join("t.db")).await.unwrap();
        let (key, public) = keys();
        let official = crate::modules::library::Repository { id: 1, name: "Breezone".into(), url: None, public_key: public.clone(), official: true, enabled: true };
        let package = |version: &str, api: &str| {
            let bytes = build(&manifest("plugtest", version, api), None, &[]).unwrap();
            let sig = sign(&key, &signed_message("plugtest", version, &bytes));
            (bytes, sig)
        };
        // Data only: it needs no module host, whatever this server hosts.
        let api = "^1.0";

        let (b, s) = package("1.0.0", api);
        assert_eq!(install(&db, &dir, &b, &s, &official, "file", None).await.unwrap().1, "1.0.0");
        assert!(crate::modules::manifest("plugtest").is_some());
        let (b, s) = package("1.1.0", api);
        install(&db, &dir, &b, &s, &official, "library", Some(("plugtest", "1.1.0"))).await.unwrap();
        let row = db::module_row(&db, "plugtest").await.unwrap().unwrap();
        assert_eq!((row.version.as_str(), row.previous_version.as_deref()), ("1.1.0", Some("1.0.0")));
        assert_eq!(crate::modules::manifest("plugtest").unwrap().version, "1.1.0");

        // Refused: an interface this server does not host, a bad signature.
        let (b, s) = package("2.0.0", "^9.0");
        assert_eq!(install(&db, &dir, &b, &s, &official, "file", None).await.unwrap_err().status, StatusCode::UNPROCESSABLE_ENTITY);
        let (b, _) = package("1.2.0", api);
        assert_eq!(install(&db, &dir, &b, "AAAA", &official, "file", None).await.unwrap_err().status, StatusCode::FORBIDDEN);

        assert_eq!(rollback(&db, &dir, "plugtest").await.unwrap(), "1.0.0");
        assert_eq!(crate::modules::manifest("plugtest").unwrap().version, "1.0.0");
        assert!(!dir.join("modules/plugtest/1.1.0").exists());

        // Read again at start.
        crate::modules::unregister("plugtest");
        load_installed(&db, &dir).await.unwrap();
        assert_eq!(crate::modules::manifest("plugtest").unwrap().version, "1.0.0");

        // A module in development: installed without a signature, again at each start.
        let devdir = dir.join("dev");
        std::fs::create_dir_all(&devdir).unwrap();
        std::fs::write(devdir.join("mine.bzm"), build(&manifest("plugmine", "0.1.0", api), None, &[]).unwrap()).unwrap();
        install_dev(&db, &dir, &devdir).await;
        assert_eq!(db::module_row(&db, "plugmine").await.unwrap().map(|r| r.origin), Some("dev".to_string()));
        install_dev(&db, &dir, &devdir).await;
        uninstall(&db, &dir, "plugmine").await.unwrap();

        uninstall(&db, &dir, "plugtest").await.unwrap();
        assert!(crate::modules::manifest("plugtest").is_none() && !dir.join("modules/plugtest").exists());
        assert!(uninstall(&db, &dir, "shelly").await.is_err());
    }
}
