//! The regulated prices of EDF (France), from the open data of the CRE (roadmap E32).
//!
//! The CRE publishes the history of the "Tarif Bleu" for homes, one CSV per option (Base, Heures
//! pleines / Heures creuses, Tempo): from each date on, for each subscribed power, the yearly
//! subscription and the price per kWh, taxes excluded and included. A contract on one of these
//! offers gets a tariff per date of the CRE, at the prices of its power, taxes included: the past
//! keeps its prices, and a new grid is added on its day. They are read again every week (the grids
//! change twice a year, in February and August), and whenever the contract is saved.

use crate::db;
use crate::price::{Calendar, ColorProfiles, Contract, DayProfile, OffPeak, PriceSlot, Tariff};
use crate::AppState;
use anyhow::{anyhow, bail};
use chrono::NaiveDate;
use std::time::Duration;

/// The offers read from the CRE.
pub const OFFERS: [&str; 3] = ["base", "hc", "tempo"];

pub fn is_regulated(offer: &str) -> bool {
    OFFERS.contains(&offer)
}

fn file(offer: &str) -> Option<&'static str> {
    match offer {
        "base" => Some("Option_Base.csv"),
        "hc" => Some("Option_HPHC.csv"),
        "tempo" => Some("Option_Tempo.csv"),
        _ => None,
    }
}

/// A line of a file: from a day on, the yearly subscription and the prices per kWh (TTC).
#[derive(Debug, Clone, PartialEq)]
struct Grid {
    from: NaiveDate,
    subscription_year: f64,
    /// Base: [price]; HPHC: [HC, HP]; Tempo: [HC, HP] blue, white, red.
    prices: Vec<f64>,
}

fn number(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse().ok()
}

/// The lines of `power` kVA, by date, from the text of a file of the CRE.
fn grids(text: &str, power: f64) -> anyhow::Result<Vec<Grid>> {
    let mut lines = text.lines();
    let head: Vec<String> = lines.next().ok_or_else(|| anyhow!("empty file"))?.trim_start_matches('\u{feff}').split(';').map(|h| h.trim().to_string()).collect();
    let col = |name: &str| head.iter().position(|h| h == name);
    let (Some(start), Some(kva), Some(fixed)) = (col("DATE_DEBUT"), col("P_SOUSCRITE"), col("PART_FIXE_TTC")) else {
        bail!("unexpected columns: {}", head.join(";"));
    };
    // Every price taxes included, in the order of the file.
    let prices: Vec<usize> = head.iter().enumerate().filter(|(_, h)| h.starts_with("PART_VARIABLE") && h.ends_with("_TTC")).map(|(i, _)| i).collect();
    if prices.is_empty() {
        bail!("no price in the file");
    }
    let mut out = vec![];
    for line in lines {
        let cells: Vec<&str> = line.split(';').collect();
        if cells.get(kva).and_then(|c| number(c)) != Some(power) {
            continue;
        }
        let Some(from) = cells.get(start).and_then(|d| NaiveDate::parse_from_str(d.trim(), "%d/%m/%Y").ok()) else { continue };
        // A power the offer did not have then has no price.
        let (Some(subscription_year), Some(values)) =
            (cells.get(fixed).and_then(|c| number(c)), prices.iter().map(|i| cells.get(*i).and_then(|c| number(c))).collect::<Option<Vec<f64>>>())
        else {
            continue;
        };
        out.push(Grid { from, subscription_year, prices: values });
    }
    out.sort_by_key(|g| g.from);
    out.dedup_by_key(|g| g.from);
    Ok(out)
}

fn minutes(text: &str) -> Option<u32> {
    let (h, m) = text.split_once(':')?;
    Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
}

fn clock(min: u32) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

/// The slots of a day with off-peak stretches: "HC" in them, "HP" elsewhere (by half hour).
pub fn hc_slots(offpeak: &[OffPeak], hc: f64, hp: f64) -> Vec<PriceSlot> {
    let default = [OffPeak { from: "22:00".into(), to: "06:00".into() }];
    let stretches = if offpeak.is_empty() { &default[..] } else { offpeak };
    let off = |m: u32| {
        stretches.iter().any(|o| match (minutes(&o.from), minutes(&o.to)) {
            (Some(f), Some(t)) if f < t => m >= f && m < t,
            (Some(f), Some(t)) => m >= f || m < t,
            _ => false,
        })
    };
    let mut slots: Vec<PriceSlot> = vec![];
    for step in 0..48 {
        let (name, price) = if off(step * 30) { ("HC", hc) } else { ("HP", hp) };
        if slots.last().is_none_or(|s| s.name != name) {
            slots.push(PriceSlot { start: clock(step * 30), name: name.into(), price });
        }
    }
    slots
}

