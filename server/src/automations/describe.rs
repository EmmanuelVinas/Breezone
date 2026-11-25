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

/// A type of device, as the clients name it (`devices::TYPES`).
pub(super) fn type_label(kind: &str) -> Text {
    match kind {
        "climate" => crate::text!("Climatisation", "Air conditioning", "Climatización"),
        "charger" => crate::text!("Recharge", "Charging", "Recarga"),
        "light" => crate::text!("Lumières", "Lights", "Luces"),
        "shutter" => crate::text!("Volets", "Shutters", "Persianas"),
        "window" => crate::text!("Fenêtres", "Windows", "Ventanas"),
        "door" => crate::text!("Portes", "Doors", "Puertas"),
        "ventilation" => crate::text!("Ventilation", "Ventilation", "Ventilación"),
        "room-air" => crate::text!("Ventilation des pièces", "Room ventilation", "Ventilación de las estancias"),
        "switch" => crate::text!("Prises et relais", "Plugs and relays", "Enchufes y relés"),
        "solar" => crate::text!("Solaire", "Solar", "Solar"),
        "energy" => crate::text!("Énergie", "Energy", "Energía"),
        "remote" => crate::text!("Télécommandes", "Remotes", "Mandos"),
        _ => crate::text!("Autres", "Others", "Otros"),
    }
}

