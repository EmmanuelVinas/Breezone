//! MCP server (Model Context Protocol, "Streamable HTTP" transport, answers in JSON only): an
//! assistant drives Breezone with the rights of the user whose access token it holds.
//!
//! Each tool is one call of the HTTP API, sent through the same router with the same
//! `Authorization`: the rights, the checks and the errors are those of the API.

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::AppState;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{ACCEPT_LANGUAGE, ALLOW, AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Map, Value};
use tower::ServiceExt;

/// Versions of the protocol this server speaks, the newest first.
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
/// The reference of the HTTP API (French), given to the assistant section by section.
const API_REFERENCE: &str = include_str!("../../../docs/API.md");
/// Longest text a tool answers with (an assistant reads it all).
const MAX_TEXT: usize = 200_000;
/// Largest answer of the API read back.
const MAX_BODY: usize = 8 * 1024 * 1024;

const INSTRUCTIONS: &str = "Breezone is a self-hosted home hub: Airzone climate zones, devices of modules (lights, plugs, \
shutters, chargers, solar inverters, meters…), routines (scenes run on demand), automations (\"when… if… then… else…\") and \
the schedules of the Airzone app. You act with the rights of the user who created your access token. \
Read the ids with get_status (zones), list_devices and list_rooms before acting. Before writing a routine, an automation or a \
schedule, read the matching section of the API reference with api_reference (in French): it gives every field. \
Ask the user before deleting anything. Prices are per kWh, powers in W, temperatures in °C.";

/// What a tool does, for the assistant (hints of the protocol).
#[derive(Clone, Copy, PartialEq)]
enum Effect {
    Read,
    Write,
    Delete,
}

/// A call of the HTTP API.
struct Call {
    method: Method,
    path: String,
    body: Option<Value>,
}

impl Call {
    fn get(path: impl Into<String>) -> Result<Call, String> {
        Ok(Call { method: Method::GET, path: path.into(), body: None })
    }
    fn with(method: Method, path: impl Into<String>, body: Value) -> Result<Call, String> {
        Ok(Call { method, path: path.into(), body: Some(body) })
    }
    fn delete(path: impl Into<String>) -> Result<Call, String> {
        Ok(Call { method: Method::DELETE, path: path.into(), body: None })
    }
}

type Build = fn(&Value) -> Result<Call, String>;

struct Tool {
    name: &'static str,
    description: &'static str,
    effect: Effect,
    /// Properties of the arguments, and which are required.
    params: fn() -> (Value, &'static [&'static str]),
    build: Build,
}

fn none() -> (Value, &'static [&'static str]) {
    (json!({}), &[])
}

fn id_only() -> (Value, &'static [&'static str]) {
    (json!({"id": {"type": "integer"}}), &["id"])
}

fn object(description: &str) -> Value {
    json!({"type": "object", "description": description})
}

fn int(args: &Value, name: &str) -> Result<i64, String> {
    args.get(name).and_then(Value::as_i64).ok_or_else(|| format!("`{name}`: an integer is expected"))
}

fn obj(args: &Value, name: &str) -> Result<Value, String> {
    args.get(name).filter(|v| v.is_object()).cloned().ok_or_else(|| format!("`{name}`: an object is expected"))
}

/// A name that goes in a path as it is (module id, Airzone ids).
fn word(args: &Value, name: &str) -> Result<String, String> {
    let v = match args.get(name) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return Err(format!("`{name}`: a string is expected")),
    };
    if v.is_empty() || v.len() > 128 || !v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) || v.contains("..") {
        return Err(format!("`{name}`: letters, digits, `_`, `-` and `.` only"));
    }
    Ok(v)
}

