//! The words of the rules: how a trigger, a condition or a step reads, in every language.

use super::*;

pub(super) fn label(l: &Label) -> Text {
    Text { fr: l.fr.clone(), en: l.en.clone(), es: l.es.clone() }
}

pub(super) fn num(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0 + 0.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r}")
    }
}

pub(super) fn amount(v: f64, unit: Option<&str>) -> String {
    match unit {
        Some(u) => format!("{} {u}", num(v)),
        None => num(v),
    }
}

pub(super) fn yes_no(b: bool) -> Text {
    if b {
        crate::text!("oui", "yes", "sí")
    } else {
        crate::text!("non", "no", "no")
    }
}

/// "45 s", "10 min", "2 h", "1 h 30".
pub(super) fn duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        crate::timers::human_delay(seconds / 60)
    }
}

pub(super) const MONTHS: [(&str, &str, &str); 12] = [
    ("janvier", "January", "enero"),
    ("février", "February", "febrero"),
    ("mars", "March", "marzo"),
    ("avril", "April", "abril"),
    ("mai", "May", "mayo"),
    ("juin", "June", "junio"),
    ("juillet", "July", "julio"),
    ("août", "August", "agosto"),
    ("septembre", "September", "septiembre"),
    ("octobre", "October", "octubre"),
    ("novembre", "November", "noviembre"),
    ("décembre", "December", "diciembre"),
];

pub(super) const DAYS: [(&str, &str, &str); 7] =
    [("lun", "Mon", "lun"), ("mar", "Tue", "mar"), ("mer", "Wed", "mié"), ("jeu", "Thu", "jue"), ("ven", "Fri", "vie"), ("sam", "Sat", "sáb"), ("dim", "Sun", "dom")];

/// The eight directions a façade faces, from the north.
pub(super) const FACINGS: [(&str, &str, &str); 8] = [
    ("nord", "north", "norte"),
    ("nord-est", "north-east", "noreste"),
    ("est", "east", "este"),
    ("sud-est", "south-east", "sureste"),
    ("sud", "south", "sur"),
    ("sud-ouest", "south-west", "suroeste"),
    ("ouest", "west", "oeste"),
    ("nord-ouest", "north-west", "noroeste"),
];

/// The colours of the weather warnings, 0 to 3.
pub(super) const VIGILANCE: [(&str, &str, &str); 4] = [("verte", "green", "verde"), ("jaune", "yellow", "amarilla"), ("orange", "orange", "naranja"), ("rouge", "red", "roja")];

pub(super) fn colour(level: f64) -> Text {
    named(&VIGILANCE, (level.round().clamp(0.0, 3.0) as u32) + 1)
}

pub(super) fn press_name(press: &str) -> Text {
    match press {
        "double" => crate::text!("double appui", "double press", "doble pulsación"),
        "long" => crate::text!("appui long", "long press", "pulsación larga"),
        _ => crate::text!("appui", "press", "pulsación"),
    }
}

pub(super) fn named(table: &[(&str, &str, &str)], n: u32) -> Text {
    let (fr, en, es) = table.get((n as usize).wrapping_sub(1)).copied().unwrap_or(("?", "?", "?"));
    Text { fr: fr.into(), en: en.into(), es: es.into() }
}

pub(super) fn day_list(days: &[u32]) -> Text {
    let mut sorted = days.to_vec();
    sorted.sort_unstable();
    join(&sorted.iter().map(|d| named(&DAYS, *d)).collect::<Vec<_>>(), ", ")
}

pub(super) fn facing(degrees: u32) -> Text {
    named(&FACINGS, ((degrees as f64 / 45.0).round() as u32) % 8 + 1)
}

impl Context {
    pub(super) fn device_name(&self, id: i64) -> Text {
        match self.device(id) {
            Some(d) => Text::plain(d.name.clone()),
            None => crate::text!("appareil {0} (supprimé)", "device {0} (deleted)", "dispositivo {0} (eliminado)", id),
        }
    }

