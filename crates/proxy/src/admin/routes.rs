// Admin server routes. Served on a separate localhost-only listener.

use crate::admin::auth::validate_admin_token;
use crate::admin::state::SharedState;
use crate::admin::ws::ws_handler;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use std::sync::Arc;
/// Check whether a host string (without port) is a localhost address.
fn is_localhost_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

/// Reject cross-origin requests to the admin API.
/// Parses the Origin URL and checks the host component exactly
/// to prevent bypass via e.g. `http://127.0.0.1.attacker.com`.
///
/// When no Origin header is present, validates the Host header instead
/// to guard against DNS rebinding attacks.
async fn reject_cross_origin(
    req: axum::extract::Request,
    next: middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    if let Some(origin) = req.headers().get("origin") {
        let origin_str = origin.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
        let is_local = match url::Url::parse(origin_str) {
            Ok(url) => url.host_str().is_some_and(is_localhost_host),
            Err(_) => false,
        };
        if !is_local {
            return Err(StatusCode::FORBIDDEN);
        }
    } else {
        // No Origin header: validate Host to prevent DNS rebinding attacks,
        // where an attacker's domain resolves to localhost, causing the
        // browser to send requests to our admin API.
        let host_valid = req
            .headers()
            .get("host")
            .and_then(|h| h.to_str().ok())
            .map(|h| {
                // Strip optional port. Bracketed IPv6 like "[::1]:9090"
                // must not be split naively on ':'.
                let host_part = if h.starts_with('[') {
                    // "[::1]:9090" -> "[::1]", or "[::1]" if no port
                    h.split_once(']').map_or(h, |(bracket, _)| {
                        // Include the closing bracket for is_localhost_host
                        &h[..bracket.len() + 1]
                    })
                } else {
                    // "localhost:9090" -> "localhost", but bare "::1" must
                    // not be split (contains colons but no port suffix).
                    // Only split if the part after the last colon is numeric.
                    match h.rsplit_once(':') {
                        Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
                        _ => h,
                    }
                };
                is_localhost_host(host_part)
            })
            .unwrap_or(false);
        if !host_valid {
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
        .route("/admin/api/env", get(get_env))
        .route("/admin/api/metrics", get(get_metrics))
        .route("/admin/api/requests", get(get_requests))
        .route("/admin/api/requests/{id}", get(get_request_by_id))
        .route("/admin/api/backends", get(get_backends))
        .route("/admin/api/keys", post(create_key).get(list_keys))
        .route("/admin/api/keys/{id}", delete(revoke_key))
        .route(
            "/admin/api/keys/{id}/spend",
            get(super::spend::get_key_spend),
        )
        .route("/admin/api/models", get(list_models).post(add_model))
        .route("/admin/api/models/{name}", delete(remove_model))
        .route("/admin/api/audit", get(get_audit_log))
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
            ("referrer-policy", "no-referrer"),
        ],
        SPA_HTML,
    )
}

// -- Env endpoint --

