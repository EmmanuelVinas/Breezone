//! Settings › Accounts: the linked accounts (see `accounts`), administrators only.

use crate::accounts::{self, AccountView};
use crate::auth::{AdminUser, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

pub async fn list(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<AccountView>>> {
    Ok(Json(accounts::list(&state).await?))
}

/// What connecting Forecast.Solar takes (the modules that are an account take the fields of their
/// form; Airzone is connected from its module).
#[derive(Deserialize, Default)]
pub struct ConnectInput {
    /// Forecast.Solar: the panels (peak power in kWp, tilt, orientation) and its optional key.
    #[serde(default)]
    pub kwp: String,
    #[serde(default)]
    pub declination: String,
    #[serde(default)]
    pub azimuth: String,
    #[serde(default)]
    pub api_key: String,
}

pub async fn connect(State(state): State<AppState>, AdminUser(user): AdminUser, Path(kind): Path<String>, Json(fields): Json<Value>) -> ApiResult<Json<AccountView>> {
    let input: ConnectInput = serde_json::from_value(fields.clone()).map_err(|e| ApiError::bad_request(e.to_string()))?;
    match kind.as_str() {
        // Its panels, with the position of the home.
        accounts::forecast_solar::KIND => accounts::forecast_solar::connect(&state, &input.kwp, &input.declination, &input.azimuth, &input.api_key).await?,
        // A module run by the host checks its account itself, from the fields of its form.
        k if accounts::is_module_account(k) => {
            accounts::connect_module(&state, k, &fields).await?;
            // A year of the meter at once: the history and its costs are there from the start.
            if k == accounts::linky::KIND {
                if let Err(e) = accounts::linky::refresh_from(&state, 365).await {
                    tracing::warn!("Linky not read: {}", e.message.en);
                }
            }
        }
        crate::cloud::ACCOUNT => return Err(ApiError::bad_request("le compte Airzone se connecte depuis Réglages › Modules › Airzone")),
        _ => return Err(ApiError::not_found("unknown account")),
    }
    log(&state, &user.username, &kind, true).await;
    Ok(Json(accounts::view(&state, &kind).await?))
}

/// Uses the account once more: its state says whether it still works.
pub async fn test(State(state): State<AppState>, AdminUser(user): AdminUser, Path(kind): Path<String>) -> ApiResult<Json<AccountView>> {
    let result = match kind.as_str() {
        accounts::forecast_solar::KIND => accounts::forecast_solar::test(&state).await,
        k if accounts::is_module_account(k) => accounts::test_module(&state, k).await,
        crate::cloud::ACCOUNT => crate::cloud::sync_stored(&state, &user.username).await.map(|_| ()),
        accounts::WEATHER => {
            state.weather.refresh().await;
            Ok(())
        }
        _ => return Err(ApiError::not_found("unknown account")),
    };
    // The view tells what failed (its last error): the test itself answers with it.
    if let Err(e) = result {
        tracing::info!("account {kind} tested: {}", e.message.en);
    }
    Ok(Json(accounts::view(&state, &kind).await?))
}

#[derive(Deserialize)]
pub struct EnabledInput {
    pub enabled: bool,
}

/// Switches an account or a service on or off, without forgetting it: what uses it does as if it
/// were not set (the Netatmo or EZVIZ module stops, the Tempo colours come from the public relay…).
pub async fn set_enabled(State(state): State<AppState>, AdminUser(user): AdminUser, Path(kind): Path<String>, Json(input): Json<EnabledInput>) -> ApiResult<Json<AccountView>> {
    if !accounts::is_kind(&kind) {
        return Err(ApiError::not_found("unknown account"));
    }
    crate::db::set_account_enabled(&state.db, &kind, input.enabled).await?;
    if accounts::comes_with_account(&kind) {
        for s in crate::db::list_sources(&state.db).await?.into_iter().filter(|s| s.driver == kind) {
            crate::db::update_source(&state.db, s.id, &s.name, &s.config, input.enabled).await?;
        }
        state.drivers.reload().await?;
    }
    if kind == accounts::WEATHER {
        let weather = state.weather.clone();
        tokio::spawn(async move { weather.refresh().await });
    }
    let text = if input.enabled {
        crate::text!("{0} activé", "{0} switched on", "{0} activado", kind.clone())
    } else {
        crate::text!("{0} désactivé", "{0} switched off", "{0} desactivado", kind.clone())
    };
    state.hub.log("system", "info", None, Some(&user.username), text, None).await;
    Ok(Json(accounts::view(&state, &kind).await?))
}

pub async fn delete(State(state): State<AppState>, AdminUser(user): AdminUser, Path(kind): Path<String>) -> ApiResult<StatusCode> {
    if !accounts::is_kind(&kind) {
        return Err(ApiError::not_found("unknown account"));
    }
    if accounts::is_service_only(&kind) {
        return Err(ApiError::bad_request("la météo se désactive, elle ne se retire pas"));
    }
    if kind == crate::cloud::ACCOUNT {
        crate::cloud::forget(&state).await?;
    } else {
        if kind == accounts::forecast_solar::KIND {
            accounts::forecast_solar::forget(&state).await?;
        }
        crate::db::delete_account(&state.db, &kind).await?;
    }
    log(&state, &user.username, &kind, false).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn log(state: &AppState, actor: &str, kind: &str, connected: bool) {
    let text = if connected {
        crate::text!("compte {0} connecté", "{0} account connected", "cuenta {0} conectada", kind.to_string())
    } else {
        crate::text!("compte {0} retiré", "{0} account removed", "cuenta {0} retirada", kind.to_string())
    };
    state.hub.log("system", "info", None, Some(actor), text, None).await;
}

#[derive(Deserialize)]
pub struct LinkyQuery {
    #[serde(default = "thirty")]
    pub days: u64,
    pub installation_id: Option<i64>,
}

fn thirty() -> u64 {
    30
}

/// What Enedis measured (Linky), day by day with its cost: for those who see the energy.
pub async fn linky(State(state): State<AppState>, user: AuthUser, Query(q): Query<LinkyQuery>) -> ApiResult<Json<accounts::linky::LinkyView>> {
    crate::api::energy::reads_energy(&state, &user).await?;
    Ok(Json(accounts::linky::view(&state, q.days, q.installation_id).await?))
}
