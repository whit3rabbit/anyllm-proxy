// Audio passthrough handlers: forward requests to the backend unchanged.
// Supports /v1/audio/transcriptions (multipart) and /v1/audio/speech (JSON -> binary).

use crate::server::state::AppState;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    Extension,
};

/// POST /v1/audio/transcriptions -- multipart/form-data passthrough.
/// Forwards the raw body (including multipart boundary) to the backend.
pub async fn audio_transcriptions(
    State(state): State<AppState>,
    vk_ctx: Option<Extension<crate::server::middleware::VirtualKeyContext>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // Multipart transcription requests are forwarded raw; reject virtual keys to
    // avoid bypassing per-key model allowlists.
    if vk_ctx.is_some() {
        let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
            anyllm_translate::anthropic::ErrorType::PermissionError,
            "This endpoint is not available for virtual API keys.".to_string(),
            None,
        );
        return (StatusCode::FORBIDDEN, Json(err)).into_response();
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("multipart/form-data")
        .to_string();

    passthrough_response(&state, "/v1/audio/transcriptions", body, &content_type).await
}

/// POST /v1/audio/speech -- JSON in, binary audio out.
/// Forwards the JSON body to the backend and streams the audio response bytes back.
pub async fn audio_speech(
    State(state): State<AppState>,
    vk_ctx: Option<Extension<crate::server::middleware::VirtualKeyContext>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Some(ref ext) = vk_ctx {
        let model = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(str::to_string));
        let ctx = &ext.0;
        match model {
            Some(model) => {
                if !crate::server::policy::is_model_allowed(&model, &ctx.allowed_models) {
                    let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                        anyllm_translate::anthropic::ErrorType::PermissionError,
                        format!("Model '{}' is not allowed for this API key.", model),
                        None,
                    );
                    return (StatusCode::FORBIDDEN, Json(err)).into_response();
                }
            }
            None if ctx.allowed_models.is_some() => {
                let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                    anyllm_translate::anthropic::ErrorType::InvalidRequestError,
                    "Request must include a 'model' field when a model allowlist is configured."
                        .to_string(),
                    None,
                );
                return (StatusCode::BAD_REQUEST, Json(err)).into_response();
            }
            None => {}
        }
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    passthrough_response(&state, "/v1/audio/speech", body, &content_type).await
}

/// Shared passthrough logic: forward to backend, return response unchanged.
async fn passthrough_response(
    state: &AppState,
    path: &str,
    body: axum::body::Bytes,
    content_type: &str,
) -> Response {
    state.metrics.record_request();

    match state
        .backend
        .raw_passthrough(path, body, content_type)
        .await
    {
        Ok((status, resp_headers, resp_body)) => {
            if status.is_success() {
                state.metrics.record_success();
            } else {
                state.metrics.record_error();
            }
            let mut response = (status, resp_body).into_response();
            for (k, v) in &resp_headers {
                response.headers_mut().insert(k, v.clone());
            }
            response
        }
        Err(e) => {
            state.metrics.record_error();
            tracing::error!("audio passthrough error: {e}");
            let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                anyllm_translate::anthropic::ErrorType::ApiError,
                "An internal error occurred while communicating with the upstream service."
                    .to_string(),
                None,
            );
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(err)).into_response()
        }
    }
}
