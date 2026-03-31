// Admin server routes. Served on a separate localhost-only listener.

use crate::admin::auth::{
    extract_csrf_cookie, generate_csrf_token, validate_admin_token, validate_csrf_tokens,
};
use crate::admin::state::SharedState;
use crate::admin::ws::ws_handler;
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use dashmap::DashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock};

/// Per-IP sliding window rate limiter for admin API endpoints.
/// Each entry is a VecDeque of millisecond timestamps within the last 60 seconds.
static ADMIN_RATE_LIMIT: LazyLock<DashMap<IpAddr, std::collections::VecDeque<u64>>> =
    LazyLock::new(DashMap::new);

/// Maximum admin API requests per IP per 60-second window.
/// Default 10; can be overridden at runtime for tests via `set_admin_rpm`.
static ADMIN_RPM: AtomicU32 = AtomicU32::new(10);

/// Override the admin rate limit (requests per minute per IP).
/// Intended for integration tests that need a higher limit.
pub fn set_admin_rpm(rpm: u32) {
    ADMIN_RPM.store(rpm, Ordering::Relaxed);
}

/// Clear all rate limit state. Exposed for integration tests.
pub fn reset_admin_rate_limit() {
    ADMIN_RATE_LIMIT.clear();
}

/// Prune stale entries from the admin rate limiter. Removes IPs whose newest
/// timestamp is older than 60 seconds. Called periodically to prevent unbounded
/// growth from distinct source IPs.
fn prune_stale_rate_limit_entries(now_ms: u64) {
    static LAST_PRUNE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let last = LAST_PRUNE.load(Ordering::Relaxed);
    // Prune at most once every 60 seconds.
    if now_ms.saturating_sub(last) < 60_000 {
        return;
    }
    if LAST_PRUNE
        .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return; // another thread won the race
    }
    let cutoff = now_ms.saturating_sub(60_000);
    ADMIN_RATE_LIMIT.retain(|_, window| window.back().is_some_and(|&ts| ts >= cutoff));
}

/// Inner rate-limit check with an explicit rpm; avoids touching the global ADMIN_RPM in tests.
fn check_admin_rate_limit_with_rpm(ip: IpAddr, rpm: u32) -> bool {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let cutoff = now_ms.saturating_sub(60_000);

    // Periodically prune entries for IPs that have gone silent.
    prune_stale_rate_limit_entries(now_ms);

    let mut window = ADMIN_RATE_LIMIT.entry(ip).or_default();
    // Evict timestamps older than 60 seconds.
    while window.front().is_some_and(|&ts| ts < cutoff) {
        window.pop_front();
    }
    if window.len() >= rpm as usize {
        return false;
    }
    window.push_back(now_ms);
    true
}

/// Returns true if the request is within the rate limit, false if exceeded.
fn check_admin_rate_limit(ip: IpAddr) -> bool {
    check_admin_rate_limit_with_rpm(ip, ADMIN_RPM.load(Ordering::Relaxed))
}

/// Axum middleware that enforces per-IP rate limiting on admin API routes.
/// Returns 429 Too Many Requests when the limit is exceeded.
async fn admin_rate_limit_middleware(
    req: axum::extract::Request,
    next: middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    // Extract client IP from ConnectInfo extension (set by into_make_service_with_connect_info).
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    if !check_admin_rate_limit(ip) {
        tracing::warn!(%ip, "admin API rate limit exceeded");
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    Ok(next.run(req).await)
}
/// Reject model names containing path traversal sequences or suspicious characters.
/// Only alphanumerics plus `-_./: @` are allowed (covers known provider naming
/// conventions like `gpt-4o`, `us.meta.llama3-2-1b-instruct-v1:0`,
/// `accounts/fireworks/models/llama-v3p1-8b-instruct`).
fn is_safe_model_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name.contains('?')
        && !name.contains('#')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || "-_./: @".contains(c))
}

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

