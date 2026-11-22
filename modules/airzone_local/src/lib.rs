//! The local API of an Airzone WebServer (`http://<host>:3000/api/v1/`, no authentication), for
//! the installations of Breezone's core (`airzone.webserver@1`): the core keeps the zones, their
//! master zone, their history, the Airzone Cloud account and the schedules; this module only
//! speaks to the WebServer.
//!
//!   Read:  POST /api/v1/hvac       {"systemID": 0, "zoneID": 0}   (0, 0: everything)
//!   Write: PUT  /api/v1/hvac       {"systemID": N, "zoneID": M, "<param>": <value>, …}
//!   Info:  POST /api/v1/webserver  {}
//!
//! Never `/api/v1/iaq`: it brings the WebServer 4.27 down (the air quality comes in the zones).
//! Some firmwares answer an HTTP that is not strictly valid: it is then asked again on TCP, read
//! tolerantly.

use breezone_module_sdk::{host, json, module, Error, ReadUntil, Result, Service, Value};

/// Longest wait for an answer of the WebServer.
const TIMEOUT_MS: u32 = 8000;

/// "192.168.1.20", a name: letters, digits, dots, dashes (the port apart).
fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}

/// The JSON body of a raw HTTP answer (chunked or not).
pub fn parse_raw_http(buf: &[u8]) -> std::result::Result<Value, String> {
    let text = String::from_utf8_lossy(buf);
    let (head, body) = text.split_once("\r\n\r\n").or_else(|| text.split_once("\n\n")).ok_or("no HTTP header terminator")?;
    let chunked = head.to_ascii_lowercase().contains("transfer-encoding: chunked");
    let body = if chunked { dechunk(body) } else { body.to_string() };
    parse_json_body(body.as_bytes())
}

fn dechunk(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some((size_line, after)) = rest.split_once("\r\n") {
        let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("0"), 16).unwrap_or(0);
        if size == 0 || after.len() < size {
            break;
        }
        out.push_str(&after[..size]);
        rest = after[size..].trim_start_matches("\r\n");
    }
    out
}

/// A JSON document, tolerant of what comes before and after it.
fn parse_json_body(bytes: &[u8]) -> std::result::Result<Value, String> {
    let text = String::from_utf8_lossy(bytes);
    let start = text.find(['{', '[']).ok_or_else(|| format!("not JSON: {}", truncate(&text)))?;
    let end = text.rfind(['}', ']']).map(|i| i + 1).unwrap_or(text.len());
    breezone_module_sdk::serde_json::from_str(&text[start..end]).map_err(|e| format!("{e}: {}", truncate(&text)))
}

/// The local API tells its errors as {"errors": [{"systemID": "systemid not available"}]}
/// without zones: an error (`api: …`, for the core to tell apart).
fn check_api_errors(value: Value) -> Result<Value> {
    let has_data = value.get("data").is_some() || value.get("systems").is_some();
    if !has_data {
        if let Some(errors) = value.get("errors").and_then(|e| e.as_array()).filter(|e| !e.is_empty()) {
            return Err(Error::Invalid(format!("api: {}", Value::Array(errors.clone()))));
        }
    }
    Ok(value)
}

fn truncate(s: &str) -> String {
    s.chars().take(200).collect()
}

/// One call to the WebServer; asked again on TCP when its HTTP is not understood.
fn call(host_: &str, port: u16, method: &str, path: &str, body: &Value) -> Result<Value> {
    let url = format!("http://{host_}:{port}/api/v1/{path}");
    let payload = body.to_string();
    let value = match host::request(method, &url, &[("Content-Type", "application/json")], Some(payload.as_bytes()), TIMEOUT_MS) {
        Ok(answer) => match parse_json_body(&answer.body) {
            Ok(v) => v,
            Err(_) => raw_call(host_, port, method, path, &payload)?,
        },
        Err(Error::Unreachable(e)) if ["connect", "timed out", "timeout", "refused", "not allowed"].iter().any(|w| e.to_lowercase().contains(w)) => {
            return Err(Error::Unreachable(format!("{host_}:{port}: {e}")));
        }
        Err(_) => raw_call(host_, port, method, path, &payload)?,
    };
    check_api_errors(value)
}

