//! Airzone Cloud installations.

use super::*;

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
