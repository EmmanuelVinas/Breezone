//! Charging profiles, and what each charger follows: its profile, a charge asked now, what it does
//! and when it charges next (see `charging`).

use crate::access::Access;
use crate::auth::{AdminUser, AuthUser};
use crate::charging::{self, Charger, DayKind, DayView, Mode, Period, Profile};
use crate::db::{self, ChargerPlanRow};
use crate::devices::{self, DeviceView};
use crate::error::{ApiError, ApiResult};
use crate::i18n::Text;
use crate::price::{Calendar, DayColor};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Days shown on the frieze of a charger.
const DAYS: i64 = 3;

#[derive(Serialize)]
pub struct UsedBy {
    pub device_id: i64,
    pub name: String,
}

#[derive(Serialize)]
pub struct ProfileView {
    pub id: i64,
    pub name: String,
    pub profiles: Vec<DayKind>,
    pub calendar: Calendar,
    /// The chargers that follow it (those this user sees).
    pub used_by: Vec<UsedBy>,
    /// The automations that give it to a charger.
    pub automations: Vec<String>,
}

fn is_charger(d: &DeviceView) -> bool {
    d.installation_id.is_none() && d.source_id.is_some() && d.traits.iter().any(|t| t.starts_with("ev.charger@"))
}

async fn profile_views(state: &AppState, user: &AuthUser) -> ApiResult<Vec<ProfileView>> {
    let access = Access::of(&state.db, user).await?;
    let seen = devices::for_user(devices::list(state).await?, &access);
    let plans = db::list_charger_plans(&state.db).await?;
    // The profiles each automation gives (by id), with its name.
    let rules: Vec<(String, Vec<i64>)> = db::list_automations(&state.db)
        .await?
        .iter()
        .filter_map(|row| {
            let rule = crate::automations::Automation::of(row)?;
            let given = rule.steps().filter_map(|s| match s {
                crate::automations::Step::ChargerProfile { profile_id: Some(id), .. } => Some(*id),
                _ => None,
            });
            Some((row.name.clone(), given.collect()))
        })
        .collect();
    Ok(db::list_charge_profiles(&state.db)
        .await?
        .iter()
        .filter_map(|row| {
            let p = Profile::of(row)?;
            let used_by = plans
                .iter()
                .filter(|c| c.profile_id == Some(row.id))
                .filter_map(|c| seen.iter().find(|d| d.id == c.device_id).map(|d| UsedBy { device_id: d.id, name: d.name.clone() }))
                .collect();
            let automations = rules.iter().filter(|(_, ids)| ids.contains(&row.id)).map(|(name, _)| name.clone()).collect();
            Some(ProfileView { id: row.id, name: p.name, profiles: p.profiles, calendar: p.calendar, used_by, automations })
        })
        .collect())
}

async fn profile_view(state: &AppState, user: &AuthUser, id: i64) -> ApiResult<ProfileView> {
    profile_views(state, user).await?.into_iter().find(|p| p.id == id).ok_or_else(|| ApiError::not_found("charging profile not found"))
}

pub async fn list_profiles(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<ProfileView>>> {
    Ok(Json(profile_views(&state, &user).await?))
}

fn checked(p: &Profile) -> ApiResult<String> {
    charging::validate(p).map_err(ApiError::bad_request)?;
    Ok(p.plan_json())
}

/// The colours of the Tempo days, read soon after a profile starts following them.
fn read_colors(state: &AppState, p: &Profile) {
    if !crate::tempo::colored(&p.calendar) {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::tempo::refresh(&state).await {
            tracing::warn!("Tempo colours not read: {e:#}");
        }
    });
}

pub async fn create_profile(State(state): State<AppState>, AdminUser(user): AdminUser, Json(p): Json<Profile>) -> ApiResult<(StatusCode, Json<ProfileView>)> {
    let plan = checked(&p)?;
    let id = db::insert_charge_profile(&state.db, p.name.trim(), &plan).await?;
    read_colors(&state, &p);
    Ok((StatusCode::CREATED, Json(profile_view(&state, &user, id).await?)))
}

pub async fn update_profile(State(state): State<AppState>, AdminUser(user): AdminUser, Path(id): Path<i64>, Json(p): Json<Profile>) -> ApiResult<Json<ProfileView>> {
    let plan = checked(&p)?;
    if !db::update_charge_profile(&state.db, id, p.name.trim(), &plan).await? {
        return Err(ApiError::not_found("charging profile not found"));
    }
    read_colors(&state, &p);
    Ok(Json(profile_view(&state, &user, id).await?))
}

