use crate::backend::BackendClient;
use crate::server::middleware::ClientAuthPath;
use crate::server::state::AppState;
use anyllm_translate::{anthropic, mapping};
use axum::{
    body::Bytes,
    extract::{OriginalUri, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

use super::super::auth::resolve_client_auth_override;
use super::errors::passthrough_error_to_response;

/// Generic catch-all passthrough for any /v1/* path in Anthropic mode.
/// Forwards batch, file CRUD, count_tokens, and other Anthropic-native endpoints
/// directly to the upstream Anthropic API. Registered after /v1/messages so that
/// route retains its dedicated streaming/model-peek logic.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn anthropic_generic_passthrough(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>,
    auth_path: Option<axum::Extension<ClientAuthPath>>,
    claims: Option<axum::Extension<crate::server::oidc::JwtClaims>>,
    OriginalUri(uri): OriginalUri,
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    state.metrics.record_request();

    // Virtual keys must use policy-aware handlers only.
    if vk_ctx.is_some() {
        let err = mapping::errors_map::create_anthropic_error(
            anthropic::ErrorType::PermissionError,
            "This endpoint is not available for virtual API keys.".to_string(),
            None,
        );
        return (StatusCode::FORBIDDEN, Json(err)).into_response();
    }
    let vk_ctx = vk_ctx.map(|axum::Extension(c)| c);
    let auth_path = auth_path.map(|axum::Extension(p)| p);
    let claims = claims.map(|axum::Extension(c)| c);
    let auth_override_ref = resolve_client_auth_override(
        state.forward_client_auth_enabled(),
        auth_path,
        &vk_ctx,
        &claims,
        &headers,
    );

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

    let mut full_path = uri.path().to_string();
    if let Some(q) = uri.query() {
        full_path.push('?');
        full_path.push_str(q);
    }

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

    let body =
        match crate::server::secret_redaction::redact_body(state.redact_secrets(), &headers, body)
            .await
        {
            Ok(body) => body,
            Err(err) => return crate::server::secret_redaction::error_response(err),
        };

    match client
        .forward_generic(method, &full_path, body, &extra, auth_override_ref)
        .await
    {
        Ok(response) => {
            let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::OK);
            if status.is_success() {
                state.metrics.record_success();
            } else {
                state.metrics.record_error();
            }
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
