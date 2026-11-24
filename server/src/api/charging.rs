//! Charging profiles (a power each), what the automations plan for the chargers, and what each
//! charger follows and does (see `charging`).

use crate::access::Access;
use crate::auth::{AdminUser, AuthUser};
use crate::automations::plan::{self, Bar, Effect, RealTariff, Segment, World};
use crate::automations::{Automation, Context, Summary};
use crate::charging::{self, Charger, Mode, ProfileInput};
use crate::db::{self, ChargeProfileRow, ChargerPlanRow};
use crate::devices::{self, DeviceView};
use crate::error::{ApiError, ApiResult};
use crate::i18n::Text;
use crate::price::DayColor;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{Duration, NaiveDate, TimeZone};
use serde::Serialize;

/// Days shown on the frieze of a charger.
const DAYS: i64 = 3;
/// How far the next charge is looked for.
const NEXT_WITHIN: i64 = 48 * 3600;

#[derive(Serialize)]
pub struct RuleRef {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize)]
pub struct ProfileView {
    pub id: i64,
    pub name: String,
    /// Up to this power (`None`: the charger's maximum).
    pub kw: Option<f64>,
    pub color: Option<String>,
    /// The automations that give it to a charger.
    pub automations: Vec<RuleRef>,
}

fn is_charger(d: &DeviceView) -> bool {
    d.installation_id.is_none() && d.source_id.is_some() && d.traits.iter().any(|t| t.starts_with("ev.charger@"))
}

/// The rules this server can read, with their ids and names.
async fn rules(state: &AppState) -> ApiResult<Vec<(db::AutomationRow, Automation)>> {
    Ok(db::list_automations(&state.db).await?.into_iter().filter_map(|row| Automation::of(&row).map(|r| (row, r))).collect())
}

fn gives(rule: &Automation, profile_id: i64) -> bool {
    rule.steps()
        .any(|s| matches!(s, crate::automations::Step::ChargerProfile { profile_id: Some(id), mode, .. } if *id == profile_id && Mode::of_step(*mode, Some(*id)) == Mode::Profile))
}

async fn profile_views(state: &AppState) -> ApiResult<Vec<ProfileView>> {
    let rules = rules(state).await?;
    Ok(db::list_charge_profiles(&state.db)
        .await?
        .into_iter()
        .map(|p| ProfileView {
            automations: rules.iter().filter(|(_, r)| gives(r, p.id)).map(|(row, _)| RuleRef { id: row.id, name: row.name.clone() }).collect(),
            id: p.id,
            name: p.name,
            kw: p.kw,
            color: p.color,
        })
        .collect())
}

async fn profile_view(state: &AppState, id: i64) -> ApiResult<ProfileView> {
    profile_views(state).await?.into_iter().find(|p| p.id == id).ok_or_else(|| ApiError::not_found("charging profile not found"))
}

pub async fn list_profiles(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Vec<ProfileView>>> {
    Ok(Json(profile_views(&state).await?))
}

fn checked(p: &ProfileInput) -> ApiResult<()> {
    charging::validate(p).map_err(ApiError::bad_request)
}

pub async fn create_profile(State(state): State<AppState>, _admin: AdminUser, Json(p): Json<ProfileInput>) -> ApiResult<(StatusCode, Json<ProfileView>)> {
    checked(&p)?;
    if db::list_charge_profiles(&state.db).await?.len() >= charging::MAX_PROFILES {
        return Err(ApiError::bad_request(format!("{} charging profiles at most", charging::MAX_PROFILES)));
    }
    let id = db::insert_charge_profile(&state.db, p.name.trim(), p.kw, p.color.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(profile_view(&state, id).await?)))
}

pub async fn update_profile(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>, Json(p): Json<ProfileInput>) -> ApiResult<Json<ProfileView>> {
    checked(&p)?;
    if !db::update_charge_profile(&state.db, id, p.name.trim(), p.kw, p.color.as_deref()).await? {
        return Err(ApiError::not_found("charging profile not found"));
    }
    Ok(Json(profile_view(&state, id).await?))
}

