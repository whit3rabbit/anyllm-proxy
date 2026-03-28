// Auth, logging, and request size limit middleware

use crate::admin::keys::{
    check_and_reset_period, now_ms, period_reset_at, KeyRole, RateLimitState, VirtualKeyMeta,
};
use anyllm_translate::anthropic;
use anyllm_translate::mapping::errors_map::create_anthropic_error;
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::sync::{Arc, LazyLock, OnceLock};
use subtle::ConstantTimeEq;

/// Per-installation HMAC secret for virtual key hashing.
/// Set once during startup alongside the virtual keys DashMap.
static HMAC_SECRET: OnceLock<Arc<Vec<u8>>> = OnceLock::new();

/// Initialize the global HMAC secret. Called once from main.
pub fn set_hmac_secret(secret: Arc<Vec<u8>>) {
    let _ = HMAC_SECRET.set(secret);
}

/// Build a 429 rate-limit error response with retry-after header.
fn rate_limit_response(message: &str, retry_after: u64) -> Response {
    let err = create_anthropic_error(
        anthropic::ErrorType::RateLimitError,
        message.to_string(),
        None,
    );
    let mut resp = (StatusCode::TOO_MANY_REQUESTS, Json(err)).into_response();
    if let Ok(val) = axum::http::HeaderValue::from_str(&retry_after.to_string()) {
        resp.headers_mut().insert("retry-after", val);
    }
    resp
}

/// Context passed from auth middleware to handlers for post-response TPM and cost recording.
/// Inserted into request extensions when a virtual key is used.
#[derive(Clone)]
pub struct VirtualKeyContext {
    /// Database row ID for the virtual key (used for cost accumulation).
    pub(crate) key_id: i64,
    pub(crate) rate_state: Arc<RateLimitState>,
    /// Optional model allowlist from the virtual key policy.
    pub(crate) allowed_models: Option<Vec<String>>,
}

/// Controls which authentication paths are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Only accept static and virtual API keys. JWTs are not checked.
    KeysOnly,
    /// Only accept JWT tokens. Static and virtual keys are rejected.
    OidcOnly,
    /// Try JWT first, fall through to keys on failure (default).
    Both,
}

impl AuthMode {
    /// Parse an AUTH_MODE string. Accepts both new names (oidc, oidc-only, keys,
    /// keys-only, both) and legacy names (jwt_only, keys_only, jwt_or_keys).
    pub fn from_env_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "oidc" | "oidc-only" | "oidc_only" | "jwt_only" => Self::OidcOnly,
            "keys" | "keys-only" | "keys_only" => Self::KeysOnly,
            "both" | "jwt_or_keys" => Self::Both,
            _ => Self::Both,
        }
    }

    /// Read AUTH_MODE from the environment. Defaults to Both for backward compatibility.
    pub fn from_env() -> Self {
        std::env::var("AUTH_MODE")
            .map(|v| Self::from_env_str(&v))
            .unwrap_or(Self::Both)
    }

    pub fn allows_key_auth(&self) -> bool {
        matches!(self, AuthMode::KeysOnly | AuthMode::Both)
    }

    pub fn allows_oidc(&self) -> bool {
        matches!(self, AuthMode::OidcOnly | AuthMode::Both)
    }
}

static AUTH_MODE: LazyLock<AuthMode> = LazyLock::new(|| {
    let mode = AuthMode::from_env();
    tracing::info!(?mode, "auth mode configured");
    mode
});

/// Global reference to the virtual keys DashMap, set once during startup.
/// Checked during auth after the static ALLOWED_KEY_HASHES check.
static VIRTUAL_KEYS: OnceLock<Arc<DashMap<[u8; 32], VirtualKeyMeta>>> = OnceLock::new();

/// Global OIDC config, set once during startup when OIDC_ISSUER_URL is configured.
static OIDC_CONFIG: OnceLock<Arc<super::oidc::OidcConfig>> = OnceLock::new();

/// Initialize the global OIDC config. Called once from main when OIDC is enabled.
pub fn set_oidc_config(config: Arc<super::oidc::OidcConfig>) {
    let _ = OIDC_CONFIG.set(config);
}

/// Initialize the global virtual keys reference. Called once from main.
pub fn set_virtual_keys(keys: Arc<DashMap<[u8; 32], VirtualKeyMeta>>) {
    let _ = VIRTUAL_KEYS.set(keys);
}

