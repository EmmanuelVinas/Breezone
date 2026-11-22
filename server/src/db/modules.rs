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

/// Forgets the rows of the drivers once compiled into the server (origin `native`): they are
/// modules shipped with Breezone now, installed at start.
pub async fn forget_native_modules(db: &Db) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM modules WHERE origin = 'native'").execute(db).await.map(|_| ())
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

/// A value of the store of a source (see `module_store`).
pub async fn store_get(db: &Db, source_id: i64, key: &str) -> sqlx::Result<Option<Vec<u8>>> {
    let row: Option<(Vec<u8>,)> = sqlx::query_as("SELECT value FROM module_store WHERE source_id = ? AND key = ?").bind(source_id).bind(key).fetch_optional(db).await?;
    Ok(row.map(|(v,)| v))
}

/// Sets (or removes, with `None`) a value of the store of a source.
pub async fn store_set(db: &Db, source_id: i64, key: &str, value: Option<&[u8]>) -> sqlx::Result<()> {
    match value {
        Some(v) => sqlx::query("INSERT INTO module_store (source_id, key, value) VALUES (?, ?, ?) ON CONFLICT (source_id, key) DO UPDATE SET value = excluded.value")
            .bind(source_id)
            .bind(key)
            .bind(v)
            .execute(db)
            .await
            .map(|_| ()),
        None => sqlx::query("DELETE FROM module_store WHERE source_id = ? AND key = ?").bind(source_id).bind(key).execute(db).await.map(|_| ()),
    }
}

/// Gives up a version of a module after a failure: it is not installed again.
pub async fn reject_version(db: &Db, id: &str, version: &str) -> sqlx::Result<()> {
    let mut rejected = rejected_versions(db, id).await?;
    if !rejected.iter().any(|v| v == version) {
        rejected.push(version.to_string());
    }
    let json = serde_json::to_string(&rejected).unwrap_or_else(|_| "[]".into());
    sqlx::query("UPDATE modules SET rejected = ? WHERE id = ?").bind(json).bind(id).execute(db).await.map(|_| ())
}

/// A source of modules (see `module_repositories`).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct RepositoryRow {
    pub id: i64,
    pub name: String,
    pub url: Option<String>,
    pub public_key: Option<String>,
    pub official: bool,
    pub enabled: bool,
    pub seq: Option<i64>,
    pub created_at: i64,
}

const REPOSITORY_COLUMNS: &str = "id, name, url, public_key, official, enabled, seq, created_at";

pub async fn list_repositories(db: &Db) -> sqlx::Result<Vec<RepositoryRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {REPOSITORY_COLUMNS} FROM module_repositories ORDER BY official DESC, id"))).fetch_all(db).await
}

pub async fn repository(db: &Db, id: i64) -> sqlx::Result<Option<RepositoryRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {REPOSITORY_COLUMNS} FROM module_repositories WHERE id = ?"))).bind(id).fetch_optional(db).await
}

pub async fn insert_repository(db: &Db, name: &str, url: &str, public_key: &str) -> sqlx::Result<i64> {
    let r = sqlx::query("INSERT INTO module_repositories (name, url, public_key, created_at) VALUES (?, ?, ?, ?)")
        .bind(name)
        .bind(url)
        .bind(public_key)
        .bind(now())
        .execute(db)
        .await?;
    Ok(r.last_insert_rowid())
}

pub async fn set_repository_enabled(db: &Db, id: i64, enabled: bool) -> sqlx::Result<bool> {
    Ok(sqlx::query("UPDATE module_repositories SET enabled = ? WHERE id = ?").bind(enabled).bind(id).execute(db).await?.rows_affected() > 0)
}

pub async fn set_repository_seq(db: &Db, id: i64, seq: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE module_repositories SET seq = ? WHERE id = ?").bind(seq).bind(id).execute(db).await.map(|_| ())
}

pub async fn delete_repository(db: &Db, id: i64) -> sqlx::Result<bool> {
    Ok(sqlx::query("DELETE FROM module_repositories WHERE id = ? AND official = 0").bind(id).execute(db).await?.rows_affected() > 0)
}

/// The source a module was installed from (`None`: compiled in, with Breezone, or unknown).
pub async fn module_repository(db: &Db, id: &str) -> sqlx::Result<Option<i64>> {
    let row: Option<(Option<i64>,)> = sqlx::query_as("SELECT repository FROM modules WHERE id = ?").bind(id).fetch_optional(db).await?;
    Ok(row.and_then(|(r,)| r))
}

pub async fn set_module_repository(db: &Db, id: &str, repository: Option<i64>) -> sqlx::Result<()> {
    sqlx::query("UPDATE modules SET repository = ? WHERE id = ?").bind(repository).bind(id).execute(db).await.map(|_| ())
}
