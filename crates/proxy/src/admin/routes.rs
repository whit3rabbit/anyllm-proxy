// Admin server routes. Served on a separate localhost-only listener.

use crate::admin::auth::validate_admin_token;
use crate::admin::state::SharedState;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{delete, get},
    Json, Router,
};
use std::sync::Arc;

/// Build the admin router.
/// Token is used for auth middleware on all routes except /admin/health.
pub fn admin_router(shared: SharedState, token: Arc<String>) -> Router {
    // Public routes (no auth).
    let public = Router::new().route("/admin/health", get(health));

    // Protected routes (require admin token).
    let protected = Router::new()
        .route("/admin/api/config", get(get_config).put(put_config))
        .route("/admin/api/config/overrides", get(get_config_overrides))
        .route(
            "/admin/api/config/overrides/{key}",
            delete(delete_config_override),
        )
        .route("/admin/api/metrics", get(get_metrics))
        .route("/admin/api/requests", get(get_requests))
        .route("/admin/api/requests/{id}", get(get_request_by_id))
        .route("/admin/api/backends", get(get_backends))
        .with_state(shared.clone())
        .layer(middleware::from_fn_with_state(
            token.clone(),
            validate_admin_token,
        ));

    // WebSocket: auth via query param since browsers can't set headers on WS.
    let ws_state = (shared.clone(), token.clone());
    let ws_route = Router::new()
        .route("/admin/ws", get(ws_handler))
        .with_state(ws_state);

    // SPA serving (no auth required, token passed via query param in browser).
    let spa_route = Router::new()
        .route("/admin/", get(serve_spa))
        .route("/admin", get(serve_spa));

    // Merge all routes.
    public.merge(protected).merge(ws_route).merge(spa_route)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

/// Serve the embedded SPA HTML.
static SPA_HTML: &str = include_str!("../../admin-ui/index.html");

async fn serve_spa() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        SPA_HTML,
    )
}

// -- Config endpoints --

/// GET /admin/api/config -- effective config (env defaults + overrides).
async fn get_config(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    let config = shared.runtime_config.read().await;

    let mut backends = serde_json::Map::new();
    for (name, mapping) in &config.model_mappings {
        backends.insert(
            name.clone(),
            serde_json::json!({
                "big_model": mapping.big_model,
                "small_model": mapping.small_model,
            }),
        );
    }

    // Get overrides to mark which fields are overridden.
    let overrides = {
        let db = shared.db.lock().await;
        crate::admin::db::get_config_overrides(&db).unwrap_or_default()
    };
    let override_keys: Vec<String> = overrides.iter().map(|(k, _, _)| k.clone()).collect();

    Json(serde_json::json!({
        "log_level": config.log_level,
        "log_bodies": config.log_bodies,
        "backends": backends,
        "overridden_keys": override_keys,
    }))
}

/// PUT /admin/api/config -- update config overrides. Partial JSON body.
async fn put_config(
    State(shared): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let db = shared.db.lock().await;
    let mut config = shared.runtime_config.write().await;
    let mut changed_keys = Vec::new();

    // Update log_level if present.
    if let Some(level) = body.get("log_level").and_then(|v| v.as_str()) {
        crate::admin::db::set_config_override(&db, "log_level", level).ok();
        config.log_level = level.to_string();
        changed_keys.push(("log_level".to_string(), level.to_string()));
    }

    // Update log_bodies if present.
    if let Some(val) = body.get("log_bodies").and_then(|v| v.as_bool()) {
        let val_str = val.to_string();
        crate::admin::db::set_config_override(&db, "log_bodies", &val_str).ok();
        config.log_bodies = val;
        changed_keys.push(("log_bodies".to_string(), val_str));
    }

    // Update per-backend model mappings.
    if let Some(backends) = body.get("backends").and_then(|v| v.as_object()) {
        for (name, settings) in backends {
            if let Some(mapping) = config.model_mappings.get_mut(name) {
                if let Some(big) = settings.get("big_model").and_then(|v| v.as_str()) {
                    let key = format!("{name}.big_model");
                    crate::admin::db::set_config_override(&db, &key, big).ok();
                    mapping.big_model = big.to_string();
                    changed_keys.push((key, big.to_string()));
                }
                if let Some(small) = settings.get("small_model").and_then(|v| v.as_str()) {
                    let key = format!("{name}.small_model");
                    crate::admin::db::set_config_override(&db, &key, small).ok();
                    mapping.small_model = small.to_string();
                    changed_keys.push((key, small.to_string()));
                }
            }
        }
    }

    // Broadcast config changes.
    for (key, value) in &changed_keys {
        let _ = shared
            .events_tx
            .send(crate::admin::state::AdminEvent::ConfigChanged {
                key: key.clone(),
                value: value.clone(),
            });
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "updated": changed_keys.len(),
            "keys": changed_keys.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        })),
    )
}

/// GET /admin/api/config/overrides -- only SQLite overrides.
async fn get_config_overrides(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    let db = shared.db.lock().await;
    let overrides = crate::admin::db::get_config_overrides(&db).unwrap_or_default();

    let entries: Vec<serde_json::Value> = overrides
        .into_iter()
        .map(|(k, v, updated_at)| {
            serde_json::json!({
                "key": k,
                "value": v,
                "updated_at": updated_at,
            })
        })
        .collect();

    Json(serde_json::json!({ "overrides": entries }))
}

/// DELETE /admin/api/config/overrides/:key -- remove a single override.
async fn delete_config_override(
    State(shared): State<SharedState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let db = shared.db.lock().await;
    match crate::admin::db::delete_config_override(&db, &key) {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"deleted": key}))).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "override not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// -- Metrics endpoint --

