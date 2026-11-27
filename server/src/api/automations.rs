//! Automations ("quand… si… alors…"): CRUD, activation and manual run.

use crate::access::Access;
use crate::auth::{AdminUser, AuthUser};
use crate::automations::{self, Automation, AutomationInput, AutomationView, Branch, Context, RunView};
use crate::db::{self, AutomationFields, AutomationRow};
use crate::error::{ApiError, ApiResult};
use crate::routines;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// What the scenes a rule starts drive.
async fn scene_scopes(state: &AppState, rule: &Automation) -> ApiResult<Vec<routines::Scope>> {
    let mut out = vec![];
    for id in rule.scenes() {
        if let Some(row) = db::routine_by_id(&state.db, id).await? {
            out.extend(routines::to_view(&row).actions);
        }
    }
    Ok(routines::DeviceScopes::load(state).await?.of(&out))
}

/// A user sees a rule when they see every installation and device of a module it reads or drives.
async fn sees(state: &AppState, access: &Access, rule: &Automation, ctx: &Context) -> ApiResult<bool> {
    let seen = rule.read_devices().into_iter().chain(rule.driven_devices()).filter_map(|id| ctx.device(id)).all(|d| match d.installation_id {
        Some(i) => access.can_view(i),
        None => access.can_view_module(&crate::devices::module_device(d)),
    });
    let groups = rule.driven_groups().into_iter().all(|g| access.can_view_module(&group_scope(g)));
    Ok(seen && groups && access.sees_scopes(&scene_scopes(state, rule).await?))
}

/// A group as an access to it is judged: the devices of a type in a room (or anywhere), including
/// those added later — given by a grant on all devices, the type or the room, not device by device.
fn group_scope((kind, room_id): (&str, Option<i64>)) -> crate::access::ModuleDevice {
    let kind = crate::devices::TYPES.iter().copied().find(|t| *t == kind).unwrap_or("other");
    crate::access::ModuleDevice { id: 0, room_id, kind }
}

/// Driving what the rule drives: the zones of an installation need the right to drive it, a device
/// of a module the right to drive it (one no longer there: an administrator).
async fn check_control(state: &AppState, access: &Access, rule: &Automation, ctx: &Context) -> ApiResult<()> {
    for id in rule.driven_devices() {
        match ctx.device(id) {
            Some(d) => match d.installation_id {
                Some(i) => access.control(i)?,
                None => access.control_module(&crate::devices::module_device(d))?,
            },
            None if !access.is_admin() => return Err(ApiError::forbidden("administrators only")),
            None => {}
        }
    }
    for g in rule.driven_groups() {
        if !access.can_control_module(&group_scope(g)) {
            return Err(ApiError::forbidden(crate::text!(
                "piloter un groupe demande le pilotage de tout ce type d'appareil, ou de toute la pièce",
                "driving a group needs control of this whole type of device, or of the whole room",
                "controlar un grupo requiere el control de todo este tipo de dispositivo, o de toda la estancia"
            )));
        }
    }
    access.control_scopes(&scene_scopes(state, rule).await?)
}

/// Creating, changing or deleting a rule: driving what it drives, with the right to change the
/// routines of these installations, and seeing what it reads.
async fn check_edit(state: &AppState, access: &Access, rule: &Automation, ctx: &Context) -> ApiResult<()> {
    for id in rule.read_devices() {
        if let Some(i) = ctx.device(id).and_then(|d| d.installation_id) {
            access.view(i)?;
        }
    }
    check_control(state, access, rule, ctx).await?;
    let driven = rule.driven_devices().into_iter().filter_map(|id| ctx.device(id)?.installation_id);
    if driven.into_iter().any(|i| !access.rights(i).routines) {
        return Err(ApiError::forbidden("vous n'avez pas le droit de modifier les routines"));
    }
    access.edit_routine(&scene_scopes(state, rule).await?)
}

/// The row and its rule, for a user who sees it.
async fn load(state: &AppState, access: &Access, ctx: &Context, id: i64) -> ApiResult<(AutomationRow, Automation)> {
    let missing = || ApiError::not_found("automation not found");
    let row = db::automation_by_id(&state.db, id).await?.ok_or_else(missing)?;
    let rule = Automation::of(&row).ok_or_else(missing)?;
    if !sees(state, access, &rule, ctx).await? {
        return Err(missing());
    }
    Ok((row, rule))
}

async fn view_of(state: &AppState, id: i64) -> ApiResult<AutomationView> {
    let missing = || ApiError::not_found("automation not found");
    let row = db::automation_by_id(&state.db, id).await?.ok_or_else(missing)?;
    let rule = Automation::of(&row).ok_or_else(missing)?;
    let ctx = Context::load(state).await?;
    let here = ctx.at(rule.place(&ctx));
    Ok(automations::view(&row, rule, &here, state.automations.is_running(id)))
}