/// GET /admin/api/env -- effective environment variable values.
/// Secrets (API keys, tokens) are masked; plain config values are shown as-is.
async fn get_env() -> Json<serde_json::Value> {
    fn plain(key: &str) -> serde_json::Value {
        match std::env::var(key) {
            Ok(v) if !v.is_empty() => serde_json::Value::String(v),
            _ => serde_json::Value::Null,
        }
    }
    fn secret(key: &str) -> serde_json::Value {
        match std::env::var(key) {
            Ok(v) if !v.is_empty() => {
                serde_json::Value::String(anyllm_translate::util::redact::redact_secret(&v))
            }
            _ => serde_json::Value::Null,
        }
    }

    Json(serde_json::json!({
        // Core proxy config
        "BACKEND":            plain("BACKEND"),
        "LISTEN_PORT":        plain("LISTEN_PORT"),
        "BIG_MODEL":          plain("BIG_MODEL"),
        "SMALL_MODEL":        plain("SMALL_MODEL"),
        "RUST_LOG":           plain("RUST_LOG"),
        "LOG_BODIES":         plain("LOG_BODIES"),
        "PROXY_CONFIG":       plain("PROXY_CONFIG"),
        // OpenAI / compatible
        "OPENAI_BASE_URL":    plain("OPENAI_BASE_URL"),
        "OPENAI_API_FORMAT":  plain("OPENAI_API_FORMAT"),
        "OPENAI_API_KEY":     secret("OPENAI_API_KEY"),
        // Vertex AI
        "VERTEX_PROJECT":     plain("VERTEX_PROJECT"),
        "VERTEX_REGION":      plain("VERTEX_REGION"),
        "VERTEX_API_KEY":     secret("VERTEX_API_KEY"),
        // Gemini
        "GEMINI_BASE_URL":    plain("GEMINI_BASE_URL"),
        "GEMINI_API_KEY":     secret("GEMINI_API_KEY"),
        // Azure OpenAI
        "AZURE_OPENAI_ENDPOINT":    plain("AZURE_OPENAI_ENDPOINT"),
        "AZURE_OPENAI_DEPLOYMENT":  plain("AZURE_OPENAI_DEPLOYMENT"),
        "AZURE_OPENAI_API_KEY":     secret("AZURE_OPENAI_API_KEY"),
        "AZURE_OPENAI_API_VERSION": plain("AZURE_OPENAI_API_VERSION"),
        // AWS Bedrock
        "AWS_REGION":               plain("AWS_REGION"),
        "AWS_ACCESS_KEY_ID":        plain("AWS_ACCESS_KEY_ID"),
        "AWS_SECRET_ACCESS_KEY":    secret("AWS_SECRET_ACCESS_KEY"),
        "AWS_SESSION_TOKEN":        secret("AWS_SESSION_TOKEN"),
        // Auth
        "PROXY_API_KEYS":     secret("PROXY_API_KEYS"),
        "PROXY_OPEN_RELAY":   plain("PROXY_OPEN_RELAY"),
        // TLS
        "TLS_CLIENT_CERT_P12": plain("TLS_CLIENT_CERT_P12"),
        "TLS_CA_CERT":         plain("TLS_CA_CERT"),
        // Network
        "IP_ALLOWLIST":        plain("IP_ALLOWLIST"),
        "TRUST_PROXY_HEADERS": plain("TRUST_PROXY_HEADERS"),
        "WEBHOOK_URLS":        plain("WEBHOOK_URLS"),
        // Admin
        "ADMIN_PORT":               plain("ADMIN_PORT"),
        "ADMIN_DB_PATH":            plain("ADMIN_DB_PATH"),
        "ADMIN_LOG_RETENTION_DAYS": plain("ADMIN_LOG_RETENTION_DAYS"),
    }))
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
        // Allowlist: trace-level logging exposes HTTP headers (including API
        // keys) in log output. Arbitrary filter directives could also be used
        // to selectively leak data. Restrict to safe levels only.
        const ALLOWED_LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug"];
        let normalized = level.trim().to_lowercase();
        if !ALLOWED_LOG_LEVELS.contains(&normalized.as_str()) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!(
                        "invalid log_level '{}': allowed values are {:?}. \
                         Set RUST_LOG at startup for advanced filter directives.",
                        level, ALLOWED_LOG_LEVELS
                    )
                })),
            )
                .into_response();
        }
        db_writes.push(("log_level".to_string(), normalized));
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

    // Serialize config writes so concurrent requests cannot interleave
    // Phase 1 (SQLite) and Phase 2 (in-memory), which would leave them
    // inconsistent.
    let _config_guard = shared.config_write_lock.lock().await;

    // Phase 1: Persist to SQLite first. If the process crashes between
    // phases, the database is the source of truth and config is restored
    // on restart. Reversing the order would lose updates on crash.
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

    drop(_config_guard);

    // Broadcast config changes.
    for (key, value) in &db_writes {
        let _ = shared
            .events_tx
            .send(crate::admin::state::AdminEvent::ConfigChanged {
                key: key.clone(),
                value: value.clone(),
            });
        emit_audit(
            &shared,
            crate::admin::db::AuditEntry {
                id: None,
                timestamp: None,
                action: "config_changed".into(),
                target_type: "config".into(),
                target_id: Some(key.clone()),
                detail: Some(format!("value={value}")),
                source_ip: None,
            },
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "updated": db_writes.len(),
            "keys": db_writes.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        })),
    )
        .into_response()
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
            emit_audit(
                &shared,
                crate::admin::db::AuditEntry {
                    id: None,
                    timestamp: None,
                    action: "config_deleted".into(),
                    target_type: "config".into(),
                    target_id: Some(key.clone()),
                    detail: None,
                    source_ip: None,
                },
            );
            (StatusCode::OK, Json(serde_json::json!({"deleted": key}))).into_response()
        }
        Some(Ok(false)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "override not found"})),
        )
            .into_response(),
        Some(Err(e)) => {
            tracing::error!(error = %e, "delete_config_override failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal database error"})),
            )
                .into_response()
        }
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
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
    key_id: Option<i64>,
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
    let key_id = params.key_id;
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::query_request_log(
            conn,
            limit,
            offset,
            backend.as_deref(),
            since.as_deref(),
            status.as_deref(),
            key_id,
        )
    })
    .await
    {
        Some(Ok(entries)) => Json(serde_json::json!({
            "requests": entries,
            "limit": limit,
            "offset": offset,
        })),
        Some(Err(e)) => {
            tracing::error!(error = %e, "query_request_log failed");
            Json(serde_json::json!({
                "error": "internal database error",
                "requests": [],
            }))
        }
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
        Some(Err(e)) => {
            tracing::error!(error = %e, "get_request_by_id failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal database error"})),
            )
                .into_response()
        }
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
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

