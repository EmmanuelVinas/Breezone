//! WebSocket push channel.
//!
//! Protocol:
//! 1. client connects to `/api/ws` and sends `{"type":"auth","token":"<access token>"}`
//!    within 10 s (the token is never put in the URL, so it does not end up in proxy logs);
//! 2. server answers `{"type":"hello",...}` then `{"type":"snapshot","installations":[...]}`;
//! 3. server pushes `installation`, `installation_removed`, `event` and `routines_changed`
//!    messages (see `HubMessage`).
//!
//! The server pings every 25 s (keeps Cloudflare Tunnel connections alive) and closes the
//! socket as soon as the device session is revoked.

use crate::auth;
use crate::db;
use crate::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

pub async fn handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| session(socket, state))
}

async fn send_json(sink: &mut futures_util::stream::SplitSink<WebSocket, Message>, value: &Value) -> bool {
    sink.send(Message::Text(value.to_string().into())).await.is_ok()
}

async fn session(socket: WebSocket, state: AppState) {
    let (mut sink, mut stream) = socket.split();

    // 1. Authentication message.
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next()).await;
    let token = match first {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(|v| v["type"] == "auth")
            .and_then(|v| v["token"].as_str().map(String::from)),
        _ => None,
    };
    let user = match token {
        Some(t) => auth::authenticate(&state, &t).await.ok(),
        None => None,
    };
    let Some(user) = user else {
        let _ = send_json(&mut sink, &json!({ "type": "error", "error": "unauthorized" })).await;
        let _ = sink.close().await;
        return;
    };

    // 2. Hello + initial snapshot. Subscribe first so no update is missed.
    let mut rx = state.hub.subscribe();
    let hello = json!({ "type": "hello", "user": { "id": user.id, "username": user.username, "role": user.role } });
    let snapshot = json!({ "type": "snapshot", "installations": state.hub.snapshot().await });
    if !send_json(&mut sink, &hello).await || !send_json(&mut sink, &snapshot).await {
        return;
    }

    // 3. Push loop.
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    let mut session_check = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            msg = rx.recv() => {
                let value = match msg {
                    Ok(m) => serde_json::to_value(&m).unwrap_or_default(),
                    Err(RecvError::Lagged(_)) => json!({ "type": "snapshot", "installations": state.hub.snapshot().await }),
                    Err(RecvError::Closed) => break,
                };
                if !send_json(&mut sink, &value).await {
                    break;
                }
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) if text.trim() == "ping" => {
                        if sink.send(Message::Text("pong".into())).await.is_err() { break; }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            _ = ping.tick() => {
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() { break; }
            }
            _ = session_check.tick() => {
                if !matches!(db::session_by_id(&state.db, user.session_id).await, Ok(Some(_))) {
                    let _ = send_json(&mut sink, &json!({ "type": "error", "error": "session revoked" })).await;
                    break;
                }
            }
        }
    }
    let _ = sink.close().await;
}