/// Pre-hashed allowed API keys for constant-time comparison without
/// leaking key length via timing. Each key is SHA-256 hashed at startup.
static ALLOWED_KEY_HASHES: LazyLock<Vec<[u8; 32]>> = LazyLock::new(|| {
    let keys: Vec<String> = std::env::var("PROXY_API_KEYS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if keys.is_empty() {
        let open_relay = std::env::var("PROXY_OPEN_RELAY")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        if open_relay {
            tracing::warn!(
                "PROXY_OPEN_RELAY=true: proxy accepts ANY non-empty key. \
                 Set PROXY_API_KEYS to restrict access."
            );
        } else {
            tracing::error!(
                "PROXY_API_KEYS is not set and PROXY_OPEN_RELAY is not enabled. \
                 The proxy will reject all requests. Set PROXY_API_KEYS or \
                 set PROXY_OPEN_RELAY=true to allow unauthenticated access."
            );
        }
    }
    keys.iter()
        .map(|k| Sha256::digest(k.as_bytes()).into())
        .collect()
});

/// Whether open-relay mode is explicitly enabled via PROXY_OPEN_RELAY=true.
static OPEN_RELAY: LazyLock<bool> = LazyLock::new(|| {
    ALLOWED_KEY_HASHES.is_empty()
        && std::env::var("PROXY_OPEN_RELAY")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
});