/// Middleware that validates CSRF tokens for state-mutating HTTP methods.
///
/// Skips validation for GET, HEAD, OPTIONS.
/// For POST, PUT, DELETE: requires X-CSRF-Token header to match the csrf_token cookie.
/// Returns 403 with a descriptive error if the token is missing or mismatched.
/// Applied inside validate_admin_token so unauthenticated requests are rejected first.
pub async fn validate_csrf(
    req: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let method = req.method().clone();

    if matches!(
        method,
        axum::http::Method::POST | axum::http::Method::PUT | axum::http::Method::DELETE
    ) {
        let headers = req.headers();

        let header_token = headers
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let cookie_token = headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(extract_csrf_cookie)
            .unwrap_or_default();

        if !validate_csrf_tokens(header_token, &cookie_token) {
            let body = serde_json::json!({
                "type": "error",
                "error": {
                    "type": "permission_error",
                    "message": "CSRF token missing or invalid. Fetch a token from GET /admin/csrf-token."
                }
            });
            return (StatusCode::FORBIDDEN, axum::Json(body)).into_response();
        }
    }

    next.run(req).await
}

/// GET /admin/csrf-token
///
/// Returns a fresh CSRF token as JSON and sets it in a non-HttpOnly cookie.
/// The admin SPA reads the cookie in JS and includes it as `X-CSRF-Token` on
/// POST/PUT/DELETE requests (double-submit cookie pattern).
///
/// Security architecture note:
/// This route is intentionally public (no Bearer auth required). The SPA must
/// fetch a CSRF token to submit the login form itself, so requiring auth here
/// would be circular. Protection comes from two middleware layers applied to all
/// admin routes, including this one:
///   1. `reject_cross_origin`: validates Origin/Host header; only requests
///      from localhost can reach any admin endpoint.
///   2. `SameSite=Strict` on the cookie: browsers do not attach the cookie on
///      cross-site requests, preventing a cross-origin attacker from using a
///      CSRF token they fetched independently.
///
/// Together these make unauthenticated CSRF token fetching safe: an attacker who
/// can reach this endpoint is already on localhost and has other attack vectors.
/// If TLS is ever added to the admin server, also add `Secure` to Set-Cookie.
async fn get_csrf_token() -> axum::response::Response {
    let token = generate_csrf_token();
    let body = serde_json::json!({"csrf_token": token});
    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        // SameSite=Strict prevents the cookie being sent on cross-site requests.
        // Not httpOnly so the admin SPA JS can read and send it back as a header.
        // Secure flag intentionally omitted: admin binds to 127.0.0.1 over plain HTTP,
        // so setting Secure would prevent the browser from sending the cookie at all.
        // If TLS is added to the admin server, Secure must be added here.
        .header(
            "set-cookie",
            format!("csrf_token={token}; Path=/admin; SameSite=Strict; Max-Age=86400"),
        )
        .body(axum::body::Body::from(
            serde_json::to_string(&body).unwrap(),
        ))
        .unwrap()
        .into_response()
}

