// Bedrock native passthrough handlers.
// Expose Bedrock Converse API and InvokeModel endpoints with SigV4 handled by the proxy.
// Callers authenticate with Bearer token; the proxy signs requests for AWS.
//
// Routes (registered under the Bedrock backend sub-router):
//   POST /model/{modelId}/converse
//   POST /model/{modelId}/converse-stream
//   POST /model/{modelId}/invoke
//   POST /model/{modelId}/invoke-with-response-stream

use crate::backend::BackendClient;
use crate::server::state::AppState;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

type VkCtx = Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>;

/// POST /model/{modelId}/converse — Bedrock Converse API (non-streaming).
pub(crate) async fn bedrock_converse(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
    headers: HeaderMap,
    vk_ctx: VkCtx,
    body: Bytes,
) -> Response {
    forward_native(&state, &model_id, &headers, vk_ctx, body, "converse", false).await
}

/// POST /model/{modelId}/converse-stream — Bedrock Converse API (streaming).
pub(crate) async fn bedrock_converse_stream(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
    headers: HeaderMap,
    vk_ctx: VkCtx,
    body: Bytes,
) -> Response {
    forward_native(
        &state,
        &model_id,
        &headers,
        vk_ctx,
        body,
        "converse-stream",
        true,
    )
    .await
}

/// POST /model/{modelId}/invoke — Bedrock InvokeModel (non-streaming, model-native format).
pub(crate) async fn bedrock_invoke(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
    headers: HeaderMap,
    vk_ctx: VkCtx,
    body: Bytes,
) -> Response {
    forward_native(&state, &model_id, &headers, vk_ctx, body, "invoke", false).await
}

/// POST /model/{modelId}/invoke-with-response-stream — Bedrock InvokeModel (streaming).
pub(crate) async fn bedrock_invoke_stream(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
    headers: HeaderMap,
    vk_ctx: VkCtx,
    body: Bytes,
) -> Response {
    forward_native(
        &state,
        &model_id,
        &headers,
        vk_ctx,
        body,
        "invoke-with-response-stream",
        true,
    )
    .await
}

async fn forward_native(
    state: &AppState,
    model_id: &str,
    headers: &HeaderMap,
    vk_ctx: VkCtx,
    body: Bytes,
    suffix: &str,
    streaming: bool,
) -> Response {
    let client = match &state.backend {
        BackendClient::Bedrock(c) => c.clone(),
        _ => {
            let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                anyllm_translate::anthropic::ErrorType::InvalidRequestError,
                "Bedrock native endpoints require BACKEND=bedrock.".to_string(),
                None,
            );
            return (StatusCode::NOT_IMPLEMENTED, axum::Json(err)).into_response();
        }
    };

    // Enforce the virtual key's model allowlist. The model comes from the URL path
    // here (Bedrock native puts modelId in the URL), so check it directly — the
    // same enforcement bedrock_passthrough applies to the body's `model` field.
    if let Some(axum::Extension(ref ctx)) = vk_ctx {
        if !crate::server::policy::is_model_allowed(model_id, &ctx.allowed_models) {
            let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                anyllm_translate::anthropic::ErrorType::PermissionError,
                format!("Model '{model_id}' is not allowed for this API key."),
                None,
            );
            return (StatusCode::FORBIDDEN, axum::Json(err)).into_response();
        }
    }

    let body =
        match super::secret_redaction::redact_body(state.redact_secrets(), headers, body).await {
            Ok(body) => body,
            Err(err) => return super::secret_redaction::error_response(err),
        };

    state.metrics.record_request();

    let url = client.native_endpoint_url(model_id, suffix);

    match client.forward_native(&url, body, streaming).await {
        Ok(response) => {
            let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::OK);

            // Forward response headers (content-type, x-amzn-* rate limit headers, etc.)
            let mut resp_headers = HeaderMap::new();
            for (name, value) in response.headers() {
                if !super::HOP_BY_HOP.contains(&name.as_str()) {
                    resp_headers.insert(name.clone(), value.clone());
                }
            }

            state.metrics.record_success();

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
            tracing::error!("Bedrock native error for {model_id}/{suffix}: {e}");

            use crate::backend::bedrock_client::BedrockClientError;
            match e {
                BedrockClientError::ApiError { status, body } => {
                    let http_status =
                        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
                    (http_status, [("content-type", "application/json")], body).into_response()
                }
                _ => {
                    let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                        anyllm_translate::anthropic::ErrorType::ApiError,
                        format!("Bedrock request failed: {e}"),
                        None,
                    );
                    (StatusCode::BAD_GATEWAY, axum::Json(err)).into_response()
                }
            }
        }
    }
}