/// GET /admin/api/metrics -- current metrics snapshot.
async fn get_metrics(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    let mut backends = serde_json::Map::new();
    let mut aggregate = crate::metrics::MetricsSnapshot::default();

    for (name, m) in shared.backend_metrics.iter() {
        let snap = m.snapshot();
        aggregate.requests_total += snap.requests_total;
        aggregate.requests_success += snap.requests_success;
        aggregate.requests_error += snap.requests_error;
        backends.insert(
            name.clone(),
            serde_json::to_value(&snap).unwrap_or_default(),
        );
    }

    let (p50, p95, p99) = {
        let db = shared.db.lock().await;
        compute_latency_percentiles(&db)
    };

    Json(serde_json::json!({
        "backends": backends,
        "total": {
            "requests_total": aggregate.requests_total,
            "requests_success": aggregate.requests_success,
            "requests_error": aggregate.requests_error,
        },
        "latency_p50_ms": p50,
        "latency_p95_ms": p95,
        "latency_p99_ms": p99,
        "error_rate": aggregate.error_rate(),
    }))
}

/// Compute p50, p95, p99 latency from the last 5 minutes of request log.
fn compute_latency_percentiles(
    conn: &rusqlite::Connection,
) -> (Option<u64>, Option<u64>, Option<u64>) {
    // Get latencies from recent requests, sorted.
    let cutoff = crate::admin::db::now_iso8601(); // We want last 5 minutes
    let mut stmt = conn
        .prepare(
            "SELECT latency_ms FROM request_log
             WHERE timestamp > datetime(?1, '-5 minutes')
             ORDER BY latency_ms ASC",
        )
        .ok();

    let latencies: Vec<u64> = stmt
        .as_mut()
        .and_then(|s| {
            s.query_map(rusqlite::params![cutoff], |row| {
                row.get::<_, i64>(0).map(|v| v as u64)
            })
            .ok()
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    if latencies.is_empty() {
        return (None, None, None);
    }

    let p = |pct: f64| -> u64 {
        let idx = ((pct / 100.0) * (latencies.len() as f64 - 1.0)).round() as usize;
        latencies[idx.min(latencies.len() - 1)]
    };

    (Some(p(50.0)), Some(p(95.0)), Some(p(99.0)))
}

// -- Request log endpoints --

#[derive(serde::Deserialize)]
struct RequestsQuery {
    limit: Option<u32>,
    offset: Option<u32>,
    backend: Option<String>,
    since: Option<String>,
    status: Option<String>,
}

/// GET /admin/api/requests -- paginated request log.
async fn get_requests(
    State(shared): State<SharedState>,
    Query(params): Query<RequestsQuery>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).min(1000);
    let offset = params.offset.unwrap_or(0);

    let db = shared.db.lock().await;
    match crate::admin::db::query_request_log(
        &db,
        limit,
        offset,
        params.backend.as_deref(),
        params.since.as_deref(),
        params.status.as_deref(),
    ) {
        Ok(entries) => Json(serde_json::json!({
            "requests": entries,
            "limit": limit,
            "offset": offset,
        })),
        Err(e) => Json(serde_json::json!({
            "error": e.to_string(),
            "requests": [],
        })),
    }
}

/// GET /admin/api/requests/:id -- single request detail.
async fn get_request_by_id(
    State(shared): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = shared.db.lock().await;
    match crate::admin::db::get_request_by_id(&db, &id) {
        Ok(Some(entry)) => {
            (StatusCode::OK, Json(serde_json::to_value(entry).unwrap())).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "request not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// -- Backends endpoint --

/// GET /admin/api/backends -- list configured backends with status.
async fn get_backends(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    let config = shared.runtime_config.read().await;

    let mut backends = Vec::new();
    for (name, mapping) in &config.model_mappings {
        let metrics = shared
            .backend_metrics
            .get(name)
            .map(|m| m.snapshot())
            .unwrap_or_default();

        backends.push(serde_json::json!({
            "name": name,
            "big_model": mapping.big_model,
            "small_model": mapping.small_model,
            "metrics": {
                "requests_total": metrics.requests_total,
                "requests_success": metrics.requests_success,
                "requests_error": metrics.requests_error,
            }
        }));
    }

    Json(serde_json::json!({ "backends": backends }))
}

// -- WebSocket endpoint --

/// GET /admin/ws -- WebSocket for live dashboard updates.
/// Auth via the first WebSocket message to avoid leaking the token in URLs/logs.
/// The client must send `{"token": "<admin_token>"}` as its first message.
async fn ws_handler(
    State((shared, expected_token)): State<(SharedState, Arc<String>)>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, shared, expected_token))
        .into_response()
}

/// Authenticate via the first WebSocket message, then stream events.
async fn handle_ws(mut socket: WebSocket, shared: SharedState, expected_token: Arc<String>) {
    // Wait for the first message containing the auth token.
    let authenticated = tokio::time::timeout(std::time::Duration::from_secs(5), socket.recv())
        .await;

    let is_valid = match authenticated {
        Ok(Some(Ok(Message::Text(text)))) => {
            // Accept either raw token string or {"token": "..."} JSON.
            let token_str = text.to_string();
            if token_str.trim() == expected_token.as_str() {
                true
            } else {
                serde_json::from_str::<serde_json::Value>(&token_str)
                    .ok()
                    .and_then(|v| v.get("token")?.as_str().map(String::from))
                    .map(|t| t == *expected_token)
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
