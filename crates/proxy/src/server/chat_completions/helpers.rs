use crate::backend::{BackendClient, BackendError};
use crate::server::state::AppState;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

/// OpenAI-shaped error response body.
pub(super) fn openai_error_response(
    message: &str,
    error_type: &str,
    status: StatusCode,
) -> Response {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": error_type,
            "param": null,
            "code": null
        }
    });
    (status, Json(body)).into_response()
}

/// Convert a BackendError into an OpenAI-shaped error response.
pub(super) fn backend_error_to_openai_response(error: BackendError) -> Response {
    if let Some((message, status)) = error.api_error_details() {
        let error_type = if status == 429 {
            "rate_limit_error"
        } else if status >= 500 {
            "server_error"
        } else {
            "invalid_request_error"
        };
        let http_status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return openai_error_response(&message, error_type, http_status);
    }
    tracing::error!("backend client error: {error}");
    openai_error_response(
        "An internal error occurred while communicating with the upstream service.",
        "server_error",
        StatusCode::INTERNAL_SERVER_ERROR,
    )
}

pub(super) fn safe_anthropic_extra_headers(
    headers: &axum::http::HeaderMap,
) -> Vec<(String, String)> {
    ["x-claude-code-session-id", "anthropic-beta"]
        .iter()
        .filter_map(|&name| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|v| (name.to_string(), v.to_string()))
        })
        .collect()
}

pub(super) fn header_refs(headers: &[(String, String)]) -> Vec<(&str, &str)> {
    headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

pub(super) fn is_anthropic_backend(state: &AppState) -> bool {
    matches!(state.backend, BackendClient::Anthropic(_))
}

pub(super) fn mapped_model_for_backend(
    original_model: &str,
    mapped_model: String,
    effective: &AppState,
) -> String {
    if is_anthropic_backend(effective) && mapped_model.is_empty() {
        original_model.to_string()
    } else {
        mapped_model
    }
}
