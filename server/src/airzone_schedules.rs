//! The schedules ("programmations") of the Airzone app, kept in sync with Airzone Cloud.
//!
//! Airzone stays the master of its schedules and runs them. The local API of the WebServer has
//! no schedules (checked on firmware 4.22 / API 1.80); the official app reads and writes them on
//! Airzone Cloud (or by Bluetooth), so Breezone does the same, with the account remembered in the
//! settings:
//!
//! * read: `GET installations/{id}/schedules` (control API, account API as a fallback), every
//!   hour, on demand, and after an installation is linked;
//! * create: `POST installations/{id}/schedules` `{schedule, opts}`;
//! * edit: `PATCH installations/{id}/schedule/{schedule id}` `{schedule, opts}`;
//! * delete: `DELETE installations/{id}/schedule/{schedule id}`.
//!
//! A weekly schedule (format of the official app):
//!
//! ```json
//! { "_id": "…", "name": "Matin", "type": "week", "prog_enabled": true,
//!   "device_ids": ["<zone or system device id>", …],
//!   "start_conf": { "days": [0, 1, 2, 3, 4], "hour": 7, "minutes": 0,
//!                   "power": true, "mode": 3, "setpoint": 21 } }
//! ```
//!
//! Days go from 0 = Monday to 6 = Sunday; Breezone shows ISO days (1 = Monday … 7 = Sunday).
//! Breezone never runs these schedules itself (Airzone does): nothing runs twice.

use crate::airzone::OperationMode;
use crate::cloud::{self, CloudInstallation, CloudSession};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::hub::ZoneCommand;
use crate::i18n::Text;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

const KEY_LAST_SYNC: &str = "schedules.last_sync";
const KEY_LAST_ERROR: &str = "schedules.last_error";
const SYNC_PERIOD: Duration = Duration::from_secs(3600);
/// Longest schedule name Airzone keeps.
const NAME_MAX: usize = 11;

/// One synchronisation at a time.
static SYNC_LOCK: Mutex<()> = Mutex::const_new(());

// ---------------------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------------------

/// Zones of a cloud installation, by Airzone device id: a zone device gives one zone, a system
/// device all the zones of the system.
pub fn device_map(c: &CloudInstallation) -> HashMap<String, Vec<(i64, i64)>> {
    let mut map: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
    for z in &c.zones {
        if let Some(id) = &z.device_id {
            map.entry(id.clone()).or_default().push((z.system_id, z.zone_id));
        }
    }
    for s in &c.systems {
        let zones: Vec<(i64, i64)> = c.zones.iter().filter(|z| z.system_id == s.system_id).map(|z| (z.system_id, z.zone_id)).collect();
        map.entry(s.device_id.clone()).or_default().extend(zones);
    }
    map
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        // {"celsius": 21, "fahrenheit": 70} or {"value": 21}
        Value::Object(o) => o.get("celsius").or_else(|| o.get("value")).and_then(num),
        _ => None,
    }
}

fn power(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|n| n != 0),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => Some(true),
            "off" | "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn mode(v: &Value) -> Option<OperationMode> {
    match v {
        Value::Number(n) => n.as_i64().and_then(OperationMode::from_code),
        Value::String(s) => s.parse::<i64>().ok().and_then(OperationMode::from_code).or_else(|| serde_json::from_value(Value::String(s.clone())).ok()),
        _ => None,
    }
}

/// The action of a `start_conf` / `end_conf`.
fn command(conf: &Value) -> ZoneCommand {
    let speed = conf.get("speed").or_else(|| conf.get("pspeed")).and_then(Value::as_i64).filter(|s| (0..=10).contains(s));
    let mut cmd = ZoneCommand {
        on: conf.get("power").and_then(power),
        mode: conf.get("mode").and_then(mode),
        setpoint: conf.get("setpoint").and_then(num),
        speed,
        ..Default::default()
    };
    // Mode "stop" is how Airzone switches a system off.
    if cmd.mode == Some(OperationMode::Stop) && cmd.on.is_none() {
        cmd.on = Some(false);
        cmd.mode = None;
    }
    if cmd.on == Some(false) {
        cmd.setpoint = None;
        cmd.speed = None;
    }
    cmd
}

