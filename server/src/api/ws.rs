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

use crate::access::Access;
use crate::auth;
use crate::i18n::{Lang, Text};
use crate::hub::HubMessage;
use crate::db;
use crate::AppState;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::sync::atomic::{AtomicI64, Ordering};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

pub async fn handler(ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>, State(state): State<AppState>) -> Response {
    match ws {
        Ok(ws) => {
            // The language of the upgrade request, unless the auth message gives another one.
            let lang = crate::i18n::current();
            ws.on_upgrade(move |socket| session(socket, state, lang))
        }
        Err(rejection) => {
            // Typically a reverse proxy that does not forward the WebSocket headers: live
            // updates cannot work through it. Say so once in a while in the activity feed.
            static LAST_HINT: AtomicI64 = AtomicI64::new(0);
            let now = crate::db::now();
            let last = LAST_HINT.load(Ordering::Relaxed);
            if now - last >= 3600 && LAST_HINT.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                state
                    .hub
                    .log("system", "warn", None, None, proxy_hint(), None)
                    .await;
            }
            tracing::warn!("websocket refused ({rejection}): {}", proxy_hint().en);
            (StatusCode::BAD_REQUEST, axum::Json(json!({ "error": proxy_hint() }))).into_response()
        }
    }
}

fn proxy_hint() -> Text {
    crate::text!(
        "Temps réel indisponible : la demande de WebSocket est arrivée sans ses en-têtes. \
Si Breezone est derrière un proxy inversé (NAS Synology, nginx…), ajoutez-y les en-têtes WebSocket \
(Synology : Proxy inversé › En-tête personnalisé › Créer › WebSocket).",
        "Live updates unavailable: the WebSocket request arrived without its headers. \
If Breezone is behind a reverse proxy (Synology NAS, nginx…), add the WebSocket headers to it \
(Synology: Reverse Proxy › Custom Header › Create › WebSocket).",
        "Tiempo real no disponible: la solicitud de WebSocket llegó sin sus encabezados. \
Si Breezone está detrás de un proxy inverso (NAS Synology, nginx…), añada los encabezados WebSocket \
(Synology: Proxy inverso › Encabezado personalizado › Crear › WebSocket)."
    )
}

async fn send_json(sink: &mut futures_util::stream::SplitSink<WebSocket, Message>, value: &Value) -> bool {
    sink.send(Message::Text(value.to_string().into())).await.is_ok()
}

async fn session(socket: WebSocket, state: AppState, lang: Lang) {
    let (sink, mut stream) = socket.split();

    // 1. Authentication message: `{type: "auth", token, lang?}`.
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next()).await;
    let auth = match first {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Value>(&text).ok().filter(|v| v["type"] == "auth"),
        _ => None,
    };
    let token = auth.as_ref().and_then(|v| v["token"].as_str().map(String::from));
    // Messages (installation states, activity feed) are written in this language.
    let lang = auth.as_ref().and_then(|v| v["lang"].as_str()).and_then(Lang::parse).unwrap_or(lang);
    crate::i18n::scope(lang, live(sink, stream, state, token)).await
}

async fn live(
    mut sink: futures_util::stream::SplitSink<WebSocket, Message>,
    mut stream: futures_util::stream::SplitStream<WebSocket>,
    state: AppState,
    token: Option<String>,
) {
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
    let Ok(mut access) = Access::of(&state.db, &user).await else { return };
    let hello = json!({ "type": "hello", "user": { "id": user.id, "username": user.username, "role": user.role } });
    let snapshot = snapshot_for(&state, &access).await;
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
                    // Only what this user can see.
                    Ok(HubMessage::Installation { installation }) => {
                        if !access.can_view(installation.id) {
                            continue;
                        }
                        serde_json::to_value(HubMessage::Installation { installation: access.apply(installation) }).unwrap_or_default()
                    }
                    Ok(HubMessage::Event { event }) if !event.installation_id.map_or(access.is_admin(), |id| access.can_view(id)) => continue,
                    Ok(HubMessage::AccessChanged { user_id }) => {
                        if user_id != user.id {
                            continue;
                        }
                        let Ok(a) = Access::of(&state.db, &user).await else { break };
                        access = a;
                        snapshot_for(&state, &access).await
                    }
                    Ok(m) => serde_json::to_value(&m).unwrap_or_default(),
                    Err(RecvError::Lagged(_)) => snapshot_for(&state, &access).await,
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

/// Installations the user can see, as a `snapshot` message.
async fn snapshot_for(state: &AppState, access: &Access) -> Value {
    let installations = access.filter(state.hub.snapshot().await);
    json!({ "type": "snapshot", "installations": installations })
}
