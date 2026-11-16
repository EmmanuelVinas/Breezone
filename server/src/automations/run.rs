//! The runs: the steps of a rule in the background, their history, the loop of the engine.

use super::*;

pub(super) fn text_json(t: &Text) -> Value {
    json!({ "fr": t.fr, "en": t.en, "es": t.es })
}

pub(super) fn text_of(v: &Value) -> Text {
    Text::load(&v.to_string())
}

/// A step done, as the history keeps it.
#[derive(Debug, Clone, Serialize)]
pub struct StepDone {
    pub what: Text,
    pub ok: bool,
    pub error: Option<Text>,
}

impl StepDone {
    pub(super) fn stored(&self) -> Value {
        json!({ "what": text_json(&self.what), "ok": self.ok, "error": self.error.as_ref().map(text_json) })
    }

    pub(super) fn load(v: &Value) -> StepDone {
        StepDone { what: text_of(&v["what"]), ok: v["ok"].as_bool().unwrap_or(false), error: v.get("error").filter(|e| !e.is_null()).map(text_of) }
    }
}

/// A run in the history.
#[derive(Debug, Clone, Serialize)]
pub struct RunView {
    pub started_at: i64,
    pub finished_at: Option<i64>,
    /// "then" | "else" | "skipped" | "busy"
    pub branch: String,
    pub reason: Text,
    /// "done" | "stopped" | "cancelled" | "interrupted"; `None` while it runs.
    pub ended: Option<String>,
    pub steps: Vec<StepDone>,
}

impl RunView {
    pub fn of(r: db::AutomationRun) -> RunView {
        let steps: Vec<Value> = serde_json::from_str(&r.steps).unwrap_or_default();
        RunView {
            started_at: r.started_at,
            finished_at: r.finished_at,
            branch: r.branch,
            reason: Text::load(&r.reason),
            ended: r.ended,
            steps: steps.iter().map(StepDone::load).collect(),
        }
    }
}

/// The runs under way, by rule.
#[derive(Default)]
pub struct Runs {
    running: Mutex<HashMap<i64, (u64, tokio::task::AbortHandle, i64)>>,
    next: AtomicU64,
}

impl Runs {
    pub fn is_running(&self, id: i64) -> bool {
        self.running.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&id)
    }

    /// Ends the run of a rule (changed, switched off, deleted); its history says it was cancelled.
    pub async fn cancel(&self, db: &db::Db, id: i64) {
        let taken = self.running.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        if let Some((_, handle, run_id)) = taken {
            handle.abort();
            if let Err(e) = db::finish_automation_run_ended(db, run_id, "cancelled").await {
                tracing::error!("automation {id}: {e}");
            }
        }
    }
}

