//! Overviews of the installations (see `dashboard`).

use super::*;

/// The layout stored for an installation: its default (`user`: `None`) or a user's own.
pub async fn dashboard(db: &Db, installation_id: i64, user: Option<i64>) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT layout FROM dashboards WHERE installation_id = ? AND IFNULL(user_id, 0) = IFNULL(?, 0)").bind(installation_id).bind(user).fetch_optional(db).await
}

pub async fn set_dashboard(db: &Db, installation_id: i64, user: Option<i64>, layout: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO dashboards (installation_id, user_id, layout, updated_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT (installation_id, IFNULL(user_id, 0)) DO UPDATE SET layout = excluded.layout, updated_at = excluded.updated_at",
    )
    .bind(installation_id)
    .bind(user)
    .bind(layout)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

/// `true` when there was one.
pub async fn delete_dashboard(db: &Db, installation_id: i64, user: Option<i64>) -> sqlx::Result<bool> {
    let done = sqlx::query("DELETE FROM dashboards WHERE installation_id = ? AND IFNULL(user_id, 0) = IFNULL(?, 0)").bind(installation_id).bind(user).execute(db).await?;
    Ok(done.rows_affected() > 0)
}
