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
}

pub async fn contract(db: &Db) -> sqlx::Result<Option<Contract>> {
    let row: Option<ContractRow> = sqlx::query_as("SELECT country, supplier, offer, power_kva, currency FROM electricity_contract WHERE id = 1").fetch_optional(db).await?;
    Ok(row.map(|r| Contract { country: r.country, supplier: r.supplier, offer: r.offer, power_kva: r.power_kva, currency: r.currency }))
}

pub async fn set_contract(db: &Db, c: &Contract) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO electricity_contract (id, country, supplier, offer, power_kva, currency, updated_at) VALUES (1, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (id) DO UPDATE SET country = excluded.country, supplier = excluded.supplier, offer = excluded.offer, \
         power_kva = excluded.power_kva, currency = excluded.currency, updated_at = excluded.updated_at",
    )
    .bind(&c.country)
    .bind(&c.supplier)
    .bind(&c.offer)
    .bind(c.power_kva)
    .bind(&c.currency)
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