/// Starts the actions of a branch in the background. `None` when the rule already runs and is set
/// to ignore a new start (the history tells it).
pub async fn start(state: &AppState, row: &AutomationRow, rule: &Automation, due: Due, ctx: Context, actor: &str) -> Option<tokio::task::JoinHandle<Vec<StepDone>>> {
    let Due { branch, reason, lasting, .. } = due;
    let runs = state.automations.clone();
    if runs.is_running(row.id) {
        match rule.mode {
            RunMode::Single => {
                let busy = crate::text!("{0} — déjà en cours", "{0} — already running", "{0} — ya en curso", reason.arg());
                if let Err(e) = db::insert_automation_run(&state.db, row.id, "busy", &busy.store(), true).await {
                    tracing::error!("automation {}: {e}", row.id);
                }
                return None;
            }
            RunMode::Restart => {
                let old = runs.running.lock().unwrap_or_else(|e| e.into_inner()).remove(&row.id);
                if let Some((_, handle, run_id)) = old {
                    handle.abort();
                    if let Err(e) = db::finish_automation_run_ended(&state.db, run_id, "cancelled").await {
                        tracing::error!("automation {}: {e}", row.id);
                    }
                }
            }
        }
    }
    let run_id = match db::insert_automation_run(&state.db, row.id, branch.as_str(), &reason.store(), false).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("automation {}: {e}", row.id);
            return None;
        }
    };
    if let Err(e) = db::set_automation_run(&state.db, row.id, db::now(), &reason.store()).await {
        tracing::error!("automation {}: {e}", row.id);
    }
    let place = rule.place(&ctx);
    let steps = if branch == Branch::Else { rule.otherwise.clone() } else { rule.actions.clone() };
    // Only a lasting trigger that started the "then" steps tells when it is over.
    let until = lasting.filter(|_| branch == Branch::Then && !rule.over.is_empty()).and_then(|i| rule.triggers.get(i).cloned());
    let over = rule.over.clone();
    let generation = runs.next.fetch_add(1, Ordering::Relaxed);
    let (state2, row2, actor2) = (state.clone(), row.clone(), actor.to_string());
    // Registered before it starts, so that a run that ends at once removes itself.
    let mut running = runs.running.lock().unwrap_or_else(|e| e.into_inner());
    let task = tokio::spawn(async move {
        let mut p = Progress { run_id, place, out: vec![], touched: vec![] };
        let here = ctx.at(place);
        let ended = match until {
            None => execute(&state2, &row2, &steps, here, &actor2, &mut p).await,
            Some(trigger) => {
                let gone = ended_holding(&state2, &trigger, place);
                tokio::pin!(gone);
                let first = tokio::select! {
                    e = execute(&state2, &row2, &steps, here.clone(), &actor2, &mut p) => Some(e),
                    () = &mut gone => None,
                };
                match first {
                    Some("stopped") => "stopped",
                    finished => {
                        if finished.is_some() {
                            gone.await;
                        }
                        let what = here.describe_trigger(&trigger);
                        let what = if finished.is_some() {
                            crate::text!("ce n'est plus vrai : {0}", "no longer true: {0}", "ya no es cierto: {0}", what.arg())
                        } else {
                            crate::text!(
                                "ce n'est plus vrai : {0} — la suite est arrêtée",
                                "no longer true: {0} — the rest is stopped",
                                "ya no es cierto: {0} — el resto se detiene",
                                what.arg()
                            )
                        };
                        p.out.push(StepDone { what, ok: true, error: None });
                        let now = Context::load(&state2).await.map(|c| c.at(place)).unwrap_or(here);
                        execute(&state2, &row2, &over, now, &actor2, &mut p).await
                    }
                }
            }
        };
        let done = p.out;
        let stored = Value::Array(done.iter().map(StepDone::stored).collect()).to_string();
        if let Err(e) = db::finish_automation_run(&state2.db, run_id, ended, &stored).await {
            tracing::error!("automation {}: {e}", row2.id);
        }
        {
            let mut running = state2.automations.running.lock().unwrap_or_else(|e| e.into_inner());
            if running.get(&row2.id).is_some_and(|(g, _, _)| *g == generation) {
                running.remove(&row2.id);
            }
        }
        log_run(&state2, &row2, branch, &reason, &done, ended, &actor2).await;
        state2.hub.routines_changed();
        done
    });
    running.insert(row.id, (generation, task.abort_handle(), run_id));
    drop(running);
    state.hub.routines_changed();
    Some(task)
}

pub(super) async fn log_run(state: &AppState, row: &AutomationRow, branch: Branch, reason: &Text, done: &[StepDone], ended: &str, actor: &str) {
    let failed = done.iter().filter(|s| !s.ok).count();
    let what = match branch {
        Branch::Else => crate::text!("{0} (sinon)", "{0} (else)", "{0} (si no)", reason.arg()),
        _ => reason.clone(),
    };
    let (level, message) = if failed == 0 && ended == "done" {
        ("info", crate::text!("Automatisme « {0} » : {1}", "Automation “{0}”: {1}", "Automatismo «{0}»: {1}", row.name, what.arg()))
    } else if failed == 0 {
        (
            "info",
            crate::text!(
                "Automatisme « {0} » : {1} — arrêté après une attente",
                "Automation “{0}”: {1} — stopped after a wait",
                "Automatismo «{0}»: {1} — detenido tras una espera",
                row.name,
                what.arg()
            ),
        )
    } else {
        (
            "warn",
            crate::text!(
                "Automatisme « {0} » : {1} — {2} échec(s) sur {3}",
                "Automation “{0}”: {1} — {2} failure(s) out of {3}",
                "Automatismo «{0}»: {1} — {2} fallo(s) de {3}",
                row.name,
                what.arg(),
                failed,
                done.len()
            ),
        )
    };
    let data = json!({ "automation_id": row.id, "results": done });
    state.hub.log("automation", level, None, Some(actor), message, Some(data)).await;
}

