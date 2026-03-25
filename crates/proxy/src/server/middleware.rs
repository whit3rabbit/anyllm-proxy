// Auth, logging, and request size limit middleware

use anyllm_translate::anthropic;
use anyllm_translate::mapping::errors_map::create_anthropic_error;
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use subtle::ConstantTimeEq;

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
    request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bearer_token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string());

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

    // Compare SHA-256 hashes of the credential against pre-hashed allowed keys.
    // Hashing eliminates the timing side-channel on key length: all comparisons
    // operate on fixed-size 32-byte digests regardless of original key length.
    let credential_hash: [u8; 32] = Sha256::digest(credential.as_bytes()).into();
    let is_allowed = ALLOWED_KEY_HASHES
        .iter()
        .any(|h| bool::from(h.ct_eq(&credential_hash)));
    if !ALLOWED_KEY_HASHES.is_empty() && !is_allowed {
        let err = create_anthropic_error(
            anthropic::ErrorType::AuthenticationError,
            "Invalid API key.".to_string(),
            None,
        );
        return Err((StatusCode::UNAUTHORIZED, Json(err)).into_response());
    }
    // Reject if no keys configured and open-relay not explicitly enabled.
    if ALLOWED_KEY_HASHES.is_empty() && !*OPEN_RELAY {
        let err = create_anthropic_error(
            anthropic::ErrorType::AuthenticationError,
            "Server not configured for access. Contact the administrator.".to_string(),
            None,
        );
        return Err((StatusCode::UNAUTHORIZED, Json(err)).into_response());
    }

    Ok(next.run(request).await)
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
    next.run(request).await
}

/// Maximum request body size (32 MB, matching Anthropic's Messages endpoint limit).
pub const MAX_BODY_SIZE: usize = 32 * 1024 * 1024;

/// Maximum concurrent requests to prevent self-DOS under 429 incidents.
pub const MAX_CONCURRENT_REQUESTS: usize = 100;