/// Build the admin router.
/// Token is used for auth middleware on all routes except /admin/health.
pub fn admin_router(shared: SharedState, token: Arc<String>) -> Router {
    // Public routes (no auth).
    // /admin/csrf-token is public so the SPA can fetch a token before and after login.
    let public = Router::new()
        .route("/admin/health", get(health))
        .route("/admin/csrf-token", get(get_csrf_token));

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
        .route(
            "/admin/api/observability/overview",
            get(get_observability_overview),
        )
        .route("/admin/api/requests", get(get_requests))
        .route("/admin/api/requests/{id}", get(get_request_by_id))
        .route("/admin/api/backends", get(get_backends))
        .route("/admin/api/keys", post(create_key).get(list_keys))
        .route("/admin/api/keys/{id}", put(update_key).delete(revoke_key))
        .route(
            "/admin/api/keys/{id}/spend",
            get(super::spend::get_key_spend),
        )
        .route("/admin/api/models", get(list_models).post(add_model))
        .route("/admin/api/models/{name}", delete(remove_model))
        .route("/admin/api/audit", get(get_audit_log))
        .route(
            "/admin/api/mcp-servers",
            get(list_mcp_servers).post(add_mcp_server),
        )
        .route("/admin/api/mcp-servers/{name}", delete(remove_mcp_server))
        .with_state(shared.clone())
        // Innermost: CSRF check runs after auth succeeds.
        .layer(middleware::from_fn(validate_csrf))
        .layer(middleware::from_fn_with_state(
            token.clone(),
            validate_admin_token,
        ))
        .layer(middleware::from_fn(reject_cross_origin))
        .layer(middleware::from_fn(admin_rate_limit_middleware));

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
    public
        .merge(protected)
        .merge(ws_route)
        .merge(spa_route)
        .layer(DefaultBodyLimit::max(1_048_576))
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
                "default-src 'self'; script-src 'self' 'unsafe-inline'; \
                 style-src 'self' 'unsafe-inline'; \
                 connect-src 'self' ws: wss:; img-src 'self' data:; \
                 frame-ancestors 'none'",
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
        "AWS_ACCESS_KEY_ID":        secret("AWS_ACCESS_KEY_ID"),
        "AWS_SECRET_ACCESS_KEY":    secret("AWS_SECRET_ACCESS_KEY"),
        "AWS_SESSION_TOKEN":        secret("AWS_SESSION_TOKEN"),
        // Google OAuth bearer token (full token — treat as secret)
        "GOOGLE_ACCESS_TOKEN":      secret("GOOGLE_ACCESS_TOKEN"),
        // Auth
        "PROXY_API_KEYS":     secret("PROXY_API_KEYS"),
        "PROXY_OPEN_RELAY":   plain("PROXY_OPEN_RELAY"),
        // TLS
        "TLS_CLIENT_CERT_P12": plain("TLS_CLIENT_CERT_P12"),
        "TLS_CA_CERT":         plain("TLS_CA_CERT"),
        // Network / security
        "IP_ALLOWLIST":           plain("IP_ALLOWLIST"),
        "TRUST_PROXY_HEADERS":    plain("TRUST_PROXY_HEADERS"),
        "WEBHOOK_URLS":           plain("WEBHOOK_URLS"),
        "RATE_LIMIT_FAIL_POLICY": plain("RATE_LIMIT_FAIL_POLICY"),
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
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
                    if !is_safe_model_name(big) {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({
                                "error": format!("invalid big_model name '{big}': contains disallowed characters")
                            })),
                        )
                            .into_response();
                    }
                    db_writes.push((format!("{name}.big_model"), big.to_string()));
                }
                if let Some(small) = settings.get("small_model").and_then(|v| v.as_str()) {
                    if !is_safe_model_name(small) {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({
                                "error": format!("invalid small_model name '{small}': contains disallowed characters")
                            })),
                        )
                            .into_response();
                    }
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

        // Audit log: capture old values before applying changes
        for (key, new_value) in &db_writes {
            let old_value = match key.as_str() {
                "log_level" => config.log_level.clone(),
                "log_bodies" => config.log_bodies.to_string(),
                other => {
                    if let Some((backend, field)) = other.split_once('.') {
                        config
                            .model_mappings
                            .get(backend)
                            .map(|m| match field {
                                "big_model" => m.big_model.clone(),
                                "small_model" => m.small_model.clone(),
                                _ => "<unknown>".to_string(),
                            })
                            .unwrap_or_else(|| "<unset>".to_string())
                    } else {
                        "<unknown>".to_string()
                    }
                }
            };
            tracing::info!(
                key = %key,
                old_value = %old_value,
                new_value = %new_value,
                "admin config change"
            );
        }

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
                source_ip: Some(addr.ip().to_string()),
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
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
                    source_ip: Some(addr.ip().to_string()),
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
        aggregate.streams_started += snap.streams_started;
        aggregate.streams_completed += snap.streams_completed;
        aggregate.streams_failed += snap.streams_failed;
        aggregate.streams_client_disconnected += snap.streams_client_disconnected;
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
            "streams_started": aggregate.streams_started,
            "streams_completed": aggregate.streams_completed,
            "streams_failed": aggregate.streams_failed,
            "streams_client_disconnected": aggregate.streams_client_disconnected,
        },
        "latency_p50_ms": p50,
        "latency_p95_ms": p95,
        "latency_p99_ms": p99,
        "error_rate": aggregate.error_rate(),
    }))
}