// --- Virtual API Key Management ---

#[derive(serde::Deserialize)]
struct CreateKeyRequest {
    description: Option<String>,
    expires_at: Option<String>,
    rpm_limit: Option<u32>,
    tpm_limit: Option<u32>,
    spend_limit: Option<f64>,
    role: Option<String>,
    max_budget_usd: Option<f64>,
    budget_duration: Option<String>,
    allowed_models: Option<Vec<String>>,
}

/// POST /admin/api/keys -- create a new virtual API key.
async fn create_key(
    State(shared): State<SharedState>,
    Json(body): Json<CreateKeyRequest>,
) -> axum::response::Response {
    let (raw_key, key_prefix, key_hash_hex) =
        super::keys::generate_virtual_key(&shared.hmac_secret);
    let role_str = body.role.as_deref().unwrap_or("developer");
    let role = super::keys::KeyRole::from_str_or_default(role_str);
    let result = super::state::with_db(&shared.db, {
        let hash = key_hash_hex.clone();
        let prefix = key_prefix.clone();
        let desc = body.description.clone();
        let exp = body.expires_at.clone();
        let rpm = body.rpm_limit;
        let tpm = body.tpm_limit;
        let spend = body.spend_limit;
        let role_s = role_str.to_string();
        let max_budget = body.max_budget_usd;
        let budget_dur = body.budget_duration.clone();
        let allowed_models_json = body
            .allowed_models
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok());
        move |conn| {
            super::db::insert_virtual_key(
                conn,
                &super::db::InsertVirtualKeyParams {
                    key_hash: &hash,
                    key_prefix: &prefix,
                    description: desc.as_deref(),
                    expires_at: exp.as_deref(),
                    rpm_limit: rpm,
                    tpm_limit: tpm,
                    spend_limit: spend,
                    role: &role_s,
                    max_budget_usd: max_budget,
                    budget_duration: budget_dur.as_deref(),
                    allowed_models: allowed_models_json,
                },
            )
        }
    })
    .await;

    match result {
        Some(Ok(id)) => {
            if let Some(hash_bytes) = super::keys::hash_from_hex(&key_hash_hex) {
                shared.virtual_keys.insert(
                    hash_bytes,
                    super::keys::VirtualKeyMeta {
                        id,
                        description: body.description.clone(),
                        expires_at: None,
                        rpm_limit: body.rpm_limit,
                        tpm_limit: body.tpm_limit,
                        rate_state: std::sync::Arc::new(super::keys::RateLimitState::new()),
                        role,
                        max_budget_usd: body.max_budget_usd,
                        budget_duration: body
                            .budget_duration
                            .as_deref()
                            .and_then(super::keys::BudgetDuration::parse),
                        period_start: Some(super::db::now_iso8601()),
                        period_spend_usd: 0.0,
                        allowed_models: body.allowed_models.clone(),
                    },
                );
            }
            emit_audit(
                &shared,
                crate::admin::db::AuditEntry {
                    id: None,
                    timestamp: None,
                    action: "key_created".into(),
                    target_type: "virtual_key".into(),
                    target_id: Some(id.to_string()),
                    detail: Some(format!(
                        "description={}, prefix={}",
                        body.description.as_deref().unwrap_or(""),
                        key_prefix
                    )),
                    source_ip: None,
                },
            );
            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "id": id,
                    "key": raw_key,
                    "key_prefix": key_prefix,
                    "description": body.description,
                    "created_at": super::db::now_iso8601(),
                    "expires_at": body.expires_at,
                    "rpm_limit": body.rpm_limit,
                    "tpm_limit": body.tpm_limit,
                    "spend_limit": body.spend_limit,
                    "role": role.as_str(),
                    "max_budget_usd": body.max_budget_usd,
                    "budget_duration": body.budget_duration,
                    "allowed_models": body.allowed_models,
                })),
            )
                .into_response()
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to create key"})),
        )
            .into_response(),
    }
}

