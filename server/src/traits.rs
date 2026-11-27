//! Catalog of device traits (ADR 0013): the data contract of each kind of device, one JSON file
//! per trait in `contracts/traits/`. Embedded in the server, served at `GET /api/traits`: clients
//! and other nodes that do not know a trait render it from this description (ADR 0012). The web
//! and mobile types are generated from the same files.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Every trait file, embedded at build time (the tests check that none is forgotten).
const FILES: &[(&str, &str)] = &[
    ("button.json", include_str!("../contracts/traits/button.json")),
    ("camera.json", include_str!("../contracts/traits/camera.json")),
    ("climate.zone.json", include_str!("../contracts/traits/climate.zone.json")),
    ("cover.position.json", include_str!("../contracts/traits/cover.position.json")),
    ("energy.battery.json", include_str!("../contracts/traits/energy.battery.json")),
    ("energy.meter.json", include_str!("../contracts/traits/energy.meter.json")),
    ("ev.charger.json", include_str!("../contracts/traits/ev.charger.json")),
    ("light.json", include_str!("../contracts/traits/light.json")),
    ("solar.inverter.json", include_str!("../contracts/traits/solar.inverter.json")),
    ("solar.panel.json", include_str!("../contracts/traits/solar.panel.json")),
    ("energy.power.json", include_str!("../contracts/traits/energy.power.json")),
    ("environment.sensor.json", include_str!("../contracts/traits/environment.sensor.json")),
    ("generic.button.json", include_str!("../contracts/traits/generic.button.json")),
    ("generic.number.json", include_str!("../contracts/traits/generic.number.json")),
    ("generic.select.json", include_str!("../contracts/traits/generic.select.json")),
    ("generic.sensor.json", include_str!("../contracts/traits/generic.sensor.json")),
    ("generic.switch.json", include_str!("../contracts/traits/generic.switch.json")),
    ("solar.forecast.json", include_str!("../contracts/traits/solar.forecast.json")),
    ("switch.relay.json", include_str!("../contracts/traits/switch.relay.json")),
    ("ventilation.room.json", include_str!("../contracts/traits/ventilation.room.json")),
    ("ventilation.unit.json", include_str!("../contracts/traits/ventilation.unit.json")),
    ("zigbee.bridge.json", include_str!("../contracts/traits/zigbee.bridge.json")),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub fr: String,
    pub en: String,
    pub es: String,
}

/// A value of the state, or a parameter of an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    /// "bool" | "integer" | "number" | "string" | "enum" | "list" (of strings)
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// The words for each value of an enum.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub value_labels: BTreeMap<String, Label>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// The fields of the device's state that hold, for this one, what only the hardware knows (a
    /// setting of a generic trait): its unit, its bounds, its step, the values it may take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values_from: Option<String>,
    #[serde(default)]
    pub optional: bool,
    /// Worth a history (a measure): false for a setting or a fixed value (`"history": false`),
    /// which is then not written every few minutes.
    #[serde(default = "crate::yes", skip_serializing_if = "is_true")]
    pub history: bool,
    pub label: Label,
}

fn is_true(b: &bool) -> bool {
    *b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub name: String,
    pub label: Label,
    #[serde(default)]
    pub params: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trait {
    #[serde(rename = "trait")]
    pub name: String,
    /// Major version: a new one for an incompatible change, in a file of its own.
    pub version: u32,
    /// Additions made since the major version came (optional fields, actions, parameters): a
    /// client or a module that needs one checks the revision a server announces.
    #[serde(default)]
    pub revision: u32,
    /// The revision that added each field or action, when it is not the first one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub since: BTreeMap<String, u32>,
    pub label: Label,
    #[serde(default)]
    pub state: Vec<Field>,
    #[serde(default)]
    pub actions: Vec<Action>,
    /// The values the device can forecast along the coming hours (the power the panels should
    /// give): served as curves at `GET /api/devices/{id}/forecast`, not kept in the state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forecast: Vec<Field>,
    /// How a client that does not know this trait shows it (words of `contracts/ui.json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<Ui>,
}

/// The presentation of a trait.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ui {
    /// The kind of device it makes (`shutter`…): its place in lists and its page.
    pub kind: String,
    /// The field that sums the device up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub card: Vec<Widget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<Widget>,
}

/// A piece of a card or of a detail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    pub widget: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// The parameter of the action the widget sets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// Fixed parameters of the action (a button « Ouvrir » sends `{"position": 100}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Map<String, serde_json::Value>>,
    /// A bool field: the widget is shown only while it is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// A chart of the forecast of the field rather than of its history.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forecast: bool,
}

/// A kind of device of `contracts/ui.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kind {
    pub id: String,
    pub icon: String,
    /// The page of the clients that handles it, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    pub label: Label,
}

/// The words of the presentation (`contracts/ui.json`), served at `GET /api/ui`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vocabulary {
    pub kinds: Vec<Kind>,
    pub widgets: BTreeMap<String, String>,
    pub icons: Vec<String>,
}