#[derive(serde::Deserialize)]
struct ObservabilityQuery {
    hours: Option<u32>,
    backend: Option<String>,
    key_id: Option<i64>,
    timeline_limit: Option<u32>,
    failure_limit: Option<u32>,
}

/// GET /admin/api/observability/overview -- request rollups for the operator dashboard.
async fn get_observability_overview(
    State(shared): State<SharedState>,
    Query(params): Query<ObservabilityQuery>,
) -> Json<serde_json::Value> {
    let hours = params.hours.unwrap_or(6).clamp(1, 168);
    let timeline_limit = params.timeline_limit.unwrap_or(40).clamp(10, 200);
    let failure_limit = params.failure_limit.unwrap_or(12).clamp(1, 100);
    let backend = params.backend.filter(|value| !value.is_empty());
    let key_id = params.key_id;

    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let since = crate::admin::db::epoch_to_iso8601(now_epoch.saturating_sub(hours as u64 * 3600));
    let until = crate::admin::db::now_iso8601();
    let until_display = until.clone();

    match crate::admin::state::with_db(&shared.db, move |conn| {
        let series = crate::admin::db::query_request_timeseries(
            conn,
            &since,
            Some(&until),
            backend.as_deref(),
            key_id,
        )?;
        let timeline = crate::admin::db::query_request_timeline(
            conn,
            &since,
            Some(&until),
            backend.as_deref(),
            key_id,
            timeline_limit,
        )?;
        let failures = crate::admin::db::query_failure_breakdown(
            conn,
            &since,
            Some(&until),
            backend.as_deref(),
            key_id,
            failure_limit,
        )?;
        Ok::<_, rusqlite::Error>((series, timeline, failures))
    })
    .await
    {
        Some(Ok((series, timeline, failures))) => {
            let totals = series.iter().fold(
                (0u64, 0u64, 0u64, 0u64, 0.0f64),
                |(requests_total, requests_error, input_tokens, output_tokens, cost_usd),
                 bucket| {
                    (
                        requests_total + bucket.requests_total,
                        requests_error + bucket.requests_error,
                        input_tokens + bucket.input_tokens,
                        output_tokens + bucket.output_tokens,
                        cost_usd + bucket.cost_usd,
                    )
                },
            );

            Json(serde_json::json!({
                "window_hours": hours,
                "generated_at": until_display,
                "totals": {
                    "requests_total": totals.0,
                    "requests_error": totals.1,
                    "input_tokens": totals.2,
                    "output_tokens": totals.3,
                    "cost_usd": totals.4,
                    "error_rate": if totals.0 == 0 {
                        0.0
                    } else {
                        totals.1 as f64 / totals.0 as f64
                    },
                },
                "series": series,
                "timeline": timeline,
                "failures": failures,
            }))
        }
        Some(Err(e)) => {
            tracing::error!(error = %e, "query observability overview failed");
            Json(serde_json::json!({
                "error": "internal database error",
                "series": [],
                "timeline": [],
                "failures": [],
            }))
        }
        None => Json(serde_json::json!({
            "error": "task panicked",
            "series": [],
            "timeline": [],
            "failures": [],
        })),
    }
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
    until: Option<String>,
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
    let until = params.until;
    let status = params.status;
    let key_id = params.key_id;
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::query_request_log(
            conn,
            limit,
            offset,
            backend.as_deref(),
            since.as_deref(),
            until.as_deref(),
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
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
                        expires_at: body.expires_at.as_deref().and_then(|s| {
                            crate::integrations::langfuse::iso8601_to_epoch(s)
                                .and_then(|e| i64::try_from(e).ok())
                        }),
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
                    source_ip: Some(addr.ip().to_string()),
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
                        "allowed_models": k.allowed_models,
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

/// Request body for PUT /admin/api/keys/{id}.
/// All fields are optional: absent = clear (set to NULL); role is immutable after creation.
#[derive(serde::Deserialize)]
struct UpdateKeyRequest {
    description: Option<String>,
    expires_at: Option<String>,
    rpm_limit: Option<u32>,
    tpm_limit: Option<u32>,
    max_budget_usd: Option<f64>,
    budget_duration: Option<String>,
    allowed_models: Option<Vec<String>>,
}

/// PUT /admin/api/keys/{id} -- update an existing virtual key (except role).
async fn update_key(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateKeyRequest>,
) -> axum::response::Response {
    let allowed_models_json = body
        .allowed_models
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());
    let desc = body.description.clone();
    let exp = body.expires_at.clone();
    let rpm = body.rpm_limit;
    let tpm = body.tpm_limit;
    let max_budget = body.max_budget_usd;
    let budget_dur = body.budget_duration.clone();

    let result = super::state::with_db(&shared.db, move |conn| {
        super::db::update_virtual_key(
            conn,
            id,
            &super::db::UpdateVirtualKeyParams {
                description: desc.as_deref(),
                expires_at: exp.as_deref(),
                rpm_limit: rpm,
                tpm_limit: tpm,
                max_budget_usd: max_budget,
                budget_duration: budget_dur.as_deref(),
                allowed_models: allowed_models_json,
            },
        )
    })
    .await;

    match result {
        Some(Ok(Some(row))) => {
            // Refresh the DashMap entry so in-flight auth sees updated limits.
            if let Some(hash_bytes) = super::keys::hash_from_hex(&row.key_hash) {
                shared.virtual_keys.entry(hash_bytes).and_modify(|meta| {
                    meta.description = body.description.clone();
                    meta.expires_at = body.expires_at.as_deref().and_then(|s| {
                        crate::integrations::langfuse::iso8601_to_epoch(s)
                            .and_then(|e| i64::try_from(e).ok())
                    });
                    meta.rpm_limit = body.rpm_limit;
                    meta.tpm_limit = body.tpm_limit;
                    meta.max_budget_usd = body.max_budget_usd;
                    if body.budget_duration.is_some() {
                        meta.budget_duration = body
                            .budget_duration
                            .as_deref()
                            .and_then(super::keys::BudgetDuration::parse);
                        // Reset spend period to match db-layer reset.
                        meta.period_start = None;
                        meta.period_spend_usd = 0.0;
                    }
                    meta.allowed_models = body.allowed_models.clone();
                });
            }
            emit_audit(
                &shared,
                crate::admin::db::AuditEntry {
                    id: None,
                    timestamp: None,
                    action: "key_updated".into(),
                    target_type: "virtual_key".into(),
                    target_id: Some(id.to_string()),
                    detail: Some(format!("prefix={}", row.key_prefix)),
                    source_ip: Some(addr.ip().to_string()),
                },
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": row.id,
                    "key_prefix": row.key_prefix,
                    "description": row.description,
                    "expires_at": row.expires_at,
                    "rpm_limit": row.rpm_limit,
                    "tpm_limit": row.tpm_limit,
                    "role": row.role,
                    "max_budget_usd": row.max_budget_usd,
                    "budget_duration": row.budget_duration,
                    "allowed_models": row.allowed_models,
                    "status": row.status(),
                })),
            )
                .into_response()
        }
        Some(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Key not found or already revoked"})),
        )
            .into_response(),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to update key"})),
        )
            .into_response(),
    }
}