fn tools() -> Vec<Tool> {
    use Effect::*;
    vec![
        // What there is.
        Tool {
            name: "get_status",
            description: "The Airzone installations the user sees, with their zones and their live state (on, mode, temperature, setpoint, humidity, air quality).",
            effect: Read,
            params: none,
            build: |_| Call::get("/api/status"),
        },
        Tool {
            name: "list_devices",
            description: "Every device the user sees (zones too), whatever its module: id, kind, traits, room, live state, can_control.",
            effect: Read,
            params: none,
            build: |_| Call::get("/api/devices"),
        },
        Tool { name: "list_rooms", description: "The rooms of the home.", effect: Read, params: none, build: |_| Call::get("/api/rooms") },
        Tool {
            name: "get_traits",
            description: "The catalogue of traits: for each, its state fields and its actions with their parameters (what device_action accepts).",
            effect: Read,
            params: none,
            build: |_| Call::get("/api/traits"),
        },
        Tool {
            name: "get_energy_now",
            description: "The power of the home now: solar production, consumption, grid, battery, and by use.",
            effect: Read,
            params: none,
            build: |_| Call::get("/api/energy/live"),
        },
        Tool {
            name: "get_electricity_price",
            description: "The price of electricity of an installation now, its periods over today, and the Tempo colours of today and tomorrow.",
            effect: Read,
            params: || (json!({"installation_id": {"type": "integer"}}), &["installation_id"]),
            build: |a| Call::get(format!("/api/installations/{}/electricity/price?today=true", int(a, "installation_id")?)),
        },
        Tool { name: "get_weather", description: "The weather of the home and the sun of today.", effect: Read, params: none, build: |_| Call::get("/api/weather") },
        // Driving.
        Tool {
            name: "control_zone",
            description: "Changes an Airzone zone. `command` (ZoneCommand, all optional): {on, setpoint, cool_setpoint, heat_setpoint, mode (stop|cooling|heating|fan|dry|aux_heating|auto, applied to the whole system), speed, sleep (0|30|60|90)}.",
            effect: Write,
            params: || {
                (
                    json!({"installation_id": {"type": "integer"}, "system_id": {"type": "integer"}, "zone_id": {"type": "integer"}, "command": object("ZoneCommand")}),
                    &["installation_id", "system_id", "zone_id", "command"],
                )
            },
            build: |a| {
                Call::with(Method::PATCH, format!("/api/installations/{}/zones/{}/{}", int(a, "installation_id")?, int(a, "system_id")?, int(a, "zone_id")?), obj(a, "command")?)
            },
        },
        Tool {
            name: "device_action",
            description: "Runs an action of a trait of a device (see get_traits), e.g. {trait: \"switch.relay\", action: \"set\", params: {on: true}}.",
            effect: Write,
            params: || {
                (
                    json!({"device_id": {"type": "integer"}, "trait": {"type": "string"}, "action": {"type": "string"}, "params": object("Parameters of the action")}),
                    &["device_id", "trait", "action"],
                )
            },
            build: |a| {
                let body = json!({"trait": a.get("trait").cloned().unwrap_or_default(), "action": a.get("action").cloned().unwrap_or_default(), "params": a.get("params").cloned().unwrap_or_else(|| json!({}))});
                Call::with(Method::POST, format!("/api/devices/{}/actions", int(a, "device_id")?), body)
            },
        },
        // Routines (scenes run on demand).
        Tool { name: "list_routines", description: "The routines (scenes run on demand).", effect: Read, params: none, build: |_| Call::get("/api/routines") },
        Tool {
            name: "create_routine",
            description: "Creates a routine. `routine` (RoutineInput): {name, enabled?, actions: [RoutineAction]} — see api_reference section \"Routines\".",
            effect: Write,
            params: || (json!({"routine": object("RoutineInput")}), &["routine"]),
            build: |a| Call::with(Method::POST, "/api/routines", obj(a, "routine")?),
        },
        Tool {
            name: "update_routine",
            description: "Replaces a routine with `routine` (RoutineInput).",
            effect: Write,
            params: || (json!({"id": {"type": "integer"}, "routine": object("RoutineInput")}), &["id", "routine"]),
            build: |a| Call::with(Method::PUT, format!("/api/routines/{}", int(a, "id")?), obj(a, "routine")?),
        },
        Tool { name: "delete_routine", description: "Deletes a routine.", effect: Delete, params: id_only, build: |a| Call::delete(format!("/api/routines/{}", int(a, "id")?)) },
        Tool {
            name: "run_routine",
            description: "Runs a routine now.",
            effect: Write,
            params: id_only,
            build: |a| Call::with(Method::POST, format!("/api/routines/{}/run", int(a, "id")?), json!({})),
        },
        // Automations ("when… if… then… else…").
        Tool { name: "list_automations", description: "The automations (\"when… if… then… else…\").", effect: Read, params: none, build: |_| Call::get("/api/automations") },
        Tool {
            name: "create_automation",
            description: "Creates an automation. `automation` (AutomationInput): {name, enabled?, triggers, match?, conditions?, actions, else?, mode?, over?} — read api_reference section \"Automatismes\" first.",
            effect: Write,
            params: || (json!({"automation": object("AutomationInput")}), &["automation"]),
            build: |a| Call::with(Method::POST, "/api/automations", obj(a, "automation")?),
        },
        Tool {
            name: "update_automation",
            description: "Replaces an automation with `automation` (AutomationInput).",
            effect: Write,
            params: || (json!({"id": {"type": "integer"}, "automation": object("AutomationInput")}), &["id", "automation"]),
            build: |a| Call::with(Method::PUT, format!("/api/automations/{}", int(a, "id")?), obj(a, "automation")?),
        },
        Tool {
            name: "set_automation_enabled",
            description: "Turns an automation on or off.",
            effect: Write,
            params: || (json!({"id": {"type": "integer"}, "enabled": {"type": "boolean"}}), &["id", "enabled"]),
            build: |a| {
                let enabled = a.get("enabled").and_then(Value::as_bool).ok_or("`enabled`: a boolean is expected")?;
                Call::with(Method::PUT, format!("/api/automations/{}/enabled", int(a, "id")?), json!({"enabled": enabled}))
            },
        },
        Tool {
            name: "run_automation",
            description: "Runs the \"then\" steps of an automation now, without looking at its triggers nor its conditions.",
            effect: Write,
            params: id_only,
            build: |a| Call::with(Method::POST, format!("/api/automations/{}/run", int(a, "id")?), json!({})),
        },
        Tool {
            name: "automation_runs",
            description: "The last 20 runs of an automation, step by step.",
            effect: Read,
            params: id_only,
            build: |a| Call::get(format!("/api/automations/{}/runs", int(a, "id")?)),
        },
        Tool { name: "delete_automation", description: "Deletes an automation.", effect: Delete, params: id_only, build: |a| Call::delete(format!("/api/automations/{}", int(a, "id")?)) },
        // Schedules of the Airzone app (kept and run by Airzone).
        Tool {
            name: "list_airzone_schedules",
            description: "The fixed-time schedules of the Airzone app (Airzone Cloud), which Airzone runs itself.",
            effect: Read,
            params: none,
            build: |_| Call::get("/api/airzone/schedules"),
        },
        Tool {
            name: "create_airzone_schedule",
            description: "Creates a schedule on Airzone. `schedule`: {cloud_id, name, enabled?, days (1 = Monday … 7), time (\"HH:MM\"), zones: [{system_id, zone_id}], action: ZoneCommand}.",
            effect: Write,
            params: || (json!({"schedule": object("Schedule")}), &["schedule"]),
            build: |a| Call::with(Method::POST, "/api/airzone/schedules", obj(a, "schedule")?),
        },
        Tool {
            name: "update_airzone_schedule",
            description: "Changes a schedule on Airzone (`schedule` as for create, without cloud_id).",
            effect: Write,
            params: || (json!({"cloud_id": {"type": "string"}, "id": {"type": "string"}, "schedule": object("Schedule")}), &["cloud_id", "id", "schedule"]),
            build: |a| Call::with(Method::PUT, format!("/api/airzone/schedules/{}/{}", word(a, "cloud_id")?, word(a, "id")?), obj(a, "schedule")?),
        },
        Tool {
            name: "delete_airzone_schedule",
            description: "Deletes a schedule on Airzone.",
            effect: Delete,
            params: || (json!({"cloud_id": {"type": "string"}, "id": {"type": "string"}}), &["cloud_id", "id"]),
            build: |a| Call::delete(format!("/api/airzone/schedules/{}/{}", word(a, "cloud_id")?, word(a, "id")?)),
        },
        // Timers.
        Tool { name: "list_timers", description: "The timers waiting to run.", effect: Read, params: none, build: |_| Call::get("/api/timers") },
        Tool {
            name: "create_timer",
            description: "Runs a zone command later. `timer`: {delay_minutes (1 … 10080), target: {kind: \"zone\", installation_id, system_id, zone_id} | {kind: \"group\", installation_id, group} | {kind: \"installation\", installation_id} | {kind: \"all\"}, command: ZoneCommand, label?}.",
            effect: Write,
            params: || (json!({"timer": object("Timer")}), &["timer"]),
            build: |a| Call::with(Method::POST, "/api/timers", obj(a, "timer")?),
        },
        Tool { name: "cancel_timer", description: "Cancels a timer.", effect: Delete, params: id_only, build: |a| Call::delete(format!("/api/timers/{}", int(a, "id")?)) },
        // Modules and sources (administrators).
        Tool { name: "list_modules", description: "The modules installed on this server (administrators).", effect: Read, params: none, build: |_| Call::get("/api/modules") },
        Tool {
            name: "module_library",
            description: "The modules that can be installed, from every enabled repository, and whether they work here (administrators).",
            effect: Read,
            params: none,
            build: |_| Call::get("/api/modules/library"),
        },
        Tool {
            name: "install_module",
            description: "Installs or updates a module from the library (administrators); the newest version that works here unless `version` is given.",
            effect: Write,
            params: || (json!({"id": {"type": "string"}, "repository": {"type": "integer"}, "version": {"type": "string"}}), &["id"]),
            build: |a| {
                let mut body = json!({"id": word(a, "id")?});
                for k in ["repository", "version"] {
                    if let Some(v) = a.get(k).filter(|v| !v.is_null()) {
                        body[k] = v.clone();
                    }
                }
                Call::with(Method::POST, "/api/modules", body)
            },
        },
        Tool {
            name: "remove_module",
            description: "Removes a module no source nor account uses (administrators).",
            effect: Delete,
            params: || (json!({"id": {"type": "string"}}), &["id"]),
            build: |a| Call::delete(format!("/api/modules/{}", word(a, "id")?)),
        },
        Tool {
            name: "module_log",
            description: "The last 200 lines of the log of a module (administrators).",
            effect: Read,
            params: || (json!({"id": {"type": "string"}}), &["id"]),
            build: |a| Call::get(format!("/api/modules/{}/log", word(a, "id")?)),
        },
        Tool {
            name: "list_drivers",
            description: "The drivers of the installed modules and the configuration each asks for (administrators).",
            effect: Read,
            params: none,
            build: |_| Call::get("/api/drivers"),
        },
        Tool { name: "list_sources", description: "The sources of devices (a piece of hardware read by a driver) (administrators).", effect: Read, params: none, build: |_| Call::get("/api/sources") },
        Tool {
            name: "add_source",
            description: "Adds a source of devices (administrators). `source`: {driver, name, config, enabled?} — see list_drivers for the config of a driver.",
            effect: Write,
            params: || (json!({"source": object("Source")}), &["source"]),
            build: |a| Call::with(Method::POST, "/api/sources", obj(a, "source")?),
        },
        Tool {
            name: "scan_products",
            description: "Looks for the supported products on the local network (a few seconds) and says which are already added (administrators).",
            effect: Read,
            params: none,
            build: |_| Call::with(Method::POST, "/api/discovery/products", json!({})),
        },
    ]
}

