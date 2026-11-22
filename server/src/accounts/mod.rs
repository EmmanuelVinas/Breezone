//! Linked accounts: a connection to an online service, apart from the modules that use it
//! (roadmap F10). Always optional: an account completes what Breezone reads locally, it never
//! replaces it. The secrets (password, client secret, tokens) are encrypted with the data key.
//!
//! - `airzone`: the Airzone Cloud account (names, groups, schedules), see `cloud`;
//! - the modules that are an account or a service (RTE, Linky, Netatmo, EZVIZ, the weather…): their
//!   form comes from their manifest, and they check their account themselves;
//! - `forecast_solar`: the panels (a key optional), set here with the position of the home, which
//!   come with its module.
//!
//! Each can be switched off without being forgotten: what uses it then does as if it were not set.

pub mod forecast_solar;
pub mod linky;

use crate::crypto::{decrypt, encrypt};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::i18n::Text;
use crate::AppState;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::modules::manifest::Role;

/// The kinds of accounts, in the order of Settings › Accounts: the Airzone Cloud account, then the
/// modules that are an account or a service.
pub fn kinds() -> Vec<&'static str> {
    std::iter::once(crate::cloud::ACCOUNT)
        .chain(crate::modules::manifests().filter(|m| (m.has(Role::Account) || m.has(Role::Service)) && !m.internal).map(|m| m.id.as_str()))
        .collect()
}

pub fn is_kind(kind: &str) -> bool {
    kinds().contains(&kind)
}

/// A module whose source comes with its account (added when it is connected, never by hand), and
/// is switched on and off with it.
pub fn comes_with_account(id: &str) -> bool {
    crate::modules::manifest(id).is_some_and(|m| m.has(Role::Account) && m.has(Role::Driver))
}

/// A module that reaches its hardware through its maker's cloud: its devices may be the twins of
/// those of a local module.
pub fn is_cloud(id: &str) -> bool {
    crate::modules::manifest(id).is_some_and(|m| m.cloud)
}

/// A service that asks no credentials (the weather): switched on and off only.
pub fn is_service_only(kind: &str) -> bool {
    crate::modules::manifest(kind).is_some_and(|m| m.has(Role::Service) && !m.has(Role::Account))
}

/// The weather: a linked service without credentials.
pub const WEATHER: &str = "weather";

/// An account as Settings › Accounts shows it: never its secrets.
#[derive(Debug, Serialize)]
pub struct AccountView {
    pub kind: &'static str,
    /// Its credentials are kept (a service without any: always): it can be used.
    pub connected: bool,
    /// Switched on (else what uses it does as if it were not set).
    pub enabled: bool,
    /// A service that asks no credentials.
    pub service: bool,
    /// "ok" | "error" | "expired" | "unknown" (and "none" when not set).
    pub state: String,
    pub last_error: Option<Text>,
    /// When it last worked.
    pub last_ok: Option<i64>,
    /// When the access the service gave ends, if it says.
    pub expires_at: Option<i64>,
    /// What is not secret: an e-mail, a client id, a meter number, a summary of what it reaches.
    pub config: Value,
}

pub async fn list(state: &AppState) -> ApiResult<Vec<AccountView>> {
    let rows = db::list_accounts(&state.db).await?;
    Ok(kinds()
        .into_iter()
        .map(|kind| match rows.iter().find(|r| r.kind == kind) {
            Some(r) => AccountView {
                kind,
                connected: r.secret.is_some() || is_service_only(kind),
                enabled: r.enabled,
                service: is_service_only(kind),
                state: r.state.clone(),
                last_error: r.last_error.as_deref().map(Text::load),
                last_ok: r.last_ok,
                expires_at: r.expires_at,
                config: serde_json::from_str(&r.config).unwrap_or_else(|_| json!({})),
            },
            None => AccountView {
                kind,
                connected: is_service_only(kind),
                enabled: true,
                service: is_service_only(kind),
                state: "none".into(),
                last_error: None,
                last_ok: None,
                expires_at: None,
                config: json!({}),
            },
        })
        .collect())
}

pub async fn view(state: &AppState, kind: &str) -> ApiResult<AccountView> {
    list(state).await?.into_iter().find(|a| a.kind == kind).ok_or_else(|| ApiError::not_found("unknown account"))
}

/// The configuration and decrypted secrets of an account, when it is connected and switched on.
pub async fn load<S: DeserializeOwned>(state: &AppState, kind: &str) -> ApiResult<Option<(Value, S)>> {
    load_in(&state.db, &state.data_key, kind).await
}

