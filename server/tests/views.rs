//! The main views of the API (`contracts/views.json`): checked against the real answers of the
//! server, and the source of the web types (`web/src/lib/api.generated.ts`) and of the samples the
//! app's models are tested with. This test fails when the server and the contract disagree, or
//! when the generated files are out of date; to write them again:
//!
//! `BREEZONE_WRITE_GENERATED=1 cargo test --test views`

mod support;

use breezone_server::mock::{self, MockState};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const HEADER: &str = "Generated from server/contracts/views.json by `BREEZONE_WRITE_GENERATED=1 cargo test --test views` (in server/): do not edit.";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn contract() -> Value {
    serde_json::from_str(&std::fs::read_to_string(repo().join("server/contracts/views.json")).unwrap()).unwrap()
}

/// The alternatives of a type: `A | B | null` → `[A, B, null]` (`Record<…>` holds no `|`).
fn alternatives(ty: &str) -> Vec<&str> {
    ty.split(" | ").map(str::trim).collect()
}

/// What is wrong with `value` as a `ty` (empty: nothing).
fn check(c: &Value, value: &Value, ty: &str, path: &str) -> Vec<String> {
    let alts = alternatives(ty);
    if value.is_null() {
        return if alts.contains(&"null") || alts.contains(&"unknown") { vec![] } else { vec![format!("{path}: null, the contract says {ty}")] };
    }
    let mut first_errors = None;
    for alt in alts.iter().filter(|a| **a != "null") {
        let errors = check_one(c, value, alt, path);
        if errors.is_empty() {
            return vec![];
        }
        first_errors.get_or_insert(errors);
    }
    first_errors.unwrap_or_else(|| vec![format!("{path}: {value}, the contract says {ty}")])
}

fn check_one(c: &Value, value: &Value, ty: &str, path: &str) -> Vec<String> {
    let wrong = || vec![format!("{path}: {value}, the contract says {ty}")];
    if let Some(item) = ty.strip_suffix("[]") {
        let Some(items) = value.as_array() else { return wrong() };
        return items.iter().enumerate().flat_map(|(i, v)| check(c, v, item, &format!("{path}[{i}]"))).collect();
    }
    if let Some(literal) = ty.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
        return if value.as_str() == Some(literal) { vec![] } else { wrong() };
    }
    let ok = match ty {
        "number" => value.is_number(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "unknown" => true,
        "Record<string, unknown>" => value.is_object(),
        _ => {
            if let Some(alias) = c["aliases"].get(ty) {
                return check(c, value, alias["type"].as_str().unwrap(), path);
            }
            if c["objects"].get(ty).is_some() {
                return check_object(c, value, ty, path);
            }
            panic!("{path}: unknown type {ty} in the contract");
        }
    };
    if ok {
        vec![]
    } else {
        wrong()
    }
}

fn check_object(c: &Value, value: &Value, name: &str, path: &str) -> Vec<String> {
    let Some(obj) = value.as_object() else { return vec![format!("{path}: not an object ({name})")] };
    let fields = c["objects"][name]["fields"].as_array().unwrap();
    let mut errors = vec![];
    for f in fields {
        let field = f["name"].as_str().unwrap();
        match obj.get(field) {
            None if f["optional"].as_bool() == Some(true) => {}
            None => errors.push(format!("{path}.{field}: in the contract of {name}, not sent by the server")),
            Some(v) => errors.extend(check(c, v, f["type"].as_str().unwrap(), &format!("{path}.{field}"))),
        }
    }
    for key in obj.keys() {
        if !fields.iter().any(|f| f["name"] == key.as_str()) {
            errors.push(format!("{path}.{key}: sent by the server, missing from the contract of {name}"));
        }
    }
    errors
}

fn doc(text: &str, indent: &str) -> String {
    format!("{indent}/** {text} */\n")
}

fn typescript(c: &Value) -> String {
    let mut out = format!("// {HEADER}\n\n");
    for (name, a) in c["aliases"].as_object().unwrap() {
        out += &format!("export type {name} = {}\n\n", a["type"].as_str().unwrap());
    }
    for (name, o) in c["objects"].as_object().unwrap() {
        out += &doc(o["doc"].as_str().unwrap(), "");
        out += &format!("export type {name} = {{\n");
        for f in o["fields"].as_array().unwrap() {
            if let Some(d) = f["doc"].as_str() {
                out += &doc(d, "  ");
            }
            let optional = if f["optional"].as_bool() == Some(true) { "?" } else { "" };
            out += &format!("  {}{optional}: {}\n", f["name"].as_str().unwrap(), f["type"].as_str().unwrap());
        }
        out += "}\n\n";
    }
    out.trim_end().to_string() + "\n"
}

/// A value of `ty` with every field filled (the first alternative that is not null).
fn example(c: &Value, ty: &str) -> Value {
    let alt = alternatives(ty).into_iter().find(|a| *a != "null").unwrap_or("null");
    if let Some(item) = alt.strip_suffix("[]") {
        return json!([example(c, item)]);
    }
    if let Some(literal) = alt.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
        return json!(literal);
    }
    match alt {
        "number" => json!(1),
        "string" => json!("x"),
        "boolean" => json!(true),
        "unknown" | "null" => Value::Null,
        "Record<string, unknown>" => json!({}),
        _ => match c["aliases"].get(alt) {
            Some(a) => example(c, a["type"].as_str().unwrap()),
            None => {
                let mut obj = Map::new();
                for f in c["objects"][alt]["fields"].as_array().unwrap() {
                    obj.insert(f["name"].as_str().unwrap().into(), example(c, f["type"].as_str().unwrap()));
                }
                Value::Object(obj)
            }
        },
    }
}

