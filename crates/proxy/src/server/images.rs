// Image generation passthrough handler: forward requests to the backend unchanged.

use crate::server::state::AppState;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    Extension,
};

/// POST /v1/images/generations -- JSON passthrough.
/// Forwards the request body to the backend and returns the response unchanged.
pub async fn image_generations(
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

    state.metrics.record_request();

    match state
        .backend
        .raw_passthrough("/v1/images/generations", body, &content_type)
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
            tracing::error!("images passthrough error: {e}");
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
