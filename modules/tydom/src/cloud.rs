//! The Delta Dore account, only to learn the password of a gateway: the Tydom 2.0 and Home keep
//! theirs in Delta Dore's cloud, never on their label. Also built into the `deltadore` module.
//!
//!   GET  {DISCOVERY}                                   → {token_endpoint}
//!   POST {token_endpoint} (multipart: username, password, grant_type=password, client_id, scope)
//!                                                      → {access_token}
//!   GET  {SITES}[?gateway_mac=001A25…] (Bearer)        → {sites: [{gateway: {mac, password}}]}

use breezone_module_sdk::{host, serde_json, Error, Result, Value};

pub const DISCOVERY: &str = "https://deltadoreadb2ciot.b2clogin.com/deltadoreadb2ciot.onmicrosoft.com/v2.0/.well-known/openid-configuration?p=B2C_1_AccountProviderROPC_SignIn";
const CLIENT_ID: &str = "8782839f-3264-472a-ab87-4d4e23524da4";
const SCOPE: &str = "openid profile offline_access \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/video_config \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/video_allowed \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/sites_management_allowed \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/sites_management_gateway_credentials \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/sites_management_camera_credentials \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/comptage_europe_collect_reader \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/comptage_europe_site_config_contributor \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/pilotage_allowed \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/consent_mgt_contributor \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/b2caccountprovider_manage_account \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/b2caccountprovider_allow_view_account \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/tydom_backend_allowed \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/websocket_remote_access \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/orkestrator_device \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/orkestrator_view \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/orkestrator_space \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/orkestrator_connector \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/orkestrator_endpoint \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/rule_management_allowed \
https://deltadoreadb2ciot.onmicrosoft.com/iotapi/collect_read_datas";
pub const SITES: &str = "https://prod.iotdeltadore.com/sitesmanagement/api/v1/sites";
const TIMEOUT_MS: u32 = 20_000;

/// "001A25123456" from "00:1a:25:12:34:56", or nothing.
pub fn normalized_mac(s: &str) -> Option<String> {
    let m: String = s.chars().filter(char::is_ascii_hexdigit).collect::<String>().to_uppercase();
    (m.len() == 12).then_some(m)
}

/// The e-mail and password of the Delta Dore account, as Breezone keeps it.
pub fn credentials(account: &Value) -> Result<(String, String)> {
    let email = account["email"].as_str().map(str::trim).unwrap_or_default();
    let password = account["password"].as_str().unwrap_or_default();
    if email.is_empty() || password.is_empty() {
        return Err(Error::Invalid("l'e-mail et le mot de passe du compte Delta Dore sont requis".into()));
    }
    Ok((email.to_string(), password.to_string()))
}

fn multipart(fields: &[(&str, &str)]) -> (String, Vec<u8>) {
    let boundary = "breezone-c7f3a1e92b4d";
    let mut body = String::new();
    for (name, value) in fields {
        body.push_str(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
    }
    body.push_str(&format!("--{boundary}--\r\n"));
    (format!("multipart/form-data; boundary={boundary}"), body.into_bytes())
}

fn detail(body: &[u8]) -> String {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let text = v["error_description"].as_str().or_else(|| v["message"].as_str()).map(str::to_string).unwrap_or_else(|| String::from_utf8_lossy(body).to_string());
    // B2C's messages start with a code ("AADB2C90225: …") and end with a trace.
    text.lines().next().unwrap_or_default().chars().take(160).collect()
}

/// An access to the account's API.
pub fn login(email: &str, password: &str) -> Result<String> {
    let (status, discovery) = host::get_json(DISCOVERY, TIMEOUT_MS)?;
    let token_url =
        discovery["token_endpoint"].as_str().filter(|_| status == 200).ok_or_else(|| Error::Unreachable(format!("Delta Dore : connexion indisponible (HTTP {status})")))?;
    let (content_type, body) = multipart(&[("username", email), ("password", password), ("grant_type", "password"), ("client_id", CLIENT_ID), ("scope", SCOPE)]);
    let a = host::request("POST", token_url, &[("Content-Type", &content_type)], Some(&body), TIMEOUT_MS)?;
    match a.status {
        200..=299 => a.json()?["access_token"].as_str().map(str::to_string).ok_or_else(|| Error::Unreachable("Delta Dore : pas d'accès dans la réponse".into())),
        400 | 401 | 403 => Err(Error::Invalid(format!("Delta Dore refuse l'e-mail ou le mot de passe ({})", detail(&a.body)))),
        s => Err(Error::Unreachable(format!("Delta Dore répond {s} : {}", detail(&a.body)))),
    }
}

/// The gateways the account reaches, with their password: (MAC, password). `mac` asks for that
/// one only.
pub fn gateways(token: &str, mac: Option<&str>) -> Result<Vec<(String, String)>> {
    let url = match mac {
        Some(m) => format!("{SITES}?gateway_mac={m}"),
        None => SITES.to_string(),
    };
    let a = host::request("GET", &url, &[("Authorization", &format!("Bearer {token}")), ("Accept", "application/json")], None, TIMEOUT_MS)?;
    if !(200..300).contains(&a.status) {
        return Err(Error::Unreachable(format!("Delta Dore : liste des passerelles refusée (HTTP {}) : {}", a.status, detail(&a.body))));
    }
    Ok(a.json()?["sites"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let g = &s["gateway"];
            Some((normalized_mac(g["mac"].as_str()?)?, g["password"].as_str().filter(|p| !p.is_empty())?.to_string()))
        })
        .collect())
}

