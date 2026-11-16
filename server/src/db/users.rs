//! Users & sessions.

use super::*;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip)]
    pub password_hash: String,
    pub role: String,
    pub created_at: i64,
    /// Language chosen in the profile ("fr" | "en" | "es"); `None` follows the browser.
    pub language: Option<String>,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

/// What a user may change on an installation they control, on top of driving it. Everything is
/// allowed by default.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRights {
    /// Zone names and groups.
    pub zones: bool,
    /// Create, change and delete routines (running them only needs control).
    pub routines: bool,
    /// Create, change and delete Airzone schedules (switching one on or off only needs control).
    pub schedules: bool,
}

impl Default for UserRights {
    fn default() -> Self {
        Self { zones: true, routines: true, schedules: true }
    }
}

impl UserRights {
    pub const NONE: UserRights = UserRights { zones: false, routines: false, schedules: false };
}

pub async fn update_user_language(db: &Db, id: i64, language: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET language = ? WHERE id = ?").bind(language).bind(id).execute(db).await?;
    Ok(())
}

/// An installation given to a user (role "user").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub installation_id: i64,
    /// "read" | "control" | "admin"
    pub permission: String,
    /// What they may change when the permission is "control".
    pub rights: UserRights,
}

pub async fn count_users(db: &Db) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(db).await
}

pub async fn list_users(db: &Db) -> sqlx::Result<Vec<User>> {
    sqlx::query_as("SELECT * FROM users ORDER BY username").fetch_all(db).await
}

pub async fn user_by_id(db: &Db, id: i64) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn user_by_name(db: &Db, username: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE username = ?").bind(username).fetch_optional(db).await
}

pub async fn insert_user(db: &Db, username: &str, password_hash: &str, role: &str) -> sqlx::Result<User> {
    sqlx::query_as("INSERT INTO users (username, password_hash, role, created_at) VALUES (?, ?, ?, ?) RETURNING *")
        .bind(username)
        .bind(password_hash)
        .bind(role)
        .bind(now())
        .fetch_one(db)
        .await
}

pub async fn update_user_role(db: &Db, id: i64, role: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET role = ? WHERE id = ?").bind(role).bind(id).execute(db).await?;
    Ok(())
}

/// Creates the first administrator, atomically: `None` if a user already exists (two
/// simultaneous first-start requests cannot both succeed).
pub async fn insert_first_admin(db: &Db, username: &str, password_hash: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as(
        "INSERT INTO users (username, password_hash, role, created_at)
         SELECT ?, ?, 'admin', ? WHERE NOT EXISTS (SELECT 1 FROM users) RETURNING *",
    )
    .bind(username)
    .bind(password_hash)
    .bind(now())
    .fetch_optional(db)
    .await
}

/// Changes a role unless it would remove the last administrator (checked in the same
/// statement, so concurrent demotions cannot remove every admin). Returns false if refused.
pub async fn update_user_role_keeping_an_admin(db: &Db, id: i64, role: &str) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE users SET role = ?1 WHERE id = ?2
         AND (?1 = 'admin' OR role != 'admin' OR (SELECT COUNT(*) FROM users WHERE role = 'admin') > 1)",
    )
    .bind(role)
    .bind(id)
    .execute(db)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// Deletes a user unless it is the last administrator. Returns false if refused.
pub async fn delete_user_keeping_an_admin(db: &Db, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM users WHERE id = ?1 AND (role != 'admin' OR (SELECT COUNT(*) FROM users WHERE role = 'admin') > 1)").bind(id).execute(db).await?;
    Ok(r.rows_affected() == 1)
}

pub async fn update_user_password(db: &Db, id: i64, password_hash: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(password_hash).bind(id).execute(db).await?;
    Ok(())
}

pub async fn delete_user(db: &Db, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM users WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}

pub async fn count_admins(db: &Db) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'").fetch_one(db).await
}

#[derive(Debug, Clone, FromRow)]
pub struct Session {
    pub id: i64,
    pub user_id: i64,
    pub token_hash: String,
    pub prev_token_hash: Option<String>,
    pub rotated_at: Option<i64>,
    pub device_name: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub expires_at: i64,
}

pub async fn insert_session(db: &Db, user_id: i64, token_hash: &str, device_name: &str, expires_at: i64) -> sqlx::Result<Session> {
    let t = now();
    sqlx::query_as(
        "INSERT INTO sessions (user_id, token_hash, device_name, created_at, last_used_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(user_id)
    .bind(token_hash)
    .bind(device_name)
    .bind(t)
    .bind(t)
    .bind(expires_at)
    .fetch_one(db)
    .await
}

pub async fn session_by_token(db: &Db, token_hash: &str) -> sqlx::Result<Option<Session>> {
    sqlx::query_as("SELECT * FROM sessions WHERE token_hash = ?").bind(token_hash).fetch_optional(db).await
}

pub async fn session_by_prev_token(db: &Db, token_hash: &str) -> sqlx::Result<Option<Session>> {
    sqlx::query_as("SELECT * FROM sessions WHERE prev_token_hash = ?").bind(token_hash).fetch_optional(db).await
}

pub async fn session_by_id(db: &Db, id: i64) -> sqlx::Result<Option<Session>> {
    sqlx::query_as("SELECT * FROM sessions WHERE id = ?").bind(id).fetch_optional(db).await
}

/// Stores a new current token. `keep_prev` keeps the existing previous token (used when a
/// client retries with the previous token during the grace window).
pub async fn rotate_session(db: &Db, id: i64, new_hash: &str, keep_prev: bool, expires_at: i64) -> sqlx::Result<()> {
    let t = now();
    if keep_prev {
        sqlx::query("UPDATE sessions SET token_hash = ?, last_used_at = ?, expires_at = ? WHERE id = ?").bind(new_hash).bind(t).bind(expires_at).bind(id).execute(db).await?;
    } else {
        sqlx::query("UPDATE sessions SET prev_token_hash = token_hash, rotated_at = ?, token_hash = ?, last_used_at = ?, expires_at = ? WHERE id = ?")
            .bind(t)
            .bind(new_hash)
            .bind(t)
            .bind(expires_at)
            .bind(id)
            .execute(db)
            .await?;
    }
    Ok(())
}

pub async fn list_sessions(db: &Db, user_id: i64) -> sqlx::Result<Vec<Session>> {
    sqlx::query_as("SELECT * FROM sessions WHERE user_id = ? ORDER BY last_used_at DESC").bind(user_id).fetch_all(db).await
}

pub async fn delete_session(db: &Db, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}

pub async fn delete_user_sessions(db: &Db, user_id: i64, except: Option<i64>) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id != ?").bind(user_id).bind(except.unwrap_or(-1)).execute(db).await?;
    Ok(())
}