fn raw_call(host_: &str, port: u16, method: &str, path: &str, payload: &str) -> Result<Value> {
    let request =
        format!("{method} /api/v1/{path} HTTP/1.1\r\nHost: {host_}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
    let answer =
        host::exchange(&format!("tcp://{host_}:{port}"), request.as_bytes(), ReadUntil::Close, TIMEOUT_MS).map_err(|e| Error::Unreachable(format!("{host_}:{port}: {e:?}")))?;
    parse_raw_http(&answer).map_err(Error::Invalid)
}

fn address(params: &Value) -> Result<(String, u16)> {
    let host_ = params["host"].as_str().map(str::trim).filter(|h| valid_host(h)).ok_or_else(|| Error::Invalid("invalid WebServer address".into()))?;
    let port = params["port"].as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0).unwrap_or(3000);
    Ok((host_.to_string(), port))
}

pub struct WebServer;

impl Service for WebServer {
    /// Nothing to check: no account.
    fn check() -> Result<()> {
        Ok(())
    }

    fn query(contract: &str, query: &str, params: Value) -> Result<Value> {
        if contract != "airzone.webserver@1" {
            return Err(Error::Unsupported(format!("{contract} {query}")));
        }
        let (host_, port) = address(&params)?;
        let answer = match query {
            "read" => call(&host_, port, "POST", "hvac", &json!({"systemID": 0, "zoneID": 0}))?,
            "write" => {
                let mut body = params["params"].as_object().cloned().ok_or_else(|| Error::Invalid("`params` is required".into()))?;
                for (k, v) in [("systemID", &params["system_id"]), ("zoneID", &params["zone_id"])] {
                    body.insert(k.into(), json!(v.as_i64().ok_or_else(|| Error::Invalid(format!("`{k}` is required")))?));
                }
                call(&host_, port, "PUT", "hvac", &Value::Object(body))?
            }
            "info" => call(&host_, port, "POST", "webserver", &json!({}))?,
            other => return Err(Error::Unsupported(format!("{contract} {other}"))),
        };
        Ok(json!({ "answer": answer }))
    }
}

module!(service: WebServer);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    #[test]
    fn raw_http_in_clear_or_chunked() {
        let plain = parse_raw_http(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"data\":[{\"systemID\":1,\"zoneID\":1}]}").unwrap();
        assert_eq!(plain["data"][0]["zoneID"], 1);
        let chunked = parse_raw_http(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1}\r\n0\r\n\r\n").unwrap();
        assert_eq!(chunked["a"], 1);
    }

    #[test]
    fn api_errors_are_errors() {
        assert!(matches!(check_api_errors(json!({"errors": [{"zoneID": "zoneid not available"}]})), Err(Error::Invalid(m)) if m.starts_with("api: ")));
        assert!(check_api_errors(json!({"data": [], "errors": []})).is_ok());
    }

    #[test]
    fn reads_and_writes_a_webserver() {
        testing::reset();
        testing::answer_json("POST", "http://192.168.1.20:3000/api/v1/hvac", &json!({"systems": [{"data": [{"systemID": 1, "zoneID": 1}]}]}));
        let read = WebServer::query("airzone.webserver@1", "read", json!({"host": "192.168.1.20", "port": 3000})).unwrap();
        assert_eq!(read["answer"]["systems"][0]["data"][0]["zoneID"], 1);
        testing::answer_json("PUT", "http://192.168.1.20:3000/api/v1/hvac", &json!({"data": [{"systemID": 1, "zoneID": 2, "setpoint": 21.5}]}));
        let written =
            WebServer::query("airzone.webserver@1", "write", json!({"host": "192.168.1.20", "port": 3000, "system_id": 1, "zone_id": 2, "params": {"setpoint": 21.5}})).unwrap();
        assert_eq!(written["answer"]["data"][0]["setpoint"], 21.5);
        let asked = testing::requests();
        assert!(asked.iter().all(|r| !r.contains("iaq")), "never the air quality endpoint: {asked:?}");
        assert!(WebServer::query("airzone.webserver@1", "read", json!({"host": "bad host", "port": 3000})).is_err());
    }
}
