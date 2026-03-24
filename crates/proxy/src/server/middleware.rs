// Auth, logging, and request size limit middleware
// PLAN.md lines 890-893

use anthropic_openai_translate::anthropic;
use anthropic_openai_translate::mapping::errors_map::create_anthropic_error;
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use std::collections::HashSet;
use std::sync::LazyLock;

/// Allowed API keys loaded from `PROXY_API_KEYS` (comma-separated).
/// When the set is empty, any non-empty key is accepted (open-relay mode).
static ALLOWED_API_KEYS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let keys: HashSet<String> = std::env::var("PROXY_API_KEYS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if keys.is_empty() {
        tracing::warn!(
            "PROXY_API_KEYS is not set: proxy accepts ANY non-empty key (open-relay mode). \
             Set PROXY_API_KEYS to restrict access."
        );
    }
    keys
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

    // If PROXY_API_KEYS is configured, validate the key against the allowlist.
    if !ALLOWED_API_KEYS.is_empty() && !ALLOWED_API_KEYS.contains(&credential) {
        let err = create_anthropic_error(
            anthropic::ErrorType::AuthenticationError,
            "Invalid API key.".to_string(),
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

    let header_value: axum::http::HeaderValue = request_id.parse().unwrap_or_else(|_| {
        // Client-provided x-request-id contained invalid header characters; replace it.
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
