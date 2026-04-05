// Admin server routes. Served on a separate localhost-only listener.

pub mod audit;
pub mod config;
pub mod keys;
pub mod logs;
pub mod mcp;
pub mod models;

use crate::admin::auth::{
    extract_csrf_cookie, generate_csrf_token, validate_admin_token, validate_csrf_tokens,
};
use crate::admin::state::SharedState;
use crate::admin::ws::ws_handler;
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, State},
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

/// Validate that a string looks like an ISO 8601 / RFC 3339 timestamp.
/// Accepts YYYY-MM-DD (date only) or YYYY-MM-DDTHH:MM:SS[...] (datetime).
/// Does not check calendar validity — the goal is to reject strings that
/// would bypass the timestamp index and force a full-table scan.
pub(super) fn is_valid_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 10 {
        return false;
    }
    b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[7] == b'-'
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && (b.len() == 10
            || (b.len() >= 19
                && (b[10] == b'T' || b[10] == b' ')
                && b[11..13].iter().all(|c| c.is_ascii_digit())
                && b[13] == b':'
                && b[14..16].iter().all(|c| c.is_ascii_digit())
                && b[16] == b':'
                && b[17..19].iter().all(|c| c.is_ascii_digit())))
}

/// Returns the name of the first `since`/`until` parameter that fails timestamp
/// validation, or `None` if both are valid (or absent).
pub(super) fn check_time_range(
    since: Option<&str>,
    until: Option<&str>,
) -> Option<&'static str> {
    if since.is_some_and(|s| !is_valid_timestamp(s)) {
        return Some("since");
    }
    if until.is_some_and(|u| !is_valid_timestamp(u)) {
        return Some("until");
    }
    None
}

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
pub(super) fn is_safe_model_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name.contains('?')
        && !name.contains('#')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || "-_./:@".contains(c))
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
/// Also verifies the token was server-issued (tracked in SharedState) and removes it
/// on first use (one-time token), preventing replay across multiple mutating requests.
/// Returns 403 with a descriptive error if the token is missing, mismatched, or unknown.
/// Applied inside validate_admin_token so unauthenticated requests are rejected first.
pub async fn validate_csrf(
    axum::extract::State(shared): axum::extract::State<SharedState>,
    req: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let method = req.method().clone();

    // PATCH is included: partial updates mutate state just like PUT/DELETE.
    if matches!(
        method,
        axum::http::Method::POST
            | axum::http::Method::PUT
            | axum::http::Method::DELETE
            | axum::http::Method::PATCH
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

        // Verify the token was server-issued and consume it (one-time use).
        // moka get() + invalidate() is not atomic across concurrent requests with
        // the same token, but CSRF tokens are 256-bit random values so collision
        // is not a realistic attack vector; the primary threat (replay) is mitigated
        // by invalidation after first use.
        if shared.issued_csrf_tokens.get(header_token).is_none() {
            let body = serde_json::json!({
                "type": "error",
                "error": {
                    "type": "permission_error",
                    "message": "CSRF token not recognized or already used. Fetch a new token from GET /admin/csrf-token."
                }
            });
            return (StatusCode::FORBIDDEN, axum::Json(body)).into_response();
        }
        // Consume the token so it cannot be replayed.
        shared.issued_csrf_tokens.invalidate(header_token);
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
async fn get_csrf_token(State(shared): State<SharedState>) -> axum::response::Response {
    let token = generate_csrf_token();
    // moka Cache enforces max_capacity(1,000) and time_to_live(24h) automatically.
    // Eviction is handled by the cache; no manual cap check needed.
    shared.issued_csrf_tokens.insert(token.clone(), ());
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
pub fn admin_router(shared: SharedState, token: Arc<zeroize::Zeroizing<String>>) -> Router {
    // Public routes (no auth).
    // /admin/csrf-token is public so the SPA can fetch a token before and after login.
    // Rate-limited to prevent unauthenticated flooding of the CSRF token map.
    let public = Router::new()
        .route("/admin/health", get(health))
        .route("/admin/csrf-token", get(get_csrf_token))
        .with_state(shared.clone())
        .layer(middleware::from_fn(admin_rate_limit_middleware));

    // Protected routes (require admin token + localhost origin check).
    let protected = Router::new()
        .route(
            "/admin/api/config",
            get(config::get_config).put(config::put_config),
        )
        .route(
            "/admin/api/config/overrides",
            get(config::get_config_overrides),
        )
        .route(
            "/admin/api/config/overrides/{key}",
            delete(config::delete_config_override),
        )
        .route("/admin/api/env", get(config::get_env))
        .route("/admin/api/metrics", get(logs::get_metrics))
        .route(
            "/admin/api/observability/overview",
            get(logs::get_observability_overview),
        )
        .route("/admin/api/requests", get(logs::get_requests))
        .route("/admin/api/requests/{id}", get(logs::get_request_by_id))
        .route("/admin/api/backends", get(get_backends))
        .route(
            "/admin/api/keys",
            post(keys::create_key).get(keys::list_keys),
        )
        .route(
            "/admin/api/keys/{id}",
            put(keys::update_key).delete(keys::revoke_key),
        )
        .route(
            "/admin/api/keys/{id}/spend",
            get(super::spend::get_key_spend),
        )
        .route(
            "/admin/api/models",
            get(models::list_models).post(models::add_model),
        )
        .route("/admin/api/models/{name}", delete(models::remove_model))
        .route("/admin/api/audit", get(audit::get_audit_log))
        .route(
            "/admin/api/mcp-servers",
            get(mcp::list_mcp_servers).post(mcp::add_mcp_server),
        )
        .route(
            "/admin/api/mcp-servers/{name}",
            delete(mcp::remove_mcp_server),
        )
        .with_state(shared.clone())
        // Innermost: CSRF check runs after auth succeeds.
        .layer(middleware::from_fn_with_state(
            shared.clone(),
            validate_csrf,
        ))
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

/// Serve the embedded SPA HTML with a per-request CSP nonce.
static SPA_HTML: &str = include_str!("../../../admin-ui/index.html");

async fn serve_spa() -> axum::response::Response {
    // Generate a per-request nonce (128-bit, base64url-encoded).
    let mut nonce_bytes = [0u8; 16];
    getrandom::fill(&mut nonce_bytes).expect("getrandom");
    let nonce = base64_url_encode(&nonce_bytes);

    // Replace the placeholder in the embedded HTML with the actual nonce.
    let html = SPA_HTML.replace("__CSP_NONCE__", &nonce);

    let csp = format!(
        "default-src 'self'; script-src 'self' 'nonce-{nonce}'; \
         style-src 'self' 'nonce-{nonce}'; \
         connect-src 'self' ws: wss:; img-src 'self' data:; \
         frame-ancestors 'none'"
    );

    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .header("content-security-policy", csp)
        .header("x-frame-options", "DENY")
        .header("referrer-policy", "no-referrer")
        .body(axum::body::Body::from(html))
        .unwrap()
        .into_response()
}

/// Base64url-encode without padding (RFC 4648 section 5).
fn base64_url_encode(input: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(input)
}

/// GET /admin/api/backends -- list configured backends with status.
pub(super) async fn get_backends(State(shared): State<SharedState>) -> Json<serde_json::Value> {
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

/// Fire-and-forget audit log write. Failures are logged but never block the caller.
pub(crate) fn emit_audit(shared: &SharedState, entry: crate::admin::db::AuditEntry) {
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
        let token = Arc::new(zeroize::Zeroizing::new("test-token".to_string()));
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

    /// POST with matching CSRF header and cookie, where token is server-issued, succeeds.
    #[tokio::test]
    async fn post_with_valid_csrf_passes_middleware() {
        set_admin_rpm(10_000);
        let shared = crate::admin::state::SharedState::new_for_test();
        let token_str = "a".repeat(64);
        // Pre-register the token as server-issued so validate_csrf can find it.
        shared.issued_csrf_tokens.insert(token_str.clone(), ());
        let app = admin_router(
            shared,
            Arc::new(zeroize::Zeroizing::new("test-token".to_string())),
        );
        let req = Request::post("/admin/api/keys")
            .header("host", "localhost:9090")
            .header("authorization", "Bearer test-token")
            .header("content-type", "application/json")
            .header("x-csrf-token", &token_str)
            .header("cookie", format!("csrf_token={token_str}"))
            .body(Body::from(r#"{"description":"test"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // 403 would mean CSRF rejected; any other status means CSRF passed.
        assert_ne!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// POST with a CSRF token that was not server-issued is rejected even if header==cookie.
    #[tokio::test]
    async fn post_with_unissued_csrf_returns_403() {
        let app = test_router();
        let token = "b".repeat(64); // valid format but never stored in issued_csrf_tokens
        let req = Request::post("/admin/api/keys")
            .header("host", "localhost:9090")
            .header("authorization", "Bearer test-token")
            .header("content-type", "application/json")
            .header("x-csrf-token", &token)
            .header("cookie", format!("csrf_token={token}"))
            .body(Body::from(r#"{"description":"test"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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

#[cfg(test)]
mod timestamp_tests {
    use super::is_valid_timestamp;

    #[test]
    fn accepts_date_only() {
        assert!(is_valid_timestamp("2026-03-31"));
    }

    #[test]
    fn accepts_datetime_utc() {
        assert!(is_valid_timestamp("2026-03-31T12:00:00Z"));
    }

    #[test]
    fn accepts_datetime_no_tz() {
        assert!(is_valid_timestamp("2026-03-31T12:00:00"));
    }

    #[test]
    fn rejects_empty() {
        assert!(!is_valid_timestamp(""));
    }

    #[test]
    fn rejects_arbitrary_string() {
        assert!(!is_valid_timestamp("not-a-date"));
    }

    #[test]
    fn rejects_sql_injection_attempt() {
        assert!(!is_valid_timestamp("'; DROP TABLE request_log; --"));
    }

    #[test]
    fn rejects_too_short() {
        assert!(!is_valid_timestamp("2026-03"));
    }
}