fn time_of(conf: &Value) -> Option<(u32, u32)> {
    let h = conf.get("hour").and_then(Value::as_u64)?;
    let m = conf.get("minutes").and_then(Value::as_u64).unwrap_or(0);
    (h < 24 && m < 60).then_some((h as u32, m as u32))
}

/// Airzone days (0 = Monday … 6 = Sunday) to ISO days (1 = Monday … 7 = Sunday).
fn iso_days(conf: &Value) -> Vec<u8> {
    let mut days: Vec<u8> = conf
        .get("days")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).filter(|d| *d <= 6).map(|d| d as u8 + 1).collect())
        .unwrap_or_default();
    days.sort_unstable();
    days.dedup();
    days
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZoneRef {
    pub system_id: i64,
    pub zone_id: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleEnd {
    pub time: String,
    pub action: ZoneCommand,
}

/// An Airzone schedule as Breezone shows it.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleView {
    /// Airzone Cloud installation.
    pub cloud_id: String,
    pub installation: String,
    /// Breezone installation linked to it, if any.
    pub installation_id: Option<i64>,
    /// Airzone schedule id.
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// "week" | "calendar" | "acs" | "vmc" | …
    pub kind: String,
    /// ISO weekdays.
    pub days: Vec<u8>,
    pub time: Option<String>,
    pub zones: Vec<ZoneRef>,
    pub action: ZoneCommand,
    pub end: Option<ScheduleEnd>,
    /// Breezone can edit it (weekly schedule of zones).
    pub editable: bool,
    /// Why it cannot be edited here.
    pub note: Option<Text>,
}

fn view(cloud_id: &str, installation: &str, installation_id: Option<i64>, raw: &Value, zones: Vec<ZoneRef>) -> ScheduleView {
    let start = raw.get("start_conf").cloned().unwrap_or(Value::Null);
    let kind = raw.get("type").and_then(Value::as_str).unwrap_or("week").to_string();
    let end = raw
        .get("end_conf")
        .filter(|e| e.is_object())
        .and_then(|e| time_of(e).map(|(h, m)| ScheduleEnd { time: format!("{h:02}:{m:02}"), action: command(e) }));
    let note = match kind.as_str() {
        "week" | "zone" | "system" => None,
        "calendar" => Some(crate::text!(
            "programmation par dates : modifiable dans l'application Airzone",
            "schedule by dates: editable in the Airzone app",
            "programación por fechas: modificable en la aplicación Airzone"
        )),
        "acs" => Some(crate::text!(
            "eau chaude sanitaire : modifiable dans l'application Airzone",
            "domestic hot water: editable in the Airzone app",
            "agua caliente sanitaria: modificable en la aplicación Airzone"
        )),
        "vmc" | "relay" => Some(crate::text!(
            "ventilation ou relais : modifiable dans l'application Airzone",
            "ventilation or relay: editable in the Airzone app",
            "ventilación o relé: modificable en la aplicación Airzone"
        )),
        other => Some(crate::text!(
            "type « {0} » : modifiable dans l'application Airzone",
            "“{0}” type: editable in the Airzone app",
            "tipo «{0}»: modificable en la aplicación Airzone",
            other
        )),
    };
    ScheduleView {
        cloud_id: cloud_id.to_string(),
        installation: installation.to_string(),
        installation_id,
        id: raw.get("_id").or_else(|| raw.get("id")).and_then(Value::as_str).unwrap_or_default().to_string(),
        name: raw.get("name").and_then(Value::as_str).unwrap_or("Programmation").to_string(),
        enabled: raw.get("prog_enabled").and_then(power).unwrap_or(true),
        days: iso_days(&start),
        time: time_of(&start).map(|(h, m)| format!("{h:02}:{m:02}")),
        action: command(&start),
        editable: note.is_none() && time_of(&start).is_some(),
        note,
        kind,
        zones,
        end,
    }
}

