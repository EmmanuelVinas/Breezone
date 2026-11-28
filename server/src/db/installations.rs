//! Installations & zone metadata.

use super::*;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Installation {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub enabled: bool,
    pub created_at: i64,
    /// City / location (from Airzone Cloud, or set by hand), shown in "Mes installations".
    pub location: Option<String>,
    /// Country (ISO 3166-1 alpha-2), set by hand.
    pub country: Option<String>,
    /// The city was set by hand: the Airzone account no longer overwrites it.
    pub location_set: bool,
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
    /// Where its city is, found once when the city is set (see `weather`); not sent to clients.
    #[serde(skip)]
    pub latitude: Option<f64>,
    #[serde(skip)]
    pub longitude: Option<f64>,
    /// The area of the city (the French "département"), for the weather warnings.
    #[serde(skip)]
    pub area: Option<String>,
    /// The country found with it (ISO 3166-1 alpha-2).
    #[serde(skip)]
    pub geo_country: Option<String>,
    /// The city and country the coordinates were found for.
    #[serde(skip)]
    pub geo_key: Option<String>,
}

/// The name of the home this server creates when it has none of its own.
pub const HOME_NAME: &str = "Maison";

impl Installation {
    pub fn is_local(&self) -> bool {
        self.node_id.is_none()
    }

    /// Airzone is one integration among others: a home of this server may have no WebServer.
    pub fn has_airzone(&self) -> bool {
        !self.is_local() || !self.host.trim().is_empty()
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

/// A home of this server, so that its modules have one even without an Airzone WebServer.
pub async fn ensure_home(db: &Db) -> sqlx::Result<Option<Installation>> {
    if list_installations(db).await?.iter().any(Installation::is_local) {
        return Ok(None);
    }
    insert_installation(db, HOME_NAME, "", 3000, true).await.map(Some)
}

/// Adds an Airzone WebServer: to the first home of this server that has none (it keeps its rooms,
/// devices and settings, and takes the WebServer's name while it has the default one), else as
/// another installation.
pub async fn add_webserver(db: &Db, name: &str, host: &str, port: i64, enabled: bool) -> sqlx::Result<Installation> {
    let home: Option<Installation> = sqlx::query_as("SELECT * FROM installations WHERE node_id IS NULL AND trim(host) = '' ORDER BY id LIMIT 1").fetch_optional(db).await?;
    let Some(home) = home else { return insert_installation(db, name, host, port, enabled).await };
    let name = if home.name == HOME_NAME { name } else { home.name.as_str() };
    sqlx::query_as("UPDATE installations SET name = ?, host = ?, port = ?, enabled = ?, mac = NULL WHERE id = ? RETURNING *")
        .bind(name)
        .bind(host)
        .bind(port)
        .bind(enabled)
        .bind(home.id)
        .fetch_one(db)
        .await
}

/// The home loses its WebServer: its zones and their history go, the home stays with its rooms,
/// devices and settings.
pub async fn remove_webserver(db: &Db, id: i64) -> sqlx::Result<()> {
    reset_installation_for_new_webserver(db, id).await?;
    sqlx::query("UPDATE installations SET host = '', port = 3000, mac = NULL WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
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

/// The city and country of an installation, set by hand (an empty city goes back to the account's).
pub async fn set_installation_place(db: &Db, id: i64, location: Option<&str>, country: Option<&str>) -> sqlx::Result<()> {
    let location = location.map(str::trim).filter(|l| !l.is_empty());
    sqlx::query("UPDATE installations SET location = ?, location_set = ?, country = ? WHERE id = ?")
        .bind(location)
        .bind(location.is_some())
        .bind(country.map(str::trim).filter(|c| !c.is_empty()).map(str::to_ascii_uppercase))
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// The city from the Airzone account: kept only when not set by hand.
pub async fn set_installation_location(db: &Db, id: i64, location: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE installations SET location = ? WHERE id = ? AND location_set = 0").bind(location).bind(id).execute(db).await?;
    Ok(())
}

/// Where the city of an installation is (`key`: the city and country it was found for; no
/// coordinates: not found).
pub async fn set_installation_geo(db: &Db, id: i64, key: &str, coords: Option<(f64, f64)>, area: Option<&str>, country: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE installations SET geo_key = ?, latitude = ?, longitude = ?, area = ?, geo_country = ? WHERE id = ?")
        .bind(key)
        .bind(coords.map(|c| c.0))
        .bind(coords.map(|c| c.1))
        .bind(area)
        .bind(country)
        .bind(id)
        .execute(db)
        .await?;
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
