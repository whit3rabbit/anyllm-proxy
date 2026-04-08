// Anthropic passthrough handler: forwards raw request bytes to the real Anthropic API.
// No translation: the proxy receives Anthropic format and returns Anthropic format.

use crate::backend::BackendClient;
use anyllm_translate::{anthropic, mapping};
use axum::{
    body::Bytes,
    extract::{OriginalUri, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

use super::state::AppState;

/// Forward an Anthropic-format request byte-for-byte to the upstream Anthropic API.
/// No translation is performed. Only active when `BACKEND=anthropic`.
pub(crate) async fn anthropic_passthrough(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<super::middleware::VirtualKeyContext>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    state.metrics.record_request();

    let client = match &state.backend {
        BackendClient::Anthropic(c) => c,
        _ => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "Backend is not configured as anthropic passthrough".to_string(),
                None,
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response();
        }
    };

    // Collect Anthropic-specific client headers to forward upstream.
    // anthropic-beta enables beta features; must reach upstream to take effect.
    // x-claude-code-session-id allows upstream and intermediary proxies to correlate sessions.
    let extra_headers: Vec<(&str, &str)> = ["x-claude-code-session-id", "anthropic-beta"]
        .iter()
        .filter_map(|&name| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|v| (name, v))
        })
        .collect();

    // Peek at just the `stream` and `model` fields instead of parsing the full body.
    // Full deserialization would be wasteful for image-heavy requests
    // (up to 32MB) when we only need one boolean to choose the handler.
    #[derive(serde::Deserialize)]
    struct BodyPeek {
        #[serde(default)]
        stream: bool,
        model: Option<String>,
    }
    let peek = serde_json::from_slice::<BodyPeek>(&body).unwrap_or(BodyPeek {
        stream: false,
        model: None,
    });
    let is_stream = peek.stream;

    // Enforce model allowlist for virtual keys.
    if let Some(axum::Extension(ref ctx)) = vk_ctx {
        match &peek.model {
            Some(m) => {
                if !super::policy::is_model_allowed(m, &ctx.allowed_models) {
                    let err = mapping::errors_map::create_anthropic_error(
                        anthropic::ErrorType::PermissionError,
                        format!("Model '{}' is not allowed for this API key.", m),
                        None,
                    );
                    return (StatusCode::FORBIDDEN, Json(err)).into_response();
                }
            }
            None => {
                // If a model allowlist is configured, we cannot verify the request
                // is permitted without knowing the model. Reject rather than bypass.
                if ctx.allowed_models.is_some() {
                    let err = mapping::errors_map::create_anthropic_error(
                        anthropic::ErrorType::InvalidRequestError,
                        "Request must include a 'model' field when a model allowlist is configured."
                            .to_string(),
                        None,
                    );
                    return (StatusCode::BAD_REQUEST, Json(err)).into_response();
                }
            }
        }
    }

    if is_stream {
        match client.forward_stream(body, &extra_headers).await {
            Ok((response, rate_limits)) => {
                state.metrics.record_success();
                // Pipe the raw SSE stream through to the client
                let stream = response.bytes_stream();
                let mut resp = axum::body::Body::from_stream(stream).into_response();
                resp.headers_mut()
                    .insert("content-type", "text/event-stream".parse().unwrap());
                resp.headers_mut()
                    .insert("cache-control", "no-cache".parse().unwrap());
                rate_limits.inject_anthropic_response_headers(resp.headers_mut());
                resp
            }
            Err(e) => {
                state.metrics.record_error();
                passthrough_error_to_response(e)
            }
        }
    } else {
        match client.forward(body, &extra_headers).await {
            Ok((resp_body, rate_limits)) => {
                state.metrics.record_success();
                let mut resp = (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    resp_body,
                )
                    .into_response();
                rate_limits.inject_anthropic_response_headers(resp.headers_mut());
                resp
            }
            Err(e) => {
                state.metrics.record_error();
                passthrough_error_to_response(e)
            }
        }
    }
}

/// Generic catch-all passthrough for any /v1/* path in Anthropic mode.
/// Forwards batch, file CRUD, count_tokens, and other Anthropic-native endpoints
/// directly to the upstream Anthropic API. Registered after /v1/messages so that
/// route retains its dedicated streaming/model-peek logic.
pub(crate) async fn anthropic_generic_passthrough(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    state.metrics.record_request();

    let client = match &state.backend {
        BackendClient::Anthropic(c) => c,
        _ => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "Backend is not configured as anthropic passthrough".to_string(),
                None,
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response();
        }
    };

    // Build full path with query string preserved.
    let mut full_path = uri.path().to_string();
    if let Some(q) = uri.query() {
        full_path.push('?');
        full_path.push_str(q);
    }

    // Collect owned Strings before building the &str slice (lifetime requirement).
    let session_id = headers
        .get("x-claude-code-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let beta = headers
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut extra: Vec<(&str, &str)> = Vec::new();
    if let Some(ref v) = session_id {
        extra.push(("x-claude-code-session-id", v));
    }
    if let Some(ref v) = beta {
        extra.push(("anthropic-beta", v));
    }

    match client
        .forward_generic(method, &full_path, body, &extra)
        .await
    {
        Ok(response) => {
            let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::OK);
            if status.is_success() {
                state.metrics.record_success();
            } else {
                state.metrics.record_error();
            }
            // Preserve upstream content-type (batches return application/x-jsonl, etc.)
            let upstream_ct = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_string();
            let stream = response.bytes_stream();
            let axum_body = axum::body::Body::from_stream(stream);
            let mut resp = (status, axum_body).into_response();
            if let Ok(hv) = axum::http::HeaderValue::from_str(&upstream_ct) {
                resp.headers_mut().insert("content-type", hv);
            }
            resp
        }
        Err(e) => {
            state.metrics.record_error();
            passthrough_error_to_response(e)
        }
    }
}

/// Convert an AnthropicClientError into a Response.
/// For API errors, return the upstream error body directly (it's already Anthropic format).
fn passthrough_error_to_response(
    error: crate::backend::anthropic_client::AnthropicClientError,
) -> Response {
    use crate::backend::anthropic_client::AnthropicClientError;
    match error {
        AnthropicClientError::ApiError { status, body } => {
            let http_status =
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (http_status, [("content-type", "application/json")], body).into_response()
        }
        AnthropicClientError::Transport(msg) => {
            tracing::error!("Anthropic passthrough transport error: {msg}");
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "An internal error occurred while communicating with the upstream service."
                    .to_string(),
                None,
            );
            (StatusCode::BAD_GATEWAY, Json(err)).into_response()
        }
    }
}