/// What the user edits.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleInput {
    /// Airzone Cloud installation (creation only).
    #[serde(default)]
    pub cloud_id: Option<String>,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// ISO weekdays.
    pub days: Vec<u8>,
    /// "HH:MM"
    pub time: String,
    pub zones: Vec<ZoneRef>,
    /// `on: true`, `on: false`, or a `mode` (with an optional `setpoint`).
    pub action: ZoneCommand,
}

fn yes() -> bool {
    true
}

/// Builds the Airzone schedule from the input, keeping the fields Breezone does not know.
fn to_airzone(input: &ScheduleInput, existing: Option<&Value>, devices: &HashMap<String, Vec<(i64, i64)>>) -> ApiResult<(Value, Option<Value>)> {
    let name = input.name.trim();
    // Airzone keeps 11 characters at most.
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(ApiError::bad_request(crate::text!(
            "le nom doit faire de 1 à {0} caractères (limite Airzone)",
            "the name must be 1 to {0} characters long (Airzone limit)",
            "el nombre debe tener de 1 a {0} caracteres (límite de Airzone)",
            NAME_MAX
        )));
    }
    if input.days.is_empty() || input.days.iter().any(|d| !(1..=7).contains(d)) {
        return Err(ApiError::bad_request("choisissez au moins un jour"));
    }
    let (h, m) = input
        .time
        .split_once(':')
        .and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)))
        .filter(|(h, m)| *h < 24 && *m < 60)
        .ok_or_else(|| ApiError::bad_request("heure invalide (HH:MM)"))?;
    if input.zones.is_empty() {
        return Err(ApiError::bad_request("choisissez au moins une zone"));
    }
    // Zone device ids (one per zone, as the official app does).
    let mut device_ids: Vec<String> = vec![];
    for z in &input.zones {
        let id = devices
            .iter()
            .filter(|(_, zones)| zones.len() == 1 && zones[0] == (z.system_id, z.zone_id))
            .map(|(id, _)| id.clone())
            .next()
            .ok_or_else(|| ApiError::bad_request(crate::text!(
                    "zone {0}/{1} inconnue d'Airzone : actualisez les programmations",
                    "zone {0}/{1} unknown to Airzone: refresh the schedules",
                    "zona {0}/{1} desconocida para Airzone: actualice las programaciones",
                    z.system_id,
                    z.zone_id
                )))?;
        if !device_ids.contains(&id) {
            device_ids.push(id);
        }
    }

    let a = &input.action;
    let mut start: Map<String, Value> = existing
        .and_then(|e| e.get("start_conf"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for k in ["power", "mode", "setpoint", "speed", "pspeed"] {
        start.remove(k);
    }
    let mut days: Vec<u8> = input.days.iter().map(|d| d - 1).collect();
    days.sort_unstable();
    days.dedup();
    start.insert("days".into(), json!(days));
    start.insert("hour".into(), json!(h));
    start.insert("minutes".into(), json!(m));
    let mut with_temp = false;
    match (a.on, a.mode) {
        (Some(false), _) => {
            start.insert("power".into(), json!(false));
        }
        (_, Some(mode)) => {
            start.insert("mode".into(), json!(mode.code()));
            if let Some(sp) = a.setpoint {
                start.insert("setpoint".into(), json!(sp));
                with_temp = true;
            }
        }
        (Some(true), None) => {
            start.insert("power".into(), json!(true));
            if let Some(sp) = a.setpoint {
                start.insert("setpoint".into(), json!(sp));
                with_temp = true;
            }
        }
        (None, None) => return Err(ApiError::bad_request("choisissez une action : allumer, éteindre ou un mode")),
    }

    let mut sched: Map<String, Value> = existing.and_then(Value::as_object).cloned().unwrap_or_default();
    for k in ["_id", "id"] {
        sched.remove(k);
    }
    sched.insert("name".into(), json!(name));
    sched.insert("prog_enabled".into(), json!(input.enabled));
    sched.entry("type").or_insert(json!("week"));
    sched.insert("device_ids".into(), json!(device_ids));
    sched.insert("start_conf".into(), Value::Object(start));
    // Units of the setpoint, as sent by the official app (0 = Celsius).
    let opts = with_temp.then(|| json!({ "units": 0 }));
    Ok((Value::Object(sched), opts))
}

// ---------------------------------------------------------------------------------------
// Synchronisation
// ---------------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SchedulesState {
    pub last_sync: Option<i64>,
    pub last_error: Option<Text>,
    /// An Airzone account is remembered (needed to read and write the schedules).
    pub account: bool,
    pub schedules: Vec<ScheduleView>,
}

