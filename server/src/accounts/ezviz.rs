//! EZVIZ (cameras, doorbells), with the account of its app, as Home Assistant does: no developer
//! key. The API is the one the EZVIZ app uses (not documented by EZVIZ): a login gives a session,
//! renewed with its refresh id, and a new login when that fails. Breezone keeps the MD5 of the
//! password, which is what EZVIZ asks, never the password itself.
//!
//! An account with two-step verification: the first login has EZVIZ send a code, the second gives
//! it; the code binds this server (its "feature code", the same at each login) to the account.
//!
//!   POST {ezviz}/v3/users/login/v5 (account, password md5, featureCode…) → {meta: {code}, loginSession, loginArea: {apiDomain}}
//!        1100: the account is in another region (`apiDomain`); 6002: a code is needed.
//!   POST {ezviz}/v3/sms/nologin/checkcode (from, bizType=TERMINAL_BIND) → sends the code
//!   PUT  {ezviz}/v3/apigateway/login (refreshSessionId, featureCode) → {sessionInfo: {sessionId, refreshSessionId}}
//!   GET  {ezviz}/v3/userdevices/v1/resources/pagelist?groupId=-1&limit=30&offset=…&filter=… → {deviceInfos, page: {hasNext}}

use super::{http, record};
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use md5::{Digest, Md5};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const KIND: &str = "ezviz";
const SERVICE: &str = "EZVIZ";
/// The name of the client given to EZVIZ at login: « Breezone » in base64.
const CLIENT: &str = "QnJlZXpvbmU=";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secret {
    pub account: String,
    /// MD5 of the password (hex), as EZVIZ asks it.
    pub password: String,
    /// Tells this server apart for EZVIZ (two-step verification binds it to the account).
    pub feature_code: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub rf_session_id: Option<String>,
    /// The API of the account's region, once known.
    #[serde(default)]
    pub api_url: Option<String>,
    /// The verification code of the cameras (on their label): it decrypts their pictures.
    #[serde(default)]
    pub image_code: Option<String>,
}

fn md5_hex(s: &str) -> String {
    hex::encode(Md5::digest(s.as_bytes()))
}

/// The same for a server and an account, so that a code asked at one login fits the next.
fn feature_code(key: &crate::crypto::Key, account: &str) -> String {
    md5_hex(&format!("{}{}", hex::encode(key), account.trim().to_lowercase()))
}

/// The address of a region as EZVIZ gives it (« apiieu.ezvizlife.com », sometimes only « eu »).
fn domain(v: &Value) -> Option<String> {
    let d = v.as_str()?.trim().trim_end_matches('/');
    if d.is_empty() {
        return None;
    }
    Some(if d.contains("://") {
        d.to_string()
    } else if !d.contains('.') {
        format!("https://apii{d}.ezvizlife.com")
    } else {
        format!("https://{d}")
    })
}

fn code_of(body: &Value) -> i64 {
    body["meta"]["code"].as_i64().or_else(|| body["resultCode"].as_str().and_then(|c| c.parse().ok())).unwrap_or(0)
}

fn unreachable(e: impl std::fmt::Display) -> ApiError {
    ApiError::bad_gateway(crate::text!("{0} injoignable : {1}", "{0} unreachable: {1}", "{0} inaccesible: {1}", SERVICE, e.to_string()))
}

/// A request as the EZVIZ app makes it.
fn request(method: Method, url: &str, s: &Secret) -> reqwest::RequestBuilder {
    http()
        .request(method, url)
        .header("featureCode", &s.feature_code)
        .header("clientType", "3")
        .header("osVersion", "")
        .header("clientVersion", "")
        .header("netType", "WIFI")
        .header("customno", "1000001")
        .header("ssid", "")
        .header("clientNo", "web_site")
        .header("appId", "ys7")
        .header("language", "fr_FR")
        .header("lang", "fr")
        .header("sessionId", s.session_id.as_deref().unwrap_or(""))
        .header("User-Agent", "okhttp/3.12.1")
}

async fn json_of(resp: reqwest::Response) -> ApiResult<Value> {
    let status = resp.status();
    let text = resp.text().await.map_err(unreachable)?;
    serde_json::from_str(&text).map_err(|_| unreachable(format!("HTTP {status}")))
}

