use axum::{extract::ConnectInfo, http::StatusCode, middleware, response::IntoResponse};
use dashmap::DashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::LazyLock;

use crate::admin::auth::{extract_csrf_cookie, validate_csrf_tokens};
use crate::admin::state::SharedState;

/// Per-IP sliding window rate limiter buckets, split by request method.
/// READ buckets track GET/HEAD/OPTIONS; WRITE buckets track POST/PUT/DELETE/PATCH.
pub(super) static READ_RATE_BUCKETS: LazyLock<DashMap<IpAddr, std::collections::VecDeque<u64>>> =
    LazyLock::new(DashMap::new);
pub(super) static WRITE_RATE_BUCKETS: LazyLock<DashMap<IpAddr, std::collections::VecDeque<u64>>> =
    LazyLock::new(DashMap::new);

/// Separate RPM limits for read vs write admin API requests per IP per 60s window.
pub(super) static ADMIN_READ_RPM: AtomicU32 = AtomicU32::new(240);
pub(super) static ADMIN_WRITE_RPM: AtomicU32 = AtomicU32::new(60);

/// Prune stale entries from a rate limiter bucket. Removes IPs whose newest
/// timestamp is older than 60 seconds. Called periodically to prevent unbounded
/// growth from distinct source IPs.
fn prune_stale_rate_limit_entries(
    now_ms: u64,
    bucket: &DashMap<IpAddr, std::collections::VecDeque<u64>>,
    last_prune: &std::sync::atomic::AtomicU64,
) {
    let last = last_prune.load(Ordering::Relaxed);
    // Prune at most once every 60 seconds.
    if now_ms.saturating_sub(last) < 60_000 {
        return;
    }
    if last_prune
        .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return; // another thread won the race
    }
    let cutoff = now_ms.saturating_sub(60_000);
    bucket.retain(|_, window| window.back().is_some_and(|&ts| ts >= cutoff));
}

/// Inner rate-limit check with an explicit rpm and bucket; avoids touching the global
/// statics in tests.
pub(super) fn check_admin_rate_limit_with_rpm(
    ip: IpAddr,
    rpm: u32,
    bucket: &DashMap<IpAddr, std::collections::VecDeque<u64>>,
    last_prune: &std::sync::atomic::AtomicU64,
) -> bool {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let cutoff = now_ms.saturating_sub(60_000);

    // Periodically prune entries for IPs that have gone silent.
    prune_stale_rate_limit_entries(now_ms, bucket, last_prune);

    let mut window = bucket.entry(ip).or_default();
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
/// Uses the higher read limit for GET/HEAD/OPTIONS, and the lower write limit for mutations.
fn check_admin_rate_limit(ip: IpAddr, is_read: bool) -> bool {
    static LAST_READ_PRUNE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static LAST_WRITE_PRUNE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if is_read {
        check_admin_rate_limit_with_rpm(
            ip,
            ADMIN_READ_RPM.load(Ordering::Relaxed),
            &READ_RATE_BUCKETS,
            &LAST_READ_PRUNE,
        )
    } else {
        check_admin_rate_limit_with_rpm(
            ip,
            ADMIN_WRITE_RPM.load(Ordering::Relaxed),
            &WRITE_RATE_BUCKETS,
            &LAST_WRITE_PRUNE,
        )
    }
}

/// Axum middleware that enforces per-IP rate limiting on admin API routes.
/// Returns 429 Too Many Requests when the limit is exceeded.
pub(super) async fn admin_rate_limit_middleware(
    req: axum::extract::Request,
    next: middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    // Liveness checks are intentionally cheap and unauthenticated.
    let path = req.uri().path();
    if path == "/admin/health" {
        return Ok(next.run(req).await);
    }

    // Extract client IP from ConnectInfo extension (set by into_make_service_with_connect_info).
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    let is_read = matches!(
        *req.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    );

    if !check_admin_rate_limit(ip, is_read) {
        tracing::warn!(%ip, "admin API rate limit exceeded");
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    Ok(next.run(req).await)
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
pub(super) async fn reject_cross_origin(
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
///
/// Long-lived sessions: the admin SPA fetches a fresh token before every mutating
/// request (not once at login), so sessions open for more than 24 h still work as
/// long as the browser can reach GET /admin/csrf-token. Both the cookie and the
/// server-side entry expire after 24 h; if the cookie is gone the next mutation
/// returns 403 until the page is refreshed.
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