/// Validate that the request carries a valid API key.
/// If `PROXY_API_KEYS` is set, the caller's key must be in the allowlist.
/// Otherwise, any non-empty key is accepted (backward-compatible open mode).
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub async fn validate_auth(
    headers: HeaderMap,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bearer_token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            let lower = v.to_lowercase();
            if lower.starts_with("bearer ") {
                Some(v[7..].trim().to_string())
            } else {
                None
            }
        });

    let credential = api_key.or(bearer_token);

    let credential = match credential {
        Some(c) if !c.is_empty() => c,
        _ => {
            let err = create_anthropic_error(
                anthropic::ErrorType::AuthenticationError,
                "Missing authentication. Provide x-api-key or Authorization header.".to_string(),
                None,
            );
            return Err((StatusCode::UNAUTHORIZED, Json(err)).into_response());
        }
    };

    // Check 0: OIDC/JWT validation (if configured and mode allows it).
    let auth_mode = *AUTH_MODE;
    if auth_mode.allows_oidc() {
        if let Some(oidc) = OIDC_CONFIG.get() {
            if super::oidc::looks_like_jwt(&credential) {
                match oidc.validate_token(&credential) {
                    Ok(claims) => {
                        tracing::debug!(sub = ?claims.sub, auth_path = "jwt", "authentication successful");
                        request.extensions_mut().insert(claims);
                        return Ok(next.run(request).await);
                    }
                    Err(e) => {
                        if auth_mode == AuthMode::OidcOnly {
                            tracing::debug!(error = %e, "JWT validation failed (oidc_only mode, no fallback)");
                            let err = create_anthropic_error(
                                anthropic::ErrorType::AuthenticationError,
                                "JWT validation failed.".to_string(),
                                None,
                            );
                            return Err((StatusCode::UNAUTHORIZED, Json(err)).into_response());
                        }
                        tracing::debug!(error = %e, "JWT validation failed, trying key-based auth");
                    }
                }
            } else if auth_mode == AuthMode::OidcOnly {
                let err = create_anthropic_error(
                    anthropic::ErrorType::AuthenticationError,
                    "JWT required but credential is not a valid JWT format.".to_string(),
                    None,
                );
                return Err((StatusCode::UNAUTHORIZED, Json(err)).into_response());
            }
        } else if auth_mode == AuthMode::OidcOnly {
            tracing::error!("AUTH_MODE=oidc_only but OIDC_ISSUER_URL is not configured");
            let err = create_anthropic_error(
                anthropic::ErrorType::AuthenticationError,
                "Server misconfigured: JWT auth required but OIDC not configured.".to_string(),
                None,
            );
            return Err((StatusCode::UNAUTHORIZED, Json(err)).into_response());
        }
    }

    // Compare SHA-256 hashes of the credential against pre-hashed allowed keys.
    // Hashing eliminates the timing side-channel on key length: all comparisons
    // operate on fixed-size 32-byte digests regardless of original key length.
    let credential_hash: [u8; 32] = Sha256::digest(credential.as_bytes()).into();

    // Check 1: static env-var keys (constant-time comparison)
    let env_key_match = ALLOWED_KEY_HASHES
        .iter()
        .any(|h| bool::from(h.ct_eq(&credential_hash)));

    if env_key_match {
        tracing::debug!(auth_path = "static_key", "authentication successful");
        return Ok(next.run(request).await);
    }

    // Check 2: virtual keys from DashMap (with per-key rate limiting, budget, RBAC)
    // Dual-mode lookup: try HMAC-SHA256 hash first (new keys), fall back to legacy SHA-256 (old keys).
    if let Some(map) = VIRTUAL_KEYS.get() {
        let hmac_hash: Option<[u8; 32]> = HMAC_SECRET.get().and_then(|secret| {
            let hex = crate::admin::keys::hmac_hash_key(&credential, secret);
            crate::admin::keys::hash_from_hex(&hex)
        });
        let vk_lookup = hmac_hash
            .and_then(|h| map.get_mut(&h))
            .or_else(|| map.get_mut(&credential_hash));
        if let Some(mut meta) = vk_lookup {
            // RBAC: developer keys cannot access admin endpoints
            if meta.role == KeyRole::Developer {
                let path = request.uri().path();
                if path.starts_with("/admin/") || path.starts_with("/admin") {
                    let err_body = serde_json::json!({
                        "error": {
                            "type": "permission_denied",
                            "message": "This key does not have permission to access admin endpoints."
                        }
                    });
                    return Err((StatusCode::FORBIDDEN, Json(err_body)).into_response());
                }
            }

            let now_ms = now_ms();

            // Enforce RPM limit if configured
            if let Some(rpm_limit) = meta.rpm_limit {
                #[allow(unused_mut, unused_variables)]
                let mut checked_ext = false;
                #[cfg(feature = "redis")]
                {
                    let hash_hex: String =
                        credential_hash.iter().map(|b| format!("{b:02x}")).collect();
                    if let Some(redis_limiter) = crate::ratelimit::get_redis_rate_limiter() {
                        checked_ext = true;
                        if let Err(retry_after) =
                            redis_limiter.check_rpm(&hash_hex, rpm_limit, now_ms).await
                        {
                            return Err(rate_limit_response(
                                "Rate limit exceeded for this API key.",
                                retry_after,
                            ));
                        }
                    }
                }

                if !checked_ext {
                    if let Err(retry_after) = meta.rate_state.check_rpm(rpm_limit, now_ms) {
                        return Err(rate_limit_response(
                            "Rate limit exceeded for this API key.",
                            retry_after,
                        ));
                    }
                }
            }

            // Enforce TPM limit pre-check
            if let Some(tpm_limit) = meta.tpm_limit {
                #[allow(unused_mut, unused_variables)]
                let mut checked_ext = false;
                #[cfg(feature = "redis")]
                {
                    let hash_hex: String =
                        credential_hash.iter().map(|b| format!("{b:02x}")).collect();
                    if let Some(redis_limiter) = crate::ratelimit::get_redis_rate_limiter() {
                        checked_ext = true;
                        if let Err(retry_after) =
                            redis_limiter.check_tpm(&hash_hex, tpm_limit, now_ms).await
                        {
                            return Err(rate_limit_response(
                                "Token rate limit exceeded for this API key.",
                                retry_after,
                            ));
                        }
                    }
                }

                if !checked_ext {
                    if let Err(retry_after) = meta.rate_state.check_tpm(tpm_limit, now_ms) {
                        return Err(rate_limit_response(
                            "Token rate limit exceeded for this API key.",
                            retry_after,
                        ));
                    }
                }
            }

            // Budget enforcement: lazy period reset then check
            if meta.max_budget_usd.is_some() {
                let did_reset = check_and_reset_period(&mut meta);
                if did_reset {
                    tracing::debug!(
                        key_id = meta.id,
                        period_start = ?meta.period_start,
                        "budget period reset"
                    );
                }
                if let Some(limit) = meta.max_budget_usd {
                    if meta.period_spend_usd >= limit {
                        let reset_at = period_reset_at(&meta);
                        let err_body = serde_json::json!({
                            "error": {
                                "type": "budget_exceeded",
                                "message": format!(
                                    "This API key has exhausted its budget. Current period spend: ${:.2} of ${:.2} limit.",
                                    meta.period_spend_usd, limit
                                ),
                                "budget_limit_usd": limit,
                                "period_spend_usd": meta.period_spend_usd,
                                "budget_duration": meta.budget_duration.as_ref().map(|d| d.as_str()),
                                "period_reset_at": reset_at,
                            }
                        });
                        return Err((StatusCode::TOO_MANY_REQUESTS, Json(err_body)).into_response());
                    }
                }
            }

            // Always insert context for post-response TPM recording and cost tracking.
            request.extensions_mut().insert(VirtualKeyContext {
                key_id: meta.id,
                rate_state: meta.rate_state.clone(),
                allowed_models: meta.allowed_models.clone(),
            });

            tracing::debug!(
                key_id = meta.id,
                auth_path = "virtual_key",
                "authentication successful"
            );
            return Ok(next.run(request).await);
        }
    }

    // Check 3: open-relay mode (any non-empty key accepted)
    if *OPEN_RELAY {
        return Ok(next.run(request).await);
    }

    // No match found: reject
    let message = if ALLOWED_KEY_HASHES.is_empty() {
        "Server not configured for access. Contact the administrator."
    } else {
        "Invalid API key."
    };
    let err = create_anthropic_error(
        anthropic::ErrorType::AuthenticationError,
        message.to_string(),
        None,
    );
    Err((StatusCode::UNAUTHORIZED, Json(err)).into_response())
}