    pub(super) fn field_label(&self, device_id: i64, field: &str) -> Text {
        self.field(device_id, field).map_or_else(|| Text::plain(field), |f| label(&f.label))
    }

    pub(super) fn measure_label(&self, m: &Measure) -> Text {
        match m {
            Measure::Energy { flow: Flow::Export } => crate::text!("Injection au réseau", "Grid export", "Inyección a la red"),
            Measure::Energy { flow: Flow::Import } => crate::text!("Soutirage du réseau", "Grid import", "Consumo de la red"),
            Measure::Energy { flow: Flow::Solar } => crate::text!("Production solaire", "Solar production", "Producción solar"),
            Measure::Energy { flow: Flow::Home } => crate::text!("Consommation de la maison", "Home consumption", "Consumo de la casa"),
            Measure::Outdoor => crate::text!("Température extérieure", "Outdoor temperature", "Temperatura exterior"),
            Measure::Weather { field } => match field {
                WeatherField::Precipitation => crate::text!("Pluie", "Rain", "Lluvia"),
                WeatherField::Wind => crate::text!("Vent", "Wind", "Viento"),
                WeatherField::RainNextHour => crate::text!("Risque de pluie dans l'heure", "Chance of rain within the hour", "Probabilidad de lluvia en la próxima hora"),
                WeatherField::TomorrowMin => crate::text!("Minimale de demain", "Tomorrow's low", "Mínima de mañana"),
                WeatherField::TomorrowMax => crate::text!("Maximale de demain", "Tomorrow's high", "Máxima de mañana"),
                WeatherField::Vigilance => crate::text!("Vigilance météo", "Weather warning", "Aviso meteorológico"),
                WeatherField::Humidity => crate::text!("Humidité extérieure", "Outdoor humidity", "Humedad exterior"),
            },
            Measure::Device { device_id, field } => {
                let (name, field) = (self.device_name(*device_id), self.field_label(*device_id, field));
                crate::text!("{0} · {1}", "{0} · {1}", "{0} · {1}", name.arg(), field.arg())
            }
        }
    }

    /// A value of a measure, in words: "742 W", "orange".
    pub(super) fn value_text(&self, m: &Measure, v: f64) -> Text {
        match m {
            Measure::Weather { field: WeatherField::Vigilance } => colour(v),
            _ => Text::plain(amount(v, self.unit(m).as_deref())),
        }
    }

    /// "Injection au réseau > 500 W".
    pub(super) fn comparison(&self, m: &Measure, op: Op, value: f64) -> Text {
        let (name, threshold) = (self.measure_label(m), self.value_text(m, value));
        let threshold = threshold.arg();
        crate::text!("{0} {1} {2}", "{0} {1} {2}", "{0} {1} {2}", name.arg(), op.sign(), threshold)
    }

    /// "Borne · État : En charge".
    pub(super) fn state_text(&self, device_id: i64, field: &str, value: &Value) -> Text {
        let named = self.field(device_id, field).and_then(|f| f.value_labels.get(value.as_str()?)).map(label);
        let (name, field) = (self.device_name(device_id), self.field_label(device_id, field));
        let value = match (named, value) {
            (Some(named), _) => named,
            (None, Value::Bool(b)) => yes_no(*b),
            (None, Value::String(s)) => Text::plain(s.clone()),
            (None, other) => Text::plain(other.to_string()),
        };
        crate::text!("{0} · {1} : {2}", "{0} · {1}: {2}", "{0} · {1}: {2}", name.arg(), field.arg(), value.arg())
    }