/// GET /admin/api/keys -- list all virtual keys.
async fn list_keys(State(shared): State<SharedState>) -> axum::response::Response {
    let result = super::state::with_db(&shared.db, super::db::list_virtual_keys).await;
    match result {
        Some(Ok(keys)) => {
            let enriched: Vec<serde_json::Value> = keys
                .iter()
                .map(|k| {
                    serde_json::json!({
                        "id": k.id,
                        "key_prefix": k.key_prefix,
                        "description": k.description,
                        "created_at": k.created_at,
                        "expires_at": k.expires_at,
                        "revoked_at": k.revoked_at,
                        "rpm_limit": k.rpm_limit,
                        "tpm_limit": k.tpm_limit,
                        "spend_limit": k.spend_limit,
                        "total_spend": k.total_spend,
                        "total_requests": k.total_requests,
                        "total_tokens": k.total_tokens,
                        "status": k.status(),
                        "role": k.role,
                        "max_budget_usd": k.max_budget_usd,
                        "budget_duration": k.budget_duration,
                        "period_spend_usd": k.period_spend_usd,
                    })
                })
                .collect();
            Json(serde_json::json!({ "keys": enriched })).into_response()
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to list keys"})),
        )
            .into_response(),
    }
}

/// DELETE /admin/api/keys/{id} -- revoke a virtual key.
async fn revoke_key(
    State(shared): State<SharedState>,
    Path(id): Path<i64>,
) -> axum::response::Response {
    let result = super::state::with_db(&shared.db, move |conn| {
        super::db::revoke_virtual_key(conn, id)
    })
    .await;
    match result {
        Some(Ok(Some(row))) => {
            if let Some(hash_bytes) = super::keys::hash_from_hex(&row.key_hash) {
                shared.virtual_keys.remove(&hash_bytes);
            }
            emit_audit(
                &shared,
                crate::admin::db::AuditEntry {
                    id: None,
                    timestamp: None,
                    action: "key_revoked".into(),
                    target_type: "virtual_key".into(),
                    target_id: Some(id.to_string()),
                    detail: None,
                    source_ip: None,
                },
            );
            Json(serde_json::json!({
                "id": row.id,
                "revoked_at": row.revoked_at,
                "status": "revoked",
            }))
            .into_response()
        }
        Some(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Key not found or already revoked"})),
        )
            .into_response(),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to revoke key"})),
        )
            .into_response(),
    }
}

// ---- Dynamic model management ----

/// GET /admin/api/models -- list all routed model names and deployment counts.
async fn list_models(State(shared): State<SharedState>) -> impl IntoResponse {
    if let Some(ref router_lock) = shared.model_router {
        let router = router_lock.read().unwrap_or_else(|e| e.into_inner());
        let models: Vec<serde_json::Value> = router
            .list_models()
            .into_iter()
            .map(|(name, count)| {
                serde_json::json!({
                    "model_name": name,
                    "deployments": count,
                })
            })
            .collect();
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "strategy": format!("{:?}", router.strategy()),
                "models": models,
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "strategy": null,
                "models": [],
                "note": "no model router active (not using LiteLLM config)"
            })),
        )
            .into_response()
    }
}

/// Request body for POST /admin/api/models.
#[derive(serde::Deserialize)]
struct AddModelRequest {
    model_name: String,
    backend_name: String,
    actual_model: String,
    #[serde(default)]
    rpm: Option<u32>,
    #[serde(default)]
    tpm: Option<u64>,
    #[serde(default = "default_weight")]
    weight: u32,
}

fn default_weight() -> u32 {
    1
}