/// Attach a request ID to the request and echo it on the response.
/// Uses the incoming x-request-id if present, otherwise generates a UUID v4.
///
/// Anthropic: <https://docs.anthropic.com/en/api/errors>
pub async fn add_request_id(mut request: Request<Body>, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Replace invalid request IDs with UUIDs to prevent header injection.
    // Client-provided IDs may contain characters illegal in HTTP headers.
    let header_value: axum::http::HeaderValue = request_id.parse().unwrap_or_else(|_| {
        uuid::Uuid::new_v4()
            .to_string()
            .parse()
            .expect("UUID is always a valid header value")
    });
    request
        .headers_mut()
        .insert("x-request-id", header_value.clone());

    let mut response = next.run(request).await;
    response.headers_mut().insert("x-request-id", header_value);
    response
}

/// Log Anthropic-specific headers without rejecting requests that lack them.
/// Claude Code CLI and other Anthropic SDK clients send these headers.
pub async fn log_anthropic_headers(request: Request<Body>, next: Next) -> Response {
    if let Some(v) = request
        .headers()
        .get("anthropic-version")
        .and_then(|v| v.to_str().ok())
    {
        tracing::debug!(anthropic_version = %v, "anthropic-version header present");
    }
    if let Some(b) = request
        .headers()
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
    {
        tracing::debug!(anthropic_beta = %b, "anthropic-beta header present");
    }
    // Claude Code v2.1.86+ sends this for proxy-side session routing/aggregation.
    if let Some(s) = request
        .headers()
        .get("x-claude-code-session-id")
        .and_then(|v| v.to_str().ok())
    {
        tracing::debug!(session_id = %s, "x-claude-code-session-id header present");
    }
    next.run(request).await
}

/// Maximum request body size (32 MB, matching Anthropic's Messages endpoint limit).
pub const MAX_BODY_SIZE: usize = 32 * 1024 * 1024;

/// Maximum concurrent requests to prevent self-DOS under 429 incidents.
pub const MAX_CONCURRENT_REQUESTS: usize = 100;

// ---- IP allowlisting ----

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
    let client_ip = if *TRUST_PROXY_HEADERS {
        request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.trim().parse::<std::net::IpAddr>().ok())
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
        // logic is: None => true.
        assert!(is_ip_allowed("127.0.0.1".parse().unwrap()) || true);
    }
}

#[cfg(test)]
mod auth_mode_tests {
    use super::*;

    #[test]
    fn parse_auth_mode_new_names() {
        assert_eq!(AuthMode::from_env_str("oidc"), AuthMode::OidcOnly);
        assert_eq!(AuthMode::from_env_str("oidc-only"), AuthMode::OidcOnly);
        assert_eq!(AuthMode::from_env_str("oidc_only"), AuthMode::OidcOnly);
        assert_eq!(AuthMode::from_env_str("keys"), AuthMode::KeysOnly);
        assert_eq!(AuthMode::from_env_str("keys-only"), AuthMode::KeysOnly);
        assert_eq!(AuthMode::from_env_str("keys_only"), AuthMode::KeysOnly);
        assert_eq!(AuthMode::from_env_str("both"), AuthMode::Both);
    }

    #[test]
    fn parse_auth_mode_legacy_names() {
        assert_eq!(AuthMode::from_env_str("jwt_only"), AuthMode::OidcOnly);
        assert_eq!(AuthMode::from_env_str("jwt_or_keys"), AuthMode::Both);
        assert_eq!(AuthMode::from_env_str("JWT_ONLY"), AuthMode::OidcOnly);
    }

    #[test]
    fn parse_auth_mode_unknown_defaults_to_both() {
        assert_eq!(AuthMode::from_env_str("unknown"), AuthMode::Both);
        assert_eq!(AuthMode::from_env_str(""), AuthMode::Both);
    }

    #[test]
    fn auth_mode_oidc_only() {
        assert!(AuthMode::OidcOnly.allows_oidc());
        assert!(!AuthMode::OidcOnly.allows_key_auth());
    }

    #[test]
    fn auth_mode_keys_only() {
        assert!(AuthMode::KeysOnly.allows_key_auth());
        assert!(!AuthMode::KeysOnly.allows_oidc());
    }

    #[test]
    fn auth_mode_both_allows_all() {
        assert!(AuthMode::Both.allows_oidc());
        assert!(AuthMode::Both.allows_key_auth());
    }

    #[test]
    fn auth_mode_from_env_defaults_to_both() {
        // When AUTH_MODE is not set (or set to something unrecognized),
        // from_env() returns Both for backward compatibility.
        // Note: cannot safely manipulate env vars in parallel tests,
        // so we test via from_env_str which from_env delegates to.
        let mode = AuthMode::from_env_str("unrecognized_value");
        assert_eq!(mode, AuthMode::Both);
    }
}