fn kotlin_samples(c: &Value) -> String {
    let mut out = format!("// {HEADER}\n\npackage com.breezone.app.data\n\n/** A sample of each view of the contract, every field filled: the app's models must read them. */\nobject ApiSamples {{\n");
    for name in c["objects"].as_object().unwrap().keys() {
        let sample = serde_json::to_string(&example(c, name)).unwrap().replace('$', "${'$'}");
        out += &format!(
            "    const val {} = \"\"\"{sample}\"\"\"\n",
            name.chars().fold(String::new(), |mut s, ch| {
                if ch.is_uppercase() && !s.is_empty() {
                    s.push('_');
                }
                s.push(ch.to_ascii_uppercase());
                s
            })
        );
    }
    out + "}\n"
}

fn up_to_date(path: PathBuf, content: &str) {
    if std::env::var("BREEZONE_WRITE_GENERATED").is_ok() {
        std::fs::write(&path, content).unwrap();
        return;
    }
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(current == content, "{} is out of date: BREEZONE_WRITE_GENERATED=1 cargo test --test views", path.display());
}

#[test]
fn the_generated_files_follow_the_contract() {
    let c = contract();
    up_to_date(repo().join("web/src/lib/api.generated.ts"), &typescript(&c));
    up_to_date(repo().join("mobile/shared/src/commonTest/kotlin/com/breezone/app/data/ApiSamples.kt"), &kotlin_samples(&c));
}

#[tokio::test]
async fn the_server_answers_as_the_contract_says() {
    let airzone: MockState = Arc::new(Mutex::new(mock::preset("maison")));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, mock::router(airzone)).await.unwrap() });
    let dir = std::env::temp_dir().join(format!("breezone-views-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = breezone_server::init_state(support::config(&dir)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(listener, breezone_server::app(state)).await.unwrap() });
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();
    let call = |method: reqwest::Method, path: &str, body: Option<Value>| {
        let mut r = http.request(method, format!("{base}{path}")).bearer_auth(&token);
        if let Some(b) = body {
            r = r.json(&b);
        }
        async move { r.send().await.unwrap().json::<Value>().await.unwrap() }
    };
    call(reqwest::Method::POST, "/api/installations", Some(json!({"name":"Maison","host":"127.0.0.1","port":mock_port}))).await;
    call(reqwest::Method::POST, "/api/rooms", Some(json!({"name":"Salon"}))).await;

    let c = contract();
    let installations = call(reqwest::Method::GET, "/api/status", None).await;
    let one = call(reqwest::Method::GET, "/api/installations/1", None).await;
    let devices = call(reqwest::Method::GET, "/api/devices", None).await;
    let rooms = call(reqwest::Method::GET, "/api/rooms", None).await;
    assert!(installations[0]["zones"].as_array().is_some_and(|z| !z.is_empty()), "{installations}");
    assert!(installations[0]["zones"].as_array().unwrap().iter().any(|z| !z["air_quality"].is_null()), "a zone with an air quality sensor is checked too");
    let mut errors = check(&c, &installations, "Installation[]", "installations");
    errors.extend(check(&c, &one, "Installation", "installations/1"));
    errors.extend(check(&c, &devices, "DeviceView[]", "devices"));
    errors.extend(check(&c, &rooms, "Room[]", "rooms"));
    // The price of electricity, with a contract and a tariff that use every field.
    call(reqwest::Method::PUT, "/api/installations/1/electricity/contract", Some(json!({"country":"FR","supplier":"EDF","offer":"custom","power_kva":9,"currency":"EUR"}))).await;
    let tempo = json!({
        "subscription": 16.0, "export_price": 0.04,
        "profiles": [
            {"id":"blue","name":"Bleu","color":"#2563eb","slots":[{"start":"00:00","name":"HC","price":0.13},{"start":"06:00","name":"HP","price":0.16},{"start":"22:00","name":"HC","price":0.13}]},
            {"id":"red","name":"Rouge","slots":[{"start":"00:00","name":"HC","price":0.15},{"start":"06:00","name":"HP","price":0.7}]}
        ],
        "calendar": {"day_start":"06:00","weekdays":["blue","blue","blue","blue","blue","blue","blue"],"colors":{"blue":"blue","red":"red"}}
    });
    call(reqwest::Method::PUT, "/api/installations/1/electricity/prices/2020-01-01", Some(tempo)).await;
    let tariff = call(reqwest::Method::GET, "/api/installations/1/electricity", None).await;
    let price = call(reqwest::Method::GET, "/api/installations/1/electricity/price", None).await;
    assert!(!price["now"].is_null() && !price["periods"].as_array().unwrap().is_empty(), "{price}");
    errors.extend(check(&c, &tariff, "TariffView", "energy/tariff"));
    errors.extend(check(&c, &price, "PriceView", "energy/price"));
    assert!(errors.is_empty(), "the server and contracts/views.json disagree:\n{}", errors.join("\n"));
}