/// The password of the gateway `mac`, asked to the account.
pub fn gateway_password(account: &Value, mac: &str) -> Result<String> {
    let (email, password) = credentials(account)?;
    let token = login(&email, &password)?;
    let found = gateways(&token, Some(mac))?;
    found
        .iter()
        .find(|(m, _)| m == mac)
        .map(|(_, p)| p.clone())
        .ok_or_else(|| Error::Invalid(format!("la passerelle {mac} n'est pas dans ce compte Delta Dore (vérifie son adresse MAC)")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::{json, testing};

    pub fn cloud(mac_answered: &str) {
        testing::answer_json("GET", "openid-configuration", &json!({"token_endpoint": "https://deltadoreadb2ciot.b2clogin.com/x/oauth2/v2.0/token?p=b2c"}));
        testing::respond("POST", "oauth2/v2.0/token", |headers, body| {
            let body = String::from_utf8_lossy(body);
            let ok = headers.iter().any(|(n, v)| *n == "Content-Type" && v.starts_with("multipart/form-data; boundary="))
                && body.contains("name=\"username\"\r\n\r\nguillaume@example.com\r\n")
                && body.contains("name=\"grant_type\"\r\n\r\npassword\r\n")
                && body.contains("sites_management_gateway_credentials");
            let (status, answer) = if ok && body.contains("\r\n\r\nbon\r\n") {
                (200, json!({"access_token": "tok"}))
            } else {
                (400, json!({"error": "invalid_grant", "error_description": "AADB2C90225: The username or password provided in the request are invalid.\r\nCorrelation ID: x"}))
            };
            breezone_module_sdk::host::Answer { status, headers: vec![], body: answer.to_string().into_bytes() }
        });
        let mac = mac_answered.to_string();
        testing::respond("GET", "sitesmanagement/api/v1/sites", move |headers, _| {
            let authorized = headers.iter().any(|(n, v)| *n == "Authorization" && *v == "Bearer tok");
            let body = json!({"sites": [{"id": 1, "gateway": {"mac": mac, "password": "secret-gw"}}]});
            breezone_module_sdk::host::Answer { status: if authorized { 200 } else { 401 }, headers: vec![], body: body.to_string().into_bytes() }
        });
    }

    #[test]
    fn the_password_of_the_gateway_comes_from_the_account() {
        testing::reset();
        cloud("001a25123456");
        let account = json!({"email": "guillaume@example.com", "password": "bon"});
        assert_eq!(gateway_password(&account, "001A25123456").unwrap(), "secret-gw");
        assert!(testing::requests().iter().any(|r| r.ends_with("/sites?gateway_mac=001A25123456")));
        assert!(matches!(gateway_password(&account, "001A25999999"), Err(Error::Invalid(_))), "another gateway");
        let wrong = gateway_password(&json!({"email": "guillaume@example.com", "password": "faux"}), "001A25123456");
        assert!(matches!(&wrong, Err(Error::Invalid(m)) if m.contains("AADB2C90225") && !m.contains("Correlation")), "{wrong:?}");
        assert!(matches!(credentials(&json!({"email": "a@b"})), Err(Error::Invalid(_))));
    }
}
