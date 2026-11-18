//! The colour of the Tempo days (EDF), for the tariffs whose calendar follows it (roadmap E32).
//!
//! Read from the public relay api-couleur-tempo.fr (RTE's data, no account): today and tomorrow
//! every half hour (tomorrow's is announced in the morning), and the days of the season once, so
//! that the costs of the past days are right. Nothing is asked while no tariff follows colours.

use crate::db;
use crate::price::{DayColor, Tariff};
use crate::AppState;
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

/// Red and white days of a season (1 September to 31 August).
pub const RED_DAYS: i64 = 22;
pub const WHITE_DAYS: i64 = 43;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Day {
    date_jour: String,
    /// 1 blue, 2 white, 3 red (0: not announced yet).
    code_jour: i64,
}

pub fn color_of(code: i64) -> Option<DayColor> {
    match code {
        1 => Some(DayColor::Blue),
        2 => Some(DayColor::White),
        3 => Some(DayColor::Red),
        _ => None,
    }
}

pub fn name(c: DayColor) -> &'static str {
    match c {
        DayColor::Blue => "blue",
        DayColor::White => "white",
        DayColor::Red => "red",
    }
}

fn parse(text: &str) -> Option<DayColor> {
    match text {
        "blue" => Some(DayColor::Blue),
        "white" => Some(DayColor::White),
        "red" => Some(DayColor::Red),
        _ => None,
    }
}

/// The first day of the season of `day` (1 September).
pub fn season_start(day: NaiveDate) -> NaiveDate {
    let year = if day.month() >= 9 { day.year() } else { day.year() - 1 };
    NaiveDate::from_ymd_opt(year, 9, 1).unwrap_or(day)
}

/// Whether a tariff of the home follows the colour of the days.
pub fn followed(tariffs: &[Tariff]) -> bool {
    tariffs.iter().any(|t| [&t.calendar.colors.blue, &t.calendar.colors.white, &t.calendar.colors.red].into_iter().any(Option::is_some))
}

/// The colours known from `from` to `until` (both included).
pub async fn colors(db: &db::Db, from: NaiveDate, until: NaiveDate) -> sqlx::Result<HashMap<NaiveDate, DayColor>> {
    let f = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    Ok(db::day_colors(db, &f(from), &f(until)).await?.into_iter().filter_map(|(day, c)| Some((NaiveDate::parse_from_str(&day, "%Y-%m-%d").ok()?, parse(&c)?))).collect())
}

async fn store(db: &db::Db, days: &[Day]) -> sqlx::Result<usize> {
    let mut n = 0;
    for d in days {
        if let Some(c) = color_of(d.code_jour) {
            db::set_day_color(db, &d.date_jour, name(c)).await?;
            n += 1;
        }
    }
    Ok(n)
}

/// Reads today's and tomorrow's colours, and the season's when it is missing.
pub async fn refresh(state: &AppState) -> anyhow::Result<()> {
    let tariffs: Vec<Tariff> = db::all_tariffs(&state.db).await?.into_iter().filter_map(|json| serde_json::from_str(&json).ok()).collect();
    if !followed(&tariffs) {
        return Ok(());
    }
    // RTE itself when an account is linked (the season once, then today and tomorrow); the public
    // relay otherwise, or when RTE fails.
    let today = crate::rollup::local_day(db::now(), state.cfg.timezone);
    let start = season_start(today);
    let season = today > start && colors(&state.db, start, start).await?.is_empty();
    match crate::accounts::rte::refresh(state, season).await {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(e) => tracing::warn!("Tempo colours not read from RTE: {}", e.message.en),
    }
    let Some(base) = state.cfg.tempo_url.clone() else { return Ok(()) };
    let http = reqwest::Client::builder().timeout(Duration::from_secs(10)).user_agent("Breezone").build()?;
    let get = |path: String| {
        let http = http.clone();
        let url = format!("{}{path}", base.trim_end_matches('/'));
        async move { http.get(url).send().await?.error_for_status()?.text().await }
    };
    let mut days = vec![];
    for path in ["/api/jourTempo/today", "/api/jourTempo/tomorrow"] {
        days.push(serde_json::from_str::<Day>(&get(path.into()).await?)?);
    }
    store(&state.db, &days).await?;
    let today = crate::rollup::local_day(db::now(), state.cfg.timezone);
    let start = season_start(today);
    if today > start && colors(&state.db, start, start).await?.is_empty() {
        let season = format!("{}-{}", start.year(), start.year() + 1);
        let list: Vec<Day> = serde_json::from_str(&get(format!("/api/joursTempo?periode={season}")).await?)?;
        let n = store(&state.db, &list).await?;
        tracing::info!("Tempo: {n} days of the {season} season read");
    }
    Ok(())
}

pub async fn run_loop(state: AppState) {
    loop {
        if let Err(e) = refresh(&state).await {
            tracing::warn!("Tempo colours not read: {e:#}");
        }
        tokio::time::sleep(Duration::from_secs(30 * 60)).await;
    }
}
