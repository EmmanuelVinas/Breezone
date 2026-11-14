//! Export and import of the configuration, to restore a server or move it to another machine.
//!
//! The file holds what was set up (installations, modules, devices and rooms, routines and timers,
//! users and their rights, a few settings) and nothing secret: no password, no Airzone account
//! password, no HomeKit pairing or Hue key, nothing of the federation (its keys are this server's
//! identity). No history either.
//!
//! An import replaces that configuration. Rows keep their ids, so what refers to them (routines,
//! roles of devices, rights) still does, and the history of what is still there is kept. What the
//! file cannot carry is kept from the server when it is the same thing: the secret of the same
//! module, the Airzone password of the same account, the password of the same user.

use crate::db::{self, Db};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sqlx::sqlite::SqliteArguments;
// Table and column names come from this file's constants and the database schema, never from a
// file being imported.
use sqlx::{Arguments, AssertSqlSafe, Sqlite, SqliteConnection};

pub const FORMAT: &str = "breezone-config";
const VERSION: i64 = 1;

/// What is exported of each table: its rows (a filter) and the columns left out.
struct Table {
    name: &'static str,
    filter: &'static str,
    without: &'static [&'static str],
}

const LOCAL: &str = "installation_id IN (SELECT id FROM installations WHERE node_id IS NULL)";

/// In the order they are inserted (rows referred to first).
const TABLES: &[Table] = &[
    Table { name: "installations", filter: "node_id IS NULL", without: &["node_id", "remote_id"] },
    Table { name: "zone_meta", filter: LOCAL, without: &[] },
    Table { name: "installation_access", filter: "1", without: &[] },
    Table { name: "rooms", filter: "1", without: &[] },
    Table { name: "sources", filter: "1", without: &["secret"] },
    Table { name: "devices", filter: "installation_id IS NULL OR installation_id IN (SELECT id FROM installations WHERE node_id IS NULL)", without: &[] },
    Table { name: "routines", filter: "1", without: &["last_run_at"] },
    Table { name: "timers", filter: "1", without: &[] },
];

/// Settings that are configuration (not secrets, not this server's identity).
const SETTINGS: &[&str] = &["cloud.email", "node.name", "node.public_url", "scan.subnet", crate::update::CHANNEL_SETTING];

async fn schema(db: &Db) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1").fetch_one(db).await
}

async fn columns(conn: &mut SqliteConnection, table: &str) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar("SELECT name FROM pragma_table_info(?)").bind(table).fetch_all(conn).await
}

async fn rows(conn: &mut SqliteConnection, table: &str, columns: &[String], filter: &str) -> sqlx::Result<Vec<Value>> {
    let fields: Vec<String> = columns.iter().map(|c| format!("'{c}', \"{c}\"")).collect();
    let texts: Vec<String> =
        sqlx::query_scalar(AssertSqlSafe(format!("SELECT json_object({}) FROM \"{table}\" WHERE {filter} ORDER BY rowid", fields.join(", ")))).fetch_all(conn).await?;
    Ok(texts.iter().filter_map(|t| serde_json::from_str(t).ok()).collect())
}

/// The configuration of this server, as a file.
pub async fn export(db: &Db, version: &str) -> sqlx::Result<Value> {
    let mut conn = db.acquire().await?;
    let mut tables = Map::new();
    for t in TABLES {
        let cols: Vec<String> = columns(&mut conn, t.name).await?.into_iter().filter(|c| !t.without.contains(&c.as_str())).collect();
        tables.insert(t.name.into(), Value::Array(rows(&mut conn, t.name, &cols, t.filter).await?));
    }
    let users = rows(&mut conn, "users", &["id".into(), "username".into(), "role".into(), "language".into(), "created_at".into()], "1").await?;
    let access_cols = columns(&mut conn, "user_installations").await?;
    let access = rows(&mut conn, "user_installations", &access_cols, LOCAL).await?;
    let mut settings = Map::new();
    for key in SETTINGS {
        if let Some(v) = db::get_setting(db, key).await? {
            settings.insert((*key).into(), Value::String(v));
        }
    }
    Ok(json!({
        "format": FORMAT,
        "version": VERSION,
        "breezone": version,
        "schema": schema(db).await?,
        "exported_at": db::now(),
        "tables": tables,
        "users": users,
        "user_installations": access,
        "settings": settings,
    }))
}

/// What an import changed, and what is left to do by hand.
#[derive(Debug, Serialize)]
pub struct Imported {
    pub installations: usize,
    pub modules: usize,
    pub devices: usize,
    pub routines: usize,
    /// Users added without a password: an administrator gives them one.
    pub without_password: Vec<String>,
    /// Modules to pair again (HomeKit, Hue): their key is not in the file.
    pub to_pair: Vec<String>,
    /// The Airzone account needs its password again.
    pub airzone_password: bool,
}