/// A device a run drove, and its state before.
pub(super) struct Touched {
    pub(super) device_id: i64,
    pub(super) trait_name: String,
    /// The last action on it, and every setting the run changed.
    pub(super) action: String,
    pub(super) keys: Vec<String>,
    pub(super) before: Map<String, Value>,
    /// The last value the run gave to each setting.
    pub(super) set: Map<String, Value>,
}

/// Settings someone changed since the run set them: neither what the run gave nor what was there
/// before (a device that has not caught up yet, or that was put back by hand, still counts as ours).
pub(super) fn changed_since(t: &Touched, now: &Map<String, Value>) -> Vec<String> {
    let same = |a: &Value, b: &Value| match (a.as_f64(), b.as_f64()) {
        // A setpoint comes back rounded to the device's step (0.5 °C on Airzone).
        (Some(x), Some(y)) => (x - y).abs() < 0.3,
        _ => a == b,
    };
    t.set
        .iter()
        .filter(|(k, v)| match now.get(*k).filter(|n| !n.is_null()) {
            Some(n) => !same(n, v) && t.before.get(*k).is_none_or(|b| !same(n, b)),
            None => false,
        })
        .map(|(k, _)| k.clone())
        .collect()
}

/// The action that puts back `before` after `action` set `keys` (`None`: nothing to put back).
pub(super) fn undo(trait_name: &str, action: &str, keys: &[String], before: &Map<String, Value>) -> Option<(String, Map<String, Value>)> {
    let get = |k: &str| before.get(k).filter(|v| !v.is_null());
    match (trait_name, action) {
        ("ev.charger", "start" | "stop") => Some((if get("status").and_then(Value::as_str) == Some("charging") { "start" } else { "stop" }.into(), Map::new())),
        ("ev.charger", "set_auto_limit") => Some(("set_auto_limit".into(), Map::from_iter([("on".into(), get("auto_limit")?.clone())]))),
        (_, "boost") => (get("boost").and_then(Value::as_bool) == Some(false)).then(|| ("stop_boost".into(), Map::new())),
        (_, "stop" | "stop_boost") => None,
        _ => {
            let params: Map<String, Value> = keys.iter().filter_map(|k| Some((k.clone(), get(k)?.clone()))).collect();
            (!params.is_empty()).then(|| (action.to_string(), params))
        }
    }
}

pub(super) async fn apply(state: &AppState, ctx: &Context, device_id: i64, input: &ActionInput, actor: &str) -> ApiResult<()> {
    let view = ctx.device(device_id).ok_or_else(|| ApiError::not_found("device not found"))?;
    devices::check_action(view, input)?;
    devices::apply(state, view, input, actor).await
}

pub(super) fn done(what: Text, r: ApiResult<()>) -> StepDone {
    StepDone { what, ok: r.is_ok(), error: r.err().map(|e| e.message) }
}

/// Returns once `trigger` no longer holds: past its threshold back by the margin (a measure), or
/// changed (a state, the sun on a façade, presence), for as long as it had to hold to start.
pub(super) async fn ended_holding(state: &AppState, trigger: &Trigger, place: Option<i64>) {
    let mut since: Option<i64> = None;
    loop {
        tokio::time::sleep(POLL).await;
        let Ok(ctx) = Context::load(state).await else { continue };
        let ctx = ctx.at(place);
        // What cannot be read is not an end.
        let Some(Look::Holds { released: true, for_minutes, .. }) = trigger.look(&ctx, ctx.now) else {
            since = None;
            continue;
        };
        if ctx.now - *since.get_or_insert(ctx.now) >= for_minutes * 60 {
            return;
        }
    }
}

/// What a run did so far: kept outside the steps, so that a run cut short (its lasting trigger is
/// over) still tells what it did and puts back what it drove.
pub(super) struct Progress {
    run_id: i64,
    /// The installation the rule is about (its sun, its weather).
    place: Option<i64>,
    out: Vec<StepDone>,
    touched: Vec<Touched>,
}

