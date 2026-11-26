//! The Delta Dore account (the one of the Tydom app), only for the passwords of the Tydom gateways:
//! they keep theirs in Delta Dore's cloud. When it is connected, the account learns those of its
//! gateways and keeps them, encrypted with it; the Tydom sources then reach their gateway on the
//! local network alone. The client is the one of the Tydom module.

use breezone_module_sdk::{host, json, module, Error, Map, Result, Service, Value};

#[allow(dead_code)]
#[path = "../../tydom/src/cloud.rs"]
mod cloud;

pub struct Account;

impl Service for Account {
    fn check() -> Result<()> {
        let (email, password) = cloud::credentials(&host::account("deltadore")?)?;
        let token = cloud::login(&email, &password)?;
        // Not every account may list its sites at once: a Tydom source then asks for its own
        // gateway when it opens.
        let found = cloud::gateways(&token, None).unwrap_or_else(|e| {
            host::log("info", &format!("gateways not listed: {e:?}"));
            vec![]
        });
        let gateways: Map<String, Value> = found.iter().map(|(mac, p)| (mac.clone(), json!(p))).collect();
        let macs: Vec<&String> = gateways.keys().collect();
        host::account_update(&json!({"gateways": gateways, "shown": {"gateways": macs}}))
    }

    fn query(contract: &str, query: &str, _params: Value) -> Result<Value> {
        Err(Error::Unsupported(format!("{contract} {query}")))
    }
}

module!(service: Account);

#[cfg(test)]
mod tests {
    use super::*;
    use breezone_module_sdk::testing;

    #[test]
    fn the_account_keeps_the_passwords_of_its_gateways() {
        testing::reset();
        testing::account("deltadore", &json!({"email": "guillaume@example.com", "password": "bon"}));
        testing::answer_json("GET", "openid-configuration", &json!({"token_endpoint": "https://deltadoreadb2ciot.b2clogin.com/x/oauth2/v2.0/token"}));
        testing::answer_json("POST", "oauth2/v2.0/token", &json!({"access_token": "tok"}));
        testing::answer_json("GET", "sitesmanagement/api/v1/sites", &json!({"sites": [{"gateway": {"mac": "00:1a:25:12:34:56", "password": "gw"}}, {"id": 2}]}));
        Account::check().unwrap();
        let kept = host::account("deltadore").unwrap();
        assert_eq!(kept["gateways"], json!({"001A25123456": "gw"}));
        assert_eq!(kept["shown"]["gateways"], json!(["001A25123456"]));
    }

    #[test]
    fn a_listing_refused_still_connects_the_account() {
        testing::reset();
        testing::account("deltadore", &json!({"email": "guillaume@example.com", "password": "bon"}));
        testing::answer_json("GET", "openid-configuration", &json!({"token_endpoint": "https://deltadoreadb2ciot.b2clogin.com/x/oauth2/v2.0/token"}));
        testing::answer_json("POST", "oauth2/v2.0/token", &json!({"access_token": "tok"}));
        testing::answer("GET", "sitesmanagement/api/v1/sites", 400, "{}");
        Account::check().unwrap();
        assert_eq!(host::account("deltadore").unwrap()["gateways"], json!({}));
    }

    #[test]
    fn a_wrong_password_is_the_user_s_to_fix() {
        testing::reset();
        testing::account("deltadore", &json!({"email": "guillaume@example.com", "password": "faux"}));
        testing::answer_json("GET", "openid-configuration", &json!({"token_endpoint": "https://deltadoreadb2ciot.b2clogin.com/x/oauth2/v2.0/token"}));
        testing::answer("POST", "oauth2/v2.0/token", 400, r#"{"error_description": "AADB2C90225: invalid"}"#);
        assert!(matches!(Account::check(), Err(Error::Invalid(_))));
    }
}