pub async fn list(state: &AppState) -> ApiResult<SchedulesState> {
    let links: HashMap<String, i64> = db::list_installations(&state.db).await?.into_iter().filter_map(|i| i.cloud_id.map(|c| (c, i.id))).collect();
    let names: HashMap<String, String> = db::list_cloud_installations(&state.db).await?.into_iter().map(|r| (r.cloud_id, r.name)).collect();
    let mut schedules: Vec<ScheduleView> = db::list_airzone_schedules(&state.db)
        .await?
        .into_iter()
        .filter_map(|r| {
            let raw: Value = serde_json::from_str(&r.data).ok()?;
            let zones: Vec<ZoneRef> = serde_json::from_str(&r.zones).unwrap_or_default();
            let installation = names.get(&r.cloud_id).cloned().unwrap_or_else(|| r.cloud_id.clone());
            Some(view(&r.cloud_id, &installation, links.get(&r.cloud_id).copied(), &raw, zones))
        })
        .collect();
    schedules.sort_by(|a, b| (&a.installation, &a.time, &a.name).cmp(&(&b.installation, &b.time, &b.name)));
    Ok(SchedulesState {
        last_sync: db::get_setting(&state.db, KEY_LAST_SYNC).await?.and_then(|v| v.parse().ok()),
        last_error: db::get_setting(&state.db, KEY_LAST_ERROR).await?.as_deref().map(Text::load),
        account: cloud::load_credentials(state).await.is_some(),
        schedules,
    })
}

async fn session(state: &AppState) -> ApiResult<CloudSession> {
    let creds = cloud::load_credentials(state)
        .await
        .ok_or_else(|| ApiError::bad_request("connectez le compte Airzone (Réglages › Compte Airzone) en mémorisant le mot de passe"))?;
    CloudSession::login(&state.cfg.airzone_cloud_url, &state.cfg.cloud_ctrl_url(), &creds.email, &creds.password).await
}

fn schedules_of(v: &Option<Value>) -> Option<Vec<Value>> {
    let v = v.as_ref()?;
    if let Some(a) = v.as_array() {
        return Some(a.clone());
    }
    for k in ["data", "schedules"] {
        if let Some(a) = v.get(k).and_then(Value::as_array) {
            return Some(a.clone());
        }
        if let Some(a) = v.get("data").and_then(|d| d.get(k)).and_then(Value::as_array) {
            return Some(a.clone());
        }
    }
    Some(vec![])
}