/// The chargers that followed it are left alone.
pub async fn delete_profile(State(state): State<AppState>, _admin: AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if !db::delete_charge_profile(&state.db, id).await? {
        return Err(ApiError::not_found("charging profile not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------------------
// What the automations plan
// ---------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct BarView {
    pub device_id: i64,
    #[serde(flatten)]
    pub bar: Bar,
}

#[derive(Serialize)]
pub struct RulePlan {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub summary: Summary,
    /// The chargers it drives (those this user sees).
    pub devices: Vec<RuleRef>,
    /// Every charger it drives lets its own app decide: it does not apply.
    pub paused: bool,
    /// None of its triggers is at a time (a measure, a device…): no frieze.
    pub placed: bool,
    /// A condition that cannot be told ahead was taken as true.
    pub assumed: bool,
    pub bars: Vec<BarView>,
}

/// The automations that drive a charger, each with its friezes (a day of each kind of the tariff
/// it names, or any day).
pub async fn automations(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<RulePlan>>> {
    let access = Access::of(&state.db, &user).await?;
    let chargers: Vec<DeviceView> = devices::for_user(devices::list(&state).await?, &access).into_iter().filter(is_charger).collect();
    let ctx = Context::load(&state).await?;
    let tariffs = crate::api::price::tariffs(&state, None).await?;
    let tariff = crate::price::current(&tariffs);
    let today_kind = ctx.tariff_now.as_ref().map(|(k, _)| k.as_str());
    let today = crate::rollup::local_day(ctx.now, ctx.tz);
    let mut out = vec![];
    for (row, rule) in rules(&state).await? {
        let driven: Vec<&DeviceView> = chargers.iter().filter(|d| plan::drives(&rule, d.id)).collect();
        if driven.is_empty() {
            continue;
        }
        let placed = rule.triggers.iter().any(plan::placed);
        let mut bars = vec![];
        let mut assumed = false;
        if placed {
            for d in &driven {
                let (list, guessed) = plan::bars(&rule, d.id, tariff, today_kind, ctx.coords, ctx.tz, today);
                assumed |= guessed;
                bars.extend(list.into_iter().map(|bar| BarView { device_id: d.id, bar }));
            }
        }
        out.push(RulePlan {
            id: row.id,
            name: row.name.clone(),
            enabled: row.enabled,
            summary: summary(&ctx, &rule),
            devices: driven.iter().map(|d| RuleRef { id: d.id, name: d.name.clone() }).collect(),
            paused: driven.iter().all(|d| Charger::of(d).own_schedule),
            placed,
            assumed,
            bars,
        });
    }
    Ok(Json(out))
}

fn summary(ctx: &Context, rule: &Automation) -> Summary {
    Summary {
        when: ctx.describe_triggers(&rule.triggers),
        triggers: rule.triggers.iter().map(|t| ctx.describe_trigger(t)).collect(),
        conditions: rule.conditions.iter().map(|c| ctx.describe_condition(c)).collect(),
        joined: rule.joined.word(),
        actions: rule.actions.iter().map(|s| ctx.describe_step(s)).collect(),
        otherwise: rule.otherwise.iter().map(|s| ctx.describe_step(s)).collect(),
        over: rule.over.iter().map(|s| ctx.describe_step(s)).collect(),
    }
}

// ---------------------------------------------------------------------------------------
// A charger
// ---------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ProfileRef {
    pub id: i64,
    pub name: String,
    pub kw: Option<f64>,
    pub color: Option<String>,
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
    pub effect: Effect,
    /// The power (kW) of a profile or of a charge now (the charger's maximum when it gives none).
    pub kw: Option<f64>,
    pub automation: Option<RuleRef>,
}

#[derive(Serialize)]
pub struct DayView {
    /// "YYYY-MM-DD".
    pub day: String,
    /// Its Tempo colour, when it is known.
    pub color: Option<DayColor>,
    pub segments: Vec<Segment>,
}

#[derive(Serialize)]
pub struct ChargingView {
    /// What it was told: "profile", "off", "solar" or "free".
    pub mode: Mode,
    pub profile_id: Option<i64>,
    /// The automation that told it (`None`: none yet, or by hand).
    pub automation: Option<RuleRef>,
    /// The profiles, for the friezes.
    pub profiles: Vec<ProfileRef>,
    /// Its own app decides when it charges: the automations do not apply.
    pub own_schedule: bool,
    pub now: Option<NowView>,
    /// What it does now (`None`: not looked at yet, or it follows nothing).
    pub status: Option<StatusView>,
    /// When the automations make it charge next (now, when they do).
    pub next: Option<NextView>,
    /// What the automations plan over the next days, from today.
    pub days: Vec<DayView>,
    /// The automations that drive it.
    pub automations: Vec<RuleRef>,
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

fn midnight(day: NaiveDate, tz: chrono_tz::Tz) -> i64 {
    tz.from_local_datetime(&day.and_time(chrono::NaiveTime::MIN)).earliest().map_or(0, |t| t.timestamp())
}

async fn view(state: &AppState, d: &DeviceView, can_control: bool) -> ApiResult<ChargingView> {
    let row = db::charger_plan(&state.db, d.id).await?.unwrap_or(ChargerPlanRow { device_id: d.id, ..Default::default() });
    let profiles: Vec<ChargeProfileRow> = db::list_charge_profiles(&state.db).await?;
    let all = rules(state).await?;
    let name_of = |id: i64| all.iter().find(|(r, _)| r.id == id).map(|(r, _)| RuleRef { id, name: r.name.clone() });
    let now = db::now();
    let tz = state.cfg.timezone;
    let c = Charger::of(d);
    // What the enabled automations that drive it plan, over the next days.
    let today = crate::rollup::local_day(now, tz);
    let known = crate::tempo::colors(&state.db, today - Duration::days(1), today + Duration::days(DAYS)).await?;
    let color = |day: NaiveDate| known.get(&day).copied();
    let tariffs = crate::api::price::tariffs(state, None).await?;
    let coords = state.weather.place(None).await.map(|p| (p.lat, p.lon));
    let snapshot = state.surplus.snapshot().await;
    let real = RealTariff { tariffs: &tariffs, colors: &color, tz };
    let world = World { tz, coords, tariff: &real, real: true };
    let driving: Vec<(i64, &Automation)> = all.iter().filter(|(row, r)| row.enabled && plan::drives(r, d.id)).map(|(row, r)| (row.id, r)).collect();
    let (from, until) = (midnight(today, tz), midnight(today + Duration::days(DAYS), tz));
    let (segments, _) = plan::segments(&world, &driving, d.id, from, until.max(now + NEXT_WITHIN));
    let days = (0..DAYS)
        .map(|i| {
            let day = today + Duration::days(i);
            let (a, b) = (midnight(day, tz), midnight(day + Duration::days(1), tz));
            let segments = segments.iter().filter(|s| s.end > a && s.start < b).map(|s| Segment { start: s.start.max(a), end: s.end.min(b), ..s.clone() }).collect();
            DayView { day: day.format("%Y-%m-%d").to_string(), color: color(day), segments }
        })
        .collect();
    let kw_of_effect = |e: &Effect| match e {
        Effect::Profile { profile_id } => {
            Some(profile_id.and_then(|id| profiles.iter().find(|p| p.id == id)).and_then(|p| p.kw).unwrap_or_else(|| charging::kw_of(c.limit_max, c.voltage, c.phases)))
        }
        Effect::Now { kw } => Some(kw.unwrap_or_else(|| charging::kw_of(c.limit_max, c.voltage, c.phases))),
        _ => None,
    };
    let next = segments
        .iter()
        .filter(|s| s.end > now && s.start < now + NEXT_WITHIN)
        .find_map(|s| s.effect.filter(|e| !matches!(e, Effect::Off | Effect::Free)).map(|e| (s, e)))
        .map(|(s, e)| NextView { start: s.start.max(now), end: s.end, effect: e, kw: kw_of_effect(&e), automation: s.automation_id.and_then(name_of) });
    let status = snapshot.statuses.get(&d.id).map(|s| StatusView { state: s.state, reason: s.text.clone() });
    let asked = row.now_amps.filter(|_| charging::now_over(&row, &c, now).is_none()).map(|amps| NowView {
        amps,
        kw: row.now_kw,
        until: row.now_until,
        kwh: row.now_kwh,
        left_kwh: row.now_kwh.zip(row.now_base_energy).map(|(k, base)| (k - (c.session_energy - base)).max(0.0)),
    });
    let mode = Mode::of_row(&row);
    Ok(ChargingView {
        mode,
        profile_id: row.profile_id.filter(|_| mode == Mode::Profile),
        automation: row.automation_id.and_then(name_of),
        profiles: profiles.iter().map(|p| ProfileRef { id: p.id, name: p.name.clone(), kw: p.kw, color: p.color.clone() }).collect(),
        own_schedule: c.own_schedule,
        now: asked,
        status,
        next,
        days,
        automations: all.iter().filter(|(_, r)| plan::drives(r, d.id)).map(|(row, _)| RuleRef { id: row.id, name: row.name.clone() }).collect(),
        can_control,
    })
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<ChargingView>> {
    let (d, can) = charger(&state, &user, id, false).await?;
    Ok(Json(view(&state, &d, can).await?))
}

/// Charge now, without a break: until it is unplugged, a time or some energy more.
pub async fn charge_now(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<charging::NowInput>) -> ApiResult<Json<ChargingView>> {
    let (d, can) = charger(&state, &user, id, true).await?;
    charging::charge_now(&state, &d, &input).await?;
    Ok(Json(view(&state, &d, can).await?))
}

/// The charge asked now is called off: back to what it was told.
pub async fn cancel_now(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<ChargingView>> {
    let (d, can) = charger(&state, &user, id, true).await?;
    db::clear_charge_now(&state.db, id).await?;
    state.surplus.release(id).await;
    Ok(Json(view(&state, &d, can).await?))
}
