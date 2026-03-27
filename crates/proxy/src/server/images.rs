// Image generation passthrough handler: forward requests to the backend unchanged.

use crate::server::routes::AppState;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

/// POST /v1/images/generations -- JSON passthrough.
/// Forwards the request body to the backend and returns the response unchanged.
pub async fn image_generations(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
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