/// The rule as stored, as JSON: (triggers, conditions, actions, else, over).
fn stored(rule: &Automation) -> [String; 5] {
    [json!(rule.triggers).to_string(), json!(rule.conditions).to_string(), json!(rule.actions).to_string(), json!(rule.otherwise).to_string(), json!(rule.over).to_string()]
}

fn fields<'a>(name: &'a str, enabled: bool, rule: &Automation, json: &'a [String; 5]) -> AutomationFields<'a> {
    AutomationFields {
        name,
        enabled,
        trigger: &json[0],
        joined: rule.joined.as_str(),
        conditions: &json[1],
        actions: &json[2],
        else_actions: &json[3],
        run_mode: rule.mode.as_str(),
        over_actions: &json[4],
    }
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<AutomationView>>> {
    let access = Access::of(&state.db, &user).await?;
    let rows = db::list_automations(&state.db).await?;
    if rows.is_empty() {
        return Ok(Json(vec![]));
    }
    let ctx = Context::load(&state).await?;
    let rules: Vec<(AutomationRow, Automation)> = rows.into_iter().filter_map(|row| Automation::of(&row).map(|rule| (row, rule))).collect();
    let mut conflicts = automations::conflicts(&rules, &ctx);
    // The devices automations keep on the surplus (a charger: told to charge on it), and by which.
    let mut steered: Vec<(i64, i64)> = db::list_surplus_loads(&state.db).await?.into_iter().filter(|l| l.enabled).filter_map(|l| Some((l.device_id, l.automation_id?))).collect();
    for plan in db::list_charger_plans(&state.db).await? {
        if let (crate::charging::Mode::Solar, Some(by)) = (crate::charging::Mode::of_row(&plan), plan.automation_id) {
            steered.push((plan.device_id, by));
        }
    }
    let ranks: std::collections::HashMap<i64, (i64, String)> = rules.iter().map(|(r, _)| (r.id, (r.priority, r.name.clone()))).collect();
    let mut out = vec![];
    for (row, rule) in rules {
        if sees(&state, &access, &rule, &ctx).await? {
            let running = state.automations.is_running(row.id);
            let here = ctx.at(rule.place(&ctx));
            let drives = rule.expanded_devices(&ctx);
            let surplus = steered
                .iter()
                .filter(|(d, by)| *by != row.id && drives.contains(d) && ranks.get(by).is_some_and(|(p, _)| *p < row.priority))
                .map(|(d, by)| {
                    let name = ctx.device_name(*d);
                    crate::text!("{0} (« {1} »)", "{0} (“{1}”)", "{0} («{1}»)", name.arg(), ranks[by].1.clone())
                })
                .collect();
            let mut view = automations::view(&row, rule, &here, running);
            view.conflicts = conflicts.remove(&row.id).unwrap_or_default();
            view.surplus = surplus;
            out.push(view);
        }
    }
    // A rule this user cannot see is not named to them.
    let seen: Vec<i64> = out.iter().map(|v| v.id).collect();
    for v in &mut out {
        v.conflicts.retain(|c| seen.contains(&c.id));
    }
    Ok(Json(out))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<AutomationView>> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    let (row, rule) = load(&state, &access, &ctx, id).await?;
    let here = ctx.at(rule.place(&ctx));
    Ok(Json(automations::view(&row, rule, &here, state.automations.is_running(id))))
}

/// The last runs of a rule, those that did not happen included.
pub async fn runs(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Vec<RunView>>> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    load(&state, &access, &ctx, id).await?;
    Ok(Json(db::automation_runs(&state.db, id).await?.into_iter().map(RunView::of).collect()))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(input): Json<AutomationInput>) -> ApiResult<(StatusCode, Json<AutomationView>)> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    let enabled = input.enabled;
    let (name, rule) = automations::validate(input, &ctx)?;
    check_edit(&state, &access, &rule, &ctx).await?;
    let json = stored(&rule);
    let row = db::insert_automation(&state.db, &fields(&name, enabled, &rule, &json)).await?;
    state
        .hub
        .log("automation", "info", None, Some(&user.username), crate::text!("Automatisme « {0} » créé", "Automation “{0}” created", "Automatismo «{0}» creado", name), None)
        .await;
    state.hub.routines_changed();
    let here = ctx.at(rule.place(&ctx));
    Ok((StatusCode::CREATED, Json(automations::view(&row, rule, &here, false))))
}