#[derive(Debug)]
pub enum ImportError {
    Invalid(String),
    Newer,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for ImportError {
    fn from(e: sqlx::Error) -> Self {
        ImportError::Db(e)
    }
}

fn bind(args: &mut SqliteArguments, v: &Value) -> Result<(), ImportError> {
    let r = match v {
        Value::Null => args.add(None::<i64>),
        Value::Bool(b) => args.add(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => args.add(i),
            None => args.add(n.as_f64().unwrap_or_default()),
        },
        Value::String(s) => args.add(s.clone()),
        other => args.add(other.to_string()),
    };
    r.map_err(|e| ImportError::Invalid(e.to_string()))
}

/// Inserts rows, with the columns this server knows (an older file has fewer: their default).
async fn insert(conn: &mut SqliteConnection, table: &str, rows: &[Value], without: &[&str]) -> Result<usize, ImportError> {
    let known = columns(conn, table).await?;
    for row in rows {
        let Some(obj) = row.as_object() else { return Err(ImportError::Invalid(format!("{table}: a row is not an object"))) };
        let cols: Vec<&String> = known.iter().filter(|c| obj.contains_key(c.as_str()) && !without.contains(&c.as_str())).collect();
        if cols.is_empty() {
            continue;
        }
        let mut args = SqliteArguments::default();
        for c in &cols {
            bind(&mut args, &obj[c.as_str()])?;
        }
        let names: Vec<String> = cols.iter().map(|c| format!("\"{c}\"")).collect();
        let marks = vec!["?"; cols.len()].join(", ");
        sqlx::query_with::<Sqlite, _>(AssertSqlSafe(format!("INSERT INTO \"{table}\" ({}) VALUES ({marks})", names.join(", "))), args).execute(&mut *conn).await?;
    }
    Ok(rows.len())
}

fn list<'a>(file: &'a Value, path: &[&str]) -> Result<&'a [Value], ImportError> {
    let mut v = file;
    for p in path {
        v = match v.get(p) {
            Some(v) => v,
            None => return Ok(&[]),
        };
    }
    v.as_array().map(Vec::as_slice).ok_or_else(|| ImportError::Invalid(format!("{} is not a list", path.join("."))))
}

fn ids(rows: &[Value]) -> Vec<i64> {
    rows.iter().filter_map(|r| r.get("id").and_then(Value::as_i64)).collect()
}

/// Replaces the configuration with the file's. `importer`: the administrator importing, who stays
/// an administrator whatever the file says.
pub async fn import(db: &Db, file: &Value, importer: i64) -> Result<Imported, ImportError> {
    if file.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(ImportError::Invalid("not a Breezone configuration".into()));
    }
    if file.get("version").and_then(Value::as_i64).unwrap_or(i64::MAX) > VERSION || file.get("schema").and_then(Value::as_i64).unwrap_or(i64::MAX) > schema(db).await? {
        return Err(ImportError::Newer);
    }
    let installations = list(file, &["tables", "installations"])?;
    let devices = list(file, &["tables", "devices"])?;
    let sources = list(file, &["tables", "sources"])?;

    // Installations and devices reached through another server come back at its next sync: those
    // whose id an imported one takes go now (with what depends on them).
    let wanted = |rows: &[Value]| serde_json::to_string(&ids(rows)).unwrap_or_else(|_| "[]".into());
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM devices WHERE id IN (SELECT value FROM json_each(?)) AND installation_id IN (SELECT id FROM installations WHERE node_id IS NOT NULL)")
        .bind(wanted(devices))
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM installations WHERE node_id IS NOT NULL AND id IN (SELECT value FROM json_each(?))").bind(wanted(installations)).execute(&mut *tx).await?;
    tx.commit().await?;

    // Without foreign keys while rows are replaced: deleting them must not take the history of
    // those that come back with the same id. They are checked before committing.
    let mut conn = db.acquire().await?;
    sqlx::query("PRAGMA foreign_keys = OFF").execute(&mut *conn).await?;
    let result = replace(&mut conn, file, importer, installations, devices, sources).await;
    sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await?;
    result
}

async fn replace(conn: &mut SqliteConnection, file: &Value, importer: i64, installations: &[Value], devices: &[Value], sources: &[Value]) -> Result<Imported, ImportError> {
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    let done = replace_in(conn, file, importer, installations, devices, sources).await;
    match done {
        Ok(imported) => {
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok(imported)
        }
        Err(e) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            Err(e)
        }
    }
}

