//! Per-installation access point.

use super::*;

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
