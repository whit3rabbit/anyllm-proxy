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

/// Global reference to the virtual keys DashMap, set once during startup.
/// Checked during auth after the static ALLOWED_KEY_HASHES check.
static VIRTUAL_KEYS: OnceLock<Arc<DashMap<[u8; 32], VirtualKeyMeta>>> = OnceLock::new();

/// Initialize the global virtual keys reference. Called once from main.
pub fn set_virtual_keys(keys: Arc<DashMap<[u8; 32], VirtualKeyMeta>>) {
    let _ = VIRTUAL_KEYS.set(keys);
}

/// Global OIDC config, set once during startup when OIDC_ISSUER_URL is configured.
static OIDC_CONFIG: OnceLock<Arc<super::super::oidc::OidcConfig>> = OnceLock::new();

/// Initialize the global OIDC config. Called once from main when OIDC is enabled.
pub fn set_oidc_config(config: Arc<super::super::oidc::OidcConfig>) {
    let _ = OIDC_CONFIG.set(config);
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
    /// Hex-encoded credential hash used as stable distributed rate-limit key.
    #[cfg(feature = "redis")]
    pub(crate) key_hash_hex: String,
    pub(crate) rate_state: Arc<RateLimitState>,
    /// Optional model allowlist from the virtual key policy.
    pub(crate) allowed_models: Option<Vec<String>>,
    /// Optional route allowlist from the virtual key policy.
    pub(crate) allowed_routes: Option<Vec<String>>,
    /// Set to the new period_start ISO string when a budget period was reset
    /// during this request's auth check. Signals `record_cost` to call
    /// `reset_period_spend` before `accumulate_spend` so SQLite stays in sync.
    pub(crate) period_reset: Option<String>,
}

/// Which of `validate_auth`'s four success paths authenticated this request.
/// Inserted into request extensions at every success branch so a handler can
/// tell what kind of credential got it in -- used by `ANTHROPIC_FORWARD_CLIENT_AUTH`
/// to decide whether it's safe to forward that same credential upstream as the
/// real Anthropic auth: only `StaticKey`/`OpenRelay` mean "the credential that
/// gated this request IS the operator's own secret" for a single-key/BYOK
/// deployment. A virtual key is deliberately not a real Anthropic credential,
/// and a JWT is a proxy-auth artifact, so those two paths must never be
/// forwarded upstream regardless of the toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAuthPath {
    /// OIDC/JWT bearer token.
    OidcJwt,
    /// A single static key from `PROXY_API_KEYS`.
    StaticKey,
    /// A per-tenant virtual key (never a real upstream credential).
    VirtualKey,
    /// `PROXY_OPEN_RELAY=true`: any non-empty credential accepted.
    OpenRelay,
    /// Loopback-open default: no proxy auth configured at all, request came
    /// from a loopback peer. Not a real credential, so never forwarded upstream
    /// (`client_auth_forwardable` returns false for it).
    LoopbackOpen,
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

    /// Returns true when virtual key / static API key authentication is accepted.
    pub fn allows_key_auth(&self) -> bool {
        matches!(self, AuthMode::KeysOnly | AuthMode::Both)
    }

    /// Returns true when OIDC/JWT bearer token authentication is accepted.
    pub fn allows_oidc(&self) -> bool {
        matches!(self, AuthMode::OidcOnly | AuthMode::Both)
    }
}

static AUTH_MODE: LazyLock<AuthMode> = LazyLock::new(|| {
    let mode = AuthMode::from_env();
    tracing::info!(?mode, "auth mode configured");
    mode
});

