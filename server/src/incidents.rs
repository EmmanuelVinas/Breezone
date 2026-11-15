//! The last errors of the server (a request that failed on its side), kept in memory for the
//! administrators: what the logs of the container say, without opening them.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};

const KEPT: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub struct Incident {
    pub ts: i64,
    pub method: String,
    /// Without its query (it may hold a token).
    pub path: String,
    pub status: u16,
    /// What the client was told.
    pub message: String,
    /// What went wrong inside ("database is locked"…).
    pub cause: Option<String>,
}

/// Attached to an error response by `ApiError`, read by [`record`].
#[derive(Debug, Clone)]
pub struct Cause {
    pub message: String,
    pub cause: Option<String>,
}

static LAST: LazyLock<Mutex<VecDeque<Incident>>> = LazyLock::new(Default::default);

pub fn push(incident: Incident) {
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.len() == KEPT {
        last.pop_back();
    }
    last.push_front(incident);
}

/// The last incidents, the newest first.
pub fn list() -> Vec<Incident> {
    LAST.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
}

/// Keeps every response of the server that failed on its side (5xx).
pub async fn record(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let resp = next.run(req).await;
    if resp.status().is_server_error() {
        let cause = resp.extensions().get::<Cause>().cloned();
        push(Incident {
            ts: crate::db::now(),
            method,
            path,
            status: resp.status().as_u16(),
            message: cause.as_ref().map(|c| c.message.clone()).unwrap_or_else(|| resp.status().to_string()),
            cause: cause.and_then(|c| c.cause),
        });
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;

    #[tokio::test]
    async fn a_server_error_is_kept_with_its_cause_and_not_its_query() {
        let app = Router::new()
            .route("/api/fails", get(|| async { Err::<(), _>(crate::error::ApiError::internal("database error").with_cause("database is locked")) }))
            .route("/api/fine", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(record));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = reqwest::Client::new();
        http.get(format!("{base}/api/fine")).send().await.unwrap();
        let resp = http.get(format!("{base}/api/fails?token=secret")).send().await.unwrap();
        assert_eq!(resp.status(), 500);
        let found = list().into_iter().find(|i| i.path == "/api/fails").expect("kept");
        assert_eq!((found.method.as_str(), found.status, found.message.as_str()), ("GET", 500, "erreur de base de données"));
        assert_eq!(found.cause.as_deref(), Some("database is locked"));
        assert!(!list().iter().any(|i| i.path.contains("token") || i.path == "/api/fine"));
    }
}
