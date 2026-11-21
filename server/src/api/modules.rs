//! The modules of this server (docs/roadmap/modules.md § 5), for administrators: what each is, how
//! it came, and whether it runs now (a module is loaded only for a source or an account that uses
//! it).

use crate::auth::AdminUser;
use crate::db;
use crate::error::ApiResult;
use crate::modules::manifest::{Manifest, Role};
use crate::AppState;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct ModuleView {
    pub id: String,
    pub version: String,
    /// "native" | "builtin" | "library" | "file"
    pub origin: String,
    pub enabled: bool,
    /// "installed" | "pending" | "incompatible" | "error"
    pub state: String,
    pub last_error: Option<String>,
    /// Its sources (enabled or not).
    pub sources: usize,
    /// Running now: a source reads its hardware, or its account or service is on.
    pub loaded: bool,
    pub manifest: &'static Manifest,
}

pub async fn list(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<ModuleView>>> {
    Ok(Json(views(&state).await?))
}

async fn views(state: &AppState) -> ApiResult<Vec<ModuleView>> {
    let rows = db::list_modules(&state.db).await?;
    let sources = db::list_sources(&state.db).await?;
    let loaded = state.drivers.loaded().await;
    let accounts = crate::accounts::list(state).await?;
    let mut out = vec![];
    for m in crate::modules::manifests() {
        let Some(row) = rows.iter().find(|r| r.id == m.id) else { continue };
        let account_on = (m.has(Role::Account) || m.has(Role::Service)) && accounts.iter().any(|a| a.kind == m.id && a.connected && a.enabled);
        out.push(ModuleView {
            id: m.id.clone(),
            version: row.version.clone(),
            origin: row.origin.clone(),
            enabled: row.enabled,
            state: row.state.clone(),
            last_error: row.last_error.clone(),
            sources: sources.iter().filter(|s| s.driver == m.id).count(),
            loaded: loaded.contains_key(&m.id) || account_on,
            manifest: m,
        });
    }
    Ok(out)
}

#[derive(serde::Deserialize)]
pub struct LibraryQuery {
    /// Read the index again now.
    #[serde(default)]
    pub refresh: bool,
}

/// The library sorted for this server: the modules that fit first, then the others with why.
pub async fn library(
    State(state): State<AppState>,
    _admin: AdminUser,
    axum::extract::Query(q): axum::extract::Query<LibraryQuery>,
) -> ApiResult<Json<Vec<crate::modules::library::LibraryView>>> {
    let index = state.library.index(&state.db, q.refresh).await.map_err(unreachable)?;
    let rows = db::list_modules(&state.db).await?;
    let installed = |id: &str| rows.iter().find(|r| r.id == id).map(|r| r.version.clone());
    Ok(Json(crate::modules::library::view(&index, &crate::capabilities::Host::local(), env!("CARGO_PKG_VERSION"), &installed)))
}

fn unreachable(e: String) -> crate::error::ApiError {
    crate::error::ApiError::bad_gateway(crate::text!(
        "Bibliothèque des modules injoignable : {0}",
        "Library of modules unreachable: {0}",
        "Biblioteca de módulos inaccesible: {0}",
        e
    ))
}

#[derive(serde::Deserialize)]
pub struct InstallInput {
    pub id: String,
    /// The newest that fits when absent.
    #[serde(default)]
    pub version: Option<String>,
}

/// Installs (or updates) a module from the library.
pub async fn install(State(state): State<AppState>, AdminUser(user): AdminUser, Json(input): Json<InstallInput>) -> ApiResult<(axum::http::StatusCode, Json<ModuleView>)> {
    use crate::error::ApiError;
    let index = state.library.index(&state.db, false).await.map_err(unreachable)?;
    let entry = index.entry(&input.id).ok_or_else(|| ApiError::not_found("unknown module"))?;
    let fits = |a: &str| crate::modules::manifest(a).is_some();
    let published = match &input.version {
        Some(v) => entry.version(v).filter(|p| !p.yanked).ok_or_else(|| ApiError::not_found("unknown version"))?,
        None => entry.best(&crate::capabilities::Host::local(), env!("CARGO_PKG_VERSION"), &fits).ok_or_else(|| {
            ApiError::new(
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                crate::text!(
                    "Aucune version de ce module ne fonctionne avec ce serveur.",
                    "No version of this module works with this server.",
                    "Ninguna versión de este módulo funciona con este servidor."
                ),
            )
        })?,
    };
    let bytes = state.library.fetch(&published.file, crate::modules::package::MAX_PACKAGE).await.map_err(unreachable)?;
    let signature = state.library.fetch(&format!("{}.sig", published.file), 1024).await.map_err(unreachable)?;
    let (_, version) = crate::modules::install::install(
        &state.db,
        &state.cfg.data_dir,
        &bytes,
        &String::from_utf8_lossy(&signature),
        state.library.public_key(),
        "library",
        Some((&entry.id, &published.manifest.version)),
    )
    .await?;
    logged(&state, &user.username, crate::text!("Module {0} {1} installé", "Module {0} {1} installed", "Módulo {0} {1} instalado", entry.id.clone(), version)).await;
    Ok((axum::http::StatusCode::CREATED, Json(view(&state, &entry.id).await?)))
}

#[derive(serde::Deserialize)]
pub struct UploadInput {
    /// The `.bzm` package, in base64.
    pub package: String,
    /// Its `.bzm.sig` signature.
    pub signature: String,
}

/// Installs a module from a package sent by hand (signed like the others: no exception).
pub async fn upload(State(state): State<AppState>, AdminUser(user): AdminUser, Json(input): Json<UploadInput>) -> ApiResult<(axum::http::StatusCode, Json<ModuleView>)> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(input.package.trim()).map_err(|_| crate::error::ApiError::bad_request("the package is not base64"))?;
    let (id, version) = crate::modules::install::install(&state.db, &state.cfg.data_dir, &bytes, &input.signature, state.library.public_key(), "file", None).await?;
    logged(
        &state,
        &user.username,
        crate::text!("Module {0} {1} installé depuis un fichier", "Module {0} {1} installed from a file", "Módulo {0} {1} instalado desde un archivo", id.clone(), version),
    )
    .await;
    Ok((axum::http::StatusCode::CREATED, Json(view(&state, &id).await?)))
}