/// Runs the steps one after the other. Returns how it ended.
pub(super) async fn execute(state: &AppState, row: &AutomationRow, steps: &[Step], mut ctx: Context, actor: &str, p: &mut Progress) -> &'static str {
    let (run_id, place) = (p.run_id, p.place);
    let who = format!("{actor} · automatisme « {} »", row.name);
    let reload = |ctx: Context| async move { Context::load(state).await.map(|c| c.at(place)).unwrap_or(ctx) };
    for step in steps {
        let what = ctx.describe_step(step);
        match step {
            Step::Device { device_id, trait_name, action, params } => {
                let keys: Vec<String> = params.keys().cloned().collect();
                match p.touched.iter_mut().find(|t| t.device_id == *device_id && t.trait_name == *trait_name) {
                    Some(t) => {
                        t.action = action.clone();
                        t.set.extend(params.clone());
                        for k in keys {
                            if !t.keys.contains(&k) {
                                t.keys.push(k);
                            }
                        }
                    }
                    None => p.touched.push(Touched {
                        device_id: *device_id,
                        trait_name: trait_name.clone(),
                        action: action.clone(),
                        keys,
                        before: ctx.device(*device_id).and_then(|d| d.state.as_object().cloned()).unwrap_or_default(),
                        set: params.clone(),
                    }),
                }
                let input = ActionInput { trait_name: trait_name.clone(), action: action.clone(), params: params.clone() };
                p.out.push(done(what, apply(state, &ctx, *device_id, &input, &who).await));
            }
            Step::Scene { routine_id } => {
                let r = async {
                    let row = db::routine_by_id(&state.db, *routine_id).await?.ok_or_else(|| ApiError::not_found("routine not found"))?;
                    let results = crate::routines::run_routine(state, &row, &who).await?;
                    match results.iter().filter(|r| !r.ok).count() {
                        0 => Ok(()),
                        failed => Err(ApiError::bad_gateway(crate::text!("{0} action(s) en échec", "{0} action(s) failed", "{0} acción(es) con fallo", failed))),
                    }
                }
                .await;
                p.out.push(done(what, r));
            }
            Step::Wait { seconds } => {
                tokio::time::sleep(Duration::from_secs(*seconds as u64)).await;
                ctx = reload(ctx).await;
                p.out.push(done(what, Ok(())));
            }
            Step::WaitUntil { condition, for_seconds, timeout_seconds, on_timeout } => {
                let started = tokio::time::Instant::now();
                let deadline = started + Duration::from_secs(*timeout_seconds as u64);
                let mut since: Option<tokio::time::Instant> = None;
                let reached = loop {
                    ctx = reload(ctx).await;
                    let now = tokio::time::Instant::now();
                    if condition.met(&ctx) == Some(true) {
                        let held = *since.get_or_insert(now);
                        if now.duration_since(held).as_secs() as i64 >= *for_seconds {
                            break true;
                        }
                    } else {
                        since = None;
                    }
                    if now >= deadline {
                        break false;
                    }
                    tokio::time::sleep(POLL.min(deadline.saturating_duration_since(now)).max(Duration::from_millis(100))).await;
                };
                if reached {
                    p.out.push(done(what, Ok(())));
                } else {
                    let late = crate::text!("{0} — temps écoulé", "{0} — time is up", "{0} — tiempo agotado", what.arg());
                    p.out.push(StepDone { what: late, ok: true, error: None });
                    if *on_timeout == OnTimeout::Stop {
                        save_steps(state, run_id, &p.out).await;
                        return "stopped";
                    }
                }
            }
            Step::Notify { message } => {
                p.out.push(done(what, notify(state, &row.name, message.trim(), actor).await));
            }
            Step::Restore => {
                p.out.push(done(what, Ok(())));
                ctx = reload(ctx).await;
                for t in p.touched.drain(..).rev() {
                    let Some((action, params)) = undo(&t.trait_name, &t.action, &t.keys, &t.before) else { continue };
                    let name = ctx.device_name(t.device_id);
                    let now = ctx.device(t.device_id).and_then(|d| d.state.as_object().cloned()).unwrap_or_default();
                    if !changed_since(&t, &now).is_empty() {
                        let what = crate::text!(
                            "{0} laissé tel quel : changé entre-temps",
                            "{0} left as it is: changed in the meantime",
                            "{0} se deja como está: cambiado mientras tanto",
                            name.arg()
                        );
                        p.out.push(StepDone { what, ok: true, error: None });
                        continue;
                    }
                    let how = ctx.describe_action(t.device_id, &t.trait_name, &action, &params);
                    let what = crate::text!("remettre {0} : {1}", "put back {0}: {1}", "dejar {0}: {1}", name.arg(), how.arg());
                    let input = ActionInput { trait_name: t.trait_name.clone(), action, params };
                    p.out.push(done(what, apply(state, &ctx, t.device_id, &input, &who).await));
                }
            }
        }
        save_steps(state, run_id, &p.out).await;
    }
    "done"
}

