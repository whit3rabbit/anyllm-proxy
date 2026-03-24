// Anthropic passthrough handler: forwards raw request bytes to the real Anthropic API.
// No translation: the proxy receives Anthropic format and returns Anthropic format.

use crate::backend::BackendClient;
use anthropic_openai_translate::{anthropic, mapping};
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

use super::routes::AppState;

pub(crate) async fn anthropic_passthrough(State(state): State<AppState>, body: Bytes) -> Response {
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

    // Peek at just the `stream` field instead of parsing the full body.
    // Full deserialization would be wasteful for image-heavy requests
    // (up to 32MB) when we only need one boolean to choose the handler.
    #[derive(serde::Deserialize)]
    struct StreamPeek {
        #[serde(default)]
        stream: bool,
    }
    let is_stream = serde_json::from_slice::<StreamPeek>(&body)
        .map(|p| p.stream)
        .unwrap_or(false);

    if is_stream {
        match client.forward_stream(body).await {
            Ok((response, rate_limits)) => {
                state.metrics.record_success();
                // Pipe the raw SSE stream through to the client
                let stream = response.bytes_stream();
                let mut resp = axum::body::Body::from_stream(stream).into_response();
                resp.headers_mut()
                    .insert("content-type", "text/event-stream".parse().unwrap());
                resp.headers_mut()
                    .insert("cache-control", "no-cache".parse().unwrap());
                rate_limits.inject_anthropic_headers(resp.headers_mut());
                resp
            }
            Err(e) => {
                state.metrics.record_error();
                passthrough_error_to_response(e)
            }
        }
    } else {
        match client.forward(body).await {
            Ok((resp_body, rate_limits)) => {
                state.metrics.record_success();
                let mut resp = (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    resp_body,
                )
                    .into_response();
                rate_limits.inject_anthropic_headers(resp.headers_mut());
                resp
            }
            Err(e) => {
                state.metrics.record_error();
                passthrough_error_to_response(e)
            }
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