fn profile(id: &str, name: &str, color: &str, slots: Vec<PriceSlot>) -> DayProfile {
    DayProfile { id: id.into(), name: name.into(), color: Some(color.into()), slots }
}

fn calendar(profile: &str, day_start: &str, colors: ColorProfiles) -> Calendar {
    Calendar { day_start: day_start.into(), weekdays: vec![profile.into(); 7], colors }
}

/// The tariffs of a regulated offer for `power` kVA, one per date of the CRE, from the text of
/// its file.
pub fn tariffs(offer: &str, text: &str, power: f64, offpeak: &[OffPeak], export_price: Option<f64>) -> anyhow::Result<Vec<(NaiveDate, Tariff)>> {
    let round = |v: f64| (v * 100.0).round() / 100.0;
    let tempo_night = [OffPeak { from: "22:00".into(), to: "06:00".into() }];
    let tariff = |g: &Grid| -> Option<Tariff> {
        let p = &g.prices;
        let (profiles, calendar) = match offer {
            "base" => (
                vec![profile("normal", "Tous les jours", "#14b8a6", vec![PriceSlot { start: "00:00".into(), name: "Base".into(), price: *p.first()? }])],
                calendar("normal", "00:00", ColorProfiles::default()),
            ),
            "hc" => (vec![profile("normal", "Tous les jours", "#14b8a6", hc_slots(offpeak, *p.first()?, *p.get(1)?))], calendar("normal", "00:00", ColorProfiles::default())),
            "tempo" => (
                vec![
                    profile("blue", "Jour bleu", "#3b82f6", hc_slots(&tempo_night, *p.first()?, *p.get(1)?)),
                    profile("white", "Jour blanc", "#e5e7eb", hc_slots(&tempo_night, *p.get(2)?, *p.get(3)?)),
                    profile("red", "Jour rouge", "#ef4444", hc_slots(&tempo_night, *p.get(4)?, *p.get(5)?)),
                ],
                calendar("blue", "06:00", ColorProfiles { blue: Some("blue".into()), white: Some("white".into()), red: Some("red".into()) }),
            ),
            _ => return None,
        };
        Some(Tariff { subscription: round(g.subscription_year / 12.0), export_price, profiles, calendar })
    };
    Ok(grids(text, power)?.iter().filter_map(|g| Some((g.from, tariff(g)?))).collect())
}

/// Reads the prices of the contract's offer from the CRE and replaces the tariffs with them.
/// Nothing for an offer of one's own. Returns how many tariffs there are.
pub async fn sync(state: &AppState) -> anyhow::Result<usize> {
    let Some(contract) = db::contract(&state.db).await? else { return Ok(0) };
    sync_contract(state, &contract).await
}

pub async fn sync_contract(state: &AppState, contract: &Contract) -> anyhow::Result<usize> {
    let Some(name) = file(&contract.offer) else { return Ok(0) };
    let power = contract.power_kva.ok_or_else(|| anyhow!("the subscribed power is needed for the prices of the CRE"))?;
    let base = state.cfg.cre_url.clone().ok_or_else(|| anyhow!("this server does not look things up on the internet (BREEZONE_WEATHER)"))?;
    let http = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent("Breezone").build()?;
    let text = http.get(format!("{}/{name}", base.trim_end_matches('/'))).send().await?.error_for_status()?.text().await?;
    // What the surplus is bought at is not the CRE's: kept from the tariffs there were.
    let export = db::tariffs(&state.db).await?.last().and_then(|(_, json)| serde_json::from_str::<Tariff>(json).ok()).and_then(|t| t.export_price);
    let list = tariffs(&contract.offer, &text, power, &contract.offpeak, export)?;
    if list.is_empty() {
        bail!("the CRE has no prices for {power} kVA in this offer");
    }
    let rows: Vec<(String, String)> = list.iter().map(|(from, t)| (from.format("%Y-%m-%d").to_string(), serde_json::to_string(t).unwrap_or_default())).collect();
    db::replace_tariffs(&state.db, &rows).await?;
    Ok(rows.len())
}

/// When the prices of the CRE were last read (a timestamp).
const SYNCED: &str = "cre.synced_at";
const WEEK: i64 = 7 * 86_400;

