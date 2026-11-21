//! The modules this server has (see `modules`).

use super::*;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ModuleRow {
    pub id: String,
    pub version: String,
    pub previous_version: Option<String>,
    pub origin: String,
    pub enabled: bool,
    pub state: String,
    pub last_error: Option<String>,
    pub auto_update: bool,
    pub installed_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "id, version, previous_version, origin, enabled, state, last_error, auto_update, installed_at, updated_at";

pub async fn list_modules(db: &Db) -> sqlx::Result<Vec<ModuleRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {COLUMNS} FROM modules ORDER BY id"))).fetch_all(db).await
}

/// Writes the modules compiled into this server at their version, and forgets those it no longer
/// has (a module of another origin is left as it is).
pub async fn sync_native_modules(db: &Db, modules: &[(&str, &str)]) -> sqlx::Result<()> {
    let now = now();
    let mut tx = db.begin().await?;
    for (id, version) in modules {
        sqlx::query(
            "INSERT INTO modules (id, version, origin, installed_at, updated_at) VALUES (?, ?, 'native', ?, ?) \
             ON CONFLICT (id) DO UPDATE SET version = excluded.version, updated_at = excluded.updated_at WHERE modules.origin = 'native' AND modules.version <> excluded.version",
        )
        .bind(id)
        .bind(version)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    let ids: Vec<&str> = modules.iter().map(|(id, _)| *id).collect();
    let all: Vec<(String,)> = sqlx::query_as("SELECT id FROM modules WHERE origin = 'native'").fetch_all(&mut *tx).await?;
    for (id,) in all.into_iter().filter(|(id,)| !ids.contains(&id.as_str())) {
        sqlx::query("DELETE FROM modules WHERE id = ?").bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await
}

pub async fn module_row(db: &Db, id: &str) -> sqlx::Result<Option<ModuleRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {COLUMNS} FROM modules WHERE id = ?"))).bind(id).fetch_optional(db).await
}

/// Records a module installed at `version` (the one before kept to go back to).
pub async fn save_module(db: &Db, id: &str, version: &str, previous: Option<&str>, origin: &str) -> sqlx::Result<()> {
    let now = now();
    sqlx::query(
        "INSERT INTO modules (id, version, previous_version, origin, state, installed_at, updated_at) VALUES (?, ?, ?, ?, 'installed', ?, ?) \
         ON CONFLICT (id) DO UPDATE SET version = excluded.version, previous_version = excluded.previous_version, origin = excluded.origin, \
         state = 'installed', last_error = NULL, updated_at = excluded.updated_at",
    )
    .bind(id)
    .bind(version)
    .bind(previous)
    .bind(origin)
    .bind(now)
    .bind(now)
    .execute(db)
    .await
    .map(|_| ())
}

pub async fn delete_module(db: &Db, id: &str) -> sqlx::Result<bool> {
    Ok(sqlx::query("DELETE FROM modules WHERE id = ? AND origin <> 'native'").bind(id).execute(db).await?.rows_affected() > 0)
}

/// The versions of a module given up after a failure: not installed again.
pub async fn rejected_versions(db: &Db, id: &str) -> sqlx::Result<Vec<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT rejected FROM modules WHERE id = ?").bind(id).fetch_optional(db).await?;
    Ok(row.and_then(|(r,)| serde_json::from_str(&r).ok()).unwrap_or_default())
}

pub async fn set_module_state(db: &Db, id: &str, state: &str, error: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE modules SET state = ?, last_error = ?, updated_at = ? WHERE id = ?").bind(state).bind(error).bind(now()).bind(id).execute(db).await.map(|_| ())
}