pub async fn update(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<AutomationInput>) -> ApiResult<Json<AutomationView>> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    let (_, before) = load(&state, &access, &ctx, id).await?;
    check_edit(&state, &access, &before, &ctx).await?;
    let enabled = input.enabled;
    let (name, rule) = automations::validate(input, &ctx)?;
    check_edit(&state, &access, &rule, &ctx).await?;
    let json = stored(&rule);
    db::update_automation(&state.db, id, &fields(&name, enabled, &rule, &json)).await?;
    // A run under way follows the rule as it was: it ends.
    state.automations.cancel(&state.db, id).await;
    state
        .hub
        .log("automation", "info", None, Some(&user.username), crate::text!("Automatisme « {0} » modifié", "Automation “{0}” changed", "Automatismo «{0}» modificado", name), None)
        .await;
    state.hub.routines_changed();
    Ok(Json(view_of(&state, id).await?))
}

#[derive(Deserialize)]
pub struct EnabledInput {
    pub enabled: bool,
}

pub async fn set_enabled(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<EnabledInput>) -> ApiResult<Json<AutomationView>> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    let (row, rule) = load(&state, &access, &ctx, id).await?;
    check_edit(&state, &access, &rule, &ctx).await?;
    db::set_automation_enabled(&state.db, id, input.enabled).await?;
    if !input.enabled {
        state.automations.cancel(&state.db, id).await;
    }
    let message = if input.enabled {
        crate::text!("Automatisme « {0} » activé", "Automation “{0}” switched on", "Automatismo «{0}» activado", row.name)
    } else {
        crate::text!("Automatisme « {0} » désactivé", "Automation “{0}” switched off", "Automatismo «{0}» desactivado", row.name)
    };
    state.hub.log("automation", "info", None, Some(&user.username), message, None).await;
    state.hub.routines_changed();
    Ok(Json(view_of(&state, id).await?))
}

/// A rule this version cannot read (written by a newer one, kept as it is).
#[derive(Serialize)]
pub struct UnreadableView {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub updated_at: i64,
}

/// The rules this version cannot read: they do not run here. Administrators only, as what a rule
/// reads or drives is not known.
pub async fn unreadable(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<UnreadableView>>> {
    let rows = db::list_automations(&state.db).await?;
    let out = rows.into_iter().filter(|r| Automation::of(r).is_none()).map(|r| UnreadableView { id: r.id, name: r.name, enabled: r.enabled, updated_at: r.updated_at }).collect();
    Ok(Json(out))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let access = Access::of(&state.db, &user).await?;
    let row = db::automation_by_id(&state.db, id).await?.ok_or_else(|| ApiError::not_found("automation not found"))?;
    // A rule this version cannot read: an administrator may still remove it.
    if Automation::of(&row).is_none() && !access.is_admin() {
        return Err(ApiError::not_found("automation not found"));
    }
    if Automation::of(&row).is_some() {
        let ctx = Context::load(&state).await?;
        let (_, rule) = load(&state, &access, &ctx, id).await?;
        check_edit(&state, &access, &rule, &ctx).await?;
    }
    state.automations.cancel(&state.db, id).await;
    db::delete_automation(&state.db, id).await?;
    state
        .hub
        .log(
            "automation",
            "info",
            None,
            Some(&user.username),
            crate::text!("Automatisme « {0} » supprimé", "Automation “{0}” deleted", "Automatismo «{0}» eliminado", row.name),
            None,
        )
        .await;
    state.hub.routines_changed();
    Ok(StatusCode::NO_CONTENT)
}

/// Runs the actions now, whatever the trigger and the conditions say (to try a rule).
#[derive(Deserialize)]
pub struct OrderInput {
    /// The automations, the first in priority first (those left out after them, as they were).
    pub ids: Vec<i64>,
}

/// Ranks the automations: the solar surplus goes to them in this order, and the higher one wins a
/// device two of them drive at the same time.
pub async fn order(State(state): State<AppState>, _admin: AdminUser, Json(input): Json<OrderInput>) -> ApiResult<StatusCode> {
    db::order_automations(&state.db, &input.ids).await?;
    state.hub.routines_changed();
    Ok(StatusCode::NO_CONTENT)
}

pub async fn run(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    let (row, rule) = load(&state, &access, &ctx, id).await?;
    check_control(&state, &access, &rule, &ctx).await?;
    let reason = crate::text!("lancé à la main", "run by hand", "lanzado a mano");
    let Some(task) = automations::start(&state, &row, &rule, automations::Due { id, branch: Branch::Then, reason, lasting: None }, ctx, &user.username).await else {
        return Err(ApiError::conflict(crate::text!("déjà en cours", "already running", "ya en curso")));
    };
    // A rule that waits goes on in the background: its history tells how it ends.
    match tokio::time::timeout(std::time::Duration::from_secs(10), task).await {
        Ok(Ok(done)) => Ok(Json(json!({ "results": done, "running": false }))),
        _ => Ok(Json(json!({ "results": [], "running": true }))),
    }
}

/// What the automations of this server share: the holiday pause, the ntfy topic.
#[derive(Serialize)]
pub struct SettingsView {
    /// While set, no rule starts by itself: until this time, or until switched back on (0).
    pub paused_until: Option<i64>,
    /// Shown to administrators only (the address of the topic is enough to write to it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ntfy_url: Option<String>,
}

pub async fn settings(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<SettingsView>> {
    let ntfy_url = if user.is_admin() { Some(db::get_setting(&state.db, automations::NTFY).await?.unwrap_or_default()) } else { None };
    Ok(Json(SettingsView { paused_until: automations::paused_until(&state.db).await?, ntfy_url }))
}

/// Only the fields given change; `null` clears.
#[derive(Deserialize)]
pub struct SettingsInput {
    #[serde(default, deserialize_with = "present")]
    pub paused_until: Option<Option<i64>>,
    #[serde(default, deserialize_with = "present")]
    pub ntfy_url: Option<Option<String>>,
}

fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Ok(Some(Option::deserialize(d)?))
}

pub async fn save_settings(State(state): State<AppState>, admin: AdminUser, Json(input): Json<SettingsInput>) -> ApiResult<Json<SettingsView>> {
    let user = &admin.0.username;
    if let Some(paused) = input.paused_until {
        match paused {
            Some(until) if until < 0 => return Err(ApiError::bad_request("paused_until: a time, or 0")),
            Some(until) => {
                db::set_setting(&state.db, automations::PAUSED, &until.to_string()).await?;
                let message = if until == 0 {
                    crate::text!("Automatismes en pause", "Automations paused", "Automatismos en pausa")
                } else {
                    let day = chrono::DateTime::from_timestamp(until, 0).unwrap_or_default().with_timezone(&state.cfg.timezone).format("%d/%m/%Y %H:%M").to_string();
                    crate::text!("Automatismes en pause jusqu'au {0}", "Automations paused until {0}", "Automatismos en pausa hasta el {0}", day)
                };
                state.hub.log("automation", "info", None, Some(user), message, None).await;
            }
            None => {
                db::delete_setting(&state.db, automations::PAUSED).await?;
                state.hub.log("automation", "info", None, Some(user), crate::text!("Automatismes repris", "Automations resumed", "Automatismos reanudados"), None).await;
            }
        }
    }
    if let Some(url) = input.ntfy_url {
        match url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()) {
            Some(u) if !(u.starts_with("https://") || u.starts_with("http://")) => {
                return Err(ApiError::bad_request("adresse invalide : https:// ou http:// attendu"));
            }
            Some(u) => db::set_setting(&state.db, automations::NTFY, &u).await?,
            None => db::delete_setting(&state.db, automations::NTFY).await?,
        }
    }
    state.hub.routines_changed();
    settings(State(state), admin.0).await
}

