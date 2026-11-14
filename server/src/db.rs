//! SQLite persistence (sqlx). Migrations are embedded in the binary.

use crate::i18n::Text;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

pub type Db = SqlitePool;

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub async fn connect(path: &Path) -> anyhow::Result<Db> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::new().filename(path).create_if_missing(true).journal_mode(SqliteJournalMode::Wal).foreign_keys(true).busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new().max_connections(4).connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

// ---------------------------------------------------------------------------------------
// Users & sessions
// ---------------------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------------------
// Installations & zone metadata
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Installation {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub enabled: bool,
    pub created_at: i64,
    /// City / location (from Airzone Cloud), shown in "Mes installations".
    pub location: Option<String>,
    /// WebServer MAC address, used to match Airzone Cloud installations.
    pub mac: Option<String>,
    /// Airzone Cloud installation this installation was matched to.
    pub cloud_id: Option<String>,
    /// Node through which the installation is reached (`None` = WebServer on this network).
    pub node_id: Option<i64>,
    /// Id of the installation on that node.
    pub remote_id: Option<i64>,
    /// Record the time each zone spends in demand (only used for the installations of this
    /// server: a shared installation follows the server that owns it).
    pub record_demand: bool,
}

impl Installation {
    pub fn is_local(&self) -> bool {
        self.node_id.is_none()
    }
}

pub async fn list_installations(db: &Db) -> sqlx::Result<Vec<Installation>> {
    sqlx::query_as("SELECT * FROM installations ORDER BY id").fetch_all(db).await
}

