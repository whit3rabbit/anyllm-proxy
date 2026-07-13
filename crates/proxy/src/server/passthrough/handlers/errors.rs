use crate::backend::anthropic_client::AnthropicClientError;
use anyllm_translate::{anthropic, mapping};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

pub(crate) fn passthrough_error_to_response(error: AnthropicClientError) -> Response {
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

pub(crate) fn passthrough_error_status(error: &AnthropicClientError) -> u16 {
    match error {
        AnthropicClientError::ApiError { status, .. } => *status,
        AnthropicClientError::Transport(_) => StatusCode::BAD_GATEWAY.as_u16(),
    }
}

pub(crate) fn virtual_key_accounting_parse_error() -> Response {
    let err = mapping::errors_map::create_anthropic_error(
        anthropic::ErrorType::ApiError,
        "Upstream response could not be accounted for this virtual API key.".to_string(),
        None,
    );
    (StatusCode::BAD_GATEWAY, Json(err)).into_response()
}
