use crate::server::routes::{
    log_request, record_virtual_key_usage, set_backend_error_kind, RequestCtx,
};
use crate::server::state::{AppState, ConcurrencyPermit};
use anyllm_translate::gemini::request::GenerateContentRequest;
use anyllm_translate::mapping::gemini_message_map;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

use super::count_tokens::gemini_count_tokens;
use super::non_streaming::{
    call_backend_non_streaming, gemini_error_from_backend, parse_model_action, GeminiAction,
};
use super::stream::gemini_stream;

/// POST /v1beta/models/{model_action}
///
/// `model_action` is the path segment after `/v1beta/models/`, e.g.:
///   `gemini-2.5-pro:generateContent`
///   `gemini-2.5-flash:streamGenerateContent`
pub(crate) async fn gemini_input_handler(
    Path(model_action): Path<String>,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    permit: Option<axum::Extension<ConcurrencyPermit>>,
    vk_ctx: Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>,
    Json(gemini_req): Json<GenerateContentRequest>,
) -> Response {
    let (model, action) = parse_model_action(&model_action);
    let permit = permit.map(|axum::Extension(p)| p);
    let vk_ctx = vk_ctx.map(|axum::Extension(c)| c);

    // countTokens: local computation only, no backend call needed.
    if matches!(action, GeminiAction::CountTokens) {
        return gemini_count_tokens(model, gemini_req).await;
    }

    let is_streaming = matches!(action, GeminiAction::Stream);

    state.metrics.record_request();

    // Translate Gemini request -> Anthropic request.
    let mut anthropic_req = gemini_message_map::gemini_to_anthropic_request(&gemini_req, model);
    if is_streaming {
        anthropic_req.stream = Some(true);
    }

    // Enforce model allowlist for virtual keys.
    if let Some(ref ctx) = vk_ctx {
        if !crate::server::policy::is_model_allowed(&anthropic_req.model, &ctx.allowed_models) {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": {"code": 403, "message": format!("Model '{}' is not allowed for this API key.", anthropic_req.model), "status": "PERMISSION_DENIED"}
                })),
            )
                .into_response();
        }
    }

    // Resolve model -> backend mapping.
    let (mapped_model, effective, deployment) =
        match state.resolve_model_and_state(&anthropic_req.model) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
    if let Some(ref ctx) = vk_ctx {
        if let Err(error) = crate::server::policy::enforce_route_scope(
            &effective.backend_name,
            &effective.shared,
            &ctx.allowed_routes,
        )
        .await
        {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": {"code": 403, "message": error.message(), "status": "PERMISSION_DENIED"}
                })),
            )
                .into_response();
        }
    }
    anthropic_req = match super::super::secret_redaction::redact_json_value(
        effective.redact_secrets(),
        anthropic_req,
    )
    .await
    {
        Ok(req) => req,
        Err(err) => {
            let status = err.status_code();
            let status_str = if status.is_client_error() {
                "INVALID_ARGUMENT"
            } else {
                "INTERNAL"
            };
            return (
                status,
                Json(serde_json::json!({
                    "error": {
                        "code": status.as_u16(),
                        "message": err.safe_message(),
                        "status": status_str
                    }
                })),
            )
                .into_response();
        }
    };
    let ctx = RequestCtx {
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string(),
        start: std::time::Instant::now(),
        model_requested: anthropic_req.model.clone(),
    };
    if let Some(ref d) = deployment {
        d.record_start();
    }

    if is_streaming {
        return gemini_stream(
            effective,
            anthropic_req,
            ctx,
            mapped_model,
            deployment,
            permit,
            vk_ctx,
        )
        .await;
    }

    // ------------------------------------------------------------------ non-streaming
    let backend_start = std::time::Instant::now();

    let result = call_backend_non_streaming(&effective, &anthropic_req, &mapped_model).await;

    if let Some(ref d) = deployment {
        d.record_finish(backend_start.elapsed().as_millis() as u64);
    }

    match result {
        Ok(anthropic_resp) => {
            effective.metrics.record_success();
            let tokens = (
                anthropic_resp.usage.input_tokens as u64,
                anthropic_resp.usage.output_tokens as u64,
            );
            let cost = record_virtual_key_usage(
                &effective.shared,
                &vk_ctx,
                &mapped_model,
                tokens.0,
                tokens.1,
            );
            log_request(
                &effective.shared,
                ctx.log_entry_with_attribution(
                    &effective.backend_name,
                    Some(mapped_model),
                    200,
                    Some(tokens),
                    false,
                    None,
                    &vk_ctx,
                    Some(cost),
                ),
            );
            let gemini_resp = gemini_message_map::anthropic_to_gemini_response(&anthropic_resp);
            Json(gemini_resp).into_response()
        }
        Err(e) => {
            effective.metrics.record_error();
            let mut entry = ctx.log_entry_with_attribution(
                &effective.backend_name,
                Some(mapped_model),
                e.status_code(),
                None,
                false,
                Some(e.to_string()),
                &vk_ctx,
                None,
            );
            set_backend_error_kind(&mut entry, &e);
            log_request(&effective.shared, entry);
            // Return Gemini-shaped error (Google API error format).
            let (status, msg) = gemini_error_from_backend(&e);
            (
                status,
                Json(serde_json::json!({
                    "error": {"code": status.as_u16(), "message": msg, "status": "INTERNAL"}
                })), // Wait! The original code used "message": msg, let's verify.
            ) // Ah! The original code had: "error": {"code": status.as_u16(), "message": msg, "status": "INTERNAL"}
                .into_response()
        }
    }
}
