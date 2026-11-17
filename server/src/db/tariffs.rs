//! The electricity contract of each installation and its prices (see `price`).

use super::*;
use crate::price::Contract;

#[derive(FromRow)]
struct ContractRow {
    installation_id: i64,
    country: Option<String>,
    supplier: Option<String>,
    offer: String,
    power_kva: Option<f64>,
    currency: String,
    offpeak: Option<String>,
}

impl ContractRow {
    fn contract(self) -> Contract {
        Contract {
            country: self.country,
            supplier: self.supplier,
            offer: self.offer,
            power_kva: self.power_kva,
            currency: self.currency,
            offpeak: self.offpeak.and_then(|o| serde_json::from_str(&o).ok()).unwrap_or_default(),
        }
    }
}

macro_rules! contract_query {
    ($rest:literal) => {
        concat!("SELECT installation_id, country, supplier, offer, power_kva, currency, offpeak FROM electricity_contracts", $rest)
    };
}

pub async fn contract(db: &Db, installation_id: i64) -> sqlx::Result<Option<Contract>> {
    let row: Option<ContractRow> = sqlx::query_as(contract_query!(" WHERE installation_id = ?")).bind(installation_id).fetch_optional(db).await?;
    Ok(row.map(ContractRow::contract))
}

/// Every contract, by installation.
pub async fn contracts(db: &Db) -> sqlx::Result<Vec<(i64, Contract)>> {
    let rows: Vec<ContractRow> = sqlx::query_as(contract_query!("")).fetch_all(db).await?;
    Ok(rows.into_iter().map(|r| (r.installation_id, r.contract())).collect())
}

pub async fn set_contract(db: &Db, installation_id: i64, c: &Contract) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO electricity_contracts (installation_id, country, supplier, offer, power_kva, currency, offpeak, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (installation_id) DO UPDATE SET country = excluded.country, supplier = excluded.supplier, offer = excluded.offer, \
         power_kva = excluded.power_kva, currency = excluded.currency, offpeak = excluded.offpeak, updated_at = excluded.updated_at",
    )
    .bind(installation_id)
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

/// The prices of an installation: (`valid_from`, JSON), oldest first.
pub async fn tariffs(db: &Db, installation_id: i64) -> sqlx::Result<Vec<(String, String)>> {
    sqlx::query_as("SELECT valid_from, tariff FROM electricity_prices WHERE installation_id = ? ORDER BY valid_from").bind(installation_id).fetch_all(db).await
}

/// The prices of every installation (JSON).
pub async fn all_tariffs(db: &Db) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar("SELECT tariff FROM electricity_prices").fetch_all(db).await
}

pub async fn set_tariff(db: &Db, installation_id: i64, valid_from: &str, tariff: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO electricity_prices (installation_id, valid_from, tariff, updated_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT (installation_id, valid_from) DO UPDATE SET tariff = excluded.tariff, updated_at = excluded.updated_at",
    )
    .bind(installation_id)
    .bind(valid_from)
    .bind(tariff)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

/// `true` when there was one.
pub async fn delete_tariff(db: &Db, installation_id: i64, valid_from: &str) -> sqlx::Result<bool> {
    Ok(sqlx::query("DELETE FROM electricity_prices WHERE installation_id = ? AND valid_from = ?").bind(installation_id).bind(valid_from).execute(db).await?.rows_affected() > 0)
}

/// The prices of an installation replaced by these (`valid_from`, JSON), at once: those of an
/// offer read from the CRE.
pub async fn replace_tariffs(db: &Db, installation_id: i64, tariffs: &[(String, String)]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM electricity_prices WHERE installation_id = ?").bind(installation_id).execute(&mut *tx).await?;
    for (from, json) in tariffs {
        sqlx::query("INSERT INTO electricity_prices (installation_id, valid_from, tariff, updated_at) VALUES (?, ?, ?, ?)")
            .bind(installation_id)
            .bind(from)
            .bind(json)
            .bind(now())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// The first installation of this server: the home, whose prices the energy of its modules is
/// counted at when no installation is given.
pub async fn home_installation(db: &Db) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar("SELECT MIN(id) FROM installations WHERE node_id IS NULL").fetch_one(db).await
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