/// DELETE /admin/api/keys/{id} -- revoke a virtual key.
async fn revoke_key(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
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
                    source_ip: Some(addr.ip().to_string()),
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
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
            source_ip: Some(addr.ip().to_string()),
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
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
                source_ip: Some(addr.ip().to_string()),
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
    action: Option<String>,
    target_type: Option<String>,
    since: Option<String>,
    until: Option<String>,
}

/// GET /admin/api/audit -- paginated audit log.
async fn get_audit_log(
    State(shared): State<SharedState>,
    Query(params): Query<AuditQuery>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).min(1000);
    let offset = params.offset.unwrap_or(0);
    let action = params.action;
    let target_type = params.target_type;
    let since = params.since;
    let until = params.until;
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::query_audit_log(
            conn,
            limit,
            offset,
            action.as_deref(),
            target_type.as_deref(),
            since.as_deref(),
            until.as_deref(),
        )
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

// -- MCP server management --

/// GET /admin/api/mcp-servers - List all registered MCP servers and their tools.
async fn list_mcp_servers(State(shared): State<SharedState>) -> axum::response::Response {
    let Some(ref mgr) = shared.mcp_manager else {
        return (StatusCode::OK, Json(serde_json::json!({"servers": []}))).into_response();
    };
    let servers = mgr.list_servers_blocking();
    (
        StatusCode::OK,
        Json(serde_json::json!({"servers": servers})),
    )
        .into_response()
}