/// Reads all the schedules from Airzone Cloud and stores them.
pub async fn sync(state: &AppState) -> ApiResult<SchedulesState> {
    let _guard = SYNC_LOCK.lock().await;
    let result = async {
        let session = session(state).await?;
        let account = cloud::fetch_account(&session, true).await;
        session.logout().await;
        let account = account?;
        let mut errors: Vec<Text> = vec![];
        for c in &account {
            // Keep the zone device ids, needed to write schedules.
            if let Ok(detail) = serde_json::to_string(c) {
                db::set_cloud_detail(&state.db, &c.cloud_id, &detail).await?;
            }
            if let Some(r) = &c.routines {
                db::set_airzone_routines(&state.db, &c.cloud_id, &r.to_string()).await?;
            }
            let Some(list) = schedules_of(&c.schedules) else {
                errors.push(crate::text!("{0} : programmations illisibles", "{0}: unreadable schedules", "{0}: programaciones ilegibles", c.name));
                continue;
            };
            let devices = device_map(c);
            let rows: Vec<(String, String, String)> = list
                .iter()
                .filter_map(|s| {
                    let id = s.get("_id").or_else(|| s.get("id")).and_then(Value::as_str)?.to_string();
                    let mut zones: Vec<ZoneRef> = s
                        .get("device_ids")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|d| d.as_str().and_then(|d| devices.get(d)))
                        .flatten()
                        .map(|(system_id, zone_id)| ZoneRef { system_id: *system_id, zone_id: *zone_id })
                        .collect();
                    zones.sort_by_key(|z| (z.system_id, z.zone_id));
                    zones.dedup();
                    Some((id, s.to_string(), serde_json::to_string(&zones).unwrap_or_default()))
                })
                .collect();
            db::replace_airzone_schedules(&state.db, &c.cloud_id, &rows).await?;
        }
        let known: Vec<String> = account.iter().map(|c| c.cloud_id.clone()).collect();
        db::retain_airzone_schedules(&state.db, &known).await?;
        Ok::<_, ApiError>(errors)
    }
    .await;
    match &result {
        Ok(errors) if errors.is_empty() => {
            db::set_setting(&state.db, KEY_LAST_SYNC, &db::now().to_string()).await?;
            db::delete_setting(&state.db, KEY_LAST_ERROR).await?;
        }
        Ok(errors) => {
            db::set_setting(&state.db, KEY_LAST_SYNC, &db::now().to_string()).await?;
            db::set_setting(&state.db, KEY_LAST_ERROR, &crate::hub::join(errors, " ; ").store()).await?;
        }
        Err(e) => db::set_setting(&state.db, KEY_LAST_ERROR, &e.message.store()).await?,
    }
    result?;
    state.hub.routines_changed();
    list(state).await
}

/// Synchronises in the background (after an installation is linked).
pub fn sync_soon(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        if cloud::load_credentials(&state).await.is_none() {
            return;
        }
        if let Err(e) = sync(&state).await {
            tracing::warn!("Airzone schedules sync failed: {}", e.message.en);
        }
    });
}