/// Logs in (following the account to its region), with the code of the two-step verification
/// when given. Without it, an account that asks one has EZVIZ send it, and the answer says so.
async fn login(url: &str, s: &mut Secret, code: Option<&str>) -> ApiResult<()> {
    let mut base = s.api_url.clone().unwrap_or_else(|| url.trim_end_matches('/').to_string());
    s.session_id = None;
    let code = code.map(str::trim).filter(|c| !c.is_empty());
    for _ in 0..3 {
        let form = [
            ("account", s.account.as_str()),
            ("password", s.password.as_str()),
            ("featureCode", s.feature_code.as_str()),
            ("msgType", if code.is_some() { "3" } else { "0" }),
            ("bizType", if code.is_some() { "TERMINAL_BIND" } else { "" }),
            ("cuName", CLIENT),
            ("smsCode", code.unwrap_or("")),
        ];
        let resp = request(Method::POST, &format!("{base}/v3/users/login/v5"), s).form(&form).send().await.map_err(unreachable)?;
        let body = json_of(resp).await?;
        match code_of(&body) {
            200 => {
                s.session_id = body["loginSession"]["sessionId"].as_str().map(str::to_string);
                s.rf_session_id = body["loginSession"]["rfSessionId"].as_str().map(str::to_string);
                s.api_url = Some(domain(&body["loginArea"]["apiDomain"]).unwrap_or(base));
                if s.session_id.is_none() {
                    return Err(ApiError::bad_gateway("EZVIZ: no session"));
                }
                return Ok(());
            }
            1100 => base = domain(&body["loginArea"]["apiDomain"]).ok_or_else(|| ApiError::bad_gateway("EZVIZ: unknown region"))?,
            6002 => {
                let resp = request(Method::POST, &format!("{base}/v3/sms/nologin/checkcode"), s)
                    .form(&[("from", s.account.as_str()), ("bizType", "TERMINAL_BIND")])
                    .send()
                    .await
                    .map_err(unreachable)?;
                let sent = json_of(resp).await?;
                if code_of(&sent) != 200 {
                    return Err(ApiError::bad_gateway(crate::text!(
                        "EZVIZ demande un code de vérification mais ne l'envoie pas ({0})",
                        "EZVIZ asks for a verification code but does not send it ({0})",
                        "EZVIZ pide un código de verificación pero no lo envía ({0})",
                        code_of(&sent)
                    )));
                }
                return Err(ApiError::bad_request(crate::text!(
                    "EZVIZ vient d'envoyer un code de vérification (SMS ou e-mail) : saisissez-le dans « Code reçu », puis connectez à nouveau.",
                    "EZVIZ has just sent a verification code (text or e-mail): type it in “Code received”, then connect again.",
                    "EZVIZ acaba de enviar un código de verificación (SMS o correo): escríbalo en «Código recibido» y conecte de nuevo."
                )));
            }
            1012 => {
                return Err(ApiError::bad_request(crate::text!("le code de vérification est faux", "the verification code is wrong", "el código de verificación es incorrecto")))
            }
            1013 | 1014 => {
                return Err(ApiError::bad_request(crate::text!(
                    "EZVIZ refuse l'identifiant ou le mot de passe",
                    "EZVIZ refuses the user name or password",
                    "EZVIZ rechaza el usuario o la contraseña"
                )))
            }
            1015 => {
                return Err(ApiError::bad_request(crate::text!(
                    "le compte EZVIZ est bloqué : ouvrez l'application EZVIZ pour le débloquer",
                    "the EZVIZ account is locked: open the EZVIZ app to unlock it",
                    "la cuenta EZVIZ está bloqueada: abra la aplicación EZVIZ para desbloquearla"
                )))
            }
            other => {
                let message = body["meta"]["message"].as_str().unwrap_or_default().to_string();
                return Err(ApiError::bad_gateway(crate::text!("EZVIZ répond {0} : {1}", "EZVIZ answers {0}: {1}", "EZVIZ responde {0}: {1}", other, message)));
            }
        }
    }
    Err(ApiError::bad_gateway("EZVIZ: too many regions"))
}

