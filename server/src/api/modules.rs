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
    /// The modules of this source only.
    #[serde(default)]
    pub repository: Option<i64>,
}

/// A source of modules that could not be read, for the settings.
#[derive(Serialize)]
pub struct Unreadable {
    pub repository: crate::modules::library::RepositoryRef,
    pub error: String,
}

#[derive(Serialize)]
pub struct LibraryAnswer {
    /// The modules of every source switched on, those that fit this server first.
    pub modules: Vec<crate::modules::library::LibraryView>,
    /// The sources that could not be read, and why.
    pub unreadable: Vec<Unreadable>,
}

fn reference(r: &crate::modules::library::Repository) -> crate::modules::library::RepositoryRef {
    crate::modules::library::RepositoryRef { id: r.id, name: r.name.clone(), official: r.official }
}

/// The library sorted for this server, every source switched on together.
pub async fn library(State(state): State<AppState>, _admin: AdminUser, axum::extract::Query(q): axum::extract::Query<LibraryQuery>) -> ApiResult<Json<LibraryAnswer>> {
    let rows = db::list_modules(&state.db).await?;
    let installed = |id: &str| rows.iter().find(|r| r.id == id).filter(|r| r.origin != "native").map(|r| r.version.clone());
    let host = crate::capabilities::Host::local();
    let mut answer = LibraryAnswer { modules: vec![], unreadable: vec![] };
    for (repo, index) in state.library.indexes(&state.db, q.refresh).await? {
        if q.repository.is_some_and(|id| id != repo.id) {
            continue;
        }
        match index {
            Ok(index) => answer.modules.extend(crate::modules::library::view(&reference(&repo), &index, &host, env!("CARGO_PKG_VERSION"), &installed)),
            Err(error) => answer.unreadable.push(Unreadable { repository: reference(&repo), error }),
        }
    }
    answer.modules.sort_by_key(|v| (v.fits.is_none(), !v.repository.official, v.manifest.name.fr.to_lowercase()));
    Ok(Json(answer))
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
    /// The source to take it from (the one it was installed from, else the official, else the
    /// first that has it, when absent).
    #[serde(default)]
    pub repository: Option<i64>,
    /// The newest that fits when absent.
    #[serde(default)]
    pub version: Option<String>,
}

/// After a module changed version: its sources run the new one, which stays `pending` until they
/// read their hardware; if none does, the version before comes back and this one is given up.
async fn confirm(state: &AppState, id: &str, version: &str) -> ApiResult<()> {
    let Some(row) = db::module_row(&state.db, id).await? else { return Ok(()) };
    db::set_module_state(&state.db, id, "pending", None).await?;
    match state.drivers.restart_module(id).await? {
        Some(false) if row.previous_version.is_some() => {
            let why = format!("{version} did not read its hardware: back to the version before");
            crate::modules::log(id, "error", why.clone());
            db::reject_version(&state.db, id, version).await?;
            crate::modules::install::rollback(&state.db, &state.cfg.data_dir, id).await?;
            state.drivers.restart_module(id).await?;
            db::set_module_state(&state.db, id, "installed", Some(&why)).await?;
        }
        _ => db::set_module_state(&state.db, id, "installed", None).await?,
    }
    Ok(())
}