pub async fn run_loop(state: AppState) {
    if state.cfg.cre_url.is_none() {
        return;
    }
    loop {
        // A restart does not read them again before the week is over.
        let last = db::get_setting(&state.db, SYNCED).await.ok().flatten().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let wait = (last + WEEK - db::now()).max(0);
        if wait > 0 {
            tokio::time::sleep(Duration::from_secs(wait as u64)).await;
            continue;
        }
        match sync(&state).await {
            Ok(n) if n > 0 => tracing::info!("CRE: {n} tariffs"),
            Ok(_) => {}
            Err(e) => tracing::warn!("prices of the CRE not read: {e:#}"),
        }
        // Read or not (no regulated offer, or the CRE out of reach), tried again in a week; saving
        // the contract reads them at once.
        let _ = db::set_setting(&state.db, SYNCED, &db::now().to_string()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPO: &str = "\u{feff}DATE_DEBUT;DATE_FIN;P_SOUSCRITE;PART_FIXE_HT;PART_FIXE_TTC;PART_VARIABLE_HCBleu_HT;PART_VARIABLE_HCBleu_TTC;PART_VARIABLE_HPBleu_HT;PART_VARIABLE_HPBleu_TTC;PART_VARIABLE_HCBlanc_HT;PART_VARIABLE_HCBlanc_TTC;PART_VARIABLE_HPBlanc_HT;PART_VARIABLE_HPBlanc_TTC;PART_VARIABLE_HCRouge_HT;PART_VARIABLE_HCRouge_TTC;PART_VARIABLE_HPRouge_HT;PART_VARIABLE_HPRouge_TTC\r\n\
01/08/2016;31/07/2017;6;;;;;;;;;;;;;;\r\n\
01/02/2026;31/07/2026;9;174,36;233,44;0,0796;0,1325;0,1035;0,1612;0,0941;0,1499;0,1251;0,1871;0,1004;0,1575;0,5575;0,706\r\n\
01/08/2026;;9;177;237,3436191;0,0824;0,1356;0,1072;0,1654;0,0974;0,1536;0,1295;0,1921;0,1040;0,1615;0,5773;0,7295\r\n\
01/08/2026;;12;220;300;0,0824;0,1356;0,1072;0,1654;0,0974;0,1536;0,1295;0,1921;0,1040;0,1615;0,5773;0,7295\r\n";

    const HPHC: &str = "DATE_DEBUT;DATE_FIN;P_SOUSCRITE;PART_FIXE_HT;PART_FIXE_TTC;PART_VARIABLE_HC_HT;PART_VARIABLE_HC_TTC;PART_VARIABLE_HP_HT;PART_VARIABLE_HP_TTC\r\n\
01/08/2026;;9;300;411,84;0,1018;0,1589;0,1479;0,2142\r\n";

    #[test]
    fn the_tempo_grid_of_a_power_by_date() {
        let list = tariffs("tempo", TEMPO, 9.0, &[], Some(0.04)).unwrap();
        assert_eq!(list.iter().map(|(d, _)| d.to_string()).collect::<Vec<_>>(), vec!["2026-02-01", "2026-08-01"]);
        let t = &list[1].1;
        assert_eq!(crate::price::validate(t), Ok(()));
        assert_eq!(t.subscription, 19.78, "237.34 a year");
        assert_eq!(t.export_price, Some(0.04));
        assert_eq!(t.calendar.day_start, "06:00");
        let red = t.profiles.iter().find(|p| p.id == "red").unwrap();
        assert_eq!(
            red.slots.iter().map(|s| (s.start.as_str(), s.name.as_str(), s.price)).collect::<Vec<_>>(),
            vec![("00:00", "HC", 0.1615), ("06:00", "HP", 0.7295), ("22:00", "HC", 0.1615)]
        );
        assert!(tariffs("tempo", TEMPO, 6.0, &[], None).unwrap().is_empty(), "6 kVA had no price then");
    }

    #[test]
    fn off_peak_hours_cut_the_day() {
        let list = tariffs("hc", HPHC, 9.0, &[OffPeak { from: "12:30".into(), to: "14:30".into() }, OffPeak { from: "23:30".into(), to: "05:30".into() }], None).unwrap();
        let slots: Vec<(&str, &str)> = list[0].1.profiles[0].slots.iter().map(|s| (s.start.as_str(), s.name.as_str())).collect();
        assert_eq!(slots, vec![("00:00", "HC"), ("05:30", "HP"), ("12:30", "HC"), ("14:30", "HP"), ("23:30", "HC")]);
        assert_eq!(crate::price::validate(&list[0].1), Ok(()));
    }
}