/// The tools that do not go to a single route.
const API_REFERENCE_TOOL: &str = "api_reference";
const API_REQUEST_TOOL: &str = "api_request";

/// Routes the generic tool does not reach: signing in, accounts and their secrets, tokens.
const DENIED: &[&str] = &[
    "/api/auth",
    "/api/setup",
    "/api/tokens",
    "/api/me/password",
    "/api/sessions",
    "/api/users",
    "/api/ws",
    "/api/system/config",
    "/api/system/update",
    "/api/cloud",
    "/api/accounts",
    "/api/federation",
    "/api/shares",
    "/api/nodes",
    "/api/node",
];

fn generic_call(args: &Value) -> Result<Call, String> {
    let method = match args.get("method").and_then(Value::as_str).map(str::to_ascii_uppercase).as_deref() {
        Some("GET") => Method::GET,
        Some("POST") => Method::POST,
        Some("PUT") => Method::PUT,
        Some("PATCH") => Method::PATCH,
        Some("DELETE") => Method::DELETE,
        _ => return Err("`method`: GET, POST, PUT, PATCH or DELETE".into()),
    };
    let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
    let route = path.split('?').next().unwrap_or_default();
    if !route.starts_with("/api/") || route.contains("..") || route.contains("//") || path.chars().any(char::is_whitespace) {
        return Err("`path`: a path of the API, starting with /api/".into());
    }
    if DENIED.iter().any(|d| route == *d || route.starts_with(&format!("{d}/"))) {
        return Err(format!("{route} is not reachable through MCP (sign-in, accounts and secrets stay in the web UI)"));
    }
    Ok(Call { method, path: path.into(), body: args.get("body").filter(|b| !b.is_null()).cloned() })
}

