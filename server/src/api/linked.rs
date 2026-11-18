//! Settings › Accounts: the linked accounts (see `accounts`), administrators only.

use crate::accounts::{self, AccountView};
use crate::auth::{AdminUser, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

pub async fn list(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<AccountView>>> {
    Ok(Json(accounts::list(&state).await?))
}

/// What connecting an account takes, by kind (Airzone is connected from its module).
#[derive(Deserialize, Default)]
pub struct ConnectInput {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub prm: String,
}

pub async fn connect(State(state): State<AppState>, AdminUser(user): AdminUser, Path(kind): Path<String>, Json(input): Json<ConnectInput>) -> ApiResult<Json<AccountView>> {
    match kind.as_str() {
        accounts::rte::KIND => accounts::rte::connect(&state, &input.client_id, &input.client_secret).await?,
        accounts::linky::KIND => accounts::linky::connect(&state, &input.token, &input.prm).await?,
        accounts::netatmo::KIND => accounts::netatmo::connect(&state, &input.client_id, &input.client_secret, &input.refresh_token).await?,
        crate::cloud::ACCOUNT => return Err(ApiError::bad_request("le compte Airzone se connecte depuis Réglages › Modules › Airzone")),
        _ => return Err(ApiError::not_found("unknown account")),
    }
    log(&state, &user.username, &kind, true).await;
    Ok(Json(accounts::view(&state, &kind).await?))
}

/// Uses the account once more: its state says whether it still works.
pub async fn test(State(state): State<AppState>, AdminUser(user): AdminUser, Path(kind): Path<String>) -> ApiResult<Json<AccountView>> {
    let result = match kind.as_str() {
        accounts::rte::KIND => accounts::rte::test(&state).await,
        accounts::linky::KIND => accounts::linky::test(&state).await,
        accounts::netatmo::KIND => accounts::netatmo::test(&state).await,
        crate::cloud::ACCOUNT => crate::cloud::sync_stored(&state, &user.username).await.map(|_| ()),
        _ => return Err(ApiError::not_found("unknown account")),
    };
    // The view tells what failed (its last error): the test itself answers with it.
    if let Err(e) = result {
        tracing::info!("account {kind} tested: {}", e.message.en);
    }
    Ok(Json(accounts::view(&state, &kind).await?))
}

pub async fn delete(State(state): State<AppState>, AdminUser(user): AdminUser, Path(kind): Path<String>) -> ApiResult<StatusCode> {
    if !accounts::KINDS.contains(&kind.as_str()) {
        return Err(ApiError::not_found("unknown account"));
    }
    if kind == crate::cloud::ACCOUNT {
        crate::cloud::forget(&state).await?;
    } else {
        crate::db::delete_account(&state.db, &kind).await?;
    }
    log(&state, &user.username, &kind, false).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn log(state: &AppState, actor: &str, kind: &str, connected: bool) {
    let text = if connected {
        crate::text!("compte {0} connecté", "{0} account connected", "cuenta {0} conectada", kind.to_string())
    } else {
        crate::text!("compte {0} retiré", "{0} account removed", "cuenta {0} retirada", kind.to_string())
    };
    state.hub.log("system", "info", None, Some(actor), text, None).await;
}

#[derive(Deserialize)]
pub struct LinkyQuery {
    #[serde(default = "thirty")]
    pub days: u64,
    pub installation_id: Option<i64>,
}

fn thirty() -> u64 {
    30
}

/// What Enedis measured (Linky), day by day with its cost: for those who see the energy.
pub async fn linky(State(state): State<AppState>, user: AuthUser, Query(q): Query<LinkyQuery>) -> ApiResult<Json<accounts::linky::LinkyView>> {
    crate::api::energy::reads_energy(&state, &user).await?;
    Ok(Json(accounts::linky::view(&state, q.days, q.installation_id).await?))
}
