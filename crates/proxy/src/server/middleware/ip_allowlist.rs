use anyllm_translate::anthropic;
use anyllm_translate::mapping::errors_map::create_anthropic_error;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use std::sync::LazyLock;

/// Parsed CIDR allowlist from IP_ALLOWLIST env var. None means allow all.
static IP_ALLOWLIST: LazyLock<Option<Vec<ipnetwork::IpNetwork>>> = LazyLock::new(|| {
    std::env::var("IP_ALLOWLIST").ok().map(|v| {
        v.split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| {
                // Accept bare IPs (e.g., "127.0.0.1") by appending /32 or /128.
                if !s.contains('/') {
                    let ip: std::net::IpAddr = s
                        .parse()
                        .unwrap_or_else(|e| panic!("invalid IP_ALLOWLIST entry '{s}': {e}"));
                    return ipnetwork::IpNetwork::from(ip);
                }
                s.parse::<ipnetwork::IpNetwork>()
                    .unwrap_or_else(|e| panic!("invalid IP_ALLOWLIST CIDR '{s}': {e}"))
            })
            .collect()
    })
});

/// Whether to trust X-Forwarded-For for IP allowlisting (production behind reverse proxy).
static TRUST_PROXY_HEADERS: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("TRUST_PROXY_HEADERS")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
});

/// Number of trusted proxy hops. The client IP is extracted as the Nth-from-right
/// entry in X-Forwarded-For. Defaults to 1 (single reverse proxy).
/// Set TRUSTED_PROXY_DEPTH=2 for chains like CDN -> LB -> proxy.
static TRUSTED_PROXY_DEPTH: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("TRUSTED_PROXY_DEPTH")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1) // minimum 1
});

/// Check if an IP address is allowed by the configured allowlist.
/// Returns true if no allowlist is set (open access).
pub fn is_ip_allowed(ip: std::net::IpAddr) -> bool {
    match IP_ALLOWLIST.as_ref() {
        None => true,
        Some(networks) => networks.iter().any(|net| net.contains(ip)),
    }
}

/// Returns true if the IP allowlist is configured (IP_ALLOWLIST env var is set).
pub fn ip_allowlist_active() -> bool {
    IP_ALLOWLIST.is_some()
}

/// Middleware that rejects requests from IPs not in the allowlist.
/// Applied before auth so blocked IPs never reach authentication.
pub async fn check_ip_allowlist(request: Request<Body>, next: Next) -> Result<Response, Response> {
    // Extract client IP from X-Forwarded-For (if trusted) or connection info.
    //
    // XFF spoofing: attacker-controlled headers appear at the *left* of the list.
    // Each hop's proxy appends the IP it received from, so the rightmost entry is
    // added by our immediate (trusted) upstream. We iterate right-to-left with
    // rsplit and skip (depth-1) entries to skip past our own trusted proxies.
    // TRUSTED_PROXY_DEPTH=1 (default) selects the rightmost entry; depth=2
    // selects the second-from-right for a two-hop CDN -> LB topology, etc.
    // Using .last() would ignore depth; using .first() would trust the attacker.
    //
    // NOT A BUG: X-Forwarded-For is only read when TRUST_PROXY_HEADERS=true.
    // Without it this block is skipped entirely and ConnectInfo is used instead,
    // so there is no XFF spoofing risk in the default (no reverse proxy) setup.
    let client_ip = if *TRUST_PROXY_HEADERS {
        let depth = *TRUSTED_PROXY_DEPTH;
        request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                s.rsplit(',')
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                    .nth(depth - 1)
            })
            .and_then(|s| s.parse::<std::net::IpAddr>().ok())
    } else {
        None
    };

    // Fall back to ConnectInfo if available.
    let client_ip = client_ip.or_else(|| {
        request
            .extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|ci| ci.0.ip())
    });

    // If we have no IP at all (unlikely), deny by default when allowlist is active.
    let Some(ip) = client_ip else {
        tracing::warn!("could not determine client IP for allowlist check");
        let err = create_anthropic_error(
            anthropic::ErrorType::PermissionError,
            "IP address could not be determined".to_string(),
            None,
        );
        return Err((StatusCode::FORBIDDEN, Json(err)).into_response());
    };

    if !is_ip_allowed(ip) {
        tracing::debug!(ip = %ip, "request rejected by IP allowlist");
        let err = create_anthropic_error(
            anthropic::ErrorType::PermissionError,
            "IP address not in allowlist".to_string(),
            None,
        );
        return Err((StatusCode::FORBIDDEN, Json(err)).into_response());
    }

    Ok(next.run(request).await)
}

#[cfg(test)]
mod ip_tests {
    use super::*;

    #[test]
    fn is_ip_allowed_no_allowlist() {
        // When IP_ALLOWLIST is not set, all IPs are allowed.
        // We cannot test this directly since LazyLock is static, but the function
        // logic is: None => true. Smoke-call to ensure it does not panic.
        let _ = is_ip_allowed("127.0.0.1".parse().unwrap());
    }

    #[test]
    fn xff_rightmost_prevents_spoofing() {
        // Attacker sends X-Forwarded-For: 127.0.0.1; trusted proxy appends real IP.
        // Must resolve to rightmost value (203.0.113.5), not the attacker-controlled leftmost.
        let header = "127.0.0.1, 203.0.113.5";
        let resolved: std::net::IpAddr = header
            .split(',')
            .map(|s| s.trim())
            .rfind(|s| !s.is_empty())
            .and_then(|s| s.parse().ok())
            .unwrap();
        assert_eq!(resolved, "203.0.113.5".parse::<std::net::IpAddr>().unwrap());
    }

    #[test]
    fn xff_single_ip_resolves() {
        let header = "10.0.1.5";
        let resolved: std::net::IpAddr = header
            .split(',')
            .map(|s| s.trim())
            .rfind(|s| !s.is_empty())
            .and_then(|s| s.parse().ok())
            .unwrap();
        assert_eq!(resolved, "10.0.1.5".parse::<std::net::IpAddr>().unwrap());
    }
}