/// A new session from the refresh id.
async fn renew(s: &mut Secret) -> ApiResult<()> {
    let (Some(base), Some(rf)) = (s.api_url.clone(), s.rf_session_id.clone()) else { return Err(ApiError::bad_gateway("EZVIZ: nothing to renew")) };
    let resp = request(Method::PUT, &format!("{base}/v3/apigateway/login"), s)
        .form(&[("refreshSessionId", rf.as_str()), ("featureCode", s.feature_code.as_str())])
        .send()
        .await
        .map_err(unreachable)?;
    let body = json_of(resp).await?;
    match (code_of(&body), body["sessionInfo"]["sessionId"].as_str(), body["sessionInfo"]["refreshSessionId"].as_str()) {
        (200, Some(id), Some(rf)) => {
            s.session_id = Some(id.to_string());
            s.rf_session_id = Some(rf.to_string());
            Ok(())
        }
        (code, ..) => Err(ApiError::bad_gateway(crate::text!(
            "EZVIZ refuse de renouveler la session ({0})",
            "EZVIZ refuses to renew the session ({0})",
            "EZVIZ rechaza renovar la sesión ({0})",
            code
        ))),
    }
}

/// The session is over: EZVIZ says so by HTTP 401, or in its answer.
fn expired(status: reqwest::StatusCode, body: &Value) -> bool {
    status == reqwest::StatusCode::UNAUTHORIZED || matches!(code_of(body), 401 | 403 | 1003 | 2003)
}

async fn send(s: &Secret, method: &Method, path: &str, query: &[(&str, String)]) -> ApiResult<(reqwest::StatusCode, Value)> {
    let base = s.api_url.clone().ok_or_else(|| ApiError::bad_gateway("EZVIZ: no region"))?;
    let resp = request(method.clone(), &format!("{base}{path}"), s).query(query).send().await.map_err(unreachable)?;
    let status = resp.status();
    Ok((status, json_of(resp).await?))
}

/// One session handling at a time: a renewal replaces the refresh id.
static SESSION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Calls the API of the account (for the module that uses it), renewing the session, or logging
/// in again, when it is over: `None` when no account is linked (or it is switched off).
pub async fn call_in(db: &db::Db, key: &crate::crypto::Key, url: &str, method: Method, path: &str, query: &[(&str, String)]) -> ApiResult<Option<Value>> {
    let _one = SESSION.lock().await;
    let Some((config, mut s)) = super::load_in::<Secret>(db, key, KIND).await? else { return Ok(None) };
    let mut changed = false;
    if s.session_id.is_none() || s.api_url.is_none() {
        login(url, &mut s, None).await?;
        changed = true;
    }
    let (mut status, mut body) = send(&s, &method, path, query).await?;
    if expired(status, &body) {
        if renew(&mut s).await.is_err() {
            let again = login(url, &mut s, None).await;
            if let Err(e) = &again {
                db::set_account_state(db, KIND, "error", Some(&e.message.store())).await?;
            }
            again?;
        }
        changed = true;
        (status, body) = send(&s, &method, path, query).await?;
    }
    if changed {
        super::save_in(db, key, KIND, &config, &s, None).await?;
    }
    let code = code_of(&body);
    if !status.is_success() || (code != 0 && code != 200) {
        let e = ApiError::bad_gateway(crate::text!("EZVIZ répond {0}", "EZVIZ answers {0}", "EZVIZ responde {0}", if code != 0 { code } else { status.as_u16() as i64 }));
        db::set_account_state(db, KIND, "error", Some(&e.message.store())).await?;
        return Err(e);
    }
    db::set_account_state(db, KIND, "ok", None).await?;
    Ok(Some(body))
}

/// The filters of the device list: the devices, their connection and their status.
pub const PAGE_FILTER: &str = "CLOUD,CONNECTION,STATUS,WIFI";

/// Every device of the account, page by page: (`deviceInfos` of all pages, the last page whole
/// for its per-device sections).
pub async fn devices_with<F, Fut>(mut get: F) -> ApiResult<Vec<Value>>
where
    F: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = ApiResult<Value>>,
{
    let mut out = vec![];
    let mut offset = 0;
    for _ in 0..20 {
        let page = get(offset).await?;
        let infos = page["deviceInfos"].as_array().cloned().unwrap_or_default();
        let more = page["page"]["hasNext"].as_bool().unwrap_or(false) && !infos.is_empty();
        offset += infos.len();
        for mut d in infos {
            // The sections are by serial: each device gets its own.
            if let Some(serial) = d["deviceSerial"].as_str().map(str::to_string) {
                for section in ["CONNECTION", "STATUS", "WIFI"] {
                    d[section] = page[section][&serial].clone();
                }
            }
            out.push(d);
        }
        if !more {
            break;
        }
    }
    Ok(out)
}