/// Tells a message: in the activity feed (the open apps show it at once) and, when set, to an
/// ntfy topic (a phone notification, with no account and no cloud of ours).
pub async fn notify(state: &AppState, rule: &str, message: &str, actor: &str) -> ApiResult<()> {
    let text = crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", rule, message);
    state.hub.log("notification", "info", None, Some(actor), text, None).await;
    let Some(url) = db::get_setting(&state.db, NTFY).await?.filter(|u| !u.trim().is_empty()) else { return Ok(()) };
    let sent = reqwest::Client::new()
        .post(url.trim())
        // In the address: a header carries no accent.
        .query(&[("title", format!("Breezone · {rule}")), ("tags", "house".into())])
        .timeout(Duration::from_secs(10))
        .body(message.to_string())
        .send()
        .await
        .and_then(|r| r.error_for_status());
    sent.map(|_| ()).map_err(|e| {
        let e = e.without_url().to_string();
        ApiError::bad_gateway(crate::text!("ntfy : {0}", "ntfy: {0}", "ntfy: {0}", e))
    })
}

/// No rule starts by itself until this time (`Some(0)`: until switched back on). A failed read is
/// an error, not "not paused": a holiday pause must not be skipped because the database hiccuped.
pub async fn paused_until(db: &db::Db) -> sqlx::Result<Option<i64>> {
    let until = db::get_setting(db, PAUSED).await?.and_then(|v| v.parse::<i64>().ok());
    Ok(until.filter(|u| *u == 0 || *u > db::now()))
}

/// Keeps the steps done so far, so that the history shows a run that waits.
pub(super) async fn save_steps(state: &AppState, run_id: i64, done: &[StepDone]) {
    let stored = Value::Array(done.iter().map(StepDone::stored).collect()).to_string();
    if let Err(e) = db::set_automation_run_steps(&state.db, run_id, &stored).await {
        tracing::error!("automation run {run_id}: {e}");
    }
}

/// Looks at the rules once: starts those that are due.
pub async fn tick(state: &AppState, engine: &mut Engine) {
    let rows = match db::list_automations(&state.db).await {
        Ok(rows) => rows,
        Err(e) => return tracing::error!("automations: {e}"),
    };
    if !rows.iter().any(|r| r.enabled) {
        engine.watches.clear();
        engine.checked = db::now();
        return;
    }
    // On holiday: nothing starts by itself; what happened meanwhile is not caught up afterwards.
    match paused_until(&state.db).await {
        Ok(None) => {}
        Ok(Some(_)) => {
            engine.checked = db::now();
            return;
        }
        // Unknown: nothing starts this time.
        Err(e) => return tracing::error!("automations: {e}"),
    }
    let ctx = match Context::load(state).await {
        Ok(ctx) => ctx,
        Err(e) => return tracing::error!("automations: {}", e.message.en),
    };
    let rules: Vec<(AutomationRow, Automation)> = rows.into_iter().filter_map(|row| Automation::of(&row).map(|rule| (row, rule))).collect();
    for due in engine.due(&rules, &ctx) {
        let Some((row, rule)) = rules.iter().find(|(row, _)| row.id == due.id) else { continue };
        if due.branch == Branch::Skipped {
            if let Err(e) = db::insert_automation_run(&state.db, row.id, "skipped", &due.reason.store(), true).await {
                tracing::error!("automation {}: {e}", row.id);
            }
            state.hub.routines_changed();
            continue;
        }
        start(state, row, rule, due, ctx.clone(), ACTOR).await;
    }
}

pub async fn run_loop(state: AppState) {
    if let Err(e) = db::interrupt_automation_runs(&state.db).await {
        tracing::error!("automations: {e}");
    }
    let now = db::now();
    // A fixed time missed during a short stop still runs.
    let kept = db::get_setting(&state.db, CHECKED).await.ok().flatten().and_then(|v| v.parse::<i64>().ok());
    let mut engine = Engine::new(kept.unwrap_or(now).clamp(now - CATCH_UP, now));
    let mut saved = now;
    let mut ticker = tokio::time::interval(Duration::from_secs(5));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        tick(&state, &mut engine).await;
        if engine.checked - saved >= 60 {
            saved = engine.checked;
            if let Err(e) = db::set_setting(&state.db, CHECKED, &saved.to_string()).await {
                tracing::error!("automations: {e}");
            }
        }
    }
}