pub async fn installation_by_id(db: &Db, id: i64) -> sqlx::Result<Option<Installation>> {
    sqlx::query_as("SELECT * FROM installations WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn insert_installation(db: &Db, name: &str, host: &str, port: i64, enabled: bool) -> sqlx::Result<Installation> {
    sqlx::query_as("INSERT INTO installations (name, host, port, enabled, created_at) VALUES (?, ?, ?, ?, ?) RETURNING *")
        .bind(name)
        .bind(host)
        .bind(port)
        .bind(enabled)
        .bind(now())
        .fetch_one(db)
        .await
}

pub async fn update_installation(db: &Db, id: i64, name: &str, host: &str, port: i64, enabled: bool) -> sqlx::Result<()> {
    // Another address may be another WebServer: forget its MAC address (read again later).
    sqlx::query(
        "UPDATE installations SET name = ?1, host = ?2, port = ?3, enabled = ?4,
         mac = CASE WHEN host = ?2 AND port = ?3 THEN mac ELSE NULL END WHERE id = ?5",
    )
    .bind(name)
    .bind(host)
    .bind(port)
    .bind(enabled)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

/// Creates an installation reached through another Breezone node.
pub async fn insert_remote_installation(db: &Db, name: &str, node_id: i64, remote_id: i64, url: &str, mac: Option<&str>, location: Option<&str>) -> sqlx::Result<Installation> {
    sqlx::query_as(
        "INSERT INTO installations (name, host, port, enabled, created_at, node_id, remote_id, mac, location)
         VALUES (?, ?, 0, 1, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(name)
    .bind(url)
    .bind(now())
    .bind(node_id)
    .bind(remote_id)
    .bind(mac)
    .bind(location)
    .fetch_one(db)
    .await
}

/// The installation now points to another WebServer: forget what belonged to the previous one
/// (zone names and groups, history, Airzone Cloud link).
pub async fn reset_installation_for_new_webserver(db: &Db, id: i64) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM zone_meta WHERE installation_id = ?").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM readings WHERE installation_id = ?").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE installations SET cloud_id = NULL, location = NULL WHERE id = ?").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE cloud_installations SET installation_id = NULL WHERE installation_id = ?").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn set_installation_mac(db: &Db, id: i64, mac: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE installations SET mac = ? WHERE id = ?").bind(mac).bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_installation_cloud_id(db: &Db, id: i64, cloud_id: &str) -> sqlx::Result<()> {
    // A cloud installation is linked to at most one local installation.
    sqlx::query("UPDATE installations SET cloud_id = NULL WHERE cloud_id = ? AND id != ?").bind(cloud_id).bind(id).execute(db).await?;
    sqlx::query("UPDATE installations SET cloud_id = ? WHERE id = ?").bind(cloud_id).bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_installation_record_demand(db: &Db, id: i64, record: bool) -> sqlx::Result<()> {
    sqlx::query("UPDATE installations SET record_demand = ? WHERE id = ?").bind(record).bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_installation_location(db: &Db, id: i64, location: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE installations SET location = ? WHERE id = ?").bind(location).bind(id).execute(db).await?;
    Ok(())
}

pub async fn delete_installation(db: &Db, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM installations WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize, Default)]
pub struct ZoneMeta {
    pub installation_id: i64,
    pub system_id: i64,
    pub zone_id: i64,
    pub display_name: Option<String>,
    pub group_name: Option<String>,
    pub position: i64,
}

pub async fn list_zone_meta(db: &Db, installation_id: i64) -> sqlx::Result<Vec<ZoneMeta>> {
    sqlx::query_as("SELECT * FROM zone_meta WHERE installation_id = ?").bind(installation_id).fetch_all(db).await
}

pub async fn upsert_zone_meta(db: &Db, m: &ZoneMeta) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO zone_meta (installation_id, system_id, zone_id, display_name, group_name, position)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (installation_id, system_id, zone_id)
         DO UPDATE SET display_name = excluded.display_name, group_name = excluded.group_name, position = excluded.position",
    )
    .bind(m.installation_id)
    .bind(m.system_id)
    .bind(m.zone_id)
    .bind(&m.display_name)
    .bind(&m.group_name)
    .bind(m.position)
    .execute(db)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Reading {
    pub ts: i64,
    pub room_temp: Option<f64>,
    pub humidity: Option<i64>,
    pub setpoint: Option<f64>,
    pub is_on: Option<bool>,
    pub mode: Option<i64>,
    /// Seconds in demand since the previous point (`None`: not recorded).
    pub demand_secs: Option<i64>,
    /// Air quality read from the zone (`aq_quality`; `None`: no sensor).
    pub air_quality: Option<i64>,
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_reading(
    db: &Db,
    installation_id: i64,
    system_id: i64,
    zone_id: i64,
    room_temp: Option<f64>,
    humidity: Option<i64>,
    setpoint: Option<f64>,
    is_on: bool,
    mode: Option<i64>,
    demand_secs: Option<i64>,
    air_quality: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO readings (installation_id, system_id, zone_id, ts, room_temp, humidity, setpoint, is_on, mode, demand_secs, air_quality)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(installation_id)
    .bind(system_id)
    .bind(zone_id)
    .bind(now())
    .bind(room_temp)
    .bind(humidity)
    .bind(setpoint)
    .bind(is_on)
    .bind(mode)
    .bind(demand_secs)
    .bind(air_quality)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn readings(db: &Db, installation_id: i64, system_id: i64, zone_id: i64, since: i64) -> sqlx::Result<Vec<Reading>> {
    sqlx::query_as(
        "SELECT ts, room_temp, humidity, setpoint, is_on, mode, demand_secs, air_quality FROM readings
         WHERE installation_id = ? AND system_id = ? AND zone_id = ? AND ts >= ? ORDER BY ts",
    )
    .bind(installation_id)
    .bind(system_id)
    .bind(zone_id)
    .bind(since)
    .fetch_all(db)
    .await
}

/// Average room temperature of a zone over one time bucket.
#[derive(Debug, Clone, FromRow)]
pub struct TrendPoint {
    pub system_id: i64,
    pub zone_id: i64,
    /// Start of the bucket.
    pub bucket: i64,
    pub room_temp: f64,
}

/// Room temperature of every zone of an installation since `since`, averaged per `bucket` seconds.
pub async fn temperature_trends(db: &Db, installation_id: i64, since: i64, bucket: i64) -> sqlx::Result<Vec<TrendPoint>> {
    sqlx::query_as(
        "SELECT system_id, zone_id, (ts / ?1) * ?1 AS bucket, AVG(room_temp) AS room_temp FROM readings
         WHERE installation_id = ?2 AND ts >= ?3 AND room_temp IS NOT NULL
         GROUP BY system_id, zone_id, bucket ORDER BY system_id, zone_id, bucket",
    )
    .bind(bucket.max(1))
    .bind(installation_id)
    .bind(since)
    .fetch_all(db)
    .await
}

// ---------------------------------------------------------------------------------------
// Routines
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct RoutineRow {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub days: i64,
    pub time: Option<String>,
    pub actions: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_run_at: Option<i64>,
    /// "airzone:…" when imported from an Airzone schedule.
    pub origin: Option<String>,
}

pub async fn list_routines(db: &Db) -> sqlx::Result<Vec<RoutineRow>> {
    sqlx::query_as("SELECT * FROM routines ORDER BY time IS NULL, time, name").fetch_all(db).await
}

pub async fn routine_by_id(db: &Db, id: i64) -> sqlx::Result<Option<RoutineRow>> {
    sqlx::query_as("SELECT * FROM routines WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn insert_routine(db: &Db, name: &str, enabled: bool, days: i64, time: Option<&str>, actions: &str) -> sqlx::Result<RoutineRow> {
    let t = now();
    sqlx::query_as("INSERT INTO routines (name, enabled, days, time, actions, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING *")
        .bind(name)
        .bind(enabled)
        .bind(days)
        .bind(time)
        .bind(actions)
        .bind(t)
        .bind(t)
        .fetch_one(db)
        .await
}

pub async fn update_routine(db: &Db, id: i64, name: &str, enabled: bool, days: i64, time: Option<&str>, actions: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE routines SET name = ?, enabled = ?, days = ?, time = ?, actions = ?, updated_at = ? WHERE id = ?")
        .bind(name)
        .bind(enabled)
        .bind(days)
        .bind(time)
        .bind(actions)
        .bind(now())
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_routine_last_run(db: &Db, id: i64, ts: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE routines SET last_run_at = ? WHERE id = ?").bind(ts).bind(id).execute(db).await?;
    Ok(())
}

pub async fn delete_routine(db: &Db, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM routines WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Events (activity feed)
// ---------------------------------------------------------------------------------------

/// An entry of the activity feed; its message is serialized in the reader's language.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub level: String,
    pub installation_id: Option<i64>,
    pub actor: Option<String>,
    pub message: Text,
    pub data: Option<String>,
}

#[derive(FromRow)]
struct EventRow {
    id: i64,
    ts: i64,
    kind: String,
    level: String,
    installation_id: Option<i64>,
    actor: Option<String>,
    message: String,
    message_en: Option<String>,
    message_es: Option<String>,
    data: Option<String>,
}

impl From<EventRow> for Event {
    fn from(r: EventRow) -> Event {
        let message = Text { en: r.message_en.unwrap_or_else(|| r.message.clone()), es: r.message_es.unwrap_or_else(|| r.message.clone()), fr: r.message };
        Event { id: r.id, ts: r.ts, kind: r.kind, level: r.level, installation_id: r.installation_id, actor: r.actor, message, data: r.data }
    }
}

pub async fn insert_event(
    db: &Db,
    kind: &str,
    level: &str,
    installation_id: Option<i64>,
    actor: Option<&str>,
    message: &Text,
    data: Option<&serde_json::Value>,
) -> sqlx::Result<Event> {
    sqlx::query_as::<_, EventRow>(
        "INSERT INTO events (ts, kind, level, installation_id, actor, message, message_en, message_es, data)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(now())
    .bind(kind)
    .bind(level)
    .bind(installation_id)
    .bind(actor)
    .bind(&message.fr)
    .bind(&message.en)
    .bind(&message.es)
    .bind(data.map(|d| d.to_string()))
    .fetch_one(db)
    .await
    .map(Event::from)
}

/// Installations a user (role 'user') can see: (installation id, "read" | "control").
pub async fn user_access(db: &Db, user_id: i64) -> sqlx::Result<Vec<Grant>> {
    let rows: Vec<(i64, String, bool, bool, bool)> = sqlx::query_as(
        "SELECT installation_id, permission, edit_zones, edit_routines, edit_schedules FROM user_installations
         WHERE user_id = ? ORDER BY installation_id",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(installation_id, permission, zones, routines, schedules)| Grant { installation_id, permission, rights: UserRights { zones, routines, schedules } })
        .collect())
}

/// Replaces the installations a user can see.
pub async fn set_user_access(db: &Db, user_id: i64, access: &[Grant]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM user_installations WHERE user_id = ?").bind(user_id).execute(&mut *tx).await?;
    for g in access {
        sqlx::query(
            "INSERT INTO user_installations (user_id, installation_id, permission, edit_zones, edit_routines, edit_schedules)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(g.installation_id)
        .bind(&g.permission)
        .bind(g.rights.zones)
        .bind(g.rights.routines)
        .bind(g.rights.schedules)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Events, newest first. `visible`: only these installations (no system events); `None` = all.
pub async fn list_visible_events(db: &Db, limit: i64, before_id: Option<i64>, installation_id: Option<i64>, visible: Option<&[i64]>) -> sqlx::Result<Vec<Event>> {
    let Some(ids) = visible else { return list_events(db, limit, before_id, installation_id).await };
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let list = serde_json::to_string(ids).unwrap_or_else(|_| "[]".into());
    sqlx::query_as::<_, EventRow>(
        "SELECT * FROM events WHERE id < ? AND (? IS NULL OR installation_id = ?)
         AND installation_id IN (SELECT value FROM json_each(?)) ORDER BY id DESC LIMIT ?",
    )
    .bind(before_id.unwrap_or(i64::MAX))
    .bind(installation_id)
    .bind(installation_id)
    .bind(list)
    .bind(limit)
    .fetch_all(db)
    .await
    .map(|rows| rows.into_iter().map(Event::from).collect())
}

pub async fn list_events(db: &Db, limit: i64, before_id: Option<i64>, installation_id: Option<i64>) -> sqlx::Result<Vec<Event>> {
    sqlx::query_as::<_, EventRow>("SELECT * FROM events WHERE id < ? AND (? IS NULL OR installation_id = ?) ORDER BY id DESC LIMIT ?")
        .bind(before_id.unwrap_or(i64::MAX))
        .bind(installation_id)
        .bind(installation_id)
        .bind(limit)
        .fetch_all(db)
        .await
        .map(|rows| rows.into_iter().map(Event::from).collect())
}

/// Deletes old history, events and expired sessions.
pub async fn purge(db: &Db, history_days: i64, events_days: i64) -> sqlx::Result<()> {
    let t = now();
    sqlx::query("DELETE FROM readings WHERE ts < ?").bind(t - history_days * 86_400).execute(db).await?;
    sqlx::query("DELETE FROM device_samples WHERE ts < ?").bind(t - history_days * 86_400).execute(db).await?;
    sqlx::query("DELETE FROM events WHERE ts < ?").bind(t - events_days * 86_400).execute(db).await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?").bind(t).execute(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Settings (key/value)
// ---------------------------------------------------------------------------------------

pub async fn get_setting(db: &Db, key: &str) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(db).await
}

pub async fn set_setting(db: &Db, key: &str, value: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value").bind(key).bind(value).execute(db).await?;
    Ok(())
}

pub async fn delete_setting(db: &Db, key: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM settings WHERE key = ?").bind(key).execute(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Airzone Cloud installations
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct CloudInstallationRow {
    pub cloud_id: String,
    pub name: String,
    pub location: Option<String>,
    pub ws_ids: String,
    pub zones: i64,
    pub installation_id: Option<i64>,
    pub synced_at: i64,
    /// JSON snapshot of the cloud installation (groups, zones).
    #[serde(skip)]
    pub detail: Option<String>,
}

pub async fn list_cloud_installations(db: &Db) -> sqlx::Result<Vec<CloudInstallationRow>> {
    sqlx::query_as("SELECT * FROM cloud_installations ORDER BY name").fetch_all(db).await
}

/// Replaces the stored list of cloud installations.
/// Records which Breezone installation drives a cloud installation.
pub async fn set_cloud_installation_link(db: &Db, cloud_id: &str, installation_id: Option<i64>) -> sqlx::Result<()> {
    if let Some(id) = installation_id {
        sqlx::query("UPDATE cloud_installations SET installation_id = NULL WHERE installation_id = ? AND cloud_id != ?").bind(id).bind(cloud_id).execute(db).await?;
    }
    sqlx::query("UPDATE cloud_installations SET installation_id = ? WHERE cloud_id = ?").bind(installation_id).bind(cloud_id).execute(db).await?;
    Ok(())
}

/// Forgets the cloud installation of a Breezone installation.
pub async fn clear_installation_cloud_link(db: &Db, installation_id: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE installations SET cloud_id = NULL WHERE id = ?").bind(installation_id).execute(db).await?;
    sqlx::query("UPDATE cloud_installations SET installation_id = NULL WHERE installation_id = ?").bind(installation_id).execute(db).await?;
    Ok(())
}

/// Detaches a cloud installation from whatever Breezone installation it was linked to.
pub async fn clear_cloud_link_of(db: &Db, cloud_id: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE installations SET cloud_id = NULL WHERE cloud_id = ?").bind(cloud_id).execute(db).await?;
    sqlx::query("UPDATE cloud_installations SET installation_id = NULL WHERE cloud_id = ?").bind(cloud_id).execute(db).await?;
    Ok(())
}

pub async fn replace_cloud_installations(db: &Db, rows: &[CloudInstallationRow]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM cloud_installations").execute(&mut *tx).await?;
    for r in rows {
        sqlx::query("INSERT INTO cloud_installations (cloud_id, name, location, ws_ids, zones, installation_id, synced_at, detail) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&r.cloud_id)
            .bind(&r.name)
            .bind(&r.location)
            .bind(&r.ws_ids)
            .bind(r.zones)
            .bind(r.installation_id)
            .bind(r.synced_at)
            .bind(&r.detail)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

pub async fn clear_cloud_installations(db: &Db) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM cloud_installations").execute(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Per-installation access point
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize, PartialEq)]
pub struct InstallationAccess {
    pub cloud_id: String,
    /// Fixed local address of the WebServer.
    pub host: String,
    pub port: i64,
    pub updated_at: i64,
}

pub async fn list_access(db: &Db) -> sqlx::Result<Vec<InstallationAccess>> {
    sqlx::query_as("SELECT * FROM installation_access").fetch_all(db).await
}

pub async fn get_access(db: &Db, cloud_id: &str) -> sqlx::Result<Option<InstallationAccess>> {
    sqlx::query_as("SELECT * FROM installation_access WHERE cloud_id = ?").bind(cloud_id).fetch_optional(db).await
}

pub async fn set_access(db: &Db, cloud_id: &str, host: &str, port: i64) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO installation_access (cloud_id, host, port, updated_at) VALUES (?, ?, ?, ?)
         ON CONFLICT (cloud_id) DO UPDATE SET host = excluded.host, port = excluded.port, updated_at = excluded.updated_at",
    )
    .bind(cloud_id)
    .bind(host)
    .bind(port)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete_access(db: &Db, cloud_id: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM installation_access WHERE cloud_id = ?").bind(cloud_id).execute(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Federation: shares exposed by this node, nodes consumed by this node (ADR 0008)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct Share {
    pub id: i64,
    pub label: String,
    pub key_hash: String,
    pub permission: String,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub remote_node_id: Option<String>,
    pub remote_name: Option<String>,
    pub last_seen: Option<i64>,
    pub last_address: Option<String>,
    /// 'invited' | 'pending' | 'active'
    pub status: String,
    pub pairing_expires_at: Option<i64>,
    pub remote_public_key: Option<String>,
    pub requested_at: Option<i64>,
    /// The invitation asks the other node to share back.
    pub reciprocal: bool,
    /// Address of the other node, sent with its access request.
    pub remote_url: Option<String>,
    /// The other node accepted to share back.
    pub remote_shares_back: bool,
    /// Capabilities of the node reading this share (JSON, ADR 0012) and when they were received.
    pub peer_caps: Option<String>,
    pub peer_caps_at: Option<i64>,
}

impl Share {
    /// Share created by this node in answer to a reciprocal invitation (bound to the inviter).
    pub fn is_reverse(&self) -> bool {
        self.key_hash.starts_with("reciprocal:")
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_share(
    db: &Db,
    label: &str,
    key_hash: &str,
    permission: &str,
    expires_at: Option<i64>,
    created_by: &str,
    installation_ids: &[i64],
    reciprocal: bool,
) -> sqlx::Result<Share> {
    let mut tx = db.begin().await?;
    let share: Share = sqlx::query_as(
        "INSERT INTO shares (label, key_hash, permission, expires_at, created_at, created_by, status, pairing_expires_at, reciprocal)
         VALUES (?, ?, ?, ?, ?, ?, 'invited', ?, ?) RETURNING *",
    )
    .bind(label)
    .bind(key_hash)
    .bind(permission)
    .bind(expires_at)
    .bind(now())
    .bind(created_by)
    .bind(now() + crate::federation::PAIRING_TTL_SECS)
    .bind(reciprocal)
    .fetch_one(&mut *tx)
    .await?;
    for id in installation_ids {
        sqlx::query("INSERT INTO share_installations (share_id, installation_id) VALUES (?, ?)").bind(share.id).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(share)
}

pub async fn list_shares(db: &Db) -> sqlx::Result<Vec<Share>> {
    sqlx::query_as("SELECT * FROM shares ORDER BY id").fetch_all(db).await
}

pub async fn share_by_id(db: &Db, id: i64) -> sqlx::Result<Option<Share>> {
    sqlx::query_as("SELECT * FROM shares WHERE id = ?").bind(id).fetch_optional(db).await
}

/// Share still waiting for its invitation code to be used.
pub async fn invited_share(db: &Db, code_hash: &str) -> sqlx::Result<Option<Share>> {
    sqlx::query_as("SELECT * FROM shares WHERE key_hash = ? AND status = 'invited'").bind(code_hash).fetch_optional(db).await
}

/// Uses the invitation code (once): records the requesting node and waits for approval.
/// Returns false if the code was used meanwhile.
/// `shares_back_from`: address of the requesting node when it shares back (reciprocal invitation).
#[allow(clippy::too_many_arguments)]
pub async fn claim_share(db: &Db, id: i64, node_id: &str, name: &str, public_key: &str, address: Option<&str>, shares_back_from: Option<&str>) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE shares SET status = 'pending', key_hash = 'used:' || id, remote_node_id = ?, remote_name = ?,
         remote_public_key = ?, last_address = ?, requested_at = ?, remote_url = ?, remote_shares_back = ?
         WHERE id = ? AND status = 'invited'",
    )
    .bind(node_id)
    .bind(name)
    .bind(public_key)
    .bind(address)
    .bind(now())
    .bind(shares_back_from)
    .bind(shares_back_from.is_some())
    .bind(id)
    .execute(db)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// The share a paired node uses (the most recent one if it paired several times).
pub async fn share_of_node(db: &Db, node_id: &str) -> sqlx::Result<Option<Share>> {
    sqlx::query_as("SELECT * FROM shares WHERE remote_node_id = ? AND status IN ('pending', 'active') ORDER BY id DESC LIMIT 1").bind(node_id).fetch_optional(db).await
}

/// Share given back to the node that invited this one (reciprocal invitation): bound to that
/// node's key right away, since this node's administrator chose to share.
#[allow(clippy::too_many_arguments)]
pub async fn insert_reverse_share(
    db: &Db,
    label: &str,
    permission: &str,
    created_by: &str,
    installation_ids: &[i64],
    node_id: &str,
    name: &str,
    public_key: &str,
) -> sqlx::Result<Share> {
    let mut tx = db.begin().await?;
    let share: Share = sqlx::query_as(
        "INSERT INTO shares (label, key_hash, permission, created_at, created_by, status, remote_node_id, remote_name, remote_public_key, reciprocal)
         VALUES (?, ?, ?, ?, ?, 'active', ?, ?, ?, 1) RETURNING *",
    )
    .bind(label)
    .bind(format!("reciprocal:{}", hex::encode(crate::auth::random_bytes::<16>())))
    .bind(permission)
    .bind(now())
    .bind(created_by)
    .bind(node_id)
    .bind(name)
    .bind(public_key)
    .fetch_one(&mut *tx)
    .await?;
    for id in installation_ids {
        sqlx::query("INSERT INTO share_installations (share_id, installation_id) VALUES (?, ?)").bind(share.id).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(share)
}

pub async fn approve_share(db: &Db, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("UPDATE shares SET status = 'active' WHERE id = ? AND status = 'pending'").bind(id).execute(db).await?;
    Ok(r.rows_affected() == 1)
}

pub async fn share_installation_ids(db: &Db, share_id: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar("SELECT installation_id FROM share_installations WHERE share_id = ? ORDER BY installation_id").bind(share_id).fetch_all(db).await
}

pub async fn set_share_installations(db: &Db, share_id: i64, installation_ids: &[i64]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM share_installations WHERE share_id = ?").bind(share_id).execute(&mut *tx).await?;
    for id in installation_ids {
        sqlx::query("INSERT INTO share_installations (share_id, installation_id) VALUES (?, ?)").bind(share_id).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn update_share(db: &Db, id: i64, label: &str, permission: &str, expires_at: Option<i64>) -> sqlx::Result<()> {
    sqlx::query("UPDATE shares SET label = ?, permission = ?, expires_at = ? WHERE id = ?").bind(label).bind(permission).bind(expires_at).bind(id).execute(db).await?;
    Ok(())
}

/// Records a call made with a share key (who, from where, when).
pub async fn touch_share(db: &Db, id: i64, remote_node_id: Option<&str>, remote_name: Option<&str>, address: Option<&str>) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE shares SET last_seen = ?, last_address = COALESCE(?, last_address),
         remote_node_id = COALESCE(?, remote_node_id), remote_name = COALESCE(?, remote_name) WHERE id = ?",
    )
    .bind(now())
    .bind(address)
    .bind(remote_node_id)
    .bind(remote_name)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete_share(db: &Db, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM shares WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}

#[derive(Debug, Clone, FromRow)]
pub struct Node {
    pub id: i64,
    pub remote_node_id: String,
    pub name: String,
    pub url: String,
    pub key_enc: String,
    pub permission: String,
    pub created_at: i64,
    pub last_seen: Option<i64>,
    pub last_error: Option<String>,
    /// Certificate trusted by an administrator (SHA-256): the only one accepted for this node.
    pub tls_fingerprint: Option<String>,
    /// Capabilities of the node (JSON, ADR 0012) and when they were received.
    pub peer_caps: Option<String>,
    pub peer_caps_at: Option<i64>,
}

pub async fn set_node_caps(db: &Db, id: i64, caps: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE nodes SET peer_caps = ?, peer_caps_at = ? WHERE id = ?").bind(caps).bind(now()).bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_share_caps(db: &Db, id: i64, caps: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE shares SET peer_caps = ?, peer_caps_at = ? WHERE id = ?").bind(caps).bind(now()).bind(id).execute(db).await?;
    Ok(())
}

pub async fn list_nodes(db: &Db) -> sqlx::Result<Vec<Node>> {
    sqlx::query_as("SELECT * FROM nodes ORDER BY name").fetch_all(db).await
}

pub async fn node_by_id(db: &Db, id: i64) -> sqlx::Result<Option<Node>> {
    sqlx::query_as("SELECT * FROM nodes WHERE id = ?").bind(id).fetch_optional(db).await
}

/// Adds a node, or replaces the URL and key of a node added before (same remote id).
pub async fn upsert_node(db: &Db, remote_node_id: &str, name: &str, url: &str, key_enc: &str, permission: &str) -> sqlx::Result<Node> {
    sqlx::query_as(
        "INSERT INTO nodes (remote_node_id, name, url, key_enc, permission, created_at, last_seen) VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (remote_node_id) DO UPDATE SET name = excluded.name, url = excluded.url, key_enc = excluded.key_enc,
           permission = excluded.permission, last_seen = excluded.last_seen, last_error = NULL
         RETURNING *",
    )
    .bind(remote_node_id)
    .bind(name)
    .bind(url)
    .bind(key_enc)
    .bind(permission)
    .bind(now())
    .bind(now())
    .fetch_one(db)
    .await
}

/// Installations of a node this node chose not to show (their ids on that node).
pub async fn node_exclusions(db: &Db, node_id: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar("SELECT remote_id FROM node_excluded_installations WHERE node_id = ? ORDER BY remote_id").bind(node_id).fetch_all(db).await
}

pub async fn set_node_exclusions(db: &Db, node_id: i64, remote_ids: &[i64]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM node_excluded_installations WHERE node_id = ?").bind(node_id).execute(&mut *tx).await?;
    for id in remote_ids {
        sqlx::query("INSERT OR IGNORE INTO node_excluded_installations (node_id, remote_id) VALUES (?, ?)").bind(node_id).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn set_node_fingerprint(db: &Db, id: i64, fingerprint: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE nodes SET tls_fingerprint = ? WHERE id = ?").bind(fingerprint).bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_node_url(db: &Db, id: i64, url: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE nodes SET url = ? WHERE id = ?").bind(url).bind(id).execute(db).await?;
    sqlx::query("UPDATE installations SET host = ? WHERE node_id = ?").bind(url).bind(id).execute(db).await?;
    Ok(())
}

/// Result of the last call to a node.
pub async fn set_node_seen(db: &Db, id: i64, name: Option<&str>, permission: Option<&str>, error: Option<&str>) -> sqlx::Result<()> {
    if error.is_some() {
        sqlx::query("UPDATE nodes SET last_error = ? WHERE id = ?").bind(error).bind(id).execute(db).await?;
    } else {
        sqlx::query("UPDATE nodes SET last_seen = ?, last_error = NULL, name = COALESCE(?, name), permission = COALESCE(?, permission) WHERE id = ?")
            .bind(now())
            .bind(name)
            .bind(permission)
            .bind(id)
            .execute(db)
            .await?;
    }
    Ok(())
}

pub async fn delete_node(db: &Db, id: i64) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM installations WHERE node_id = ?").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM nodes WHERE id = ?").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Airzone schedules (mirror of Airzone Cloud)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct AirzoneScheduleRow {
    pub cloud_id: String,
    pub sched_id: String,
    pub data: String,
    pub zones: String,
    pub synced_at: i64,
}

pub async fn list_airzone_schedules(db: &Db) -> sqlx::Result<Vec<AirzoneScheduleRow>> {
    sqlx::query_as("SELECT * FROM airzone_schedules").fetch_all(db).await
}

/// Replaces the schedules of one cloud installation: rows = (schedule id, JSON, zones JSON).
pub async fn replace_airzone_schedules(db: &Db, cloud_id: &str, rows: &[(String, String, String)]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM airzone_schedules WHERE cloud_id = ?").bind(cloud_id).execute(&mut *tx).await?;
    for (id, data, zones) in rows {
        sqlx::query("INSERT OR REPLACE INTO airzone_schedules (cloud_id, sched_id, data, zones, synced_at) VALUES (?, ?, ?, ?, ?)")
            .bind(cloud_id)
            .bind(id)
            .bind(data)
            .bind(zones)
            .bind(now())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Forgets the schedules of cloud installations no longer in the account.
pub async fn retain_airzone_schedules(db: &Db, cloud_ids: &[String]) -> sqlx::Result<()> {
    for row in list_airzone_schedules(db).await? {
        if !cloud_ids.contains(&row.cloud_id) {
            sqlx::query("DELETE FROM airzone_schedules WHERE cloud_id = ?").bind(&row.cloud_id).execute(db).await?;
        }
    }
    Ok(())
}

pub async fn clear_airzone_schedules(db: &Db) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM airzone_schedules").execute(db).await?;
    Ok(())
}

/// Updates the stored organisation (zones, device ids) of a cloud installation.
pub async fn set_cloud_detail(db: &Db, cloud_id: &str, detail: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE cloud_installations SET detail = ? WHERE cloud_id = ?").bind(detail).bind(cloud_id).execute(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Timers and Airzone routines
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct TimerRow {
    pub id: i64,
    pub label: String,
    pub run_at: i64,
    pub target: String,
    pub command: String,
    pub created_by: Option<String>,
    pub created_at: i64,
}

pub async fn insert_timer(db: &Db, label: &str, run_at: i64, target: &str, command: &str, created_by: &str) -> sqlx::Result<TimerRow> {
    sqlx::query_as("INSERT INTO timers (label, run_at, target, command, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?) RETURNING *")
        .bind(label)
        .bind(run_at)
        .bind(target)
        .bind(command)
        .bind(created_by)
        .bind(now())
        .fetch_one(db)
        .await
}

pub async fn list_timers(db: &Db) -> sqlx::Result<Vec<TimerRow>> {
    sqlx::query_as("SELECT * FROM timers ORDER BY run_at").fetch_all(db).await
}

pub async fn timer_by_id(db: &Db, id: i64) -> sqlx::Result<Option<TimerRow>> {
    sqlx::query_as("SELECT * FROM timers WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn due_timers(db: &Db, now: i64) -> sqlx::Result<Vec<TimerRow>> {
    sqlx::query_as("SELECT * FROM timers WHERE run_at <= ? ORDER BY run_at").bind(now).fetch_all(db).await
}

pub async fn delete_timer(db: &Db, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM timers WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_airzone_routines(db: &Db, cloud_id: &str, data: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO airzone_routines (cloud_id, data, synced_at) VALUES (?, ?, ?) ON CONFLICT (cloud_id) DO UPDATE SET data = excluded.data, synced_at = excluded.synced_at",
    )
    .bind(cloud_id)
    .bind(data)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn list_airzone_routines(db: &Db) -> sqlx::Result<Vec<(String, String)>> {
    sqlx::query_as("SELECT cloud_id, data FROM airzone_routines").fetch_all(db).await
}

// ---------------------------------------------------------------------------------------
// Rooms and devices (ADR 0013)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Room {
    pub id: i64,
    pub name: String,
    pub position: i64,
    pub created_at: i64,
}

pub async fn list_rooms(db: &Db) -> sqlx::Result<Vec<Room>> {
    sqlx::query_as("SELECT * FROM rooms ORDER BY position, name").fetch_all(db).await
}

pub async fn room_by_id(db: &Db, id: i64) -> sqlx::Result<Option<Room>> {
    sqlx::query_as("SELECT * FROM rooms WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn insert_room(db: &Db, name: &str, position: i64) -> sqlx::Result<Room> {
    let id = sqlx::query("INSERT INTO rooms (name, position, created_at) VALUES (?, ?, ?)").bind(name).bind(position).bind(now()).execute(db).await?.last_insert_rowid();
    Ok(room_by_id(db, id).await?.expect("room just inserted"))
}

pub async fn update_room(db: &Db, id: i64, name: &str, position: i64) -> sqlx::Result<bool> {
    Ok(sqlx::query("UPDATE rooms SET name = ?, position = ? WHERE id = ?").bind(name).bind(position).bind(id).execute(db).await?.rows_affected() > 0)
}

/// The devices of the room stay, without a room.
pub async fn delete_room(db: &Db, id: i64) -> sqlx::Result<bool> {
    Ok(sqlx::query("DELETE FROM rooms WHERE id = ?").bind(id).execute(db).await?.rows_affected() > 0)
}

#[derive(Debug, Clone, FromRow)]
pub struct Device {
    pub id: i64,
    pub source: String,
    pub source_ref: String,
    pub installation_id: Option<i64>,
    pub kind: String,
    pub name: Option<String>,
    pub room_id: Option<i64>,
    pub position: i64,
    pub created_at: i64,
    /// Configured source providing it (drivers other than Airzone).
    pub source_id: Option<i64>,
    /// For an energy measure: "grid" | "solar" | "battery" | "consumption".
    pub role: Option<String>,
    /// What a "consumption" measure measures: a device, or a room.
    pub measures_device_id: Option<i64>,
    pub measures_room_id: Option<i64>,
}

pub async fn set_device_role(db: &Db, id: i64, role: Option<&str>, device: Option<i64>, room: Option<i64>) -> sqlx::Result<()> {
    sqlx::query("UPDATE devices SET role = ?, measures_device_id = ?, measures_room_id = ? WHERE id = ?").bind(role).bind(device).bind(room).bind(id).execute(db).await?;
    Ok(())
}

/// Records a device of a source if it is new (its id then stays the same for good).
pub async fn ensure_source_device(db: &Db, source_id: i64, driver: &str, reference: &str, kind: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO devices (source, source_ref, source_id, kind, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(driver)
        .bind(format!("{source_id}/{reference}"))
        .bind(source_id)
        .bind(kind)
        .bind(now())
        .execute(db)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, FromRow)]
pub struct Source {
    pub id: i64,
    pub driver: String,
    pub name: String,
    /// JSON, specific to the driver.
    pub config: String,
    pub enabled: bool,
    pub created_at: i64,
    /// Encrypted with the server key (a HomeKit pairing); never sent by the API.
    pub secret: Option<String>,
}

pub async fn list_sources(db: &Db) -> sqlx::Result<Vec<Source>> {
    sqlx::query_as("SELECT * FROM sources ORDER BY name, id").fetch_all(db).await
}

pub async fn source_by_id(db: &Db, id: i64) -> sqlx::Result<Option<Source>> {
    sqlx::query_as("SELECT * FROM sources WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn insert_source(db: &Db, driver: &str, name: &str, config: &str) -> sqlx::Result<i64> {
    Ok(sqlx::query("INSERT INTO sources (driver, name, config, created_at) VALUES (?, ?, ?, ?)")
        .bind(driver)
        .bind(name)
        .bind(config)
        .bind(now())
        .execute(db)
        .await?
        .last_insert_rowid())
}

pub async fn update_source(db: &Db, id: i64, name: &str, config: &str, enabled: bool) -> sqlx::Result<bool> {
    Ok(sqlx::query("UPDATE sources SET name = ?, config = ?, enabled = ? WHERE id = ?").bind(name).bind(config).bind(enabled).bind(id).execute(db).await?.rows_affected() > 0)
}

pub async fn set_source_secret(db: &Db, id: i64, secret: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE sources SET secret = ? WHERE id = ?").bind(secret).bind(id).execute(db).await?;
    Ok(())
}

pub async fn delete_source(db: &Db, id: i64) -> sqlx::Result<bool> {
    Ok(sqlx::query("DELETE FROM sources WHERE id = ?").bind(id).execute(db).await?.rows_affected() > 0)
}

/// Records a device if it is new (its id then stays the same for good).
pub async fn ensure_device(db: &Db, source: &str, source_ref: &str, installation_id: Option<i64>, kind: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO devices (source, source_ref, installation_id, kind, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(source)
        .bind(source_ref)
        .bind(installation_id)
        .bind(kind)
        .bind(now())
        .execute(db)
        .await?;
    Ok(())
}

pub async fn list_devices(db: &Db) -> sqlx::Result<Vec<Device>> {
    sqlx::query_as("SELECT * FROM devices ORDER BY position, id").fetch_all(db).await
}

pub async fn device_by_id(db: &Db, id: i64) -> sqlx::Result<Option<Device>> {
    sqlx::query_as("SELECT * FROM devices WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn update_device(db: &Db, id: i64, name: Option<&str>, room_id: Option<i64>, position: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE devices SET name = ?, room_id = ?, position = ? WHERE id = ?").bind(name).bind(room_id).bind(position).bind(id).execute(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Device history
// ---------------------------------------------------------------------------------------

/// A number of a device's state at a point of its history.
#[derive(Debug, Clone, PartialEq, FromRow)]
pub struct DeviceSample {
    pub device_id: i64,
    pub ts: i64,
    pub field: String,
    pub value: f64,
}

/// Id of each device of the configured sources, by `source_id/reference`.
pub async fn source_device_ids(db: &Db) -> sqlx::Result<HashMap<String, i64>> {
    let rows: Vec<(String, i64)> = sqlx::query_as("SELECT source_ref, id FROM devices WHERE source_id IS NOT NULL").fetch_all(db).await?;
    Ok(rows.into_iter().collect())
}

pub async fn insert_device_samples(db: &Db, samples: &[DeviceSample]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    for s in samples {
        sqlx::query("INSERT OR REPLACE INTO device_samples (device_id, ts, field, value) VALUES (?, ?, ?, ?)")
            .bind(s.device_id)
            .bind(s.ts)
            .bind(&s.field)
            .bind(s.value)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// History of some devices from `since` to `until` (excluded), sorted by device then time; every
/// field when `fields` is empty.
pub async fn device_samples(db: &Db, devices: &[i64], fields: &[&str], since: i64, until: i64) -> sqlx::Result<Vec<DeviceSample>> {
    let mut out = vec![];
    for d in devices {
        let rows: Vec<DeviceSample> = sqlx::query_as("SELECT device_id, ts, field, value FROM device_samples WHERE device_id = ? AND ts >= ? AND ts < ? ORDER BY ts")
            .bind(d)
            .bind(since)
            .bind(until)
            .fetch_all(db)
            .await?;
        out.extend(rows.into_iter().filter(|r| fields.is_empty() || fields.contains(&r.field.as_str())));
    }
    Ok(out)
}
