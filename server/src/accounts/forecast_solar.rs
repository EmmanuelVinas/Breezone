//! Forecast.Solar: an online service (no account, an API key optional) that forecasts the solar
//! production from the weather. It is set in Settings › Accounts and services, like the weather:
//! its panels (peak power, tilt, orientation) and the key; the place is the installation's city.
//! Connecting it adds (or sets again) the « Forecast.Solar » module, which reads the forecast.

use super::{record, save};
use crate::db;
use crate::drivers::DriverError;
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const KIND: &str = "forecast_solar";

#[derive(Serialize, Deserialize, Default)]
pub struct Secret {
    #[serde(default)]
    pub api_key: String,
}

/// A number typed in a form ("4,5" as well as "4.5").
fn number(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse().ok()
}

/// The configuration of the module, from the panels and the place of the home.
async fn module_config(state: &AppState, panels: &Value) -> ApiResult<Value> {
    let place = state.weather.place(None).await.ok_or_else(|| {
        ApiError::bad_request(crate::text!(
            "donnez d'abord la ville de l'installation : la prévision se fait pour sa position",
            "give the city of the installation first: the forecast is made for its position",
            "indique primero la ciudad de la instalación: la previsión se hace para su posición"
        ))
    })?;
    let mut config = panels.clone();
    config["latitude"] = json!(place.lat);
    config["longitude"] = json!(place.lon);
    config["timezone"] = json!(state.cfg.timezone.name());
    Ok(config)
}

/// Asks the forecast once with these panels: an answer, or why there is none.
async fn check(config: &Value, secret: &Secret) -> ApiResult<()> {
    let secret = json!({"api_key": secret.api_key}).to_string();
    let driver = crate::modules::open_source(KIND, None, config, Some(&secret)).await.map_err(ApiError::bad_request)?;
    driver.read().await.map(|_| ()).map_err(|e| match e {
        DriverError::Invalid(m) => ApiError::bad_request(m),
        e => ApiError::bad_gateway(format!("Forecast.Solar : {e}")),
    })
}

pub async fn connect(state: &AppState, kwp: &str, declination: &str, azimuth: &str, api_key: &str) -> ApiResult<()> {
    let kwp = number(kwp).filter(|k| *k > 0.0 && *k <= 1000.0).ok_or_else(|| ApiError::bad_request("donnez la puissance crête des panneaux, en kWc (par exemple 4,5)"))?;
    let declination = number(declination).unwrap_or(30.0).clamp(0.0, 90.0);
    let azimuth = number(azimuth).unwrap_or(0.0).clamp(-180.0, 180.0);
    let panels = json!({"kwp": kwp, "declination": declination, "azimuth": azimuth});
    let secret = Secret { api_key: api_key.trim().to_string() };
    let config = module_config(state, &panels).await?;
    check(&config, &secret).await?;
    save(state, KIND, &panels, &secret, None).await?;
    db::set_account_state(&state.db, KIND, "ok", None).await?;
    set_module(state, &config, &secret).await
}

/// The module that reads the forecast, added or set again with the panels of the service.
async fn set_module(state: &AppState, config: &Value, secret: &Secret) -> ApiResult<()> {
    let sources = db::list_sources(&state.db).await?;
    let id = match sources.iter().find(|s| s.driver == KIND) {
        Some(s) => {
            // The choices made on its device (ignored…) stay.
            let mut next = config.clone();
            let before: Value = serde_json::from_str(&s.config).unwrap_or(Value::Null);
            for key in ["ignored", "url"] {
                if let Some(v) = before.get(key) {
                    next[key] = v.clone();
                }
            }
            db::update_source(&state.db, s.id, &s.name, &next.to_string(), true).await?;
            s.id
        }
        None => db::insert_source(&state.db, KIND, "Forecast.Solar", &config.to_string()).await?,
    };
    let secret = json!({"api_key": secret.api_key}).to_string();
    db::set_source_secret(&state.db, id, Some(&crate::crypto::encrypt(&state.data_key, &secret))).await?;
    state.drivers.reload().await?;
    Ok(())
}

pub async fn test(state: &AppState) -> ApiResult<()> {
    let Some((panels, secret)) = super::load::<Secret>(state, KIND).await? else { return Err(ApiError::bad_request("Forecast.Solar n'est pas réglé")) };
    let config = module_config(state, &panels).await?;
    record(state, KIND, check(&config, &secret).await).await
}

/// The service forgotten: its module goes with it.
pub async fn forget(state: &AppState) -> ApiResult<()> {
    for s in db::list_sources(&state.db).await?.into_iter().filter(|s| s.driver == KIND) {
        db::delete_source(&state.db, s.id).await?;
    }
    state.drivers.reload().await?;
    Ok(())
}
