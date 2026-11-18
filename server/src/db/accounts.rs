//! Linked accounts (see `accounts`), and what Enedis measured through one (Linky).

use super::*;

#[derive(Debug, Clone, FromRow)]
pub struct AccountRow {
    pub kind: String,
    pub config: String,
    pub secret: Option<String>,
    pub state: String,
    pub last_error: Option<String>,
    pub last_ok: Option<i64>,
    pub expires_at: Option<i64>,
    pub updated_at: i64,
    /// Switched on: what uses it may use it.
    pub enabled: bool,
}

const COLUMNS: &str = "kind, config, secret, state, last_error, last_ok, expires_at, updated_at, enabled";

pub async fn list_accounts(db: &Db) -> sqlx::Result<Vec<AccountRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {COLUMNS} FROM accounts ORDER BY id"))).fetch_all(db).await
}

pub async fn account(db: &Db, kind: &str) -> sqlx::Result<Option<AccountRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {COLUMNS} FROM accounts WHERE kind = ?"))).bind(kind).fetch_optional(db).await
}

/// Creates or replaces the configuration and secrets of an account (its state is kept).
pub async fn save_account(db: &Db, kind: &str, config: &str, secret: Option<&str>, expires_at: Option<i64>) -> sqlx::Result<()> {
    let now = now();
    sqlx::query(
        "INSERT INTO accounts (kind, config, secret, expires_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT (kind) DO UPDATE SET config = excluded.config, secret = excluded.secret, expires_at = excluded.expires_at, updated_at = excluded.updated_at",
    )
    .bind(kind)
    .bind(config)
    .bind(secret)
    .bind(expires_at)
    .bind(now)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// What the last use of an account said: `error` none means it worked.
pub async fn set_account_state(db: &Db, kind: &str, state: &str, error: Option<&str>) -> sqlx::Result<()> {
    let now = now();
    sqlx::query("UPDATE accounts SET state = ?, last_error = ?, last_ok = CASE WHEN ? IS NULL THEN ? ELSE last_ok END WHERE kind = ?")
        .bind(state)
        .bind(error)
        .bind(error)
        .bind(now)
        .bind(kind)
        .execute(db)
        .await?;
    Ok(())
}

/// Switches an account or a service on or off, without forgetting it.
pub async fn set_account_enabled(db: &Db, kind: &str, enabled: bool) -> sqlx::Result<()> {
    let now = now();
    sqlx::query("INSERT INTO accounts (kind, enabled, created_at, updated_at) VALUES (?, ?, ?, ?) ON CONFLICT (kind) DO UPDATE SET enabled = excluded.enabled, updated_at = excluded.updated_at")
        .bind(kind)
        .bind(enabled)
        .bind(now)
        .bind(now)
        .execute(db)
        .await?;
    Ok(())
}

/// Whether an account or a service is switched on (on when never set).
pub async fn account_enabled(db: &Db, kind: &str) -> bool {
    !matches!(account(db, kind).await, Ok(Some(a)) if !a.enabled)
}

pub async fn delete_account(db: &Db, kind: &str) -> sqlx::Result<bool> {
    Ok(sqlx::query("DELETE FROM accounts WHERE kind = ?").bind(kind).execute(db).await?.rows_affected() > 0)
}

/// Energy of local days (Wh), `None` for what Enedis did not give.
pub async fn set_linky_day(db: &Db, prm: &str, day: &str, consumption: Option<f64>, production: Option<f64>) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO linky_days (prm, day, consumption, production) VALUES (?, ?, ?, ?) \
         ON CONFLICT (prm, day) DO UPDATE SET consumption = COALESCE(excluded.consumption, consumption), production = COALESCE(excluded.production, production)",
    )
    .bind(prm)
    .bind(day)
    .bind(consumption)
    .bind(production)
    .execute(db)
    .await?;
    Ok(())
}

/// (day, consumption Wh, production Wh) from `from` to `until` (both included).
pub async fn linky_days(db: &Db, prm: &str, from: &str, until: &str) -> sqlx::Result<Vec<(String, Option<f64>, Option<f64>)>> {
    sqlx::query_as("SELECT day, consumption, production FROM linky_days WHERE prm = ? AND day >= ? AND day <= ? ORDER BY day").bind(prm).bind(from).bind(until).fetch_all(db).await
}

pub async fn set_linky_curve(db: &Db, prm: &str, points: &[(i64, f64)]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    for (ts, power) in points {
        sqlx::query("INSERT INTO linky_curve (prm, ts, power) VALUES (?, ?, ?) ON CONFLICT (prm, ts) DO UPDATE SET power = excluded.power")
            .bind(prm)
            .bind(ts)
            .bind(power)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// (start of the half hour, mean power W) from `from` (included) to `until` (excluded).
pub async fn linky_curve(db: &Db, prm: &str, from: i64, until: i64) -> sqlx::Result<Vec<(i64, f64)>> {
    sqlx::query_as("SELECT ts, power FROM linky_curve WHERE prm = ? AND ts >= ? AND ts < ? ORDER BY ts").bind(prm).bind(from).bind(until).fetch_all(db).await
}
