//! Events (activity feed).

use super::*;

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

/// What a user may see and drive among the devices of the modules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceGrant {
    /// "all" | "room" | "type" | "device"
    pub scope: String,
    /// The room id, the type ("light", "shutter"…) or the device id; empty for "all".
    #[serde(default, rename = "ref")]
    pub reference: String,
    /// "read" | "control"
    pub permission: String,
}

pub async fn user_device_access(db: &Db, user_id: i64) -> sqlx::Result<Vec<DeviceGrant>> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT scope, ref, permission FROM user_device_access WHERE user_id = ? ORDER BY scope, ref").bind(user_id).fetch_all(db).await?;
    Ok(rows.into_iter().map(|(scope, reference, permission)| DeviceGrant { scope, reference, permission }).collect())
}

/// Replaces what a user may see and drive among the devices of the modules.
pub async fn set_user_device_access(db: &Db, user_id: i64, grants: &[DeviceGrant]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM user_device_access WHERE user_id = ?").bind(user_id).execute(&mut *tx).await?;
    for g in grants {
        sqlx::query("INSERT OR REPLACE INTO user_device_access (user_id, scope, ref, permission) VALUES (?, ?, ?, ?)")
            .bind(user_id)
            .bind(&g.scope)
            .bind(&g.reference)
            .bind(&g.permission)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
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
