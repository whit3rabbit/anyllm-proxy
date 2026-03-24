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
/// Reject cross-origin requests to the admin API.
/// Parses the Origin URL and checks the host component exactly
/// to prevent bypass via e.g. `http://127.0.0.1.attacker.com`.
async fn reject_cross_origin(
    req: axum::extract::Request,
    next: middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    if let Some(origin) = req.headers().get("origin") {
        let origin_str = origin.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
        let is_localhost = match url::Url::parse(origin_str) {
            Ok(url) => matches!(
                url.host_str(),
                Some("127.0.0.1") | Some("localhost") | Some("[::1]") | Some("::1")
            ),
            Err(_) => false,
        };
        if !is_localhost {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    Ok(next.run(req).await)
}

/// Build the admin router.
/// Token is used for auth middleware on all routes except /admin/health.
pub fn admin_router(shared: SharedState, token: Arc<String>) -> Router {
    // Public routes (no auth).
    let public = Router::new().route("/admin/health", get(health));

    // Protected routes (require admin token + localhost origin check).
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
        ))
        .layer(middleware::from_fn(reject_cross_origin));

    // WebSocket: auth via first message since browsers can't set headers on WS.
    // Origin check applied here too to prevent cross-site WebSocket hijacking.
    let ws_state = (shared.clone(), token.clone());
    let ws_route = Router::new()
        .route("/admin/ws", get(ws_handler))
        .with_state(ws_state)
        .layer(middleware::from_fn(reject_cross_origin));

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
        [
            ("content-type", "text/html; charset=utf-8"),
            // Restrictive CSP: inline script/style needed because SPA is a single HTML file.
            // frame-ancestors 'none' prevents clickjacking.
            (
                "content-security-policy",
                "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
                 connect-src 'self'; frame-ancestors 'none'",
            ),
            ("x-frame-options", "DENY"),
        ],
        SPA_HTML,
    )
}

// -- Config endpoints --