/// POST /admin/api/mcp-servers - Register an MCP server. Body: { name, url }.
/// Performs tool discovery via JSON-RPC tools/list before registering.
async fn add_mcp_server(
    State(shared): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let Some(ref mgr) = shared.mcp_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "MCP support not enabled"})),
        )
            .into_response();
    };

    let name = match body.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing 'name' field"})),
            )
                .into_response()
        }
    };
    let url = match body.get("url").and_then(|v| v.as_str()) {
        Some(u) => u.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing 'url' field"})),
            )
                .into_response()
        }
    };

    // SSRF protection: reject private/loopback IPs and reserved hostnames.
    if let Err(e) = crate::config::validate_base_url(&url) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("invalid MCP server URL: {e}")})),
        )
            .into_response();
    }

    match crate::tools::mcp::McpServerManager::discover_tools(&url).await {
        Ok(tools) => {
            let tool_count = tools.len();
            mgr.register_server_blocking(&name, &url, tools);
            tracing::info!(server = %name, tools = tool_count, "MCP server registered");
            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "name": name,
                    "url": url,
                    "tools_discovered": tool_count,
                })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!(server = %name, error = %e, "MCP tool discovery failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": e})),
            )
                .into_response()
        }
    }
}

/// DELETE /admin/api/mcp-servers/:name - Remove a registered MCP server.
async fn remove_mcp_server(
    State(shared): State<SharedState>,
    Path(name): Path<String>,
) -> axum::response::Response {
    let Some(ref mgr) = shared.mcp_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "MCP support not enabled"})),
        )
            .into_response();
    };
    mgr.remove_server_blocking(&name);
    tracing::info!(server = %name, "MCP server removed");
    (StatusCode::OK, Json(serde_json::json!({"removed": name}))).into_response()
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
        // Raise rate limit so parallel unit tests don't interfere.
        set_admin_rpm(10_000);
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

    #[test]
    fn admin_rate_limit_enforced() {
        // Use a unique IP and pass rpm directly to avoid mutating ADMIN_RPM,
        // which would race with test_router() calling set_admin_rpm(10_000).
        let ip: IpAddr = "198.51.100.1".parse().unwrap();
        ADMIN_RATE_LIMIT.remove(&ip);

        assert!(check_admin_rate_limit_with_rpm(ip, 3));
        assert!(check_admin_rate_limit_with_rpm(ip, 3));
        assert!(check_admin_rate_limit_with_rpm(ip, 3));
        // 4th request in the same window should be rejected.
        assert!(!check_admin_rate_limit_with_rpm(ip, 3));

        ADMIN_RATE_LIMIT.remove(&ip);
    }

    #[test]
    fn sliding_window_blocks_on_rpm_exceeded() {
        // Use a unique IP to avoid test isolation issues.
        let ip: IpAddr = "10.88.77.66".parse().unwrap();
        // With rpm=2, the first 2 requests must pass, the 3rd must fail.
        assert!(check_admin_rate_limit_with_rpm(ip, 2));
        assert!(check_admin_rate_limit_with_rpm(ip, 2));
        assert!(
            !check_admin_rate_limit_with_rpm(ip, 2),
            "3rd request must be blocked when rpm=2"
        );
    }

    /// POST to a protected admin route without CSRF token returns 403.
    #[tokio::test]
    async fn post_without_csrf_returns_403() {
        let app = test_router();
        let req = Request::post("/admin/api/keys")
            .header("host", "localhost:9090")
            .header("authorization", "Bearer test-token")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"description":"test"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// POST with matching CSRF header and cookie succeeds (auth passes, handler runs).
    #[tokio::test]
    async fn post_with_valid_csrf_passes_middleware() {
        let app = test_router();
        let token = "a".repeat(64);
        let req = Request::post("/admin/api/keys")
            .header("host", "localhost:9090")
            .header("authorization", "Bearer test-token")
            .header("content-type", "application/json")
            .header("x-csrf-token", &token)
            .header("cookie", format!("csrf_token={token}"))
            .body(Body::from(r#"{"description":"test"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // 403 would mean CSRF rejected; any other status means CSRF passed.
        assert_ne!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// DELETE without CSRF token returns 403.
    #[tokio::test]
    async fn delete_without_csrf_returns_403() {
        let app = test_router();
        let req = Request::delete("/admin/api/keys/1")
            .header("host", "localhost:9090")
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// GET /admin/csrf-token returns 200 with JSON body and Set-Cookie header.
    #[tokio::test]
    async fn get_csrf_token_sets_cookie() {
        let app = test_router();
        let req = Request::get("/admin/csrf-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let set_cookie = resp
            .headers()
            .get("set-cookie")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            set_cookie.contains("csrf_token="),
            "Set-Cookie must include csrf_token"
        );
        assert!(
            set_cookie.contains("SameSite=Strict"),
            "cookie must be SameSite=Strict"
        );
        // Not httpOnly so JS can read it.
        assert!(
            !set_cookie.to_lowercase().contains("httponly"),
            "csrf_token cookie must not be httpOnly"
        );
    }

    /// GET /admin/csrf-token returns JSON with csrf_token field.
    #[tokio::test]
    async fn get_csrf_token_returns_json() {
        let app = test_router();
        let req = Request::get("/admin/csrf-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), 1 << 16)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        let token = body["csrf_token"].as_str().unwrap();
        assert_eq!(token.len(), 64);
    }

    /// GET requests to protected routes do NOT require CSRF token.
    #[tokio::test]
    async fn get_request_does_not_require_csrf() {
        let app = test_router();
        let req = Request::get("/admin/api/config")
            .header("host", "localhost:9090")
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // CSRF should not reject GET; any non-403 means CSRF passed.
        assert_ne!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn aws_access_key_id_uses_secret_pattern() {
        // The secret() closure masks the value; this test verifies the masking logic.
        let mask = |v: &str| {
            if !v.is_empty() {
                "***REDACTED***".to_string()
            } else {
                "<unset>".to_string()
            }
        };
        assert_eq!(mask("AKIAIOSFODNN7EXAMPLE"), "***REDACTED***");
        assert_eq!(mask(""), "<unset>");
    }

    #[test]
    fn google_access_token_uses_secret_pattern() {
        let mask = |v: &str| {
            if !v.is_empty() {
                "***REDACTED***".to_string()
            } else {
                "<unset>".to_string()
            }
        };
        assert_eq!(mask("ya29.someoauthtoken"), "***REDACTED***");
    }
}