static VOCABULARY: LazyLock<Vocabulary> = LazyLock::new(|| serde_json::from_str(include_str!("../contracts/ui.json")).unwrap_or_else(|e| panic!("contracts/ui.json: {e}")));

pub fn vocabulary() -> &'static Vocabulary {
    &VOCABULARY
}

impl Trait {
    /// `name@version`, as announced in the capabilities.
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }

    pub fn action(&self, name: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.name == name)
    }
}

static CATALOG: LazyLock<Vec<Trait>> =
    LazyLock::new(|| FILES.iter().map(|(file, json)| serde_json::from_str(json).unwrap_or_else(|e| panic!("contracts/traits/{file}: {e}"))).collect());

/// All the traits this server knows.
pub fn catalog() -> &'static [Trait] {
    &CATALOG
}

pub fn get(name: &str, version: u32) -> Option<&'static Trait> {
    catalog().iter().find(|t| t.name == name && t.version == version)
}

/// A field worth a history: a measure, not a setting or a fixed value of the hardware.
pub fn keeps_history(field: &str) -> bool {
    !catalog().iter().flat_map(|t| &t.state).any(|f| f.name == field && !f.history)
}

/// A field that counts up (an energy in kWh): its history keeps the last value, not the mean.
pub fn is_counter(field: &str) -> bool {
    catalog().iter().flat_map(|t| &t.state).any(|f| f.name == field && f.unit.as_deref() == Some("kWh"))
}

#[cfg(test)]
const KINDS: &[&str] = &["bool", "integer", "number", "string", "enum", "list"];

/// Checks a field of a contract (traits and services), for the tests.
#[cfg(test)]
pub(crate) fn check_field(t: &str, f: &Field) {
    assert!(KINDS.contains(&f.kind.as_str()), "{t}.{}: unknown type {}", f.name, f.kind);
    assert_eq!(f.kind == "enum", !f.values.is_empty(), "{t}.{}: an enum lists its values, and only an enum", f.name);
    let named: Vec<&String> = f.value_labels.keys().collect();
    let mut values: Vec<&String> = f.values.iter().collect();
    values.sort();
    assert_eq!(named, values, "{t}.{}: one label per value, in value_labels", f.name);
    for l in f.value_labels.values() {
        assert!(![&l.fr, &l.en, &l.es].iter().any(|s| s.trim().is_empty()), "{t}.{}: a value label is missing", f.name);
    }
    for l in [&f.label.fr, &f.label.en, &f.label.es] {
        assert!(!l.trim().is_empty(), "{t}.{}: a label is missing", f.name);
    }
    if let (Some(min), Some(max)) = (f.min, f.max) {
        assert!(min <= max, "{t}.{}: min > max", f.name);
    }
    assert!(f.kind != "list" || !f.history, "{t}.{}: a list has no history", f.name);
}

