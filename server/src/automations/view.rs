//! What a rule looks like to the clients, and the checks of a rule before it is stored.

use super::*;

/// The rule as a sentence: "Quand {when}, si {conditions}, alors {actions}, sinon {else}".
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// The triggers, joined by "ou".
    pub when: Text,
    pub triggers: Vec<Text>,
    /// One per condition (a group in brackets), to be joined by `joined`.
    pub conditions: Vec<Text>,
    /// "et" or "ou", in the language of the request.
    pub joined: Text,
    pub actions: Vec<Text>,
    #[serde(rename = "else")]
    pub otherwise: Vec<Text>,
    pub over: Vec<Text>,
}

/// What the rule sees now.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// The measure of the first measure trigger, now.
    pub value: Option<f64>,
    pub unit: Option<String>,
    /// That measure is past its threshold.
    pub holds: Option<bool>,
    /// Each condition: true, false, or unknown.
    pub conditions: Vec<Option<bool>>,
    /// Next run of a time or sun trigger.
    pub next_at: Option<i64>,
    /// A run is under way (it waits).
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationView {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    /// The first trigger (for the clients of 0.11.0-beta.18).
    pub when: Trigger,
    pub triggers: Vec<Trigger>,
    #[serde(rename = "match")]
    pub joined: Match,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Step>,
    #[serde(rename = "else")]
    pub otherwise: Vec<Step>,
    pub mode: RunMode,
    pub over: Vec<Step>,
    pub summary: Summary,
    pub status: Status,
    /// Enabled rules that may set the same device otherwise at the same time.
    pub conflicts: Vec<Conflict>,
    pub last_run_at: Option<i64>,
    pub last_reason: Option<Text>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn view(row: &AutomationRow, rule: Automation, ctx: &Context, running: bool) -> AutomationView {
    let measured = rule.triggers.iter().find_map(|t| match t {
        Trigger::Measure { measure, op, value, .. } => Some((measure, op, value)),
        _ => None,
    });
    let (value, unit, holds) = match measured {
        Some((measure, op, threshold)) => {
            let read = ctx.read(measure);
            (read, ctx.unit(measure), read.map(|v| op.holds(v, *threshold)))
        }
        None => (None, None, None),
    };
    AutomationView {
        id: row.id,
        name: row.name.clone(),
        enabled: row.enabled,
        summary: Summary {
            when: ctx.describe_triggers(&rule.triggers),
            triggers: rule.triggers.iter().map(|t| ctx.describe_trigger(t)).collect(),
            conditions: rule.conditions.iter().map(|c| ctx.describe_condition(c)).collect(),
            joined: rule.joined.word(),
            actions: rule.actions.iter().map(|s| ctx.describe_step(s)).collect(),
            otherwise: rule.otherwise.iter().map(|s| ctx.describe_step(s)).collect(),
            over: rule.over.iter().map(|s| ctx.describe_step(s)).collect(),
        },
        status: Status {
            value,
            unit,
            holds,
            conditions: rule.conditions.iter().map(|c| c.met(ctx)).collect(),
            next_at: rule.triggers.iter().filter_map(|t| t.next_at(ctx)).min(),
            running,
        },
        when: rule.triggers[0].clone(),
        triggers: rule.triggers,
        joined: rule.joined,
        conditions: rule.conditions,
        actions: rule.actions,
        otherwise: rule.otherwise,
        mode: rule.mode,
        over: rule.over,
        conflicts: vec![],
        last_run_at: row.last_run_at,
        last_reason: row.last_reason.as_deref().map(Text::load),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub(super) fn check_days(days: &[u32]) -> ApiResult<()> {
    let mut seen = days.to_vec();
    seen.sort_unstable();
    seen.dedup();
    if seen.len() != days.len() || days.iter().any(|d| !(1..=7).contains(d)) {
        return Err(ApiError::bad_request(crate::text!(
            "jours : de 1 (lundi) à 7 (dimanche), chacun une fois",
            "days: 1 (Monday) to 7 (Sunday), each once",
            "días: de 1 (lunes) a 7 (domingo), una vez cada uno"
        )));
    }
    Ok(())
}

pub(super) fn check_time(s: &str) -> ApiResult<()> {
    time_of(s).map(|_| ()).ok_or_else(|| ApiError::bad_request(crate::text!("heure attendue : HH:MM", "time expected: HH:MM", "hora esperada: HH:MM")))
}

pub(super) fn check_lasting(minutes: i64) -> ApiResult<()> {
    if (0..=MAX_FOR_MINUTES).contains(&minutes) {
        Ok(())
    } else {
        Err(ApiError::bad_request(crate::text!("durée entre 0 minute et 24 heures", "duration between 0 minutes and 24 hours", "duración entre 0 minutos y 24 horas")))
    }
}

pub(super) fn check_facing(facing: u32) -> ApiResult<()> {
    if facing < 360 {
        Ok(())
    } else {
        Err(ApiError::bad_request(crate::text!("orientation de 0 à 359°", "facing from 0 to 359°", "orientación de 0 a 359°")))
    }
}

impl Context {
    pub(super) fn known(&self, device_id: i64) -> ApiResult<&DeviceView> {
        self.device(device_id).ok_or_else(|| ApiError::bad_request(crate::text!("appareil introuvable", "unknown device", "dispositivo desconocido")))
    }

    pub(super) fn check_measure(&self, m: &Measure, value: f64) -> ApiResult<()> {
        if !value.is_finite() {
            return Err(ApiError::bad_request(crate::text!("seuil invalide", "invalid threshold", "umbral no válido")));
        }
        if let Measure::Device { device_id, field } = m {
            self.known(*device_id)?;
            if !self.field(*device_id, field).is_some_and(|f| matches!(f.kind.as_str(), "number" | "integer")) {
                return Err(ApiError::bad_request(crate::text!(
                    "« {0} » n'est pas une mesure de cet appareil",
                    "“{0}” is not a measure of this device",
                    "«{0}» no es una medida de este dispositivo",
                    field
                )));
            }
        }
        Ok(())
    }

    pub(super) fn check_state(&self, device_id: i64, field: &str, value: &Value) -> ApiResult<()> {
        self.known(device_id)?;
        let ok = self.field(device_id, field).is_some_and(|f| match f.kind.as_str() {
            "bool" => value.is_boolean(),
            "enum" => value.as_str().is_some_and(|v| f.values.iter().any(|x| x == v)),
            _ => false,
        });
        if ok {
            Ok(())
        } else {
            Err(ApiError::bad_request(crate::text!(
                "« {0} » n'est pas un état de cet appareil",
                "“{0}” is not a state of this device",
                "«{0}» no es un estado de este dispositivo",
                field
            )))
        }
    }

    pub(super) fn check_trigger(&self, t: &Trigger) -> ApiResult<()> {
        match t {
            Trigger::Measure { measure, value, for_minutes, .. } => {
                check_lasting(*for_minutes)?;
                self.check_measure(measure, *value)
            }
            Trigger::State { device_id, field, value, for_minutes } => {
                check_lasting(*for_minutes)?;
                self.check_state(*device_id, field, value)
            }
            Trigger::Time { time, days } => {
                check_time(time)?;
                check_days(days)
            }
            Trigger::Sun { offset_minutes, days, .. } => {
                if offset_minutes.abs() > MAX_SUN_OFFSET {
                    return Err(ApiError::bad_request(crate::text!("décalage de 3 heures au plus", "offset of 3 hours at most", "desfase de 3 horas como máximo")));
                }
                check_days(days)
            }
            Trigger::Facade { facing } => check_facing(*facing),
            Trigger::Presence { .. } => Ok(()),
            Trigger::Button { device_id, button, press } => {
                let device = self.known(*device_id)?;
                if !device.traits.iter().any(|t| t.starts_with("button@")) {
                    return Err(ApiError::bad_request(crate::text!("cet appareil n'a pas de boutons", "this device has no buttons", "este dispositivo no tiene botones")));
                }
                let buttons = device.state.get("buttons").and_then(Value::as_u64).unwrap_or(u64::MAX);
                if button.is_some_and(|b| b == 0 || b as u64 > buttons) || press.as_deref().is_some_and(|p| !PRESSES.contains(&p)) {
                    return Err(ApiError::bad_request(crate::text!("bouton ou appui inconnu", "unknown button or press", "botón o pulsación desconocidos")));
                }
                Ok(())
            }
        }
    }

    pub(super) fn check_condition(&self, c: &Condition) -> ApiResult<()> {
        match c {
            Condition::Measure { measure, value, .. } => self.check_measure(measure, *value),
            Condition::State { device_id, field, value } => self.check_state(*device_id, field, value),
            Condition::Months { from, to } => {
                if (1..=12).contains(from) && (1..=12).contains(to) {
                    Ok(())
                } else {
                    Err(ApiError::bad_request(crate::text!("mois de 1 à 12", "months from 1 to 12", "meses de 1 a 12")))
                }
            }
            Condition::Hours { from, to } => {
                check_time(from)?;
                check_time(to)
            }
            Condition::Days { days } => {
                if days.is_empty() {
                    return Err(ApiError::bad_request(crate::text!("choisissez au moins un jour", "choose at least one day", "elija al menos un día")));
                }
                check_days(days)
            }
            Condition::Daylight { .. } | Condition::Presence { .. } => Ok(()),
            Condition::Facade { facing } => check_facing(*facing),
            Condition::Group { conditions, .. } => {
                if conditions.len() < 2 {
                    return Err(ApiError::bad_request(crate::text!(
                        "un groupe réunit au moins deux conditions",
                        "a group joins at least two conditions",
                        "un grupo reúne al menos dos condiciones"
                    )));
                }
                if conditions.iter().any(|c| matches!(c, Condition::Group { .. })) {
                    return Err(ApiError::bad_request(crate::text!("pas de groupe dans un groupe", "no group within a group", "ningún grupo dentro de un grupo")));
                }
                conditions.iter().try_for_each(|c| self.check_condition(c))
            }
        }
    }

    pub(super) fn check_wait(seconds: i64, min: i64) -> ApiResult<()> {
        if (min..=MAX_WAIT).contains(&seconds) {
            Ok(())
        } else {
            Err(ApiError::bad_request(crate::text!("attente de 24 heures au plus", "a wait of 24 hours at most", "espera de 24 horas como máximo")))
        }
    }

    pub(super) fn check_step(&self, s: &Step) -> ApiResult<()> {
        match s {
            Step::Scene { routine_id } => {
                if self.scenes.contains_key(routine_id) {
                    Ok(())
                } else {
                    Err(ApiError::bad_request(crate::text!("scène introuvable", "unknown scene", "escena desconocida")))
                }
            }
            Step::Device { device_id, trait_name, action, params } => {
                let device = self.known(*device_id)?;
                devices::check_action(device, &ActionInput { trait_name: trait_name.clone(), action: action.clone(), params: params.clone() })?;
                if (trait_name.as_str(), action.as_str()) == ("climate.zone", "set") && ZoneCommand::from_value_strict(Value::Object(params.clone()))?.is_empty() {
                    return Err(ApiError::bad_request("every action must change at least one setting"));
                }
                Ok(())
            }
            Step::Wait { seconds } => Self::check_wait(*seconds, 1),
            Step::WaitUntil { condition, for_seconds, timeout_seconds, .. } => {
                Self::check_wait(*timeout_seconds, 1)?;
                Self::check_wait(*for_seconds, 0)?;
                self.check_condition(condition)
            }
            Step::Restore => Ok(()),
            Step::Notify { message } => {
                let n = message.trim().chars().count();
                if n == 0 || n > MAX_MESSAGE {
                    return Err(ApiError::bad_request(crate::text!(
                        "message de 1 à {0} caractères",
                        "a message of 1 to {0} characters",
                        "mensaje de 1 a {0} caracteres",
                        MAX_MESSAGE
                    )));
                }
                Ok(())
            }
        }
    }

    /// `before`: the steps run before these in the same run (what "put back" can undo).
    pub(super) fn check_branch(&self, steps: &[Step], required: bool, before: &[Step]) -> ApiResult<()> {
        if steps.len() > MAX_ACTIONS || (required && steps.is_empty()) {
            return Err(ApiError::bad_request(crate::text!("de 1 à {0} actions", "1 to {0} actions", "de 1 a {0} acciones", MAX_ACTIONS)));
        }
        for (i, s) in steps.iter().enumerate() {
            self.check_step(s)?;
            if *s == Step::Restore && !before.iter().chain(&steps[..i]).any(|p| matches!(p, Step::Device { .. })) {
                return Err(ApiError::bad_request(crate::text!(
                    "« remettre comme avant » vient après une action sur un appareil",
                    "“put back as before” comes after an action on a device",
                    "«dejar como antes» va después de una acción sobre un dispositivo"
                )));
            }
        }
        Ok(())
    }
}

/// Checks a rule against the devices of the home; returns its name and the rule to store.
pub fn validate(input: AutomationInput, ctx: &Context) -> ApiResult<(String, Automation)> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(ApiError::bad_request("name must be 1-80 characters"));
    }
    let triggers: Vec<Trigger> = input.when.into_iter().chain(input.triggers).collect();
    if triggers.is_empty() || triggers.len() > MAX_TRIGGERS {
        return Err(ApiError::bad_request(crate::text!("de 1 à {0} déclencheurs", "1 to {0} triggers", "de 1 a {0} disparadores", MAX_TRIGGERS)));
    }
    let leaves: usize = input.conditions.iter().map(|c| if let Condition::Group { conditions, .. } = c { conditions.len() } else { 1 }).sum();
    if leaves > MAX_CONDITIONS {
        return Err(ApiError::bad_request(crate::text!("{0} conditions au plus", "{0} conditions at most", "{0} condiciones como máximo", MAX_CONDITIONS)));
    }
    triggers.iter().try_for_each(|t| ctx.check_trigger(t))?;
    input.conditions.iter().try_for_each(|c| ctx.check_condition(c))?;
    ctx.check_branch(&input.actions, true, &[])?;
    ctx.check_branch(&input.otherwise, false, &[])?;
    ctx.check_branch(&input.over, false, &input.actions)?;
    if !input.over.is_empty() && !triggers.iter().any(Trigger::lasts) {
        return Err(ApiError::bad_request(crate::text!(
            "« quand ce n'est plus vrai » demande un déclencheur qui dure : une mesure, un état, le soleil sur une façade ou la présence",
            "“when it is no longer true” needs a trigger that lasts: a measure, a state, the sun on a façade or presence",
            "«cuando deja de ser cierto» necesita un disparador que dure: una medida, un estado, el sol en una fachada o la presencia"
        )));
    }
    let rule = Automation { triggers, joined: input.joined, conditions: input.conditions, actions: input.actions, otherwise: input.otherwise, mode: input.mode, over: input.over };
    Ok((name, rule))
}
