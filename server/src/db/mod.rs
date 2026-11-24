//! SQLite persistence (sqlx). Migrations are embedded in the binary.
//!
//! The reads and writes of each table live here, one file per domain, so that a change of schema
//! lands in one place. The computations over the history (hourly and daily rollups, the energy of
//! the home, the configuration backup) keep their aggregated queries next to the computation.

use crate::i18n::Text;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{FromRow, SqlitePool};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

mod access;
mod accounts;
mod automations;
mod charging;
mod cloud;
mod dashboards;
mod devices;
mod events;
mod federation;
mod history;
mod installations;
mod modules;
mod routines;
mod samples;
mod schedules;
mod settings;
mod tariffs;
mod timers;
mod tokens;
mod users;

pub use access::*;
pub use accounts::*;
pub use automations::*;
pub use charging::*;
pub use cloud::*;
pub use dashboards::*;
pub use devices::*;
pub use events::*;
pub use federation::*;
pub use history::*;
pub use installations::*;
pub use modules::*;
pub use routines::*;
pub use samples::*;
pub use schedules::*;
pub use settings::*;
pub use tariffs::*;
pub use timers::*;
pub use tokens::*;
pub use users::*;

pub type Db = SqlitePool;

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub async fn connect(path: &Path) -> anyhow::Result<Db> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // WAL with NORMAL: a commit no longer waits for the disk (slow on a NAS), and a power cut can
    // only lose the last commits, never corrupt the base.
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new().max_connections(4).connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