    pub fn describe_trigger(&self, t: &Trigger) -> Text {
        let with_days = |text: Text, days: &[u32]| {
            if days.is_empty() || days.len() == 7 {
                return text;
            }
            let days = day_list(days);
            crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", text.arg(), days.arg())
        };
        let lasting = |text: Text, minutes: i64| {
            if minutes > 0 {
                crate::text!("{0} pendant {1} min", "{0} for {1} min", "{0} durante {1} min", text.arg(), minutes)
            } else {
                text
            }
        };
        match t {
            Trigger::Measure { measure, op, value, for_minutes } => lasting(self.comparison(measure, *op, *value), *for_minutes),
            Trigger::State { device_id, field, value, for_minutes } => {
                let state = self.state_text(*device_id, field, value);
                lasting(crate::text!("{0} devient vrai", "{0} becomes true", "{0} pasa a ser cierto", state.arg()), *for_minutes)
            }
            Trigger::Time { time, days } => with_days(crate::text!("à {0}", "at {0}", "a las {0}", time), days),
            Trigger::Sun { event, offset_minutes, days } => {
                // "au lever du soleil", "30 min avant le lever du soleil"; "al amanecer", "antes del amanecer".
                let (at, of) = match event {
                    SunEvent::Sunrise => (crate::text!("au lever du soleil", "at sunrise", "al amanecer"), crate::text!("le lever du soleil", "sunrise", "del amanecer")),
                    SunEvent::Sunset => (crate::text!("au coucher du soleil", "at sunset", "al atardecer"), crate::text!("le coucher du soleil", "sunset", "del atardecer")),
                    SunEvent::Dawn => (crate::text!("à l'aube", "at dawn", "al alba"), crate::text!("l'aube", "dawn", "del alba")),
                    SunEvent::Dusk => (crate::text!("au crépuscule", "at dusk", "al anochecer"), crate::text!("le crépuscule", "dusk", "del anochecer")),
                };
                let n = offset_minutes.abs();
                let text = match offset_minutes.signum() {
                    0 => at,
                    -1 => crate::text!("{0} min avant {1}", "{0} min before {1}", "{0} min antes {1}", n, of.arg()),
                    _ => crate::text!("{0} min après {1}", "{0} min after {1}", "{0} min después {1}", n, of.arg()),
                };
                with_days(text, days)
            }
            Trigger::Facade { facing: f } => {
                let f = facing(*f);
                crate::text!("quand le soleil arrive sur la façade {0}", "when the sun reaches the {0} façade", "cuando el sol llega a la fachada {0}", f.arg())
            }
            Trigger::Presence { home: true } => crate::text!("quand le premier arrive à la maison", "when the first one gets home", "cuando llega el primero a casa"),
            Trigger::Presence { home: false } => crate::text!("quand le dernier part de la maison", "when the last one leaves home", "cuando se va el último de casa"),
            Trigger::Button { device_id, button, press } => {
                let name = self.device_name(*device_id);
                let how = press_name(press.as_deref().unwrap_or("single"));
                match (button, press) {
                    (Some(b), Some(_)) => crate::text!("{0} : bouton {1}, {2}", "{0}: button {1}, {2}", "{0}: botón {1}, {2}", name.arg(), b, how.arg()),
                    (Some(b), None) => crate::text!("{0} : bouton {1}", "{0}: button {1}", "{0}: botón {1}", name.arg(), b),
                    (None, Some(_)) => crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", name.arg(), how.arg()),
                    (None, None) => crate::text!("{0} : un bouton", "{0}: a button", "{0}: un botón", name.arg()),
                }
            }
        }
    }

    /// The triggers, joined by "ou".
    pub fn describe_triggers(&self, triggers: &[Trigger]) -> Text {
        let parts: Vec<Text> = triggers.iter().map(|t| self.describe_trigger(t)).collect();
        let word = Match::Any.word();
        Text::build(|l| parts.iter().map(|p| p.get(l)).collect::<Vec<_>>().join(&format!(" {} ", word.get(l))))
    }

