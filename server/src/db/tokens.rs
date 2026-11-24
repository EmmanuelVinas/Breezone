//! Personal access tokens (`api_tokens`).

use super::{now, Db};
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ApiToken {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    #[serde(skip)]
    pub token_hash: String,
    /// The start of the token ("bzt_Ab3d…").
    pub hint: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

pub async fn insert_api_token(db: &Db, user_id: i64, name: &str, token_hash: &str, hint: &str) -> sqlx::Result<ApiToken> {
    sqlx::query_as("INSERT INTO api_tokens (user_id, name, token_hash, hint, created_at) VALUES (?, ?, ?, ?, ?) RETURNING *")
        .bind(user_id)
        .bind(name)
        .bind(token_hash)
        .bind(hint)
        .bind(now())
        .fetch_one(db)
        .await
}

pub async fn api_tokens(db: &Db, user_id: i64) -> sqlx::Result<Vec<ApiToken>> {
    sqlx::query_as("SELECT * FROM api_tokens WHERE user_id = ? ORDER BY created_at DESC, id DESC").bind(user_id).fetch_all(db).await
}

pub async fn api_token_by_hash(db: &Db, token_hash: &str) -> sqlx::Result<Option<ApiToken>> {
    sqlx::query_as("SELECT * FROM api_tokens WHERE token_hash = ?").bind(token_hash).fetch_optional(db).await
}

pub async fn api_token_exists(db: &Db, id: i64) -> sqlx::Result<bool> {
    Ok(sqlx::query_scalar::<_, i64>("SELECT id FROM api_tokens WHERE id = ?").bind(id).fetch_optional(db).await?.is_some())
}

/// Notes a use, at most once a minute (each call of a tool would write otherwise).
pub async fn touch_api_token(db: &Db, id: i64) -> sqlx::Result<()> {
    let t = now();
    sqlx::query("UPDATE api_tokens SET last_used_at = ? WHERE id = ? AND (last_used_at IS NULL OR last_used_at < ?)").bind(t).bind(id).bind(t - 60).execute(db).await?;
    Ok(())
}

/// Removes a token of a user; `false` when they have none with this id.
pub async fn delete_api_token(db: &Db, user_id: i64, id: i64) -> sqlx::Result<bool> {
    Ok(sqlx::query("DELETE FROM api_tokens WHERE id = ? AND user_id = ?").bind(id).bind(user_id).execute(db).await?.rows_affected() > 0)
}