/// Installs (or updates) a module from the library.
pub async fn install(State(state): State<AppState>, AdminUser(user): AdminUser, Json(input): Json<InstallInput>) -> ApiResult<(axum::http::StatusCode, Json<ModuleView>)> {
    use crate::error::ApiError;
    let from = db::module_repository(&state.db, &input.id).await?;
    let mut candidates = vec![];
    for (repo, index) in state.library.indexes(&state.db, false).await? {
        let Ok(index) = index else { continue };
        if index.entry(&input.id).is_some() {
            candidates.push((repo, index));
        }
    }
    let wanted = input.repository.or(from);
    let pick = match wanted {
        Some(id) => candidates.into_iter().find(|(r, _)| r.id == id),
        None => {
            candidates.sort_by_key(|(r, _)| !r.official);
            candidates.into_iter().next()
        }
    };
    let (repo, index) = pick.ok_or_else(|| ApiError::not_found("unknown module"))?;
    let entry = index.entry(&input.id).expect("picked for having it");
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
    let bytes = state.library.fetch(&repo, &published.file, crate::modules::package::MAX_PACKAGE).await.map_err(unreachable)?;
    let signature = state.library.fetch(&repo, &format!("{}.sig", published.file), 1024).await.map_err(unreachable)?;
    let (_, version) = crate::modules::install::install(
        &state.db,
        &state.cfg.data_dir,
        &bytes,
        &String::from_utf8_lossy(&signature),
        &repo,
        "library",
        Some((&entry.id, &published.manifest.version)),
    )
    .await?;
    confirm(&state, &entry.id, &version).await?;
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
    // Signed by the key of one of the sources switched on: the official one first.
    let repositories: Vec<_> = state.library.repositories(&state.db).await?.into_iter().filter(|r| r.enabled).collect();
    let mut last = None;
    let mut installed = None;
    for repo in &repositories {
        match crate::modules::install::install(&state.db, &state.cfg.data_dir, &bytes, &input.signature, repo, "file", None).await {
            Ok(done) => {
                installed = Some(done);
                break;
            }
            // Another key may sign it; any other refusal is the answer.
            Err(e) if e.status == axum::http::StatusCode::FORBIDDEN => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    let Some((id, version)) = installed else { return Err(last.unwrap_or_else(|| crate::error::ApiError::forbidden("no source of modules"))) };
    confirm(&state, &id, &version).await?;
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
    state.drivers.restart_module(&id).await?;
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
    let mut out = vec![];
    // Those that come with Breezone come with the next version too.
    for row in db::list_modules(&state.db).await?.into_iter().filter(|r| r.origin != "native" && r.origin != "builtin") {
        let repo = match db::module_repository(&state.db, &row.id).await? {
            Some(id) => state.library.repository(&state.db, id).await?,
            None => Some(state.library.official(&state.db).await?),
        };
        let index = match repo.filter(|r| r.enabled) {
            Some(r) => state.library.index(&state.db, &r, false).await.ok(),
            None => None,
        };
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

/// The log of a module: what it said, and what befell it (stops, calls refused), the latest lines.
pub async fn log(_admin: AdminUser, axum::extract::Path(id): axum::extract::Path<String>) -> ApiResult<Json<Vec<crate::modules::LogLine>>> {
    if crate::modules::manifest(&id).is_none() {
        return Err(crate::error::ApiError::not_found("unknown module"));
    }
    Ok(Json(crate::modules::logs(&id)))
}

/// A source of modules as the settings show it.
#[derive(Serialize)]
pub struct RepositoryView {
    #[serde(flatten)]
    pub repository: crate::modules::library::Repository,
    /// The modules installed from it.
    pub modules: Vec<String>,
}

async fn repository_views(state: &AppState) -> ApiResult<Vec<RepositoryView>> {
    let mut out = vec![];
    let rows = db::list_modules(&state.db).await?;
    for repo in state.library.repositories(&state.db).await? {
        let mut modules = vec![];
        for r in &rows {
            if db::module_repository(&state.db, &r.id).await? == Some(repo.id) {
                modules.push(r.id.clone());
            }
        }
        out.push(RepositoryView { repository: repo, modules });
    }
    Ok(out)
}

/// The sources of modules: the official one, and those added.
pub async fn repositories(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<RepositoryView>>> {
    Ok(Json(repository_views(&state).await?))
}

#[derive(serde::Deserialize)]
pub struct RepositoryInput {
    pub name: String,
    /// The folder of its `index.json` (HTTPS, or HTTP on the local network).
    pub url: String,
    /// Its Ed25519 public key, in base64.
    pub public_key: String,
}

/// Adds a source of modules: its index must be readable and signed by its key.
pub async fn add_repository(
    State(state): State<AppState>,
    AdminUser(user): AdminUser,
    Json(input): Json<RepositoryInput>,
) -> ApiResult<(axum::http::StatusCode, Json<Vec<RepositoryView>>)> {
    use crate::error::ApiError;
    let name: String = input.name.trim().chars().take(60).collect();
    let url = input.url.trim().trim_end_matches('/').to_string();
    if name.is_empty() {
        return Err(ApiError::bad_request(crate::text!("le nom est vide", "the name is empty", "el nombre está vacío")));
    }
    if !crate::modules::library::repository_url_ok(&url) {
        return Err(ApiError::bad_request(crate::text!(
            "L'adresse doit être en HTTPS, ou en HTTP sur le réseau local.",
            "The address must be HTTPS, or HTTP on the local network.",
            "La dirección debe ser HTTPS, o HTTP en la red local."
        )));
    }
    if !crate::modules::library::public_key_ok(&input.public_key) {
        return Err(ApiError::bad_request(crate::text!(
            "La clé publique doit être une clé Ed25519 en base64 (32 octets).",
            "The public key must be an Ed25519 key in base64 (32 bytes).",
            "La clave pública debe ser una clave Ed25519 en base64 (32 bytes)."
        )));
    }
    let id = db::insert_repository(&state.db, &name, &url, input.public_key.trim()).await?;
    let repo = state.library.repository(&state.db, id).await?.expect("just added");
    if let Err(e) = state.library.index(&state.db, &repo, true).await {
        db::delete_repository(&state.db, id).await?;
        return Err(ApiError::new(
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            crate::text!("Cette source ne répond pas comme il faut : {0}", "This source does not answer as it should: {0}", "Esta fuente no responde como debe: {0}", e),
        ));
    }
    logged(&state, &user.username, crate::text!("Source de modules « {0} » ajoutée", "Source of modules « {0} » added", "Fuente de módulos « {0} » añadida", name)).await;
    Ok((axum::http::StatusCode::CREATED, Json(repository_views(&state).await?)))
}

#[derive(serde::Deserialize)]
pub struct RepositoryEnabled {
    pub enabled: bool,
}

/// Switches a source on or off (its modules stay, without updates while it is off).
pub async fn set_repository(
    State(state): State<AppState>,
    _admin: AdminUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(input): Json<RepositoryEnabled>,
) -> ApiResult<Json<Vec<RepositoryView>>> {
    if !db::set_repository_enabled(&state.db, id, input.enabled).await? {
        return Err(crate::error::ApiError::not_found("unknown source"));
    }
    state.library.forget(id).await;
    Ok(Json(repository_views(&state).await?))
}

/// Removes a source no installed module comes from (the official one stays).
pub async fn remove_repository(State(state): State<AppState>, AdminUser(user): AdminUser, axum::extract::Path(id): axum::extract::Path<i64>) -> ApiResult<axum::http::StatusCode> {
    use crate::error::ApiError;
    let views = repository_views(&state).await?;
    let view = views.iter().find(|v| v.repository.id == id).ok_or_else(|| ApiError::not_found("unknown source"))?;
    if view.repository.official {
        return Err(ApiError::bad_request("the library of Breezone stays"));
    }
    if !view.modules.is_empty() {
        return Err(ApiError::new(
            axum::http::StatusCode::CONFLICT,
            crate::text!(
                "Des modules viennent de cette source ({0}) : retirez-les d'abord.",
                "Modules come from this source ({0}): remove them first.",
                "Hay módulos de esta fuente ({0}): quítelos primero.",
                view.modules.join(", ")
            ),
        ));
    }
    let name = view.repository.name.clone();
    db::delete_repository(&state.db, id).await?;
    state.library.forget(id).await;
    logged(&state, &user.username, crate::text!("Source de modules « {0} » retirée", "Source of modules « {0} » removed", "Fuente de módulos « {0} » quitada", name)).await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