    pub fn describe_condition(&self, c: &Condition) -> Text {
        match c {
            Condition::Measure { measure, op, value } => self.comparison(measure, *op, *value),
            Condition::State { device_id, field, value } => self.state_text(*device_id, field, value),
            Condition::Months { from, to } if from == to => {
                let m = named(&MONTHS, *from);
                crate::text!("en {0}", "in {0}", "en {0}", m.arg())
            }
            Condition::Months { from, to } => {
                let (a, b) = (named(&MONTHS, *from), named(&MONTHS, *to));
                crate::text!("de {0} à {1}", "from {0} to {1}", "de {0} a {1}", a.arg(), b.arg())
            }
            Condition::Hours { from, to } => crate::text!("entre {0} et {1}", "between {0} and {1}", "entre las {0} y las {1}", from, to),
            Condition::Days { days } => day_list(days),
            Condition::Daylight { day: true } => crate::text!("de jour", "during the day", "de día"),
            Condition::Daylight { day: false } => crate::text!("de nuit", "at night", "de noche"),
            Condition::Facade { facing: f } => {
                let f = facing(*f);
                crate::text!("le soleil est sur la façade {0}", "the sun is on the {0} façade", "el sol da en la fachada {0}", f.arg())
            }
            Condition::Presence { home: true } => crate::text!("quelqu'un est à la maison", "someone is home", "hay alguien en casa"),
            Condition::Presence { home: false } => crate::text!("personne n'est à la maison", "nobody is home", "no hay nadie en casa"),
            Condition::Group { joined, conditions } => {
                let inner = self.describe_conditions(*joined, conditions);
                crate::text!("({0})", "({0})", "({0})", inner.arg())
            }
        }
    }

    /// "A et B", "A ou B ou C".
    pub fn describe_conditions(&self, joined: Match, conditions: &[Condition]) -> Text {
        let word = joined.word();
        let parts: Vec<Text> = conditions.iter().map(|c| self.describe_condition(c)).collect();
        Text::build(|l| parts.iter().map(|p| p.get(l)).collect::<Vec<_>>().join(&format!(" {} ", word.get(l))))
    }

    pub fn describe_step(&self, s: &Step) -> Text {
        match s {
            Step::Scene { routine_id } => match self.scenes.get(routine_id) {
                Some(name) => crate::text!("scène « {0} »", "scene “{0}”", "escena «{0}»", name),
                None => crate::text!("scène supprimée", "deleted scene", "escena eliminada"),
            },
            Step::Device { device_id, trait_name, action, params } => self.describe_device_action(*device_id, trait_name, action, params),
            Step::Wait { seconds } => crate::text!("attendre {0}", "wait {0}", "esperar {0}", duration(*seconds)),
            Step::WaitUntil { condition, for_seconds, timeout_seconds, on_timeout } => {
                let what = self.describe_condition(condition);
                let what = if *for_seconds > 0 { crate::text!("{0} pendant {1}", "{0} for {1}", "{0} durante {1}", what.arg(), duration(*for_seconds)) } else { what };
                let max = duration(*timeout_seconds);
                match on_timeout {
                    OnTimeout::Continue => crate::text!("attendre que {0} (au plus {1})", "wait until {0} ({1} at most)", "esperar a que {0} (como máximo {1})", what.arg(), max),
                    OnTimeout::Stop => crate::text!(
                        "attendre que {0} (au plus {1}, sinon s'arrêter)",
                        "wait until {0} ({1} at most, else stop)",
                        "esperar a que {0} (como máximo {1}; si no, parar)",
                        what.arg(),
                        max
                    ),
                }
            }
            Step::Restore => crate::text!("remettre comme avant", "put back as before", "dejar como antes"),
            Step::Notify { message } => crate::text!("prévenir : « {0} »", "notify: “{0}”", "avisar: «{0}»", message),
        }
    }

