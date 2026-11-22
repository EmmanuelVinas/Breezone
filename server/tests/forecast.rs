//! The forecast curves of a device: the power Forecast.Solar expects along the day, against a fake
//! Forecast.Solar; a device that forecasts nothing says so.

mod support;

use axum::routing::get;
use axum::{Json, Router};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

async fn serve(router: Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

async fn fake_forecast_solar() -> u16 {
    let router = Router::new().route(
        "/estimate/{*rest}",
        get(|| async {
            Json(json!({
                "result": {
                    "watts": {"2026-11-19 08:00:00": 0, "2026-11-19 12:00:00": 2000, "2026-11-19 17:00:00": 0},
                    "watt_hours": {"2026-11-19 08:00:00": 0, "2026-11-19 12:00:00": 4000, "2026-11-19 17:00:00": 9000},
                    "watt_hours_day": {"2026-11-19": 9000}
                },
                "message": {"type": "success", "info": {"timezone": "UTC"}}
            }))
        }),
    );
    serve(router).await
}

async fn fake_shelly() -> u16 {
    let router = Router::new()
        .route("/rpc/Shelly.GetDeviceInfo", get(|| async { Json(json!({"id": "shellyplus1-a1", "app": "Plus1", "ver": "1.4.4", "gen": 2})) }))
        .route("/rpc/Shelly.GetStatus", get(|| async { Json(json!({"switch:0": {"id": 0, "output": false}, "sys": {"uptime": 10}})) }));
    serve(router).await
}

struct Client {
    base: String,
    http: reqwest::Client,
    token: String,
}

impl Client {
    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = self.http.request(method, format!("{}{}", self.base, path)).bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, serde_json::from_str(&resp.text().await.unwrap()).unwrap_or(Value::Null))
    }
}

#[tokio::test]
async fn a_solar_forecast_gives_its_curve_of_the_day() {
    let dir = std::env::temp_dir().join(format!("breezone-forecast-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = breezone_server::init_state(support::config(&dir)).await.unwrap();
    // The module as connecting the service adds it (that needs a city found by Open-Meteo).
    let forecast = fake_forecast_solar().await;
    let config = json!({"kwp": 3, "declination": 30, "azimuth": 0, "latitude": 43.6, "longitude": 1.4, "timezone": "UTC", "url": format!("http://127.0.0.1:{forecast}")});
    let sun_source = breezone_server::db::insert_source(&state.db, "forecast_solar", "Forecast.Solar", &config.to_string()).await.unwrap();
    state.drivers.reload().await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state)).await);
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username": "admin", "password": "motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let c = Client { base, http, token: v["access_token"].as_str().unwrap().to_string() };

    let shelly = fake_shelly().await;
    let (s, v) = c.call(Method::POST, "/api/sources", Some(json!({"driver": "shelly", "name": "Garage", "config": {"host": format!("127.0.0.1:{shelly}")}}))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");

    let (s, v) = c.call(Method::POST, &format!("/api/sources/{sun_source}/refresh"), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, devices) = c.call(Method::GET, "/api/devices", None).await;
    let find =
        |trait_id: &str| devices.as_array().unwrap().iter().find(|d| d["traits"].as_array().unwrap().iter().any(|t| t == trait_id)).cloned().unwrap_or_else(|| panic!("{devices}"));
    let sun = find("solar.forecast@1");
    let (s, v) = c.call(Method::GET, &format!("/api/devices/{}/forecast", sun["id"]), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let at = |h: u32| chrono::NaiveDate::from_ymd_opt(2026, 11, 19).unwrap().and_hms_opt(h, 0, 0).unwrap().and_utc().timestamp();
    assert_eq!(v["series"]["power"], json!([[at(8), 0.0], [at(12), 2000.0], [at(17), 0.0]]));

    // The trait says what it forecasts.
    let (_, traits) = c.call(Method::GET, "/api/traits", None).await;
    let described = traits.as_array().unwrap().iter().find(|t| t["trait"] == "solar.forecast").cloned().unwrap();
    assert_eq!(described["forecast"][0]["name"], "power");

    // A relay forecasts nothing.
    let relay = find("switch.relay@1");
    let (s, _) = c.call(Method::GET, &format!("/api/devices/{}/forecast", relay["id"]), None).await;
    assert_eq!(s, StatusCode::CONFLICT);
}
