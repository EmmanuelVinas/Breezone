//! Automations ("quand… si… alors…"): CRUD, activation and manual run.

use crate::access::Access;
use crate::auth::AuthUser;
use crate::automations::{self, Automation, AutomationInput, AutomationView, Context};
use crate::db::{self, AutomationRow};
use crate::error::{ApiError, ApiResult};
use crate::routines;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

/// The actions of the scenes a rule starts.
async fn scene_actions(state: &AppState, rule: &Automation) -> ApiResult<Vec<routines::Action>> {
    let mut out = vec![];
    for id in rule.scenes() {
        if let Some(row) = db::routine_by_id(&state.db, id).await? {
            out.extend(routines::to_view(&row).actions);
        }
    }
    Ok(out)
}

/// A user sees a rule when they see every installation it reads or drives.
async fn sees(state: &AppState, access: &Access, rule: &Automation, ctx: &Context) -> ApiResult<bool> {
    let devices = rule.read_devices().into_iter().chain(rule.driven_devices());
    let installations = devices.filter_map(|id| ctx.device(id)?.installation_id).collect::<Vec<_>>();
    Ok(installations.into_iter().all(|i| access.can_view(i)) && access.sees_actions(&scene_actions(state, rule).await?))
}

/// Driving what the rule drives: the zones of an installation need the right to drive it, the
/// devices of the modules an administrator (until rights per type of device).
async fn check_control(state: &AppState, access: &Access, rule: &Automation, ctx: &Context) -> ApiResult<()> {
    for id in rule.driven_devices() {
        match ctx.device(id).and_then(|d| d.installation_id) {
            Some(i) => access.control(i)?,
            None if !access.is_admin() => return Err(ApiError::forbidden("administrators only")),
            None => {}
        }
    }
    access.control_actions(&scene_actions(state, rule).await?)
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
    access.edit_routine(&scene_actions(state, rule).await?)
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
    Ok(automations::view(&row, rule, &Context::load(state).await?))
}

fn stored(rule: &Automation) -> (String, String, String) {
    (json!(rule.when).to_string(), json!(rule.conditions).to_string(), json!(rule.actions).to_string())
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<AutomationView>>> {
    let access = Access::of(&state.db, &user).await?;
    let rows = db::list_automations(&state.db).await?;
    if rows.is_empty() {
        return Ok(Json(vec![]));
    }
    let ctx = Context::load(&state).await?;
    let mut out = vec![];
    for row in rows {
        let Some(rule) = Automation::of(&row) else { continue };
        if sees(&state, &access, &rule, &ctx).await? {
            out.push(automations::view(&row, rule, &ctx));
        }
    }
    Ok(Json(out))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<AutomationView>> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    let (row, rule) = load(&state, &access, &ctx, id).await?;
    Ok(Json(automations::view(&row, rule, &ctx)))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(input): Json<AutomationInput>) -> ApiResult<(StatusCode, Json<AutomationView>)> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    let enabled = input.enabled;
    let (name, rule) = automations::validate(input, &ctx)?;
    check_edit(&state, &access, &rule, &ctx).await?;
    let (when, conditions, actions) = stored(&rule);
    let row = db::insert_automation(&state.db, &name, enabled, &when, &conditions, &actions).await?;
    state
        .hub
        .log("automation", "info", None, Some(&user.username), crate::text!("Automatisme « {0} » créé", "Automation “{0}” created", "Automatismo «{0}» creado", name), None)
        .await;
    state.hub.routines_changed();
    Ok((StatusCode::CREATED, Json(automations::view(&row, rule, &ctx))))
}

pub async fn update(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(input): Json<AutomationInput>) -> ApiResult<Json<AutomationView>> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    let (_, before) = load(&state, &access, &ctx, id).await?;
    check_edit(&state, &access, &before, &ctx).await?;
    let enabled = input.enabled;
    let (name, rule) = automations::validate(input, &ctx)?;
    check_edit(&state, &access, &rule, &ctx).await?;
    let (when, conditions, actions) = stored(&rule);
    db::update_automation(&state.db, id, &name, enabled, &when, &conditions, &actions).await?;
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
    let message = if input.enabled {
        crate::text!("Automatisme « {0} » activé", "Automation “{0}” switched on", "Automatismo «{0}» activado", row.name)
    } else {
        crate::text!("Automatisme « {0} » désactivé", "Automation “{0}” switched off", "Automatismo «{0}» desactivado", row.name)
    };
    state.hub.log("automation", "info", None, Some(&user.username), message, None).await;
    state.hub.routines_changed();
    Ok(Json(view_of(&state, id).await?))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    let (row, rule) = load(&state, &access, &ctx, id).await?;
    check_edit(&state, &access, &rule, &ctx).await?;
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
pub async fn run(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let access = Access::of(&state.db, &user).await?;
    let ctx = Context::load(&state).await?;
    let (row, rule) = load(&state, &access, &ctx, id).await?;
    check_control(&state, &access, &rule, &ctx).await?;
    let reason = crate::text!("lancé à la main", "run by hand", "lanzado a mano");
    let results = automations::run(&state, &row, &rule, &ctx, reason, &user.username).await;
    Ok(Json(json!({ "results": results })))
}
