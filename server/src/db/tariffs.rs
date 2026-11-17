//! The electricity contract of the home and its tariffs (see `price`).

use super::*;
use crate::price::Contract;

#[derive(FromRow)]
struct ContractRow {
    country: Option<String>,
    supplier: Option<String>,
    offer: String,
    power_kva: Option<f64>,
    currency: String,
    offpeak: Option<String>,
}

pub async fn contract(db: &Db) -> sqlx::Result<Option<Contract>> {
    let row: Option<ContractRow> =
        sqlx::query_as("SELECT country, supplier, offer, power_kva, currency, offpeak FROM electricity_contract WHERE id = 1").fetch_optional(db).await?;
    Ok(row.map(|r| Contract {
        country: r.country,
        supplier: r.supplier,
        offer: r.offer,
        power_kva: r.power_kva,
        currency: r.currency,
        offpeak: r.offpeak.and_then(|o| serde_json::from_str(&o).ok()).unwrap_or_default(),
    }))
}

pub async fn set_contract(db: &Db, c: &Contract) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO electricity_contract (id, country, supplier, offer, power_kva, currency, offpeak, updated_at) VALUES (1, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (id) DO UPDATE SET country = excluded.country, supplier = excluded.supplier, offer = excluded.offer, \
         power_kva = excluded.power_kva, currency = excluded.currency, offpeak = excluded.offpeak, updated_at = excluded.updated_at",
    )
    .bind(&c.country)
    .bind(&c.supplier)
    .bind(&c.offer)
    .bind(c.power_kva)
    .bind(&c.currency)
    .bind((!c.offpeak.is_empty()).then(|| serde_json::to_string(&c.offpeak).unwrap_or_default()))
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

/// The stored tariffs: (`valid_from`, JSON), oldest first.
pub async fn tariffs(db: &Db) -> sqlx::Result<Vec<(String, String)>> {
    sqlx::query_as("SELECT valid_from, tariff FROM electricity_tariffs ORDER BY valid_from").fetch_all(db).await
}

pub async fn set_tariff(db: &Db, valid_from: &str, tariff: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO electricity_tariffs (valid_from, tariff, updated_at) VALUES (?, ?, ?) ON CONFLICT (valid_from) DO UPDATE SET tariff = excluded.tariff, updated_at = excluded.updated_at")
        .bind(valid_from)
        .bind(tariff)
        .bind(now())
        .execute(db)
        .await?;
    Ok(())
}

/// `true` when there was one.
pub async fn delete_tariff(db: &Db, valid_from: &str) -> sqlx::Result<bool> {
    Ok(sqlx::query("DELETE FROM electricity_tariffs WHERE valid_from = ?").bind(valid_from).execute(db).await?.rows_affected() > 0)
}

/// The colour announced for a local day ("blue", "white", "red").
pub async fn set_day_color(db: &Db, day: &str, color: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO day_colors (day, color, fetched_at) VALUES (?, ?, ?) ON CONFLICT (day) DO UPDATE SET color = excluded.color, fetched_at = excluded.fetched_at")
        .bind(day)
        .bind(color)
        .bind(now())
        .execute(db)
        .await?;
    Ok(())
}

/// The colours known from `from` to `until` (local days, both included).
pub async fn day_colors(db: &Db, from: &str, until: &str) -> sqlx::Result<Vec<(String, String)>> {
    sqlx::query_as("SELECT day, color FROM day_colors WHERE day >= ? AND day <= ? ORDER BY day").bind(from).bind(until).fetch_all(db).await
}

/// All the tariffs replaced by these (`valid_from`, JSON), at once: the prices of an offer read
/// from the CRE.
pub async fn replace_tariffs(db: &Db, tariffs: &[(String, String)]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM electricity_tariffs").execute(&mut *tx).await?;
    for (from, json) in tariffs {
        sqlx::query("INSERT INTO electricity_tariffs (valid_from, tariff, updated_at) VALUES (?, ?, ?)").bind(from).bind(json).bind(now()).execute(&mut *tx).await?;
    }
    tx.commit().await
}
