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

/// Validate that the request carries some form of authentication.
/// The proxy does not verify the key itself; it just prevents accidental
/// open proxying by requiring callers to present a credential.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub async fn validate_auth(
    headers: HeaderMap,
    request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let has_api_key = headers.contains_key("x-api-key");
    let has_bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("Bearer "))
        .unwrap_or(false);

    if !has_api_key && !has_bearer {
        let err = create_anthropic_error(
            anthropic::ErrorType::AuthenticationError,
            "Missing authentication. Provide x-api-key or Authorization header.".to_string(),
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

    let header_value: axum::http::HeaderValue = request_id.parse().unwrap();
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