fn page_query(offset: usize) -> Vec<(&'static str, String)> {
    vec![("groupId", "-1".into()), ("limit", "30".into()), ("offset", offset.to_string()), ("filter", PAGE_FILTER.into())]
}

/// Logs in, reads the devices, and keeps everything.
async fn check(state: &AppState, mut s: Secret, code: Option<&str>) -> ApiResult<()> {
    let _one = SESSION.lock().await;
    login(&state.cfg.ezviz_url, &mut s, code).await?;
    let devices = devices_with(|offset| {
        let s = s.clone();
        async move {
            let (_, body) = send(&s, &Method::GET, "/v3/userdevices/v1/resources/pagelist", &page_query(offset)).await?;
            match code_of(&body) {
                200 => Ok(body),
                c => Err(ApiError::bad_gateway(crate::text!("EZVIZ répond {0}", "EZVIZ answers {0}", "EZVIZ responde {0}", c))),
            }
        }
    })
    .await?;
    let names: Vec<&str> = devices.iter().filter_map(|d| d["name"].as_str()).collect();
    let config = json!({ "email": s.account, "devices": names, "image_code": s.image_code.is_some() });
    super::save(state, KIND, &config, &s, None).await?;
    Ok(())
}

pub async fn connect(state: &AppState, account: &str, password: &str, code: &str, image_code: &str) -> ApiResult<()> {
    let account = account.trim();
    if account.is_empty() || password.is_empty() {
        return Err(ApiError::bad_request("l'identifiant et le mot de passe du compte EZVIZ sont requis"));
    }
    let image_code = image_code.trim();
    let s = Secret {
        account: account.to_string(),
        password: md5_hex(password),
        feature_code: feature_code(&state.data_key, account),
        session_id: None,
        rf_session_id: None,
        api_url: None,
        image_code: (!image_code.is_empty()).then(|| image_code.to_string()),
    };
    check(state, s, Some(code)).await?;
    db::set_account_state(&state.db, KIND, "ok", None).await?;
    ensure_source(state).await
}

pub async fn test(state: &AppState) -> ApiResult<()> {
    let Some((_, s)) = super::load::<Secret>(state, KIND).await? else { return Err(ApiError::bad_request("aucun compte EZVIZ")) };
    let s = Secret { api_url: None, ..s };
    record(state, KIND, check(state, s, None).await).await?;
    ensure_source(state).await
}

/// The EZVIZ module, added with the account (once): its cameras and doorbells.
pub async fn ensure_source(state: &AppState) -> ApiResult<()> {
    if db::account(&state.db, KIND).await?.and_then(|a| a.secret).is_none() {
        return Ok(());
    }
    if db::list_sources(&state.db).await?.iter().any(|s| s.driver == KIND) {
        return Ok(());
    }
    db::insert_source(&state.db, KIND, "EZVIZ", "{}").await?;
    state.drivers.reload().await?;
    Ok(())
}

/// The verification code kept for the pictures, if any.
pub async fn image_code_in(db: &db::Db, key: &crate::crypto::Key) -> Option<String> {
    super::load_in::<Secret>(db, key, KIND).await.ok().flatten().and_then(|(_, s)| s.image_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_is_an_address() {
        assert_eq!(domain(&json!("apiieu.ezvizlife.com")).as_deref(), Some("https://apiieu.ezvizlife.com"));
        assert_eq!(domain(&json!("eu")).as_deref(), Some("https://apiieu.ezvizlife.com"));
        assert_eq!(domain(&json!("http://127.0.0.1:8080/")).as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(domain(&json!("")), None);
    }

    #[test]
    fn the_feature_code_is_the_same_for_a_server_and_an_account() {
        let key = [7u8; 32];
        assert_eq!(feature_code(&key, "Moi@Exemple.fr "), feature_code(&key, "moi@exemple.fr"));
        assert_ne!(feature_code(&key, "moi@exemple.fr"), feature_code(&[8u8; 32], "moi@exemple.fr"));
        assert_eq!(feature_code(&key, "x").len(), 32);
    }
}