/// POST /admin/api/models -- add a deployment for a model name.
async fn add_model(
    State(shared): State<SharedState>,
    Json(body): Json<AddModelRequest>,
) -> impl IntoResponse {
    let Some(ref router_lock) = shared.model_router else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no model router active"})),
        )
            .into_response();
    };

    let deployment = std::sync::Arc::new(crate::config::model_router::Deployment::with_weight(
        body.backend_name.clone(),
        body.actual_model.clone(),
        body.rpm,
        body.tpm,
        body.weight,
    ));

    let mut router = router_lock.write().unwrap_or_else(|e| e.into_inner());
    router.add_deployment(body.model_name.clone(), deployment);

    tracing::info!(
        model_name = %body.model_name,
        backend = %body.backend_name,
        actual_model = %body.actual_model,
        "added model deployment via admin API"
    );

    emit_audit(
        &shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: "model_added".into(),
            target_type: "model".into(),
            target_id: Some(body.model_name.clone()),
            detail: Some(format!(
                "backend={}, actual_model={}",
                body.backend_name, body.actual_model
            )),
            source_ip: None,
        },
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "status": "added",
            "model_name": body.model_name,
            "backend_name": body.backend_name,
            "actual_model": body.actual_model,
        })),
    )
        .into_response()
}

/// DELETE /admin/api/models/{name} -- remove all deployments for a model.
async fn remove_model(
    State(shared): State<SharedState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let Some(ref router_lock) = shared.model_router else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no model router active"})),
        )
            .into_response();
    };

    let mut router = router_lock.write().unwrap_or_else(|e| e.into_inner());
    if router.remove_model(&name) {
        tracing::info!(model_name = %name, "removed model via admin API");
        emit_audit(
            &shared,
            crate::admin::db::AuditEntry {
                id: None,
                timestamp: None,
                action: "model_removed".into(),
                target_type: "model".into(),
                target_id: Some(name.clone()),
                detail: None,
                source_ip: None,
            },
        );
        (
            StatusCode::OK,
            Json(serde_json::json!({"status": "removed", "model_name": name})),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "model not found", "model_name": name})),
        )
            .into_response()
    }
}

// --- Audit log ---

#[derive(serde::Deserialize)]
struct AuditQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}

/// GET /admin/api/audit -- paginated audit log.
async fn get_audit_log(
    State(shared): State<SharedState>,
    Query(params): Query<AuditQuery>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).min(1000);
    let offset = params.offset.unwrap_or(0);
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::query_audit_log(conn, limit, offset)
    })
    .await
    {
        Some(Ok(entries)) => Json(serde_json::json!({
            "entries": entries,
            "limit": limit,
            "offset": offset,
        })),
        Some(Err(e)) => {
            tracing::error!(error = %e, "query_audit_log failed");
            Json(serde_json::json!({
                "error": "internal database error",
                "entries": [],
            }))
        }
        None => Json(serde_json::json!({
            "error": "task panicked",
            "entries": [],
        })),
    }
}

/// Fire-and-forget audit log write. Failures are logged but never block the caller.
fn emit_audit(shared: &SharedState, entry: crate::admin::db::AuditEntry) {
    let db = shared.db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = crate::admin::db::insert_audit_entry(&conn, &entry) {
            tracing::warn!(error = %e, action = %entry.action, "failed to write audit log");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    /// Build a minimal admin router for origin/host tests.
    fn test_router() -> Router {
        let shared = crate::admin::state::SharedState::new_for_test();
        let token = Arc::new("test-token".to_string());
        admin_router(shared, token)
    }

    #[tokio::test]
    async fn origin_localhost_allowed() {
        let app = test_router();
        let req = Request::get("/admin/api/config")
            .header("origin", "http://localhost:9090")
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_ne!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn origin_evil_rejected() {
        let app = test_router();
        let req = Request::get("/admin/api/config")
            .header("origin", "http://evil.com")
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn no_origin_localhost_host_allowed() {
        let app = test_router();
        let req = Request::get("/admin/api/config")
            .header("host", "localhost:9090")
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_ne!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn no_origin_127_host_allowed() {
        let app = test_router();
        let req = Request::get("/admin/api/config")
            .header("host", "127.0.0.1:9090")
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_ne!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn no_origin_evil_host_rejected() {
        let app = test_router();
        let req = Request::get("/admin/api/config")
            .header("host", "evil.com")
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn no_origin_no_host_rejected() {
        let app = test_router();
        let req = Request::get("/admin/api/config")
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
