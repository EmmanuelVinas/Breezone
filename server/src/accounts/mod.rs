//! Linked accounts: a connection to an online service, apart from the modules that use it
//! (roadmap F10). Always optional: an account completes what Breezone reads locally, it never
//! replaces it. The secrets (password, client secret, tokens) are encrypted with the data key.
//!
//! - `airzone`: the Airzone Cloud account (names, groups, schedules), see `cloud`;
//! - `rte`: RTE's data portal, for the official colour of the Tempo days;
//! - `linky`: Conso API, which reads what Enedis measured (Linky) with a personal token;
//! - `netatmo`: Netatmo (Legrand Home + Control), by OAuth with one's own Netatmo app.

pub mod linky;
pub mod netatmo;
pub mod rte;

use crate::crypto::{decrypt, encrypt};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::i18n::Text;
use crate::AppState;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

/// The kinds of accounts, in the order of Settings › Accounts.
pub const KINDS: &[&str] = &[crate::cloud::ACCOUNT, rte::KIND, linky::KIND, netatmo::KIND];

/// An account as Settings › Accounts shows it: never its secrets.
#[derive(Debug, Serialize)]
pub struct AccountView {
    pub kind: &'static str,
    /// Its credentials are kept: it can be used.
    pub connected: bool,
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
    Ok(KINDS
        .iter()
        .map(|kind| match rows.iter().find(|r| r.kind == *kind) {
            Some(r) => AccountView {
                kind,
                connected: r.secret.is_some(),
                state: r.state.clone(),
                last_error: r.last_error.as_deref().map(Text::load),
                last_ok: r.last_ok,
                expires_at: r.expires_at,
                config: serde_json::from_str(&r.config).unwrap_or_else(|_| json!({})),
            },
            None => AccountView { kind, connected: false, state: "none".into(), last_error: None, last_ok: None, expires_at: None, config: json!({}) },
        })
        .collect())
}

pub async fn view(state: &AppState, kind: &str) -> ApiResult<AccountView> {
    list(state).await?.into_iter().find(|a| a.kind == kind).ok_or_else(|| ApiError::not_found("unknown account"))
}

/// The configuration and decrypted secrets of an account, when it is connected.
pub async fn load<S: DeserializeOwned>(state: &AppState, kind: &str) -> ApiResult<Option<(Value, S)>> {
    load_in(&state.db, &state.data_key, kind).await
}

/// Same, from the database and key alone (a module that uses an account).
pub async fn load_in<S: DeserializeOwned>(db: &db::Db, key: &crate::crypto::Key, kind: &str) -> ApiResult<Option<(Value, S)>> {
    let Some(row) = db::account(db, kind).await? else { return Ok(None) };
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
