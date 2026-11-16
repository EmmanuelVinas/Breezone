//! Settings (key/value).

use super::*;

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