/// A shutter sent to a position: "Fermer" at 0 %, "Ouvrir" at 100 %, "Ouvrir à 40 %" between.
pub(super) fn cover_position(position: f64) -> Text {
    if position <= 0.0 {
        crate::text!("Fermer", "Close", "Cerrar")
    } else if position >= 100.0 {
        crate::text!("Ouvrir", "Open", "Abrir")
    } else {
        let at = amount(position, Some("%"));
        crate::text!("Ouvrir à {0}", "Open to {0}", "Abrir al {0}", at)
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

pub(super) const TEMPO_COLOURS: [(&str, &str, &str); 3] = [("bleu", "blue", "azul"), ("blanc", "white", "blanco"), ("rouge", "red", "rojo")];

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
    pub(crate) fn device_name(&self, id: i64) -> Text {
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
            Measure::Price => crate::text!("Prix de l'électricité", "Electricity price", "Precio de la electricidad"),
            Measure::Tempo { day: TempoDay::Today } => crate::text!("Couleur Tempo du jour", "Today's Tempo colour", "Color Tempo de hoy"),
            Measure::Tempo { day: TempoDay::Tomorrow } => crate::text!("Couleur Tempo de demain", "Tomorrow's Tempo colour", "Color Tempo de mañana"),
            Measure::Device { device_id, field } => {
                let (name, field) = (self.device_name(*device_id), self.field_label(*device_id, field));
                crate::text!("{0} · {1}", "{0} · {1}", "{0} · {1}", name.arg(), field.arg())
            }
            Measure::Module { module, field } => self.module_label(module, field),
        }
    }

    /// "RTE · Couleur Tempo de demain": a condition of a module, by the words of its manifest.
    pub(super) fn module_label(&self, module: &str, field: &str) -> Text {
        let name = crate::modules::manifest(module).map_or_else(|| Text::plain(module), |m| label(&m.name));
        let field = crate::modules::conditions::field(module, field).map_or_else(|| Text::plain(field), |f| label(&f.label));
        crate::text!("{0} · {1}", "{0} · {1}", "{0} · {1}", name.arg(), field.arg())
    }

    /// "RTE · Couleur Tempo de demain : rouge".
    pub(super) fn module_state_text(&self, module: &str, field: &str, value: &Value) -> Text {
        let named = crate::modules::conditions::field(module, field).and_then(|f| f.value_labels.get(value.as_str()?)).map(label);
        let value = match (named, value) {
            (Some(named), _) => named,
            (None, Value::Bool(b)) => yes_no(*b),
            (None, other) => Text::plain(other.as_str().map_or_else(|| other.to_string(), str::to_string)),
        };
        let what = self.module_label(module, field);
        crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", what.arg(), value.arg())
    }

    /// A value of a measure, in words: "742 W", "orange".
    pub(super) fn value_text(&self, m: &Measure, v: f64) -> Text {
        match m {
            Measure::Weather { field: WeatherField::Vigilance } => colour(v),
            Measure::Tempo { .. } => named(&TEMPO_COLOURS, v.round().clamp(1.0, 3.0) as u32),
            Measure::Price => {
                let text = format!("{v:.4}");
                let text = text.trim_end_matches('0').trim_end_matches('.');
                Text::plain(format!("{text} {}", self.unit(m).unwrap_or_default()))
            }
            _ => Text::plain(amount(v, self.unit(m).as_deref())),
        }
    }

    /// "Injection au réseau > 500 W"; "Couleur Tempo de demain : rouge".
    pub(super) fn comparison(&self, m: &Measure, op: Op, value: f64) -> Text {
        if let Measure::Tempo { .. } = m {
            let name = self.measure_label(m);
            let which = match (op, value.round() as i64) {
                (Op::Above, 2) => crate::text!("rouge", "red", "rojo"),
                (Op::Above, 1) => crate::text!("blanc ou rouge", "white or red", "blanco o rojo"),
                (Op::Below, 2) => crate::text!("bleu", "blue", "azul"),
                (Op::Below, 3) => crate::text!("bleu ou blanc", "blue or white", "azul o blanco"),
                _ => {
                    let v = self.value_text(m, value);
                    let v = v.arg();
                    crate::text!("{0} {1}", "{0} {1}", "{0} {1}", op.sign(), v)
                }
            };
            let which = which.arg();
            return crate::text!("{0} : {1}", "{0}: {1}", "{0}: {1}", name.arg(), which);
        }
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
            Trigger::Module { module, field, value, for_minutes } => {
                let state = self.module_state_text(module, field, value);
                lasting(crate::text!("{0} devient vrai", "{0} becomes true", "{0} pasa a ser cierto", state.arg()), *for_minutes)
            }
            Trigger::Tariff { profile, slot } => {
                let what = self.tariff_stretch(profile.as_deref(), slot.as_deref());
                crate::text!("quand le tarif passe en {0}", "when the tariff comes into {0}", "cuando la tarifa pasa a {0}", what.arg())
            }
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
            Condition::Module { module, field, value } => self.module_state_text(module, field, value),
            Condition::Tariff { day, profile, slot } => {
                let what = self.tariff_stretch(profile.as_deref(), slot.as_deref());
                match day {
                    TempoDay::Today => crate::text!("le tarif est en {0}", "the tariff is in {0}", "la tarifa está en {0}", what.arg()),
                    TempoDay::Tomorrow => crate::text!("le tarif de demain est en {0}", "tomorrow's tariff is in {0}", "la tarifa de mañana está en {0}", what.arg()),
                }
            }
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

    /// "Jour bleu · HC", "HC", "Jour rouge": a kind of day of the tariff by its name.
    fn tariff_stretch(&self, profile: Option<&str>, slot: Option<&str>) -> Text {
        let kind = profile.map(|id| self.tariff_profiles.iter().find(|(p, _)| p == id).map_or_else(|| id.to_string(), |(_, n)| n.clone()));
        Text::plain(match (kind, slot) {
            (Some(k), Some(s)) => format!("{k} · {s}"),
            (Some(k), None) => k,
            (None, Some(s)) => s.to_string(),
            (None, None) => String::new(),
        })
    }

    pub fn describe_step(&self, s: &Step) -> Text {
        match s {
            Step::Scene { routine_id } => match self.scenes.get(routine_id) {
                Some(name) => crate::text!("scène « {0} »", "scene “{0}”", "escena «{0}»", name),
                None => crate::text!("scène supprimée", "deleted scene", "escena eliminada"),
            },
            Step::Device { device_id, trait_name, action, params } => self.describe_device_action(*device_id, trait_name, action, params),
            Step::Group { device_type, room_id, trait_name, action, params } => {
                let members = self.members(device_type, *room_id, trait_name);
                let what = self.describe_action(members.first().map_or(0, |d| d.id), trait_name, action, params);
                let kind = type_label(device_type);
                let group =
                    match room_id.map(|r| self.rooms.get(&r).map_or_else(|| crate::text!("pièce supprimée", "deleted room", "estancia eliminada"), |n| Text::plain(n.clone()))) {
                        Some(room) => crate::text!("{0} · {1}", "{0} · {1}", "{0} · {1}", kind.arg(), room.arg()),
                        None => crate::text!("{0} · toute la maison", "{0} · whole home", "{0} · toda la casa", kind.arg()),
                    };
                crate::text!("{0} ({1}) : {2}", "{0} ({1}): {2}", "{0} ({1}): {2}", group.arg(), members.len(), what.arg())
            }
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
            Step::Surplus { device_id, enabled } => {
                let name = self.device_name(*device_id);
                if *enabled {
                    crate::text!("{0} : suivre le surplus solaire", "{0}: follow the solar surplus", "{0}: seguir el excedente solar", name.arg())
                } else {
                    crate::text!("{0} : ne plus suivre le surplus solaire", "{0}: no longer follow the solar surplus", "{0}: dejar de seguir el excedente solar", name.arg())
                }
            }
            Step::ChargerProfile { device_id, profile_id, mode, solar } => {
                use crate::charging::Mode;
                let name = self.device_name(*device_id);
                let kw = |k: f64| Text { fr: format!("{k}").replace('.', ","), en: format!("{k}"), es: format!("{k}").replace('.', ",") };
                match Mode::of_step(*mode, *profile_id) {
                    Mode::Profile => match profile_id.map(|id| self.charge_profiles.get(&id)) {
                        Some(Some(p)) => crate::text!("{0} : profil « {1} »", "{0}: profile “{1}”", "{0}: perfil «{1}»", name.arg(), p),
                        Some(None) => crate::text!("{0} : profil supprimé", "{0}: deleted profile", "{0}: perfil eliminado", name.arg()),
                        None => crate::text!("{0} : charger au maximum de la borne", "{0}: charge at the charger's maximum", "{0}: cargar al máximo del cargador", name.arg()),
                    },
                    Mode::Off => crate::text!("{0} : ne pas charger", "{0}: do not charge", "{0}: no cargar", name.arg()),
                    Mode::Free => crate::text!("{0} : laisser la borne faire", "{0}: leave it to the charger", "{0}: dejar al cargador", name.arg()),
                    Mode::Solar => {
                        let s = solar.unwrap_or_default();
                        let how = match (s.fixed, s.kw) {
                            (true, Some(k)) => {
                                let k = kw(k);
                                crate::text!(" à {0} kW", " at {0} kW", " a {0} kW", k.arg())
                            }
                            (_, Some(k)) => {
                                let k = kw(k);
                                crate::text!(" jusqu'à {0} kW", " up to {0} kW", " hasta {0} kW", k.arg())
                            }
                            _ => Text::default(),
                        };
                        let from = match s.start_kw {
                            Some(k) => {
                                let k = kw(k);
                                crate::text!(" dès {0} kW de surplus", " from {0} kW of surplus", " desde {0} kW de excedente", k.arg())
                            }
                            None => Text::default(),
                        };
                        crate::text!(
                            "{0} : charger sur le surplus solaire{1}{2}",
                            "{0}: charge on the solar surplus{1}{2}",
                            "{0}: cargar con el excedente solar{1}{2}",
                            name.arg(),
                            how.arg(),
                            from.arg()
                        )
                    }
                }
            }
            Step::ChargerNow { device_id, kw, until, kwh } => {
                let name = self.device_name(*device_id);
                let power = match kw {
                    Some(k) => {
                        let k = Text { fr: format!("{k}").replace('.', ","), en: format!("{k}"), es: format!("{k}").replace('.', ",") };
                        crate::text!("jusqu'à {0} kW", "up to {0} kW", "hasta {0} kW", k.arg())
                    }
                    None => crate::text!("au maximum", "at the maximum", "al máximo"),
                };
                let end = match (until, kwh) {
                    (Some(t), _) => crate::text!("jusqu'à {0}", "until {0}", "hasta las {0}", t),
                    (None, Some(k)) => {
                        let k = Text { fr: format!("{k}").replace('.', ","), en: format!("{k}"), es: format!("{k}").replace('.', ",") };
                        crate::text!("+{0} kWh", "+{0} kWh", "+{0} kWh", k.arg())
                    }
                    (None, None) => crate::text!("jusqu'au débranchement", "until unplugged", "hasta desconectar"),
                };
                crate::text!("{0} : charger maintenant {1}, {2}", "{0}: charge now {1}, {2}", "{0}: cargar ahora {1}, {2}", name.arg(), power.arg(), end.arg())
            }
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
        if (trait_name, action) == ("cover.position", "set") {
            if let Some(text) = params.get("position").and_then(Value::as_f64).map(cover_position) {
                return text;
            }
        }
        if (trait_name, action) == ("switch.relay", "set") {
            if let (Some(on), 1) = (params.get("on").and_then(Value::as_bool), params.len()) {
                return if on { crate::text!("Allumer", "Switch on", "Encender") } else { crate::text!("Éteindre", "Switch off", "Apagar") };
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
            Trigger::Module { module, field, value, .. } => self.module_state_text(module, field, value),
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

#[cfg(test)]
mod tests {
    use super::cover_position;

    #[test]
    fn a_shutter_closes_opens_or_opens_to_a_position() {
        assert_eq!(cover_position(0.0).fr, "Fermer");
        assert_eq!(cover_position(100.0).en, "Open");
        assert_eq!(cover_position(40.0).fr, "Ouvrir à 40 %");
    }
}
