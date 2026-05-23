// Generic catch-all passthrough for any /v1/* path not handled by an explicit route.
// Active only in Translate mode (OpenAI-compatible backends).
// Forwards the request method, body, and selected headers to the backend,
// then streams the response back — handles JSON, binary, and SSE equally.

use crate::backend::BackendClient;
use crate::server::state::AppState;
use anyllm_translate::{anthropic, mapping};
use axum::{
    body::Bytes,
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Json, Response},
};

/// Catch-all for `ANY /v1/{*path}` paths without an explicit handler.
/// Registered last in the Translate-mode router so explicit routes take priority.
pub(crate) async fn v1_generic_passthrough(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<super::middleware::VirtualKeyContext>>,
    OriginalUri(uri): OriginalUri,
    Path(tail): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    state.metrics.record_request();

    // Virtual keys must use explicit handlers that enforce per-key policy and
    // accounting before forwarding with the proxy's provider credentials.
    if vk_ctx.is_some() {
        let err = mapping::errors_map::create_anthropic_error(
            anthropic::ErrorType::PermissionError,
            "This endpoint is not available for virtual API keys.".to_string(),
            None,
        );
        return (StatusCode::FORBIDDEN, Json(err)).into_response();
    }

    // This handler is only registered for OpenAI-compatible (Translate) backends.
    let client = match &state.backend {
        BackendClient::OpenAI(c)
        | BackendClient::AzureOpenAI(c)
        | BackendClient::Vertex(c)
        | BackendClient::GeminiOpenAI(c)
        | BackendClient::OpenAIResponses(c) => c,
        _ => {
            let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                anyllm_translate::anthropic::ErrorType::InvalidRequestError,
                format!("/v1/{tail} is not supported by this backend."),
                None,
            );
            return (StatusCode::NOT_IMPLEMENTED, axum::Json(err)).into_response();
        }
    };

    // Build backend URL: passthrough_url handles per-backend path rewriting (Azure, Vertex, etc.)
    let path = format!("/v1/{tail}");
    let mut url = client.passthrough_url(&path);

    // Preserve query string (e.g. GET /v1/files?purpose=batch&after=...)
    if let Some(query) = uri.query() {
        url.push('?');
        url.push_str(query);
    }

    // Forward safe client headers
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let body_opt = if body.is_empty() { None } else { Some(body) };

    match client
        .generic_proxy_request(method, &url, content_type.as_deref(), body_opt)
        .await
    {
        Ok(response) => {
            let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::OK);

            // Collect response headers before consuming the body
            let mut resp_headers = HeaderMap::new();
            for (name, value) in response.headers() {
                if !super::HOP_BY_HOP.contains(&name.as_str()) {
                    resp_headers.insert(name.clone(), value.clone());
                }
            }

            if status.is_success() {
                state.metrics.record_success();
            } else {
                state.metrics.record_error();
            }

            // Stream response body — works for JSON, binary, and SSE
            let stream = response.bytes_stream();
            let axum_body = axum::body::Body::from_stream(stream);
            let mut resp = (status, axum_body).into_response();
            for (k, v) in &resp_headers {
                resp.headers_mut().insert(k, v.clone());
            }
            resp
        }
        Err(e) => {
            state.metrics.record_error();
            tracing::error!("generic passthrough error for /v1/{tail}: {e}");
            let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                anyllm_translate::anthropic::ErrorType::ApiError,
                "An internal error occurred while communicating with the upstream service."
                    .to_string(),
                None,
            );
            (StatusCode::BAD_GATEWAY, axum::Json(err)).into_response()
        }
    }
}