/// The fields a field takes its unit, bounds, step or values from: (which, the field named).
#[cfg(test)]
fn sources(f: &Field) -> Vec<(&'static str, &String)> {
    [("unit_from", &f.unit_from), ("min_from", &f.min_from), ("max_from", &f.max_from), ("step_from", &f.step_from), ("values_from", &f.values_from)]
        .into_iter()
        .filter_map(|(k, v)| Some((k, v.as_ref()?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_trait_file_is_embedded_and_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("contracts/traits");
        let on_disk: HashSet<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        let embedded: HashSet<String> = FILES.iter().map(|(f, _)| f.to_string()).collect();
        assert_eq!(on_disk, embedded, "add the new trait files to FILES");

        let mut ids = HashSet::new();
        for t in catalog() {
            assert!(t.version >= 1, "{}", t.name);
            assert!(ids.insert(t.id()), "{} defined twice", t.id());
            let mut names = HashSet::new();
            for f in &t.state {
                check_field(&t.name, f);
                assert!(names.insert(&f.name), "{}.{} twice", t.name, f.name);
            }
            let mut forecast = HashSet::new();
            for f in &t.forecast {
                check_field(&format!("{}.forecast", t.name), f);
                assert!(f.kind == "number" || f.kind == "integer", "{}.forecast.{}: a forecast is a curve of numbers", t.name, f.name);
                assert!(forecast.insert(&f.name), "{}.forecast.{} twice", t.name, f.name);
            }
            let mut actions = HashSet::new();
            for a in &t.actions {
                assert!(actions.insert(&a.name), "{}.{} twice", t.name, a.name);
                for p in &a.params {
                    check_field(&format!("{}.{}", t.name, a.name), p);
                }
            }
            // What a field takes from the state is a field of the state, of the right type.
            let params = t.actions.iter().flat_map(|a| &a.params);
            for f in t.state.iter().chain(params) {
                for (which, from) in sources(f) {
                    let source = t.state.iter().find(|x| &x.name == from).unwrap_or_else(|| panic!("{}.{}: {which} {from} is not a field", t.name, f.name));
                    let kind = match which {
                        "unit_from" => "string",
                        "values_from" => "list",
                        _ => "number",
                    };
                    assert_eq!(source.kind, kind, "{}.{}: {which} {from} is not a {kind}", t.name, f.name);
                }
            }
            for (what, rev) in &t.since {
                assert!(names.contains(what) || actions.contains(what), "{}: since names {what}, which it does not have", t.name);
                assert!(*rev >= 1 && *rev <= t.revision, "{}.{what}: since {rev} is not a revision of the trait (0..={})", t.name, t.revision);
            }
            check_ui(t);
        }
    }

    fn check_ui(t: &Trait) {
        let ui = t.ui.as_ref().unwrap_or_else(|| panic!("{}: no ui", t.name));
        let v = vocabulary();
        assert!(v.kinds.iter().any(|k| k.id == ui.kind), "{}: unknown kind {}", t.name, ui.kind);
        let field = |n: &str| t.state.iter().find(|f| f.name == n);
        if let Some(p) = &ui.primary {
            assert!(field(p).is_some(), "{}: primary {p} is not a field", t.name);
        }
        for w in ui.card.iter().chain(&ui.detail) {
            let at = format!("{}.ui.{}", t.name, w.widget);
            assert!(v.widgets.contains_key(&w.widget), "{at}: unknown widget");
            if let Some(f) = &w.field {
                let known = if w.forecast { t.forecast.iter().any(|x| &x.name == f) } else { field(f).is_some() };
                assert!(known, "{at}: no field {f}");
            }
            if let Some(a) = &w.action {
                let action = t.action(a).unwrap_or_else(|| panic!("{at}: no action {a}"));
                if let Some(p) = &w.param {
                    assert!(action.params.iter().any(|x| &x.name == p), "{at}: {a} has no parameter {p}");
                }
                for p in w.params.iter().flat_map(|m| m.keys()) {
                    assert!(action.params.iter().any(|x| &x.name == p), "{at}: {a} has no parameter {p}");
                }
            }
            if let Some(b) = &w.when {
                assert_eq!(field(b).map(|f| f.kind.as_str()), Some("bool"), "{at}: when {b} is not a bool field");
            }
            let needs_action = ["toggle", "slider", "stepper", "enum", "color"].contains(&w.widget.as_str());
            assert!(!needs_action || (w.action.is_some() && w.param.is_some() && w.field.is_some()), "{at}: needs a field, an action and its param");
            assert!(w.widget != "button" || w.action.is_some(), "{at}: a button runs an action");
        }
    }

    #[test]
    fn the_vocabulary_is_consistent() {
        let v = vocabulary();
        let mut ids = HashSet::new();
        for k in &v.kinds {
            assert!(ids.insert(&k.id), "kind {} twice", k.id);
            assert!(v.icons.contains(&k.icon), "kind {}: unknown icon {}", k.id, k.icon);
            assert!(![&k.label.fr, &k.label.en, &k.label.es].iter().any(|s| s.trim().is_empty()), "kind {}: a label is missing", k.id);
        }
        assert_eq!(v.kinds.last().map(|k| k.id.as_str()), Some("other"), "`other` comes last: it takes what no kind fits");
    }

    #[test]
    fn measures_have_a_history_settings_do_not() {
        for f in ["power", "energy_in", "position", "brightness", "flow", "humidity", "on"] {
            assert!(keeps_history(f), "{f}");
        }
        for f in ["limit_min", "color_temp_max", "nominal", "target", "last_at"] {
            assert!(!keeps_history(f), "{f}");
        }
    }

    #[test]
    fn announced_traits_are_in_the_catalog() {
        let caps = crate::capabilities::Capabilities::local();
        for (name, versions) in &caps.traits {
            for v in versions {
                assert!(get(name, *v).is_some(), "{name}@{v} is announced but not described in contracts/traits");
            }
        }
        for t in catalog() {
            assert!(caps.traits.get(&t.name).is_some_and(|v| v.contains(&t.version)), "{} is described but not announced", t.id());
            assert_eq!(caps.host.traits.get(&t.name).and_then(|m| m.get(&t.version.to_string())), Some(&t.revision), "{}: revision announced", t.id());
        }
    }

    #[test]
    fn climate_zone_matches_the_zone_commands() {
        let t = get("climate.zone", 1).unwrap();
        let params: Vec<&str> = t.action("set").unwrap().params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(params, crate::hub::ZoneCommand::FIELDS, "the `set` action and ZoneCommand must list the same fields");
        let modes = &t.state.iter().find(|f| f.name == "mode").unwrap().values;
        for m in modes {
            assert!(serde_json::from_value::<crate::airzone::OperationMode>(serde_json::json!(m)).is_ok(), "mode {m}");
        }
    }
}
