//! Messages follow the language of the request; the profile remembers the user's choice.

use breezone_server::config::Config;
use reqwest::StatusCode;
use serde_json::{json, Value};

async fn serve(router: axum::Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

#[tokio::test]
async fn messages_follow_the_request_language() {
    let dir = std::env::temp_dir().join(format!("breezone-i18n-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = breezone_server::init_state(Config::for_tests(dir)).await.unwrap();
    let base = format!("http://127.0.0.1:{}", serve(breezone_server::app(state)).await);
    let http = reqwest::Client::new();
    let v: Value = http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap().json().await.unwrap();
    let token = v["access_token"].as_str().unwrap().to_string();

    let error = |lang: Option<&'static str>| {
        let (http, base, token) = (http.clone(), base.clone(), token.clone());
        async move {
            let mut req = http.get(format!("{base}/api/installations/999")).bearer_auth(&token);
            if let Some(l) = lang {
                req = req.header("Accept-Language", l);
            }
            let resp = req.send().await.unwrap();
            assert_eq!(resp.status(), StatusCode::NOT_FOUND);
            resp.json::<Value>().await.unwrap()["error"].as_str().unwrap().to_string()
        }
    };
    assert_eq!(error(None).await, "installation not found");
    assert_eq!(error(Some("fr-FR,fr;q=0.9,en;q=0.8")).await, "installation introuvable");
    assert_eq!(error(Some("es")).await, "instalación no encontrada");
    assert_eq!(error(Some("de")).await, "installation not found");

    // The activity feed is written in every language.
    let feed = |lang: &'static str| {
        let (http, base, token) = (http.clone(), base.clone(), token.clone());
        async move {
            let v: Value = http.get(format!("{base}/api/events")).bearer_auth(&token).header("Accept-Language", lang).send().await.unwrap().json().await.unwrap();
            v[0]["message"].as_str().unwrap().to_string()
        }
    };
    assert_eq!(feed("fr").await, "Compte administrateur créé");
    assert_eq!(feed("en").await, "Administrator account created");

    // Language of the profile.
    let me = |http: reqwest::Client| {
        let (base, token) = (base.clone(), token.clone());
        async move { http.get(format!("{base}/api/me")).bearer_auth(&token).send().await.unwrap().json::<Value>().await.unwrap() }
    };
    assert_eq!(me(http.clone()).await["language"], Value::Null);
    let put = |body: Value| http.put(format!("{base}/api/me/language")).bearer_auth(&token).json(&body).send();
    assert_eq!(put(json!({"language": "es"})).await.unwrap().status(), StatusCode::NO_CONTENT);
    assert_eq!(me(http.clone()).await["language"], "es");
    assert_eq!(put(json!({"language": "de"})).await.unwrap().status(), StatusCode::BAD_REQUEST);
    assert_eq!(put(json!({"language": null})).await.unwrap().status(), StatusCode::NO_CONTENT);
    assert_eq!(me(http.clone()).await["language"], Value::Null);
}
