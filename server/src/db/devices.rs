//! Rooms and devices (ADR 0013).

use super::*;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Room {
    pub id: i64,
    pub name: String,
    pub position: i64,
    pub created_at: i64,
    /// Chosen by hand (a Lucide name, "sofa"); none: the clients guess it from the name.
    pub icon: Option<String>,
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

/// Sets the icon of a room (`None`: guessed from its name again).
pub async fn set_room_icon(db: &Db, id: i64, icon: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE rooms SET icon = ? WHERE id = ?").bind(icon).bind(id).execute(db).await?;
    Ok(())
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
    // The kind may get finer with a new version (a shutter, not just a cover): it follows.
    sqlx::query("INSERT INTO devices (source, source_ref, source_id, kind, created_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT (source, source_ref) DO UPDATE SET kind = excluded.kind WHERE devices.kind <> excluded.kind")
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
/// Records the devices not known yet, in one transaction; known ones cost a read only.
pub async fn ensure_devices(db: &Db, source: &str, devices: &[(String, Option<i64>, &str)]) -> sqlx::Result<()> {
    let known: std::collections::HashSet<String> =
        sqlx::query_scalar::<_, String>("SELECT source_ref FROM devices WHERE source = ?").bind(source).fetch_all(db).await?.into_iter().collect();
    let new: Vec<_> = devices.iter().filter(|(r, _, _)| !known.contains(r)).collect();
    if new.is_empty() {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    for (source_ref, installation_id, kind) in new {
        sqlx::query("INSERT OR IGNORE INTO devices (source, source_ref, installation_id, kind, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(source)
            .bind(source_ref)
            .bind(installation_id)
            .bind(kind)
            .bind(now())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
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

/// Removes the devices of a source with these references (those no longer its own).
pub async fn delete_source_devices(db: &Db, source_id: i64, references: &[String]) -> sqlx::Result<u64> {
    let mut n = 0;
    for r in references {
        n += sqlx::query("DELETE FROM devices WHERE source_id = ? AND source_ref = ?").bind(source_id).bind(format!("{source_id}/{r}")).execute(db).await?.rows_affected();
    }
    Ok(n)
}
