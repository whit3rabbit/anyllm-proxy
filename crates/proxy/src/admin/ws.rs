// WebSocket endpoint for live admin dashboard updates.
// Auth via the first WebSocket message to avoid leaking the token in URLs/logs.

use crate::admin::state::SharedState;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use std::sync::Arc;

/// GET /admin/ws -- WebSocket for live dashboard updates.
/// Auth via the first WebSocket message to avoid leaking the token in URLs/logs.
/// The client must send `{"token": "<admin_token>"}` as its first message.
pub(crate) async fn ws_handler(
    State((shared, expected_token)): State<(SharedState, Arc<zeroize::Zeroizing<String>>)>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, shared, expected_token))
        .into_response()
}

/// Authenticate via the first WebSocket message, then stream events.
async fn handle_ws(
    mut socket: WebSocket,
    shared: SharedState,
    expected_token: Arc<zeroize::Zeroizing<String>>,
) {
    // Wait for the first message containing the auth token.
    let authenticated =
        tokio::time::timeout(std::time::Duration::from_secs(5), socket.recv()).await;

    let is_valid = match authenticated {
        Ok(Some(Ok(Message::Text(text)))) => {
            // Accept either raw token string or {"token": "..."} JSON.
            let token_str = text.to_string();
            let trimmed = token_str.trim();
            let expected = expected_token.as_str();
            if super::auth::constant_time_eq(trimmed, expected) {
                true
            } else {
                serde_json::from_str::<serde_json::Value>(&token_str)
                    .ok()
                    .and_then(|v| v.get("token")?.as_str().map(String::from))
                    .map(|t| super::auth::constant_time_eq(&t, expected))
                    .unwrap_or(false)
            }
        }
        _ => false,
    };

    if !is_valid {
        let _ = socket
            .send(Message::Text(
                r#"{"error":"authentication required: send token as first message"}"#.into(),
            ))
            .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    }

    // Send auth success confirmation.
    let _ = socket
        .send(Message::Text(r#"{"status":"authenticated"}"#.into()))
        .await;

    let mut rx = shared.events_tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        let json = match serde_json::to_string(&event) {
                            Ok(j) => j,
                            Err(_) => continue,
                        };
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break; // Client disconnected.
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!(skipped = n, "WebSocket client lagged, skipping events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break; // Channel closed, server shutting down.
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    _ => {} // Ignore other messages from client.
                }
            }
        }
    }
}