/// Same, from the database and key alone (a module that uses an account).
pub async fn load_in<S: DeserializeOwned>(db: &db::Db, key: &crate::crypto::Key, kind: &str) -> ApiResult<Option<(Value, S)>> {
    let Some(row) = db::account(db, kind).await?.filter(|r| r.enabled) else { return Ok(None) };
    let Some(secret) = row.secret.as_deref().and_then(|s| decrypt(key, s)).and_then(|s| serde_json::from_str(&s).ok()) else { return Ok(None) };
    Ok(Some((serde_json::from_str(&row.config).unwrap_or_else(|_| json!({})), secret)))
}

/// Keeps an account: its configuration as it is, its secrets encrypted.
pub async fn save<S: Serialize>(state: &AppState, kind: &str, config: &Value, secret: &S, expires_at: Option<i64>) -> ApiResult<()> {
    save_in(&state.db, &state.data_key, kind, config, secret, expires_at).await
}

pub async fn save_in<S: Serialize>(db: &db::Db, key: &crate::crypto::Key, kind: &str, config: &Value, secret: &S, expires_at: Option<i64>) -> ApiResult<()> {
    let secret = encrypt(key, &serde_json::to_string(secret).unwrap_or_default());
    db::save_account(db, kind, &config.to_string(), Some(&secret), expires_at).await?;
    Ok(())
}

/// Notes how the last use of an account went, and hands the result back.
pub async fn record<T>(state: &AppState, kind: &str, result: ApiResult<T>) -> ApiResult<T> {
    match &result {
        Ok(_) => db::set_account_state(&state.db, kind, "ok", None).await?,
        Err(e) => db::set_account_state(&state.db, kind, "error", Some(&e.message.store())).await?,
    }
    result
}

/// An HTTP client for the services of the accounts.
pub fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).user_agent(concat!("Breezone/", env!("CARGO_PKG_VERSION"))).build().unwrap_or_default()
}

/// What a service said when it refused: its status, and its message when it gives one.
pub async fn refused(service: &str, resp: reqwest::Response) -> ApiError {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let detail: String = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            ["error_description", "message", "detail", "error"]
                .iter()
                .find_map(|k| v.get(k).and_then(Value::as_str).map(str::to_string))
                .or_else(|| v["error"]["message"].as_str().map(str::to_string))
        })
        .unwrap_or_else(|| body.chars().take(160).collect());
    let text = if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        crate::text!("{0} refuse les identifiants ({1})", "{0} refuses the credentials ({1})", "{0} rechaza las credenciales ({1})", service, detail)
    } else {
        crate::text!("{0} répond {1} : {2}", "{0} answers {1}: {2}", "{0} responde {1}: {2}", service, status.as_u16(), detail)
    };
    ApiError::bad_gateway(text)
}

/// The service could not be reached.
pub fn unreachable(service: &str, e: reqwest::Error) -> ApiError {
    ApiError::bad_gateway(crate::text!("{0} injoignable : {1}", "{0} unreachable: {1}", "{0} inaccesible: {1}", service, e.to_string()))
}

/// An account that is a module run by the host (installed, or shipped with Breezone): connected,
/// checked and tested from the fields of its manifest's form.
pub fn is_module_account(kind: &str) -> bool {
    crate::modules::manifest(kind).is_some_and(|m| m.has(Role::Account)) && crate::modules::installed_dir(kind).is_some()
}

/// The fields of the form of a module's account, from what was typed: (what is not secret, all of
/// them). A field not optional must be given.
fn account_fields(m: &crate::modules::manifest::Manifest, input: &Value) -> ApiResult<(Value, Value)> {
    let (mut config, mut all) = (serde_json::Map::new(), serde_json::Map::new());
    for f in &m.config {
        let value = match &input[&f.name] {
            Value::String(s) => Value::String(s.trim().to_string()),
            Value::Null => Value::Null,
            other => other.clone(),
        };
        if value.is_null() || value == Value::String(String::new()) {
            if f.optional {
                continue;
            }
            return Err(ApiError::bad_request(crate::text!("{0} est requis", "{0} is required", "{0} es obligatorio", f.label.fr.clone())));
        }
        if f.kind != "password" {
            config.insert(f.name.clone(), value.clone());
        }
        all.insert(f.name.clone(), value);
    }
    Ok((Value::Object(config), Value::Object(all)))
}