/// The chargers that followed it follow nothing any more.
pub async fn delete_profile(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if !db::delete_charge_profile(&state.db, id).await? {
        return Err(ApiError::not_found("charging profile not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------------------
// A charger
// ---------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ProfileRef {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize)]
pub struct NowView {
    pub amps: i64,
    pub kw: Option<f64>,
    /// Until then (`None`: until it is unplugged, or its energy).
    pub until: Option<i64>,
    /// Energy to add (kWh), and what is still to add.
    pub kwh: Option<f64>,
    pub left_kwh: Option<f64>,
}

#[derive(Serialize)]
pub struct StatusView {
    /// See `surplus::Status::state`.
    pub state: &'static str,
    pub reason: Text,
}

#[derive(Serialize)]
pub struct NextView {
    pub start: i64,
    pub end: i64,
    pub mode: Mode,
    pub kw: Option<f64>,
    pub color: Option<DayColor>,
}

#[derive(Serialize)]
pub struct ChargingView {
    pub profile_id: Option<i64>,
    /// The profiles it may follow.
    pub profiles: Vec<ProfileRef>,
    pub now: Option<NowView>,
    /// What it does now (`None`: not looked at yet, or it follows nothing).
    pub status: Option<StatusView>,
    /// When its profile charges next (now, when it does).
    pub next: Option<NextView>,
    /// Its profile over the next days, from today.
    pub days: Vec<DayView>,
    /// This user may change it.
    pub can_control: bool,
}

/// The charger, for whom sees it (`control`: and may drive it).
async fn charger(state: &AppState, user: &AuthUser, id: i64, control: bool) -> ApiResult<(DeviceView, bool)> {
    let access = Access::of(&state.db, user).await?;
    let d = devices::get(state, id).await?.filter(is_charger).ok_or_else(|| ApiError::not_found("charger not found"))?;
    let md = devices::module_device(&d);
    if !access.can_view_module(&md) {
        return Err(ApiError::not_found("charger not found"));
    }
    if control {
        access.control_module(&md)?;
    }
    Ok((d, access.can_control_module(&md)))
}

async fn view(state: &AppState, d: &DeviceView, can_control: bool) -> ApiResult<ChargingView> {
    let row = db::charger_plan(&state.db, d.id).await?.unwrap_or(ChargerPlanRow { device_id: d.id, ..Default::default() });
    let rows = db::list_charge_profiles(&state.db).await?;
    let profiles: Vec<(i64, Profile)> = rows.iter().filter_map(|r| Profile::of(r).map(|p| (r.id, p))).collect();
    let profile = row.profile_id.and_then(|id| profiles.iter().find(|(i, _)| *i == id)).map(|(_, p)| p);
    let now = db::now();
    let tz = state.cfg.timezone;
    let known = charging::colors_around(state, now).await?;
    let color = |day: NaiveDate| known.get(&day).copied();
    let c = Charger::of(d);
    // A `max` without a power: the charger's maximum.
    let full = |p: Period| Period { kw: p.kw.or_else(|| (p.mode == Mode::Max).then(|| charging::kw_of(c.limit_max, c.voltage, c.phases))), ..p };
    let next = profile.and_then(|p| charging::next(p, &color, tz, now)).map(full).map(|p| NextView { start: p.start, end: p.end, mode: p.mode, kw: p.kw, color: p.color });
    let days = profile
        .map(|p| charging::days(p, &color, tz, crate::rollup::local_day(now, tz), DAYS))
        .unwrap_or_default()
        .into_iter()
        .map(|d| DayView { periods: d.periods.into_iter().map(full).collect(), ..d })
        .collect();
    let snapshot = state.surplus.snapshot().await;
    let status = snapshot.statuses.get(&d.id).map(|s| StatusView { state: s.state, reason: s.text.clone() });
    let asked = row.now_amps.filter(|_| charging::now_over(&row, &c, now).is_none()).map(|amps| NowView {
        amps,
        kw: row.now_kw,
        until: row.now_until,
        kwh: row.now_kwh,
        left_kwh: row.now_kwh.zip(row.now_base_energy).map(|(k, base)| (k - (c.session_energy - base)).max(0.0)),
    });
    Ok(ChargingView {
        profile_id: row.profile_id.filter(|_| profile.is_some()),
        profiles: profiles.iter().map(|(id, p)| ProfileRef { id: *id, name: p.name.clone() }).collect(),
        now: asked,
        status,
        next,
        days,
        can_control,
    })
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<ChargingView>> {
    let (d, can) = charger(&state, &user, id, false).await?;
    Ok(Json(view(&state, &d, can).await?))
}

#[derive(Deserialize)]
pub struct ProfileInput {
    pub profile_id: Option<i64>,
}

/// The profile a charger follows (`null`: none).
pub async fn set_profile(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<ProfileInput>) -> ApiResult<Json<ChargingView>> {
    let (d, can) = charger(&state, &user, id, true).await?;
    charging::set_profile(&state, id, input.profile_id).await?;
    Ok(Json(view(&state, &d, can).await?))
}

/// Charge now, without a break: until it is unplugged, a time or some energy more.
pub async fn charge_now(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<charging::NowInput>) -> ApiResult<Json<ChargingView>> {
    let (d, can) = charger(&state, &user, id, true).await?;
    charging::charge_now(&state, &d, &input).await?;
    Ok(Json(view(&state, &d, can).await?))
}

/// The charge asked now is called off: back to its profile.
pub async fn cancel_now(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<ChargingView>> {
    let (d, can) = charger(&state, &user, id, true).await?;
    db::clear_charge_now(&state.db, id).await?;
    state.surplus.release(id).await;
    Ok(Json(view(&state, &d, can).await?))
}
