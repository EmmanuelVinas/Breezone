//! Web and mobile types generated from the catalog of traits (`contracts/traits`, ADR 0013), so that
//! the three languages share one contract. This test fails when the generated files are out of
//! date; to write them again:
//!
//! `BREEZONE_WRITE_GENERATED=1 cargo test --test generated`

use breezone_server::traits::{catalog, Field, Trait};
use std::path::PathBuf;

const HEADER: &str = "Generated from server/contracts/traits by `BREEZONE_WRITE_GENERATED=1 cargo test --test generated` (in server/): do not edit.";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// `climate.zone` → `ClimateZone`, `air_quality` → `AirQuality`.
fn pascal(s: &str) -> String {
    s.split(['.', '_']).map(|w| w[..1].to_uppercase() + &w[1..]).collect()
}

fn camel(s: &str) -> String {
    let p = pascal(s);
    p[..1].to_lowercase() + &p[1..]
}

fn ts_type(f: &Field) -> String {
    match f.kind.as_str() {
        "bool" => "boolean".to_string(),
        "integer" | "number" => "number".to_string(),
        "enum" => f.values.iter().map(|v| format!("\"{v}\"")).collect::<Vec<_>>().join(" | "),
        _ => "string".to_string(),
    }
}

fn typescript() -> String {
    let mut out = format!("// {HEADER}\n\n");
    let mut ids = vec![];
    for t in catalog() {
        ids.push(format!("\"{}\"", t.id()));
        let name = pascal(&t.name);
        out += &format!("/** {} (`{}`): state. */\nexport type {name}State = {{\n", t.label.en, t.id());
        for f in &t.state {
            let ty = ts_type(f);
            let ty = if f.optional { format!("{ty} | null") } else { ty };
            out += &format!("  /** {}{} */\n  {}: {ty}\n", f.label.en, f.unit.as_deref().map(|u| format!(" ({u})")).unwrap_or_default(), f.name);
        }
        out += "}\n\n";
        for a in &t.actions {
            if a.params.is_empty() {
                out +=
                    &format!("/** {} (`{}`): action `{}`, without parameters. */\nexport type {name}{} = Record<string, never>\n\n", t.label.en, t.id(), a.name, pascal(&a.name));
                continue;
            }
            out += &format!("/** {} (`{}`): action `{}`. */\nexport type {name}{} = {{\n", t.label.en, t.id(), a.name, pascal(&a.name));
            for p in &a.params {
                out += &format!("  {}{}: {}\n", p.name, if p.optional { "?" } else { "" }, ts_type(p));
            }
            out += "}\n\n";
        }
    }
    out += &format!("/** Traits described by this version (`trait@version`). */\nexport const TRAITS = [{}] as const\n", ids.join(", "));
    out
}

fn kotlin_type(t: &Trait, f: &Field) -> String {
    match f.kind.as_str() {
        "bool" => "Boolean".into(),
        "integer" => "Long".into(),
        "number" => "Double".into(),
        "enum" => format!("{}{}", pascal(&t.name), pascal(&f.name)),
        _ => "String".into(),
    }
}

fn kotlin() -> String {
    let mut out = format!("// {HEADER}\n\npackage com.breezone.app.data\n\nimport kotlinx.serialization.SerialName\nimport kotlinx.serialization.Serializable\n");
    let mut ids = vec![];
    for t in catalog() {
        ids.push(format!("\"{}\"", t.id()));
        let name = pascal(&t.name);
        // Enums of the trait (state and parameters share their names).
        let mut enums: Vec<&Field> = vec![];
        for f in t.state.iter().chain(t.actions.iter().flat_map(|a| a.params.iter())) {
            if f.kind == "enum" && !enums.iter().any(|e| e.name == f.name) {
                enums.push(f);
            }
        }
        for e in enums {
            out += &format!("\n@Serializable\nenum class {} {{\n", kotlin_type(t, e));
            for v in &e.values {
                out += &format!("    @SerialName(\"{v}\") {},\n", pascal(v));
            }
            out += "}\n";
        }
        out += &format!("\n/** {} (`{}`): state. */\n@Serializable\ndata class {name}State(\n", t.label.en, t.id());
        for f in &t.state {
            let ty = kotlin_type(t, f);
            let decl = if f.optional {
                format!("{ty}? = null")
            } else {
                match f.kind.as_str() {
                    "bool" => format!("{ty} = false"),
                    "integer" => format!("{ty} = 0"),
                    "number" => format!("{ty} = 0.0"),
                    _ => format!("{ty}? = null"),
                }
            };
            out += &format!("    val {}: {decl},\n", camel(&f.name));
        }
        out += ")\n";
        for a in &t.actions {
            if a.params.is_empty() {
                out += &format!("\n/** {} (`{}`): action `{}`, without parameters. */\n@Serializable\nclass {name}{}\n", t.label.en, t.id(), a.name, pascal(&a.name));
                continue;
            }
            out += &format!("\n/** {} (`{}`): action `{}`. */\n@Serializable\ndata class {name}{}(\n", t.label.en, t.id(), a.name, pascal(&a.name));
            for p in &a.params {
                out += &format!("    val {}: {}? = null,\n", camel(&p.name), kotlin_type(t, p));
            }
            out += ")\n";
        }
    }
    out += &format!("\n/** Traits described by this version (`trait@version`). */\nval TRAITS = listOf({})\n", ids.join(", "));
    out
}

fn check(path: PathBuf, expected: String) {
    if std::env::var("BREEZONE_WRITE_GENERATED").is_ok() {
        std::fs::write(&path, expected).unwrap();
        return;
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(actual == expected, "{} is out of date: run `BREEZONE_WRITE_GENERATED=1 cargo test --test generated` in server/", path.display());
}

#[test]
fn web_types_are_up_to_date() {
    check(repo().join("web/src/lib/traits.generated.ts"), typescript());
}

#[test]
fn mobile_types_are_up_to_date() {
    check(repo().join("mobile/shared/src/commonMain/kotlin/com/breezone/app/data/Traits.kt"), kotlin());
}