/// Sends a notification now, to try the ntfy topic.
pub async fn notify_test(State(state): State<AppState>, admin: AdminUser) -> ApiResult<StatusCode> {
    let message = crate::text!("Les notifications de Breezone arrivent bien.", "Breezone notifications get through.", "Las notificaciones de Breezone llegan bien.");
    automations::notify(&state, "Test", &message.current(), &admin.0.username).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Who is at home, as each user told it.
#[derive(Serialize)]
pub struct PresenceView {
    /// This user (`None`: never told).
    pub me: Option<bool>,
    /// Someone is at home (`None`: nobody ever told).
    pub someone_home: Option<bool>,
    pub people: Vec<db::Presence>,
}

/// What the modules bring to the rules (`conditions` of their manifests), with their values now.
pub async fn module_conditions(State(state): State<AppState>, _user: AuthUser) -> Json<Vec<crate::modules::conditions::ModuleConditions>> {
    Json(crate::modules::conditions::view(&state.db).await)
}

pub async fn presence(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<PresenceView>> {
    let people = db::list_presence(&state.db).await?;
    Ok(Json(PresenceView { me: people.iter().find(|p| p.user_id == user.id).map(|p| p.home), someone_home: (!people.is_empty()).then(|| people.iter().any(|p| p.home)), people }))
}

#[derive(Deserialize)]
pub struct PresenceInput {
    pub home: bool,
}

/// A user tells whether they are at home (the app, by itself or by hand; the web, by hand).
pub async fn set_presence(State(state): State<AppState>, user: AuthUser, Json(input): Json<PresenceInput>) -> ApiResult<Json<PresenceView>> {
    if db::set_presence(&state.db, user.id, input.home).await? {
        let message = if input.home {
            crate::text!("{0} : à la maison", "{0}: at home", "{0}: en casa", user.username)
        } else {
            crate::text!("{0} : parti de la maison", "{0}: away from home", "{0}: fuera de casa", user.username)
        };
        state.hub.log("presence", "info", None, Some(&user.username), message, None).await;
        state.hub.routines_changed();
    }
    presence(State(state), user).await
}