fn tool_list() -> Value {
    let mut list: Vec<Value> = tools()
        .iter()
        .map(|t| {
            let (properties, required) = (t.params)();
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": {"type": "object", "properties": properties, "required": required},
                "annotations": {
                    "readOnlyHint": t.effect == Effect::Read,
                    "destructiveHint": t.effect == Effect::Delete,
                    "openWorldHint": false,
                },
            })
        })
        .collect();
    list.push(json!({
        "name": API_REFERENCE_TOOL,
        "description": "The reference of the Breezone HTTP API (in French). Without `section`, the list of its sections; with one (its title, or a part of it), that section: every route, field and rule.",
        "inputSchema": {"type": "object", "properties": {"section": {"type": "string"}}, "required": []},
        "annotations": {"readOnlyHint": true, "openWorldHint": false},
    }));
    list.push(json!({
        "name": API_REQUEST_TOOL,
        "description": "Calls any route of the Breezone HTTP API that no other tool covers (see api_reference), with the rights of the user. Sign-in, users, tokens, linked accounts and their secrets are not reachable.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "method": {"type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"]},
                "path": {"type": "string", "description": "Path starting with /api/, query string included"},
                "body": {"description": "JSON body"},
            },
            "required": ["method", "path"],
        },
        "annotations": {"readOnlyHint": false, "destructiveHint": true, "openWorldHint": false},
    }));
    json!({ "tools": list })
}

