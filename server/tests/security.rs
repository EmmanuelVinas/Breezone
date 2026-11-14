//! Browser protections on every response, and brute-force protection on sign-in.

use breezone_server::config::Config;
use serde_json::json;

#[tokio::test]
async fn responses_carry_the_security_headers_and_sign_in_is_throttled() {
    let dir = std::env::temp_dir().join(format!("breezone-security-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = breezone_server::init_state(Config::for_tests(dir)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, breezone_server::app(state).into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    let http = reqwest::Client::new();

    let resp = http.get(format!("{base}/health")).send().await.unwrap();
    let h = resp.headers();
    assert!(h["content-security-policy"].to_str().unwrap().contains("frame-ancestors 'none'"));
    assert_eq!(h["x-frame-options"], "DENY");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert_eq!(h["referrer-policy"], "no-referrer");

    http.post(format!("{base}/api/setup")).json(&json!({"username":"admin","password":"motdepasse1"})).send().await.unwrap();
    let login = |password: &'static str, from: &'static str| {
        http.post(format!("{base}/api/auth/login")).header("X-Forwarded-For", from).json(&json!({"username":"admin","password": password})).send()
    };
    for _ in 0..5 {
        assert_eq!(login("mauvais", "203.0.113.7").await.unwrap().status(), 401);
    }
    assert_eq!(login("motdepasse1", "203.0.113.7").await.unwrap().status(), 429, "locked from that address");
    assert_eq!(login("motdepasse1", "198.51.100.2").await.unwrap().status(), 200, "the owner still signs in from elsewhere");
}