/// GET /admin/api/config -- effective config (env defaults + overrides).
async fn get_config(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    // Clone config snapshot and drop the read guard before any .await points.
    // std::sync::RwLockReadGuard is !Send, cannot be held across awaits.
    let (log_level, log_bodies, backends) = {
        let config = shared
            .runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner());
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
        (config.log_level.clone(), config.log_bodies, backends)
    };

    // Get overrides to mark which fields are overridden.
    let overrides = crate::admin::state::with_db(&shared.db, |conn| {
        crate::admin::db::get_config_overrides(conn).unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    let override_keys: Vec<String> = overrides.iter().map(|(k, _, _)| k.clone()).collect();

    Json(serde_json::json!({
        "log_level": log_level,
        "log_bodies": log_bodies,
        "backends": backends,
        "overridden_keys": override_keys,
    }))
}

/// PUT /admin/api/config -- update config overrides. Partial JSON body.
async fn put_config(
    State(shared): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Collect the key-value pairs to persist, then do all SQLite I/O
    // before touching in-memory state. This avoids holding the async
    // MutexGuard across block_in_place.
    let mut db_writes: Vec<(String, String)> = Vec::new();

    if let Some(level) = body.get("log_level").and_then(|v| v.as_str()) {
        if level == "trace" {
            tracing::warn!(
                "admin API: log_level set to 'trace' -- this may expose secrets in logs \
                 (e.g., Authorization headers, request bodies)"
            );
        }
        db_writes.push(("log_level".to_string(), level.to_string()));
    }
    if let Some(val) = body.get("log_bodies").and_then(|v| v.as_bool()) {
        if val {
            tracing::warn!(
                "admin API: log_bodies enabled -- request/response bodies will be logged, \
                 which may include sensitive data (PII, API keys in forwarded requests)"
            );
        }
        db_writes.push(("log_bodies".to_string(), val.to_string()));
    }
    if let Some(backends) = body.get("backends").and_then(|v| v.as_object()) {
        // Read current config to validate backend names exist
        let config = shared
            .runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner());
        for (name, settings) in backends {
            if config.model_mappings.contains_key(name) {
                if let Some(big) = settings.get("big_model").and_then(|v| v.as_str()) {
                    db_writes.push((format!("{name}.big_model"), big.to_string()));
                }
                if let Some(small) = settings.get("small_model").and_then(|v| v.as_str()) {
                    db_writes.push((format!("{name}.small_model"), small.to_string()));
                }
            }
        }
    }

    // Phase 1: Persist to SQLite before touching in-memory config.
    {
        let writes = db_writes.clone();
        crate::admin::state::with_db(&shared.db, move |conn| {
            for (key, value) in &writes {
                crate::admin::db::set_config_override(conn, key, value).ok();
            }
        })
        .await;
    }

    // Phase 2: Apply to in-memory config (no async lock held)
    {
        let mut config = shared
            .runtime_config
            .write()
            .unwrap_or_else(|e| e.into_inner());
        for (key, value) in &db_writes {
            match key.as_str() {
                "log_level" => {
                    config.log_level = value.clone();
                    if let Some(ref reload) = shared.log_reload {
                        if !reload(value) {
                            tracing::warn!(filter = value, "failed to apply log level change");
                        }
                    }
                }
                "log_bodies" => {
                    config.log_bodies = value == "true";
                }
                _ => {
                    if let Some((backend, field)) = key.split_once('.') {
                        if let Some(mapping) = config.model_mappings.get_mut(backend) {
                            match field {
                                "big_model" => mapping.big_model = value.clone(),
                                "small_model" => mapping.small_model = value.clone(),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    // Broadcast config changes.
    for (key, value) in &db_writes {
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
            "updated": db_writes.len(),
            "keys": db_writes.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        })),
    )
}

/// GET /admin/api/config/overrides -- only SQLite overrides.
async fn get_config_overrides(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    let overrides = crate::admin::state::with_db(&shared.db, |conn| {
        crate::admin::db::get_config_overrides(conn).unwrap_or_default()
    })
    .await
    .unwrap_or_default();

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
    let key_clone = key.clone();
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::delete_config_override(conn, &key_clone)
    })
    .await
    {
        Some(Ok(true)) => {
            (StatusCode::OK, Json(serde_json::json!({"deleted": key}))).into_response()
        }
        Some(Ok(false)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "override not found"})),
        )
            .into_response(),
        Some(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "task panicked"})),
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

    let (p50, p95, p99) = crate::admin::state::with_db(&shared.db, compute_latency_percentiles)
        .await
        .unwrap_or((None, None, None));

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

    let backend = params.backend;
    let since = params.since;
    let status = params.status;
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::query_request_log(
            conn,
            limit,
            offset,
            backend.as_deref(),
            since.as_deref(),
            status.as_deref(),
        )
    })
    .await
    {
        Some(Ok(entries)) => Json(serde_json::json!({
            "requests": entries,
            "limit": limit,
            "offset": offset,
        })),
        Some(Err(e)) => Json(serde_json::json!({
            "error": e.to_string(),
            "requests": [],
        })),
        None => Json(serde_json::json!({
            "error": "task panicked",
            "requests": [],
        })),
    }
}

/// GET /admin/api/requests/:id -- single request detail.
async fn get_request_by_id(
    State(shared): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::get_request_by_id(conn, &id)
    })
    .await
    {
        Some(Ok(Some(entry))) => {
            (StatusCode::OK, Json(serde_json::to_value(entry).unwrap())).into_response()
        }
        Some(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "request not found"})),
        )
            .into_response(),
        Some(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "task panicked"})),
        )
            .into_response(),
    }
}

// -- Backends endpoint --

/// GET /admin/api/backends -- list configured backends with status.
async fn get_backends(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    let config = shared
        .runtime_config
        .read()
        .unwrap_or_else(|e| e.into_inner());

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