/// Pre-hashed allowed API keys for constant-time comparison without
/// leaking key length via timing. Each key is SHA-256 hashed at startup.
static ALLOWED_KEY_HASHES: LazyLock<Vec<[u8; 32]>> = LazyLock::new(|| {
    let keys: Vec<String> = std::env::var("PROXY_API_KEYS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    // Posture logging (open-relay warn / loopback-open warn) is emitted once at
    // startup by `log_effective_auth_posture`, AFTER virtual keys and OIDC are
    // registered, so it reflects the true posture instead of the partial
    // static-key/open-relay state visible at this LazyLock's init time.
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

/// Whether open-relay mode is actually active for this process. This is the
/// canonical answer -- PROXY_OPEN_RELAY=true alone is not enough once any
/// PROXY_API_KEYS entry exists, see the `OPEN_RELAY` static above. Exposed so
/// callers outside this module (the startup safeguard in
/// `main_helpers::async_main`, the admin API's `PUT /admin/api/config`) can
/// check the real gate instead of re-deriving it from env vars a second time.
pub fn open_relay_active() -> bool {
    *OPEN_RELAY
}

/// Number of distinct entries in `PROXY_API_KEYS`, deduplicated. Derived from
/// the same hashed list `validate_auth`'s Check 1 uses, not a separate
/// re-parse of the raw env var, so it can never diverge from what actually
/// authenticates a request. An identical key string repeated in
/// `PROXY_API_KEYS` (e.g. "keyA,keyA") hashes to the same digest and counts
/// once.
pub fn distinct_static_key_count() -> usize {
    ALLOWED_KEY_HASHES
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len()
}

/// Whether any virtual key is registered right now. Live state (the DashMap is
/// mutated at runtime by the admin API), unlike the startup-fixed static-key
/// list, so a key added after boot flips this on.
fn has_virtual_keys() -> bool {
    VIRTUAL_KEYS.get().map(|m| !m.is_empty()).unwrap_or(false)
}

/// Whether the proxy has NO auth configured at all: no static keys, no open
/// relay, no virtual keys, no OIDC. In this state the proxy falls back to the
/// loopback-open default (see [`validate_auth`]): localhost is accepted without
/// a credential, LAN/remote peers are still rejected with 401.
fn no_auth_configured() -> bool {
    ALLOWED_KEY_HASHES.is_empty()
        && !*OPEN_RELAY
        && !has_virtual_keys()
        && OIDC_CONFIG.get().is_none()
}

/// Effective auth posture for display/telemetry, surfaced via
/// `GET /admin/api/status` (`auth_mode`) and the admin UI warning banner.
/// Serialized `snake_case` so the wire value stays stable for the admin UI's
/// TS union; adding a variant is a compile error at every match site instead
/// of silently desyncing from a bare `&'static str`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveAuthMode {
    /// `PROXY_OPEN_RELAY=true`: any non-empty key accepted on all interfaces.
    OpenRelay,
    /// At least one static key, virtual key, or OIDC configured (auth enforced).
    Keys,
    /// Nothing configured: localhost open, LAN rejected (the default).
    LoopbackOnly,
}

/// The effective auth posture for this process:
/// - [`EffectiveAuthMode::OpenRelay`] when `PROXY_OPEN_RELAY=true` (any non-empty
///   key accepted on all interfaces).
/// - [`EffectiveAuthMode::LoopbackOnly`] when nothing is configured (localhost
///   open, LAN rejected, the default).
/// - [`EffectiveAuthMode::Keys`] otherwise (auth enforced).
pub fn effective_auth_mode() -> EffectiveAuthMode {
    if open_relay_active() {
        EffectiveAuthMode::OpenRelay
    } else if no_auth_configured() {
        EffectiveAuthMode::LoopbackOnly
    } else {
        EffectiveAuthMode::Keys
    }
}

/// Emit a single startup log line describing the effective auth posture. Called
/// from `async_main` after virtual keys and OIDC are registered, so the message
/// is accurate even for an OIDC-only or virtual-key-only deployment (unlike the
/// old boot-time warn, which fired from the static-key `LazyLock` before those
/// sources existed and could mislabel such setups as "loopback-only").
pub fn log_effective_auth_posture() {
    match effective_auth_mode() {
        EffectiveAuthMode::OpenRelay => tracing::warn!(
            "PROXY_OPEN_RELAY=true: proxy accepts ANY non-empty key on all \
             interfaces. Set PROXY_API_KEYS to restrict access."
        ),
        EffectiveAuthMode::LoopbackOnly => tracing::warn!(
            "No PROXY_API_KEYS, virtual keys, OIDC, or PROXY_OPEN_RELAY set: \
             accepting unauthenticated requests from localhost only; LAN/remote \
             peers get 401. Set PROXY_API_KEYS to require a key, or \
             PROXY_OPEN_RELAY=true to accept any key on all interfaces."
        ),
        EffectiveAuthMode::Keys => {
            tracing::info!("proxy auth enforced via keys / virtual keys / OIDC")
        }
    }
}

/// Whether the request's TCP peer is a loopback address. Reads `ConnectInfo`
/// (the real connection peer), NOT `X-Forwarded-For`, which is client-spoofable.
/// Fails closed: if `ConnectInfo` is absent (proxy not served with connect
/// info), returns false so the loopback-open default never accidentally opens.
fn peer_is_loopback(request: &Request<Body>) -> bool {
    request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| is_loopback_ip(ci.0.ip()))
        .unwrap_or(false)
}

/// Loopback test that also accepts IPv4-mapped IPv6 (`::ffff:127.0.0.1`):
/// dual-stack listeners present IPv4 loopback peers as mapped v6, which std's
/// `Ipv6Addr::is_loopback()` (only `::1`) would wrongly classify as remote and
/// reject with 401. Never widens the surface beyond a genuine loopback peer.
fn is_loopback_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6
                    .to_ipv4_mapped()
                    .map(|v4| v4.is_loopback())
                    .unwrap_or(false)
        }
    }
}