async fn replace_in(conn: &mut SqliteConnection, file: &Value, importer: i64, installations: &[Value], devices: &[Value], sources: &[Value]) -> Result<Imported, ImportError> {
    // Secrets kept for the same module (same driver and hardware).
    let secrets: Vec<(i64, String, String, String)> =
        sqlx::query_as("SELECT id, driver, COALESCE(json_extract(config, '$.device_id'), ''), secret FROM sources WHERE secret IS NOT NULL").fetch_all(&mut *conn).await?;
    let email: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'cloud.email'").fetch_optional(&mut *conn).await?;

    // Before the installations its filter looks at.
    sqlx::query(AssertSqlSafe(format!("DELETE FROM user_installations WHERE {LOCAL}"))).execute(&mut *conn).await?;
    for t in TABLES.iter().rev() {
        sqlx::query(AssertSqlSafe(format!("DELETE FROM \"{}\" WHERE {}", t.name, t.filter))).execute(&mut *conn).await?;
    }
    let mut counts = vec![];
    for t in TABLES {
        counts.push(insert(conn, t.name, list(file, &["tables", t.name])?, t.without).await?);
    }

    for (id, driver, device, secret) in &secrets {
        sqlx::query("UPDATE sources SET secret = ? WHERE id = ? AND driver = ? AND COALESCE(json_extract(config, '$.device_id'), '') = ?")
            .bind(secret)
            .bind(id)
            .bind(driver)
            .bind(device)
            .execute(&mut *conn)
            .await?;
    }
    let to_pair: Vec<String> = sqlx::query_scalar("SELECT name FROM sources WHERE driver IN ('homekit', 'hue') AND secret IS NULL ORDER BY id").fetch_all(&mut *conn).await?;

    // Users: matched by name. Those already here keep their password; the others get none.
    let mut user_ids = std::collections::HashMap::new();
    let mut without_password = vec![];
    for u in list(file, &["users"])? {
        let (Some(file_id), Some(name)) = (u.get("id").and_then(Value::as_i64), u.get("username").and_then(Value::as_str)) else { continue };
        let role = match u.get("role").and_then(Value::as_str) {
            Some("admin") => "admin",
            _ => "user",
        };
        let language = u.get("language").and_then(Value::as_str);
        let here: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE username = ?").bind(name).fetch_optional(&mut *conn).await?;
        let id = match here {
            Some(id) => {
                let role = if id == importer { "admin" } else { role };
                sqlx::query("UPDATE users SET role = ?, language = ? WHERE id = ?").bind(role).bind(language).bind(id).execute(&mut *conn).await?;
                id
            }
            None => {
                without_password.push(name.to_string());
                sqlx::query_scalar("INSERT INTO users (username, password_hash, role, language, created_at) VALUES (?, '', ?, ?, ?) RETURNING id")
                    .bind(name)
                    .bind(role)
                    .bind(language)
                    .bind(u.get("created_at").and_then(Value::as_i64).unwrap_or_else(db::now))
                    .fetch_one(&mut *conn)
                    .await?
            }
        };
        user_ids.insert(file_id, id);
    }
    let access: Vec<Value> = list(file, &["user_installations"])?
        .iter()
        .filter_map(|a| {
            let user = user_ids.get(&a.get("user_id")?.as_i64()?)?;
            let mut a = a.clone();
            a["user_id"] = json!(user);
            Some(a)
        })
        .collect();
    insert(conn, "user_installations", &access, &[]).await?;

    let settings = file.get("settings").and_then(Value::as_object).cloned().unwrap_or_default();
    for key in SETTINGS {
        sqlx::query("DELETE FROM settings WHERE key = ?").bind(key).execute(&mut *conn).await?;
        if let Some(v) = settings.get(*key).and_then(Value::as_str) {
            sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?)").bind(key).bind(v).execute(&mut *conn).await?;
        }
    }
    let new_email = settings.get("cloud.email").and_then(Value::as_str);
    if new_email != email.as_deref() {
        sqlx::query("DELETE FROM settings WHERE key = 'cloud.password'").execute(&mut *conn).await?;
    }
    let has_password: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM settings WHERE key = 'cloud.password')").fetch_one(&mut *conn).await?;

    // What was about rows that are gone: history and links of this server.
    for q in [
        "DELETE FROM readings WHERE installation_id NOT IN (SELECT id FROM installations)",
        "DELETE FROM device_samples WHERE device_id NOT IN (SELECT id FROM devices)",
        "DELETE FROM share_installations WHERE installation_id NOT IN (SELECT id FROM installations)",
        "UPDATE cloud_installations SET installation_id = NULL WHERE installation_id NOT IN (SELECT id FROM installations)",
    ] {
        sqlx::query(q).execute(&mut *conn).await?;
    }
    let broken: Vec<(String, String)> = sqlx::query_as("SELECT \"table\", parent FROM pragma_foreign_key_check").fetch_all(&mut *conn).await?;
    if let Some((table, parent)) = broken.first() {
        return Err(ImportError::Invalid(format!("{table} refers to a missing row of {parent}")));
    }

    Ok(Imported {
        installations: installations.len(),
        modules: sources.len(),
        devices: devices.len(),
        routines: counts[TABLES.iter().position(|t| t.name == "routines").unwrap_or(0)],
        without_password,
        to_pair,
        airzone_password: new_email.is_some() && !has_password,
    })
}