/// Hourly synchronisation when an Airzone account is remembered.
pub async fn run_sync(state: AppState) {
    tokio::time::sleep(Duration::from_secs(45)).await;
    let mut ticker = tokio::time::interval(SYNC_PERIOD);
    loop {
        ticker.tick().await;
        if cloud::load_credentials(&state).await.is_some() {
            if let Err(e) = sync(&state).await {
                tracing::warn!("Airzone schedules sync failed: {}", e.message.en);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Writes (Airzone Cloud)
// ---------------------------------------------------------------------------------------

async fn devices_of(state: &AppState, cloud_id: &str) -> ApiResult<HashMap<String, Vec<(i64, i64)>>> {
    let row = db::list_cloud_installations(&state.db)
        .await?
        .into_iter()
        .find(|r| r.cloud_id == cloud_id)
        .ok_or_else(|| ApiError::not_found("installation Airzone inconnue"))?;
    let c: CloudInstallation = row
        .detail
        .and_then(|d| serde_json::from_str(&d).ok())
        .ok_or_else(|| ApiError::bad_request("actualisez d'abord les programmations"))?;
    Ok(device_map(&c))
}

async fn write(state: &AppState, actor: &str, what: Text, op: impl AsyncFnOnce(&CloudSession) -> ApiResult<Value>) -> ApiResult<SchedulesState> {
    let session = session(state).await?;
    let result = op(&session).await;
    session.logout().await;
    match result {
        Ok(_) => {
            state.hub.log("routine", "info", None, Some(actor), crate::text!("Airzone : {0}", "Airzone: {0}", "Airzone: {0}", what.arg()), None).await;
            sync(state).await
        }
        Err(e) => {
            let text = crate::text!(
                "Airzone : échec ({0}) : {1}",
                "Airzone: failed ({0}): {1}",
                "Airzone: fallo ({0}): {1}",
                what.arg(),
                e.message.arg()
            );
            state.hub.log("routine", "error", None, Some(actor), text, None).await;
            Err(e)
        }
    }
}

pub async fn create(state: &AppState, input: &ScheduleInput, actor: &str) -> ApiResult<SchedulesState> {
    let cloud_id = input.cloud_id.clone().ok_or_else(|| ApiError::bad_request("installation Airzone manquante"))?;
    let devices = devices_of(state, &cloud_id).await?;
    let (schedule, opts) = to_airzone(input, None, &devices)?;
    let mut body = json!({ "schedule": schedule });
    if let Some(o) = opts {
        body["opts"] = o;
    }
    let path = format!("installations/{cloud_id}/schedules");
    let name = input.name.trim();
    let what = crate::text!("programmation « {0} » créée", "schedule “{0}” created", "programación «{0}» creada", name);
    write(state, actor, what, async |s: &CloudSession| {
        s.ctrl(reqwest::Method::POST, &path, Some(&body)).await
    })
    .await
}

async fn existing(state: &AppState, cloud_id: &str, id: &str) -> ApiResult<Value> {
    let row = db::list_airzone_schedules(&state.db)
        .await?
        .into_iter()
        .find(|r| r.cloud_id == cloud_id && r.sched_id == id)
        .ok_or_else(|| ApiError::not_found("programmation introuvable : actualisez"))?;
    serde_json::from_str(&row.data).map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn update(state: &AppState, cloud_id: &str, id: &str, input: &ScheduleInput, actor: &str) -> ApiResult<SchedulesState> {
    let current = existing(state, cloud_id, id).await?;
    let devices = devices_of(state, cloud_id).await?;
    let (schedule, opts) = to_airzone(input, Some(&current), &devices)?;
    let mut body = json!({ "schedule": schedule });
    if let Some(o) = opts {
        body["opts"] = o;
    }
    let path = format!("installations/{cloud_id}/schedule/{id}");
    let name = input.name.trim();
    let what = crate::text!("programmation « {0} » modifiée", "schedule “{0}” changed", "programación «{0}» modificada", name);
    write(state, actor, what, async |s: &CloudSession| {
        s.ctrl(reqwest::Method::PATCH, &path, Some(&body)).await
    })
    .await
}

/// Enables or disables a schedule (any type), keeping everything else.
pub async fn set_enabled(state: &AppState, cloud_id: &str, id: &str, enabled: bool, actor: &str) -> ApiResult<SchedulesState> {
    let mut current = existing(state, cloud_id, id).await?;
    let name = current.get("name").and_then(Value::as_str).unwrap_or("Programmation").to_string();
    if let Some(o) = current.as_object_mut() {
        o.remove("_id");
        o.remove("id");
        o.insert("prog_enabled".into(), json!(enabled));
    }
    let body = json!({ "schedule": current });
    let path = format!("installations/{cloud_id}/schedule/{id}");
    let what = if enabled {
        crate::text!("programmation « {0} » activée", "schedule “{0}” enabled", "programación «{0}» activada", name)
    } else {
        crate::text!("programmation « {0} » désactivée", "schedule “{0}” disabled", "programación «{0}» desactivada", name)
    };
    write(state, actor, what, async |s: &CloudSession| s.ctrl(reqwest::Method::PATCH, &path, Some(&body)).await).await
}

pub async fn delete(state: &AppState, cloud_id: &str, id: &str, actor: &str) -> ApiResult<SchedulesState> {
    let current = existing(state, cloud_id, id).await?;
    let name = current.get("name").and_then(Value::as_str).unwrap_or("Programmation").to_string();
    let path = format!("installations/{cloud_id}/schedule/{id}");
    let what = crate::text!("programmation « {0} » supprimée", "schedule “{0}” deleted", "programación «{0}» eliminada", name);
    write(state, actor, what, async |s: &CloudSession| {
        s.ctrl(reqwest::Method::DELETE, &path, None).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> HashMap<String, Vec<(i64, i64)>> {
        HashMap::from([
            ("z1".to_string(), vec![(1, 1)]),
            ("z2".to_string(), vec![(1, 2)]),
            ("sys1".to_string(), vec![(1, 1), (1, 2)]),
        ])
    }

    #[test]
    fn reads_a_week_schedule() {
        let s = json!({ "_id": "s1", "name": "Matin", "type": "week", "prog_enabled": false, "device_ids": ["z1"],
            "start_conf": { "days": [0, 1, 2, 3, 4], "hour": 6, "minutes": 30, "mode": 3, "setpoint": { "celsius": 21, "fahrenheit": 70 } } });
        let v = view("c", "Maison", Some(1), &s, vec![ZoneRef { system_id: 1, zone_id: 1 }]);
        assert_eq!(v.days, vec![1, 2, 3, 4, 5], "Airzone 0 = Monday");
        assert_eq!(v.time.as_deref(), Some("06:30"));
        assert!(!v.enabled);
        assert!(v.editable);
        assert_eq!(v.action, ZoneCommand { mode: Some(OperationMode::Heating), setpoint: Some(21.0), ..Default::default() });
        let cal = json!({ "_id": "c1", "name": "Vacances", "type": "calendar", "start_conf": { "start_month": 6 } });
        assert!(!view("c", "Maison", None, &cal, vec![]).editable);
    }

    #[test]
    fn writes_the_app_format_and_keeps_unknown_fields() {
        let input = ScheduleInput {
            cloud_id: None,
            name: "Soir".into(),
            enabled: true,
            days: vec![5, 7],
            time: "18:15".into(),
            zones: vec![ZoneRef { system_id: 1, zone_id: 2 }, ZoneRef { system_id: 1, zone_id: 1 }],
            action: ZoneCommand { on: Some(true), setpoint: Some(20.5), ..Default::default() },
        };
        let existing = json!({ "_id": "x", "name": "Old", "type": "week", "extra": 1, "start_conf": { "days": [0], "hour": 1, "minutes": 0, "mode": 2, "aq_vent_mode_conf": 1 } });
        let (s, opts) = to_airzone(&input, Some(&existing), &devices()).unwrap();
        assert_eq!(s["start_conf"]["days"], json!([4, 6]));
        assert_eq!(s["start_conf"]["hour"], 18);
        assert_eq!(s["start_conf"]["minutes"], 15);
        assert_eq!(s["start_conf"]["power"], true);
        assert_eq!(s["start_conf"]["setpoint"], 20.5);
        assert!(s["start_conf"].get("mode").is_none(), "previous action replaced");
        assert_eq!(s["start_conf"]["aq_vent_mode_conf"], 1, "unknown fields kept");
        assert_eq!(s["extra"], 1);
        assert!(s.get("_id").is_none());
        assert_eq!(s["device_ids"], json!(["z2", "z1"]));
        assert_eq!(opts, Some(json!({ "units": 0 })));
        let off = ScheduleInput { action: ZoneCommand { on: Some(false), ..Default::default() }, ..input.clone() };
        let (s, opts) = to_airzone(&off, None, &devices()).unwrap();
        assert_eq!(s["start_conf"]["power"], false);
        assert_eq!(s["type"], "week");
        assert!(opts.is_none());
        let bad = ScheduleInput { zones: vec![ZoneRef { system_id: 9, zone_id: 9 }], ..input };
        assert!(to_airzone(&bad, None, &devices()).is_err());
    }
}
