//! Rules that may set the same device otherwise at the same time.

use super::*;

/// Two triggers may start their rules at the same time.
pub(super) fn coincide(a: &Trigger, b: &Trigger) -> bool {
    let days = |x: &[u32], y: &[u32]| x.is_empty() || y.is_empty() || x.iter().any(|d| y.contains(d));
    match (a, b) {
        (Trigger::Time { time: t1, days: d1 }, Trigger::Time { time: t2, days: d2 }) => t1 == t2 && days(d1, d2),
        (Trigger::Sun { event: e1, offset_minutes: o1, days: d1 }, Trigger::Sun { event: e2, offset_minutes: o2, days: d2 }) => e1 == e2 && o1 == o2 && days(d1, d2),
        (Trigger::Measure { measure: m1, op: o1, .. }, Trigger::Measure { measure: m2, op: o2, .. }) => m1 == m2 && o1 == o2,
        (Trigger::Button { device_id: d1, button: b1, .. }, Trigger::Button { device_id: d2, button: b2, .. }) => d1 == d2 && (b1.is_none() || b2.is_none() || b1 == b2),
        _ => a == b,
    }
}

/// A setting both rules give a device, with different values: (device, setting).
pub(super) fn clash(a: &[Step], b: &[Step]) -> Option<(i64, String)> {
    for x in a {
        let Step::Device { device_id, trait_name, action, params } = x else { continue };
        for y in b {
            let Step::Device { device_id: d2, trait_name: t2, action: a2, params: p2 } = y else { continue };
            if device_id != d2 || trait_name != t2 {
                continue;
            }
            if action != a2 && matches!((action.as_str(), a2.as_str()), ("start", "stop") | ("stop", "start") | ("boost", "stop_boost") | ("stop_boost", "boost")) {
                return Some((*device_id, action.clone()));
            }
            if action == a2 {
                if let Some(k) = params.keys().find(|k| p2.get(*k).is_some_and(|v| Some(v) != params.get(*k))) {
                    return Some((*device_id, k.clone()));
                }
            }
        }
    }
    None
}

/// The conditions a rule needs, all of them (`None`: any one of several would do).
fn required(r: &Automation) -> &[Condition] {
    if r.joined == Match::All || r.conditions.len() == 1 {
        &r.conditions
    } else {
        &[]
    }
}

/// Two conditions that cannot both be true: a choice of the same device set otherwise (« Salon ·
/// Mode : Chaud » and « … : Froid »), a measure above one value and below a lower one, someone home
/// and nobody, day and night, months apart.
fn excludes(a: &Condition, b: &Condition) -> bool {
    let months = |from: u32, to: u32| -> Vec<u32> { (1..=12).filter(|m| if from <= to { (from..=to).contains(m) } else { *m >= from || *m <= to }).collect() };
    match (a, b) {
        (Condition::State { device_id: d1, field: f1, value: v1 }, Condition::State { device_id: d2, field: f2, value: v2 }) => d1 == d2 && f1 == f2 && v1 != v2,
        (Condition::Measure { measure: m1, op: o1, value: v1 }, Condition::Measure { measure: m2, op: o2, value: v2 }) if m1 == m2 => match (o1, o2) {
            (Op::Above, Op::Below) => v2 <= v1,
            (Op::Below, Op::Above) => v1 <= v2,
            _ => false,
        },
        (Condition::Presence { home: h1 }, Condition::Presence { home: h2 }) => h1 != h2,
        (Condition::Daylight { day: d1 }, Condition::Daylight { day: d2 }) => d1 != d2,
        (Condition::Months { from: f1, to: t1 }, Condition::Months { from: f2, to: t2 }) => {
            let (x, y) = (months(*f1, *t1), months(*f2, *t2));
            !x.iter().any(|m| y.contains(m))
        }
        _ => false,
    }
}

/// The two rules never do their actions at the same time: their conditions exclude each other.
fn exclusive(a: &Automation, b: &Automation) -> bool {
    required(a).iter().any(|x| required(b).iter().any(|y| excludes(x, y)))
}

/// A rule that may contradict another one.
#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub id: i64,
    pub name: String,
    /// "Salon (consigne)".
    pub what: Text,
}

/// For each enabled rule, the enabled ones that may set the same device otherwise at the same time.
pub fn conflicts(rules: &[(AutomationRow, Automation)], ctx: &Context) -> HashMap<i64, Vec<Conflict>> {
    let mut out: HashMap<i64, Vec<Conflict>> = HashMap::new();
    let enabled: Vec<&(AutomationRow, Automation)> = rules.iter().filter(|(r, _)| r.enabled).collect();
    for (i, (ra, a)) in enabled.iter().enumerate() {
        for (rb, b) in enabled.iter().skip(i + 1) {
            if !a.triggers.iter().any(|x| b.triggers.iter().any(|y| coincide(x, y))) {
                continue;
            }
            let expand = |steps: Vec<&Step>| ctx.expand(&steps.into_iter().cloned().collect::<Vec<_>>());
            let found = if exclusive(a, b) {
                // When one does its actions, the other can only do what it does otherwise.
                let (main_a, main_b) = (expand(a.actions.iter().chain(&a.over).collect()), expand(b.actions.iter().chain(&b.over).collect()));
                let (else_a, else_b) = (expand(a.otherwise.iter().collect()), expand(b.otherwise.iter().collect()));
                clash(&main_a, &else_b).or_else(|| clash(&else_a, &main_b)).or_else(|| clash(&else_a, &else_b))
            } else {
                clash(&expand(a.steps().collect()), &expand(b.steps().collect()))
            };
            let Some((device, setting)) = found else { continue };
            let name = ctx.device_name(device);
            let setting = ctx.device(device).and_then(|d| {
                d.traits.iter().find_map(|t| {
                    let (n, v) = t.split_once('@')?;
                    crate::traits::get(n, v.parse().ok()?)?.actions.iter().flat_map(|a| &a.params).find(|p| p.name == setting).map(|p| label(&p.label))
                })
            });
            let what = match setting {
                Some(s) => crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", name.arg(), s.arg()),
                None => name,
            };
            out.entry(ra.id).or_default().push(Conflict { id: rb.id, name: rb.name.clone(), what: what.clone() });
            out.entry(rb.id).or_default().push(Conflict { id: ra.id, name: ra.name.clone(), what });
        }
    }
    out
}