/// True when forwarding the client's own credential upstream
/// (`ANTHROPIC_FORWARD_CLIENT_AUTH`) could let different callers each
/// redirect the real Anthropic credential: 2+ distinct static keys with no
/// open relay. Pure/parameterized (not reading env or the `LazyLock` statics
/// directly) so it stays deterministically testable; callers pass
/// `distinct_static_key_count()`/`open_relay_active()` (this module) or,
/// for the pre-request startup check, freshly-computed equivalents.
pub fn forward_client_auth_misconfigured(key_count: usize, open_relay: bool) -> bool {
    key_count > 1 && !open_relay
}

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
    // Loopback-open default: when NO proxy auth is configured (no static keys,
    // no open relay, no virtual keys, no OIDC), accept requests whose TCP peer
    // is loopback so `localhost` works out of the box. LAN/remote peers fall
    // through to the checks below and get 401. Runs before credential parsing so
    // a header-less localhost call succeeds.
    // ponytail: trusts the TCP peer. Behind a reverse proxy on localhost every
    // request looks loopback -> effectively open; set PROXY_API_KEYS then.
    if no_auth_configured() && peer_is_loopback(&request) {
        request
            .extensions_mut()
            .insert(ClientAuthPath::LoopbackOpen);
        return Ok(next.run(request).await);
    }

    // Accept x-api-key (Anthropic), x-goog-api-key (Gemini CLI), or Authorization: Bearer.
    let api_key = headers
        .get("x-api-key")
        .or_else(|| headers.get("x-goog-api-key"))
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
            if super::super::oidc::looks_like_jwt(&credential) {
                match oidc.validate_token(&credential) {
                    Ok(claims) => {
                        tracing::debug!(sub = ?claims.sub, auth_path = "jwt", "authentication successful");
                        request.extensions_mut().insert(claims);
                        request.extensions_mut().insert(ClientAuthPath::OidcJwt);
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
        request.extensions_mut().insert(ClientAuthPath::StaticKey);
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
            // Reject expired virtual keys at auth time (lazy eviction).
            if let Some(exp) = meta.expires_at {
                let now_secs = (now_ms() / 1000) as i64;
                if now_secs >= exp {
                    drop(meta);
                    // Remove expired key from cache so future lookups skip it.
                    if let Some(h) = hmac_hash {
                        map.remove(&h);
                    } else {
                        map.remove(&credential_hash);
                    }
                    let err_body = serde_json::json!({
                        "error": {
                            "type": "authentication_error",
                            "message": "Virtual key has expired."
                        }
                    });
                    return Err((StatusCode::UNAUTHORIZED, Json(err_body)).into_response());
                }
            }

            // RBAC: developer keys cannot access admin endpoints.
            // Case-insensitive to prevent bypass via `/Admin/`, `/ADMIN/`, etc.
            if meta.role == KeyRole::Developer {
                let path = request.uri().path().to_ascii_lowercase();
                if path.starts_with("/admin/") || path == "/admin" {
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

            #[cfg(feature = "redis")]
            let key_hash_hex: String = credential_hash.iter().map(|b| format!("{b:02x}")).collect();

            // Enforce TPM limit pre-check
            if let Some(tpm_limit) = meta.tpm_limit {
                #[allow(unused_mut, unused_variables)]
                let mut checked_ext = false;
                #[cfg(feature = "redis")]
                {
                    if let Some(redis_limiter) = crate::ratelimit::get_redis_rate_limiter() {
                        checked_ext = true;
                        if let Err(retry_after) = redis_limiter
                            .check_tpm(&key_hash_hex, tpm_limit, now_ms)
                            .await
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
            let mut period_reset: Option<String> = None;
            if meta.max_budget_usd.is_some() {
                let did_reset = check_and_reset_period(&mut meta);
                if did_reset {
                    period_reset = meta.period_start.clone();
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
                #[cfg(feature = "redis")]
                key_hash_hex,
                rate_state: meta.rate_state.clone(),
                allowed_models: meta.allowed_models.clone(),
                allowed_routes: meta.allowed_routes.clone(),
                period_reset,
            });
            request.extensions_mut().insert(ClientAuthPath::VirtualKey);

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
        request.extensions_mut().insert(ClientAuthPath::OpenRelay);
        return Ok(next.run(request).await);
    }

    // No match found: reject. `no_auth_configured()` here means the peer is
    // non-loopback (loopback short-circuits at the top), so explain the
    // localhost-only default rather than a generic "not configured".
    let message = if no_auth_configured() {
        "This proxy accepts unauthenticated requests from localhost only. \
         Set PROXY_API_KEYS to allow authenticated remote access."
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

#[cfg(test)]
#[path = "auth/tests.rs"]
mod tests;
