//! The API announces its version, and refuses clients older than the oldest one it still serves.

use breezone_server::config::Config;
use serde_json::Value;

#[tokio::test]
async fn the_api_version_is_announced_and_old_clients_are_told_to_update() {
    let dir = std::env::temp_dir().join(format!("breezone-apiversion-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = breezone_server::init_state(Config::for_tests(dir)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, breezone_server::app(state).into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    let http = reqwest::Client::new();

    let health: Value = http.get(format!("{base}/health")).send().await.unwrap().json().await.unwrap();
    assert_eq!(health["status"], "ok");
    assert_eq!(health["api_version"], breezone_server::api::API_VERSION);
    assert_eq!(health["min_client_api"], breezone_server::api::MIN_CLIENT_API);

    // Clients without the header (older apps, scripts) and current clients are served.
    let setup = |api: Option<u32>| {
        let mut request = http.get(format!("{base}/api/setup"));
        if let Some(api) = api {
            request = request.header("Breezone-Api", api.to_string());
        }
        request.send()
    };
    assert_eq!(setup(None).await.unwrap().status(), 200);
    assert_eq!(setup(Some(breezone_server::api::API_VERSION)).await.unwrap().status(), 200);

    // A client older than the oldest API still served is told to update.
    let resp = setup(Some(breezone_server::api::MIN_CLIENT_API - 1)).await.unwrap();
    assert_eq!(resp.status(), 426);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["min_client_api"], breezone_server::api::MIN_CLIENT_API);
}