pub async fn rollback(State(state): State<AppState>, AdminUser(user): AdminUser, axum::extract::Path(id): axum::extract::Path<String>) -> ApiResult<Json<ModuleView>> {
    let version = crate::modules::install::rollback(&state.db, &state.cfg.data_dir, &id).await?;
    logged(&state, &user.username, crate::text!("Module {0} revenu à {1}", "Module {0} back to {1}", "Módulo {0} vuelto a {1}", id.clone(), version)).await;
    Ok(Json(view(&state, &id).await?))
}

pub async fn remove(State(state): State<AppState>, AdminUser(user): AdminUser, axum::extract::Path(id): axum::extract::Path<String>) -> ApiResult<axum::http::StatusCode> {
    crate::modules::install::uninstall(&state.db, &state.cfg.data_dir, &id).await?;
    logged(&state, &user.username, crate::text!("Module {0} retiré", "Module {0} removed", "Módulo {0} quitado", id)).await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn logged(state: &AppState, who: &str, text: crate::i18n::Text) {
    state.hub.log("system", "info", None, Some(who), text, None).await;
}

async fn view(state: &AppState, id: &str) -> ApiResult<ModuleView> {
    views(state).await?.into_iter().find(|m| m.id == id).ok_or_else(|| crate::error::ApiError::not_found("unknown module"))
}

#[derive(serde::Deserialize)]
pub struct NextQuery {
    /// The version of Breezone to update to.
    pub version: String,
}

/// What becomes of a module installed here with another version of Breezone.
#[derive(Serialize)]
pub struct NextView {
    pub id: String,
    pub version: String,
    /// It works as it is with that version.
    pub fits: bool,
    pub reasons: Vec<crate::modules::compat::Incompatibility>,
    /// The version of the library that works with it, if another one does (installed with it).
    pub replacement: Option<String>,
}

/// Before updating Breezone: whether each module installed here works with `version` (what it
/// hosts is published beside its archives, `host.json`), and which version of the library does.
pub async fn next(State(state): State<AppState>, _admin: AdminUser, axum::extract::Query(q): axum::extract::Query<NextQuery>) -> ApiResult<Json<Vec<NextView>>> {
    use crate::error::ApiError;
    let target = crate::update::Version::parse(&q.version).ok_or_else(|| ApiError::bad_request("not a version"))?;
    let bytes = state.library.fetch_release_file(target, "host.json", 1 << 20).await.map_err(unreachable)?;
    let host: crate::capabilities::Host = serde_json::from_slice(&bytes).map_err(|e| unreachable(e.to_string()))?;
    let running = target.to_string();
    let index = state.library.index(&state.db, false).await.ok();
    let mut out = vec![];
    for row in db::list_modules(&state.db).await?.into_iter().filter(|r| r.origin != "native") {
        let Some(m) = crate::modules::manifest(&row.id) else { continue };
        let fits_account = |a: &str| crate::modules::manifest(a).is_some();
        let code = state.cfg.data_dir.join("modules").join(&row.id).join(&row.version).join("module.wasm").is_file();
        let mut checked = host.clone();
        if !code {
            checked.api = vec![crate::modules::INTERFACE.to_string()];
        }
        let reasons = crate::modules::compat::check(m, &checked, &running, fits_account).err().unwrap_or_default();
        let replacement = if reasons.is_empty() {
            None
        } else {
            index.as_ref().and_then(|i| i.entry(&row.id)).and_then(|e| e.best(&host, &running, &fits_account)).map(|p| p.manifest.version.clone())
        };
        out.push(NextView { id: row.id, version: row.version, fits: reasons.is_empty(), reasons, replacement });
    }
    Ok(Json(out))
}