/// Asks the module to check its account's fields (`fields`, as JSON): those to keep (with an
/// OAuth access renewed meanwhile).
async fn check_module_account(kind: &str, fields: &str) -> ApiResult<String> {
    #[cfg(feature = "wasm")]
    {
        let m = crate::modules::manifest(kind).ok_or_else(|| ApiError::not_found("unknown account"))?;
        let dir = crate::modules::installed_dir(kind).ok_or_else(|| ApiError::not_found("unknown account"))?;
        let module = crate::modules::wasm::loaded(m, &dir).await.map_err(ApiError::bad_gateway)?;
        // What was typed is refused (a wrong password): the user's to fix; else the service fails.
        module.check_account(fields).await.map_err(|e| match e {
            crate::drivers::DriverError::Unreachable(m) => ApiError::bad_gateway(m),
            crate::drivers::DriverError::Invalid(m) | crate::drivers::DriverError::Unsupported(m) => ApiError::bad_request(m),
        })
    }
    #[cfg(not(feature = "wasm"))]
    {
        let _ = (kind, fields);
        Err::<String, _>(ApiError::bad_request("this build of Breezone has no module host"))
    }
}

/// What is shown of an account, from what its module kept: the fields of its form that are not
/// secret, as the module kept them (a number without its spaces), and what it gives to be shown
/// (`shown`: the homes the account reaches…), which leaves the secret part.
fn shown(m: &crate::modules::manifest::Manifest, mut config: Value, mut kept: Value) -> (Value, Value) {
    for f in m.config.iter().filter(|f| f.kind != "password") {
        if let Some(v) = kept.get(&f.name).filter(|v| !v.is_null()) {
            config[&f.name] = v.clone();
        }
    }
    if let Some(Value::Object(extra)) = kept.as_object_mut().and_then(|k| k.remove("shown")) {
        for (k, v) in extra {
            config[&k] = v;
        }
    }
    (config, kept)
}

/// Connects the account of a module: the module checks it, then it is kept (what is not secret
/// shown in Settings › Accounts, all the fields encrypted).
pub async fn connect_module(state: &AppState, kind: &str, input: &Value) -> ApiResult<()> {
    let m = crate::modules::manifest(kind).ok_or_else(|| ApiError::not_found("unknown account"))?;
    let (config, all) = account_fields(m, input)?;
    let kept: Value = serde_json::from_str(&check_module_account(kind, &all.to_string()).await?).unwrap_or(all);
    let expires = kept["expires_at"].as_i64();
    let (config, kept) = shown(m, config, kept);
    save(state, kind, &config, &kept, expires).await?;
    db::set_account_state(&state.db, kind, "ok", None).await?;
    ensure_module_source(state, kind).await
}

/// A module whose devices come with its account (Netatmo): its source, added once the account is
/// connected.
pub async fn ensure_module_source(state: &AppState, kind: &str) -> ApiResult<()> {
    let Some(m) = crate::modules::manifest(kind).filter(|_| comes_with_account(kind)) else { return Ok(()) };
    if db::account(&state.db, kind).await?.and_then(|a| a.secret).is_none() || db::list_sources(&state.db).await?.iter().any(|s| s.driver == kind) {
        return Ok(());
    }
    db::insert_source(&state.db, kind, &m.name.fr, "{}").await?;
    state.drivers.reload().await?;
    Ok(())
}

/// Checks the account of a module again, with what is kept.
pub async fn test_module(state: &AppState, kind: &str) -> ApiResult<()> {
    let Some((config, fields)) = load::<Value>(state, kind).await? else { return Err(ApiError::bad_request("not connected")) };
    let result = check_module_account(kind, &fields.to_string()).await;
    if let (Ok(kept), Some(m)) = (&result, crate::modules::manifest(kind)) {
        let kept: Value = serde_json::from_str(kept).unwrap_or(fields);
        let expires = kept["expires_at"].as_i64();
        let (config, kept) = shown(m, config, kept);
        save(state, kind, &config, &kept, expires).await?;
    }
    record(state, kind, result.map(|_| ())).await?;
    ensure_module_source(state, kind).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fields_of_an_account_are_split() {
        let m = crate::modules::manifest::Manifest::parse(include_str!("../../../modules/rte/manifest.json")).unwrap();
        let (config, all) = account_fields(&m, &serde_json::json!({"client_id": " id ", "client_secret": "s", "other": 1})).unwrap();
        assert_eq!(config, serde_json::json!({"client_id": "id"}));
        assert_eq!(all, serde_json::json!({"client_id": "id", "client_secret": "s"}));
        assert!(account_fields(&m, &serde_json::json!({"client_id": "id"})).is_err());
    }
}
