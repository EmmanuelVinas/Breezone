//! Client for the Airzone **local** API exposed by a WebServer on the LAN
//! (`http://<host>:3000/api/v1/`). This API has no authentication.
//!
//! Read:  POST /api/v1/hvac       { "systemID": 0, "zoneID": 0 }   (0,0 = everything)
//! Write: PUT  /api/v1/hvac       { "systemID": N, "zoneID": M, "<param>": <value>, ... }
//! Info:  POST /api/v1/webserver  {}
//!
//! Breezone's server is the only component talking to this API; the mobile app and the
//! web UI only ever talk to Breezone.

pub mod model;

pub use model::{master_zone, HvacResponse, OperationMode, ZoneData};

use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, thiserror::Error)]
pub enum AirzoneError {
    #[error("Airzone WebServer unreachable at {0}: {1}")]
    Unreachable(String, String),
    #[error("Airzone API error: {0}")]
    Api(String),
    #[error("invalid Airzone response: {0}")]
    InvalidResponse(String),
}

/// Client for one Airzone WebServer.
#[derive(Clone)]
pub struct AirzoneClient {
    host: String,
    port: u16,
    http: reqwest::Client,
    timeout: Duration,
}

impl AirzoneClient {
    pub fn new(host: &str, port: u16) -> Self {
        let timeout = Duration::from_secs(8);
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(4))
            .build()
            .expect("reqwest client");
        Self { host: host.to_string(), port, http, timeout }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}:{}/api/v1/{}", self.host, self.port, path)
    }

    /// Reads every zone of every system.
    pub async fn read_all(&self) -> Result<HvacResponse, AirzoneError> {
        let v = self.call("POST", "hvac", &json!({ "systemID": 0, "zoneID": 0 })).await?;
        serde_json::from_value(v).map_err(|e| AirzoneError::InvalidResponse(e.to_string()))
    }

    /// Writes parameters on a zone; `params` must contain the fields to change
    /// (e.g. {"on":1}, {"setpoint":21.5}, {"mode":3}). Returns the updated zones.
    pub async fn write(&self, system_id: i64, zone_id: i64, params: &serde_json::Map<String, Value>) -> Result<HvacResponse, AirzoneError> {
        let mut body = params.clone();
        body.insert("systemID".into(), json!(system_id));
        body.insert("zoneID".into(), json!(zone_id));
        let v = self.call("PUT", "hvac", &Value::Object(body)).await?;
        serde_json::from_value(v).map_err(|e| AirzoneError::InvalidResponse(e.to_string()))
    }

    /// WebServer information (MAC, firmware, Wi-Fi). Used for discovery and tests.
    pub async fn webserver_info(&self) -> Result<Value, AirzoneError> {
        self.call("POST", "webserver", &json!({})).await
    }

    /// Performs a call with `reqwest`; if the WebServer answers with an HTTP response
    /// that hyper refuses to parse (some Airzone firmwares are not strictly compliant),
    /// falls back to a tolerant raw HTTP/1.1 exchange.
    async fn call(&self, method: &str, path: &str, body: &Value) -> Result<Value, AirzoneError> {
        let req = match method {
            "PUT" => self.http.put(self.url(path)),
            _ => self.http.post(self.url(path)),
        };
        let value = match req.json(body).send().await {
            Ok(resp) => match resp.bytes().await {
                Ok(bytes) => parse_json_body(&bytes)?,
                Err(_) => self.raw_call(method, path, body).await?,
            },
            Err(e) if e.is_connect() || e.is_timeout() => {
                return Err(AirzoneError::Unreachable(format!("{}:{}", self.host, self.port), e.to_string()))
            }
            Err(_) => self.raw_call(method, path, body).await?,
        };
        check_api_errors(value)
    }

    async fn raw_call(&self, method: &str, path: &str, body: &Value) -> Result<Value, AirzoneError> {
        let addr = format!("{}:{}", self.host, self.port);
        let payload = body.to_string();
        let request = format!(
            "{method} /api/v1/{path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{payload}",
            host = self.host,
            len = payload.len(),
        );
        let exchange = async {
            let mut stream = TcpStream::connect(&addr).await?;
            stream.write_all(request.as_bytes()).await?;
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await?;
            Ok::<_, std::io::Error>(buf)
        };
        let buf = tokio::time::timeout(self.timeout, exchange)
            .await
            .map_err(|_| AirzoneError::Unreachable(addr.clone(), "timeout".into()))?
            .map_err(|e| AirzoneError::Unreachable(addr.clone(), e.to_string()))?;
        parse_raw_http(&buf)
    }
}

/// Extracts the JSON body from a raw HTTP response (handles chunked encoding).
pub fn parse_raw_http(buf: &[u8]) -> Result<Value, AirzoneError> {
    let text = String::from_utf8_lossy(buf);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .ok_or_else(|| AirzoneError::InvalidResponse("no HTTP header terminator".into()))?;
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

fn parse_json_body(bytes: &[u8]) -> Result<Value, AirzoneError> {
    let text = String::from_utf8_lossy(bytes);
    // Be tolerant to garbage before/after the JSON document.
    let start = text.find(['{', '[']).ok_or_else(|| AirzoneError::InvalidResponse(format!("not JSON: {}", truncate(&text))))?;
    let end = text.rfind(['}', ']']).map(|i| i + 1).unwrap_or(text.len());
    serde_json::from_str(&text[start..end]).map_err(|e| AirzoneError::InvalidResponse(format!("{e}: {}", truncate(&text))))
}

/// The local API reports errors as {"errors":[{"systemID":"systemid not available"}]} with
/// no zone data. Turn that into an error.
fn check_api_errors(value: Value) -> Result<Value, AirzoneError> {
    let has_data = value.get("data").is_some() || value.get("systems").is_some();
    if !has_data {
        if let Some(errors) = value.get("errors").and_then(|e| e.as_array()) {
            if !errors.is_empty() {
                return Err(AirzoneError::Api(Value::Array(errors.clone()).to_string()));
            }
        }
    }
    Ok(value)
}

fn truncate(s: &str) -> String {
    s.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_http_plain() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"data\":[{\"systemID\":1,\"zoneID\":1}]}";
        let v = parse_raw_http(raw).unwrap();
        assert_eq!(v["data"][0]["zoneID"], 1);
    }

    #[test]
    fn parses_raw_http_chunked() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1}\r\n0\r\n\r\n";
        let v = parse_raw_http(raw).unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn api_errors_are_detected() {
        let v = json!({"errors":[{"zoneID":"zoneid not available"}]});
        assert!(matches!(check_api_errors(v), Err(AirzoneError::Api(_))));
        let ok = json!({"data":[], "errors": []});
        assert!(check_api_errors(ok).is_ok());
    }
}
