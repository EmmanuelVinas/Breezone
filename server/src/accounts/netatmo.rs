//! Netatmo (Legrand Home + Control, its Ecometer…), with one's own app on dev.netatmo.com: its
//! client id and secret, and a refresh token from its "Token generator" with the scopes
//! `read_magellan` and `write_magellan` (Legrand). No address of Breezone has to be reachable
//! from the internet: Breezone renews the access itself, and keeps the new refresh token each time
//! (Netatmo gives a new one at each renewal).
//!
//!   POST {netatmo}/oauth2/token (grant_type=refresh_token) → {access_token, refresh_token, expires_in}
//!   GET  {netatmo}/api/homesdata                         → {body: {homes: [{id, name, rooms, modules}]}}

use super::{http, record, refused, unreachable};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const KIND: &str = "netatmo";
const SERVICE: &str = "Netatmo";

#[derive(Serialize, Deserialize)]
pub struct Secret {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<i64>,
}

/// A new access from the refresh token (which Netatmo replaces at the same time).
async fn renew(state: &AppState, s: &mut Secret) -> ApiResult<()> {
    renew_at(&state.cfg.netatmo_url, s).await
}

async fn renew_at(url: &str, s: &mut Secret) -> ApiResult<()> {
    let resp = http()
        .post(format!("{}/oauth2/token", url.trim_end_matches('/')))
        .form(&[("grant_type", "refresh_token"), ("refresh_token", s.refresh_token.as_str()), ("client_id", s.client_id.as_str()), ("client_secret", s.client_secret.as_str())])
        .send()
        .await
        .map_err(|e| unreachable(SERVICE, e))?;
    if !resp.status().is_success() {
        return Err(refused(SERVICE, resp).await);
    }
    let body: Value = resp.json().await.map_err(|e| unreachable(SERVICE, e))?;
    s.access_token = Some(body["access_token"].as_str().ok_or_else(|| ApiError::bad_gateway("Netatmo: no access token"))?.to_string());
    if let Some(r) = body["refresh_token"].as_str() {
        s.refresh_token = r.to_string();
    }
    s.expires_at = Some(db::now() + body["expires_in"].as_i64().unwrap_or(10_800));
    Ok(())
}

async fn homes(state: &AppState, access: &str) -> ApiResult<Value> {
    let resp = http().get(format!("{}/api/homesdata", state.cfg.netatmo_url.trim_end_matches('/'))).bearer_auth(access).send().await.map_err(|e| unreachable(SERVICE, e))?;
    if !resp.status().is_success() {
        return Err(refused(SERVICE, resp).await);
    }
    resp.json().await.map_err(|e| unreachable(SERVICE, e))
}

/// What the account reaches, in a few figures: each home, its rooms and its modules.
fn summary(data: &Value) -> Value {
    let homes: Vec<Value> = data["body"]["homes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|h| {
            json!({
                "name": h["name"].as_str().unwrap_or_default(),
                "rooms": h["rooms"].as_array().map_or(0, Vec::len),
                "modules": h["modules"].as_array().map_or(0, Vec::len),
            })
        })
        .collect();
    Value::Array(homes)
}

/// Renews the access, reads the homes, and keeps everything (the new refresh token above all).
async fn check(state: &AppState, mut s: Secret) -> ApiResult<()> {
    let _one = RENEWING.lock().await;
    renew(state, &mut s).await?;
    let data = homes(state, s.access_token.as_deref().unwrap_or_default()).await?;
    let config = json!({ "client_id": s.client_id, "homes": summary(&data) });
    let expires_at = s.expires_at;
    super::save(state, KIND, &config, &s, expires_at).await?;
    Ok(())
}

pub async fn connect(state: &AppState, client_id: &str, client_secret: &str, refresh_token: &str) -> ApiResult<()> {
    let s = Secret {
        client_id: client_id.trim().to_string(),
        client_secret: client_secret.trim().to_string(),
        refresh_token: refresh_token.trim().to_string(),
        access_token: None,
        expires_at: None,
    };
    if s.client_id.is_empty() || s.client_secret.is_empty() || s.refresh_token.is_empty() {
        return Err(ApiError::bad_request("l'ID client, le secret et le refresh token de l'application Netatmo sont requis"));
    }
    check(state, s).await?;
    db::set_account_state(&state.db, KIND, "ok", None).await?;
    ensure_source(state).await?;
    Ok(())
}

/// The Netatmo module, added with the account (once): it reads the homes the account reaches.
pub async fn ensure_source(state: &AppState) -> ApiResult<()> {
    if db::account(&state.db, KIND).await?.and_then(|a| a.secret).is_none() {
        return Ok(());
    }
    if db::list_sources(&state.db).await?.iter().any(|s| s.driver == KIND) {
        return Ok(());
    }
    db::insert_source(&state.db, KIND, "Netatmo · Legrand", "{}").await?;
    state.drivers.reload().await?;
    Ok(())
}

pub async fn test(state: &AppState) -> ApiResult<()> {
    let Some((_, s)) = super::load::<Secret>(state, KIND).await? else { return Err(ApiError::bad_request("aucun compte Netatmo")) };
    record(state, KIND, check(state, s).await).await?;
    ensure_source(state).await
}

/// One renewal at a time: Netatmo replaces the refresh token at each, so two at once would lose one.
static RENEWING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// An access for a call to Netatmo, renewed when it is about to end (for the module that uses it):
/// `None` when no account is linked.
pub async fn access_in(db: &db::Db, key: &crate::crypto::Key, url: &str) -> ApiResult<Option<String>> {
    let _one = RENEWING.lock().await;
    let Some((config, mut s)) = super::load_in::<Secret>(db, key, KIND).await? else { return Ok(None) };
    if s.access_token.is_none() || s.expires_at.is_none_or(|t| t - 60 < db::now()) {
        let renewed = renew_at(url, &mut s).await;
        match &renewed {
            Ok(()) => db::set_account_state(db, KIND, "ok", None).await?,
            Err(e) => db::set_account_state(db, KIND, "error", Some(&e.message.store())).await?,
        }
        renewed?;
        let expires_at = s.expires_at;
        super::save_in(db, key, KIND, &config, &s, expires_at).await?;
    }
    Ok(s.access_token)
}