    /// "Salon : allumer, 21°".
    pub(crate) fn describe_device_action(&self, device_id: i64, trait_name: &str, action: &str, params: &Map<String, Value>) -> Text {
        let (name, what) = (self.device_name(device_id), self.describe_action(device_id, trait_name, action, params));
        crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", name.arg(), what.arg())
    }

    pub(super) fn describe_action(&self, device_id: i64, trait_name: &str, action: &str, params: &Map<String, Value>) -> Text {
        if (trait_name, action) == ("climate.zone", "set") {
            if let Ok(cmd) = serde_json::from_value::<ZoneCommand>(Value::Object(params.clone())) {
                return cmd.describe();
            }
        }
        let described = self.device(device_id).and_then(|d| {
            let version = d.traits.iter().find_map(|t| t.split_once('@').filter(|(n, _)| *n == trait_name)?.1.parse().ok())?;
            crate::traits::get(trait_name, version)?.action(action)
        });
        let Some(a) = described else { return Text::plain(format!("{trait_name}.{action}")) };
        let given: Vec<Text> = a
            .params
            .iter()
            .filter_map(|p| {
                let value = match params.get(&p.name)? {
                    Value::Null => return None,
                    Value::Bool(b) => yes_no(*b),
                    Value::String(s) => Text::plain(s.clone()),
                    other => Text::plain(other.as_f64().map_or_else(|| other.to_string(), |v| amount(v, p.unit.as_deref()))),
                };
                let name = label(&p.label);
                Some(crate::text!("{0} {1}", "{0} {1}", "{0} {1}", name.arg(), value.arg()))
            })
            .collect();
        let name = label(&a.label);
        if given.is_empty() {
            return name;
        }
        let given = join(&given, ", ");
        crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", name.arg(), given.arg())
    }

    /// What was true when a trigger started the rule: "Injection au réseau : 742 W depuis 5 min".
    pub(super) fn reason(&self, t: &Trigger) -> Text {
        match t {
            Trigger::Measure { measure, for_minutes, .. } => {
                let name = self.measure_label(measure);
                let mut value = self.read(measure).map_or_else(|| Text::plain("?"), |v| self.value_text(measure, v));
                if let (Measure::Weather { field: WeatherField::Vigilance }, Some(v)) = (measure, self.weather.as_ref().and_then(|w| w.vigilance.as_ref())) {
                    if !v.events.is_empty() {
                        let events = v.events.join(", ");
                        value = crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", value.arg(), events);
                    }
                }
                let value = value.arg();
                if *for_minutes > 0 {
                    crate::text!("{0} : {1} depuis {2} min", "{0}: {1} for {2} min", "{0}: {1} desde hace {2} min", name.arg(), value, for_minutes)
                } else {
                    crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", name.arg(), value)
                }
            }
            Trigger::State { device_id, field, value, .. } => self.state_text(*device_id, field, value),
            other => self.describe_trigger(other),
        }
    }

    /// The conditions that are not true now, with what they read: "Salon · Température < 18 °C
    /// (19,1 °C)".
    pub(super) fn why_not(&self, rule: &Automation) -> Text {
        let parts: Vec<Text> = rule
            .conditions
            .iter()
            .filter_map(|c| {
                let met = c.met(self);
                if met == Some(true) {
                    return None;
                }
                let text = self.describe_condition(c);
                Some(match (met, c) {
                    (None, _) => crate::text!("{0} : inconnu", "{0}: unknown", "{0}: desconocido", text.arg()),
                    (_, Condition::Measure { measure, .. }) => match self.read(measure) {
                        Some(v) => {
                            let now = self.value_text(measure, v);
                            crate::text!("{0} ({1})", "{0} ({1})", "{0} ({1})", text.arg(), now.arg())
                        }
                        None => text,
                    },
                    _ => text,
                })
            })
            .collect();
        let list = join(&parts, ", ");
        crate::text!("conditions non remplies : {0}", "conditions not met: {0}", "condiciones no cumplidas: {0}", list.arg())
    }
}