/// The sections of the reference: (title, text with its title).
fn sections() -> Vec<(&'static str, &'static str)> {
    let mut out = vec![];
    let mut start = None;
    let mut offset = 0;
    for line in API_REFERENCE.split_inclusive('\n') {
        if line.starts_with("## ") {
            if let Some((title, from)) = start {
                out.push((title, &API_REFERENCE[from..offset]));
            }
            start = Some((line[3..].trim(), offset));
        }
        offset += line.len();
    }
    if let Some((title, from)) = start {
        out.push((title, &API_REFERENCE[from..]));
    }
    out
}

fn reference(args: &Value) -> (String, bool) {
    let all = sections();
    let Some(wanted) = args.get("section").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) else {
        let titles: Vec<String> = all.iter().map(|(t, _)| format!("- {t}")).collect();
        return (format!("Sections of the API reference (ask for one with `section`):\n{}", titles.join("\n")), false);
    };
    let lower = wanted.to_lowercase();
    match all.iter().find(|(t, _)| t.to_lowercase() == lower).or_else(|| all.iter().find(|(t, _)| t.to_lowercase().contains(&lower))) {
        Some((_, text)) => (text.to_string(), false),
        None => (format!("No section matches “{wanted}”. Call api_reference without `section` for the list."), true),
    }
}

/// Sends a call through the API router, as the user who called the tool.
async fn forward(state: &AppState, headers: &HeaderMap, call: Call) -> (StatusCode, String) {
    let mut req = Request::builder().method(call.method).uri(&call.path);
    for name in [AUTHORIZATION, ACCEPT_LANGUAGE] {
        if let Some(v) = headers.get(&name) {
            req = req.header(name, v.clone());
        }
    }
    let body = match call.body {
        Some(b) => {
            req = req.header(CONTENT_TYPE, "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let Ok(req) = req.body(body) else {
        return (StatusCode::BAD_REQUEST, "invalid path".into());
    };
    let resp = match super::router().with_state(state.clone()).oneshot(req).await {
        Ok(r) => r,
        Err(e) => match e {},
    };
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), MAX_BODY).await.unwrap_or_default();
    let text = match serde_json::from_slice::<Value>(&bytes) {
        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
        Err(_) if bytes.is_empty() => String::new(),
        Err(_) => String::from_utf8_lossy(&bytes).into_owned(),
    };
    (status, text)
}

fn tool_result(text: String, is_error: bool) -> Value {
    let text = if text.len() > MAX_TEXT {
        let mut cut = MAX_TEXT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}\n… (cut: {} bytes in all)", &text[..cut], text.len())
    } else {
        text
    };
    json!({ "content": [{"type": "text", "text": text}], "isError": is_error })
}

