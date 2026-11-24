//! Personal access tokens: a user creates one for a tool (an assistant over MCP, a script).

use crate::auth::{self, AuthUser};
use crate::db::{self, ApiToken};
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

/// Tokens a user may hold.
const MAX_TOKENS: usize = 20;

#[derive(Deserialize)]
pub struct NewToken {
    pub name: String,
}

#[derive(Serialize)]
pub struct CreatedToken {
    /// Shown once: only its hash is kept.
    pub token: String,
    #[serde(flatten)]
    pub view: ApiToken,
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<ApiToken>>> {
    Ok(Json(db::api_tokens(&state.db, user.id).await?))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(req): Json<NewToken>) -> ApiResult<(StatusCode, Json<CreatedToken>)> {
    if user.token_id.is_some() {
        return Err(ApiError::forbidden("an access token cannot create another one"));
    }
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::bad_request("name: 1 to 64 characters"));
    }
    if db::api_tokens(&state.db, user.id).await?.len() >= MAX_TOKENS {
        return Err(ApiError::conflict(format!("at most {MAX_TOKENS} access tokens: remove one first")));
    }
    let (token, hint) = auth::new_api_token();
    let view = db::insert_api_token(&state.db, user.id, name, &auth::hash_token(&token), &hint).await?;
    state
        .hub
        .log("auth", "info", None, Some(&user.username), crate::text!("Jeton d'accès « {0} » créé", "Access token “{0}” created", "Token de acceso «{0}» creado", name), None)
        .await;
    Ok((StatusCode::CREATED, Json(CreatedToken { token, view })))
}

/// Revokes a token: the tool using it is cut off at once.
pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if !db::delete_api_token(&state.db, user.id, id).await? {
        return Err(ApiError::not_found("access token not found"));
    }
    state.hub.log("auth", "info", None, Some(&user.username), crate::text!("Jeton d'accès révoqué", "Access token revoked", "Token de acceso revocado"), None).await;
    Ok(StatusCode::NO_CONTENT)
}