async fn call_tool(state: &AppState, headers: &HeaderMap, params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "`name` is missing".to_string()))?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    if !args.is_object() {
        return Err((-32602, "`arguments` must be an object".into()));
    }
    let call = match name {
        API_REFERENCE_TOOL => {
            let (text, is_error) = reference(&args);
            return Ok(tool_result(text, is_error));
        }
        API_REQUEST_TOOL => generic_call(&args),
        _ => match tools().into_iter().find(|t| t.name == name) {
            Some(t) => (t.build)(&args),
            None => return Err((-32602, format!("unknown tool: {name}"))),
        },
    };
    let call = match call {
        Ok(c) => c,
        Err(e) => return Ok(tool_result(e, true)),
    };
    let (status, text) = forward(state, headers, call).await;
    let text = if text.is_empty() { format!("HTTP {}", status.as_u16()) } else { text };
    Ok(tool_result(if status.is_success() { text } else { format!("HTTP {}: {text}", status.as_u16()) }, !status.is_success()))
}

fn resource_list() -> Value {
    json!({"resources": [{
        "uri": "breezone://docs/api",
        "name": "api",
        "title": "Breezone HTTP API",
        "description": "The reference of the Breezone HTTP API (in French)",
        "mimeType": "text/markdown",
    }]})
}

/// Answers one JSON-RPC request (`None`: a notification or a response, nothing to answer).
async fn answer(state: &AppState, headers: &HeaderMap, msg: &Value) -> Option<Value> {
    let Some(obj) = msg.as_object() else {
        return Some(error(Value::Null, -32600, "invalid request"));
    };
    let id = obj.get("id").cloned();
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // A response of the client (to a request this server never sends).
        return id.is_none().then(|| error(Value::Null, -32600, "invalid request"));
    };
    let id = id?;
    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false}},
                "serverInfo": {"name": "breezone", "title": "Breezone", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tool_list()),
        "tools/call" => call_tool(state, headers, &params).await,
        "resources/list" => Ok(resource_list()),
        "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
        "resources/read" => match params.get("uri").and_then(Value::as_str) {
            Some(uri @ "breezone://docs/api") => Ok(json!({"contents": [{"uri": uri, "mimeType": "text/markdown", "text": API_REFERENCE}]})),
            _ => Err((-32002, "resource not found".into())),
        },
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => error(id, code, &message),
    })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// `POST /mcp`: one JSON-RPC message, or a batch of them.
pub async fn handle(State(state): State<AppState>, user: Result<AuthUser, ApiError>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    if let Err(e) = user {
        let mut resp = e.into_response();
        resp.headers_mut().insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer realm=\"breezone\""));
        return resp;
    }
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(error(Value::Null, -32700, "parse error"))).into_response(),
    };
    match msg {
        Value::Array(batch) => {
            let mut out = vec![];
            for m in &batch {
                if let Some(a) = answer(&state, &headers, m).await {
                    out.push(a);
                }
            }
            if out.is_empty() {
                StatusCode::ACCEPTED.into_response()
            } else {
                Json(Value::Array(out)).into_response()
            }
        }
        m => match answer(&state, &headers, &m).await {
            Some(a) => Json(a).into_response(),
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

/// `GET` / `DELETE /mcp`: no stream of messages from the server, no session to end.
pub async fn no_stream() -> Response {
    let mut resp = (StatusCode::METHOD_NOT_ALLOWED, Json(Map::from_iter([("error".to_string(), json!("POST only"))]))).into_response();
    resp.headers_mut().insert(ALLOW, HeaderValue::from_static("POST"));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_is_cut_in_sections() {
        let s = sections();
        assert!(s.len() > 10);
        assert!(s.iter().any(|(t, text)| t.starts_with("Automatismes") && text.contains("AutomationInput")));
        let (text, err) = reference(&json!({"section": "routines"}));
        assert!(!err && text.starts_with("## Routines"), "{text}");
    }

    #[test]
    fn the_generic_tool_stays_out_of_the_secrets() {
        assert!(generic_call(&json!({"method": "GET", "path": "/api/energy/day?date=2026-01-01"})).is_ok());
        for path in ["/api/users", "/api/users/2", "/api/tokens", "/api/auth/login", "/api/accounts/linky", "/api/../api/users", "/mcp", "/api/cloud"] {
            assert!(generic_call(&json!({"method": "GET", "path": path})).is_err(), "{path}");
        }
        assert!(generic_call(&json!({"method": "TRACE", "path": "/api/devices"})).is_err());
    }

    #[test]
    fn path_words_are_checked() {
        assert!(word(&json!({"id": "forecast_solar"}), "id").is_ok());
        assert!(word(&json!({"id": "../users"}), "id").is_err());
        assert!(word(&json!({"id": "a/b"}), "id").is_err());
    }
}
