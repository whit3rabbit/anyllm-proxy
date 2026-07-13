use crate::backend::{BackendClient, MAX_SSE_BUFFER_SIZE};
use crate::openai_tool_policy::{
    backend_kind_for_policy, validate_anthropic_tool_request, OpenAiToolPolicyContext,
};
use crate::server::middleware::ClientAuthPath;
use crate::server::routes::{log_request, record_virtual_key_usage, RequestCtx};
use crate::server::state::{AppState, ConcurrencyPermit};
use crate::server::streaming::{observe_anthropic_sse_frames, AnthropicStreamUsage, StreamOutcome};
use anyllm_translate::{anthropic, mapping};
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use bytes::BytesMut;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::super::auth::resolve_client_auth_override;
use super::errors::{
    passthrough_error_status, passthrough_error_to_response, virtual_key_accounting_parse_error,
};

/// Forward an Anthropic-format request byte-for-byte to the upstream Anthropic API.
/// No translation is performed. Only active when `BACKEND=anthropic`.
pub(crate) async fn anthropic_passthrough(
    State(state): State<AppState>,
    permit: Option<axum::Extension<ConcurrencyPermit>>,
    vk_ctx: Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>,
    auth_path: Option<axum::Extension<ClientAuthPath>>,
    claims: Option<axum::Extension<crate::server::oidc::JwtClaims>>,
    headers: axum::http::HeaderMap,
    mut body: Bytes,
) -> Response {
    let permit = permit.map(|axum::Extension(p)| p);
    let vk_ctx = vk_ctx.map(|axum::Extension(c)| c);
    let auth_path = auth_path.map(|axum::Extension(p)| p);
    let claims = claims.map(|axum::Extension(c)| c);
    state.metrics.record_request();

    let auth_override_ref = resolve_client_auth_override(
        state.forward_client_auth_enabled(),
        auth_path,
        &vk_ctx,
        &claims,
        &headers,
    );

    let thinking_repair_namespace = match &vk_ctx {
        Some(ctx) => format!("{}\u{0}{}", state.backend_name, ctx.key_id),
        None => state.backend_name.clone(),
    };

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

    let extra_headers: Vec<(&str, &str)> = ["x-claude-code-session-id", "anthropic-beta"]
        .iter()
        .filter_map(|&name| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|v| (name, v))
        })
        .collect();

    #[derive(serde::Deserialize)]
    struct BodyPeek {
        #[serde(default)]
        stream: bool,
        model: Option<String>,
    }
    let peek = serde_json::from_slice::<BodyPeek>(&body).unwrap_or(BodyPeek {
        stream: false,
        model: None,
    });
    let is_stream = peek.stream;
    let ctx = RequestCtx {
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string(),
        start: std::time::Instant::now(),
        model_requested: peek.model.clone().unwrap_or_else(|| "unknown".to_string()),
    };

    if let Some(ref ctx) = vk_ctx {
        match &peek.model {
            Some(m) => {
                if !crate::server::policy::is_model_allowed(m, &ctx.allowed_models) {
                    let err = mapping::errors_map::create_anthropic_error(
                        anthropic::ErrorType::PermissionError,
                        format!("Model '{}' is not allowed for this API key.", m),
                        None,
                    );
                    return (StatusCode::FORBIDDEN, Json(err)).into_response();
                }
            }
            None => {
                if ctx.allowed_models.is_some() {
                    let err = mapping::errors_map::create_anthropic_error(
                        anthropic::ErrorType::InvalidRequestError,
                        "Request must include a 'model' field when a model allowlist is configured."
                            .to_string(),
                        None,
                    );
                    return (StatusCode::BAD_REQUEST, Json(err)).into_response();
                }
            }
        }
    }

    let mut pxpipe_apply: Option<(std::sync::Arc<crate::pxpipe::PxpipeEngine>, String)> = None;
    let mut rtk_apply: Option<(std::sync::Arc<crate::rtk::RtkEngine>, String)> = None;
    let optimizer_apply = state
        .effective_optimizer()
        .filter(|e| e.mode() != anyllm_optimize_core::Mode::Off);
    if let Ok(mut parsed_req) = serde_json::from_slice::<anthropic::MessageCreateRequest>(&body) {
        if let Err(err) = validate_anthropic_tool_request(
            &parsed_req,
            OpenAiToolPolicyContext {
                backend_kind: backend_kind_for_policy(&state.backend),
                provider_id: state.provider_id.as_deref(),
                model: &parsed_req.model,
                provider_catalog: &state.provider_catalog,
            },
        ) {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::InvalidRequestError,
                err.to_string(),
                None,
            );
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }

        if let Some(store) = state.active_thinking_repair() {
            if let Some(what) = crate::thinking_repair::repair_request(
                &store,
                &thinking_repair_namespace,
                &mut parsed_req,
            )
            .await
            {
                match crate::thinking_repair::patch_repaired_body(&body, &parsed_req) {
                    Ok(bytes) => {
                        tracing::info!(repair = %what, "anthropic thinking-block repair applied");
                        body = bytes;
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "failed to patch repaired anthropic request; forwarding original body"
                        );
                    }
                }
            }
        }

        if let Some(engine) = state.pxpipe_engine_for(&parsed_req.model) {
            pxpipe_apply = Some((engine, parsed_req.model.clone()));
        }
        if let Some(engine) = state.rtk_engine_for(&parsed_req.model) {
            rtk_apply = Some((engine, parsed_req.model.clone()));
        }
    }

    let mut body = match crate::server::secret_redaction::redact_body_with_content_type(
        state.redact_secrets(),
        Some("application/json"),
        body,
    )
    .await
    {
        Ok(body) => body,
        Err(err) => return crate::server::secret_redaction::error_response(err),
    };

    if let Some(engine) = optimizer_apply {
        body = engine.optimize_anthropic_bytes(body, "messages", &state.metrics);
    }

    if let Some((engine, model)) = rtk_apply {
        body = engine.compress_anthropic(body, &model, &state.metrics);
    }
    if let Some((engine, model)) = pxpipe_apply {
        body = engine.compress_anthropic(body, &model, &state.metrics);
    }

    if is_stream {
        match client
            .forward_stream(body, &extra_headers, auth_override_ref)
            .await
        {
            Ok((response, rate_limits)) => {
                let (tx, rx) = mpsc::channel::<Result<Bytes, std::convert::Infallible>>(32);
                let metrics = state.metrics.clone();
                let log_shared = state.shared.clone();
                let log_backend_name = state.backend_name.clone();
                let cost_model = peek.model.clone().unwrap_or_else(|| "unknown".to_string());
                let thinking_repair = state.active_thinking_repair();
                let thinking_repair_namespace = thinking_repair_namespace.clone();

                tokio::spawn(async move {
                    let _permit = permit;
                    metrics.record_stream_started();
                    let mut byte_stream = response.bytes_stream();
                    let mut buffer = BytesMut::new();
                    let mut search_from = 0;
                    let mut usage = AnthropicStreamUsage::default();
                    let mut outcome = StreamOutcome::Completed;
                    let mut recorder = thinking_repair
                        .is_some()
                        .then(crate::thinking_repair::ThinkingRecorder::new);
                    let mut ready_to_commit: Vec<(String, Vec<anthropic::ContentBlock>)> =
                        Vec::new();

                    while let Some(chunk_result) = byte_stream.next().await {
                        let bytes = match chunk_result {
                            Ok(b) => b,
                            Err(e) => {
                                tracing::error!("Anthropic passthrough stream read error: {e}");
                                metrics.record_error();
                                outcome = StreamOutcome::UpstreamError;
                                break;
                            }
                        };

                        if tx.send(Ok(bytes.clone())).await.is_err() {
                            outcome = StreamOutcome::ClientDisconnected;
                            break;
                        }

                        if buffer.len() + bytes.len() > MAX_SSE_BUFFER_SIZE {
                            tracing::error!(
                                buffer_len = buffer.len(),
                                "Anthropic passthrough SSE buffer exceeded maximum size"
                            );
                            metrics.record_error();
                            outcome = StreamOutcome::UpstreamError;
                            break;
                        }
                        buffer.extend_from_slice(&bytes);
                        observe_anthropic_sse_frames(
                            &mut buffer,
                            &mut search_from,
                            &mut usage,
                            recorder.as_mut(),
                            &mut ready_to_commit,
                        );
                    }

                    if let Some(store) = &thinking_repair {
                        for (msg_id, blocks) in ready_to_commit {
                            store
                                .commit(&thinking_repair_namespace, &msg_id, blocks)
                                .await;
                        }
                    }

                    let tokens = usage.tokens();
                    let cost = tokens.map(|(input_t, output_t)| {
                        record_virtual_key_usage(
                            &log_shared,
                            &vk_ctx,
                            &cost_model,
                            input_t,
                            output_t,
                        )
                    });
                    let (status, err) = outcome.record(&metrics);
                    log_request(
                        &log_shared,
                        ctx.log_entry_with_attribution(
                            &log_backend_name,
                            Some(cost_model),
                            status,
                            tokens,
                            true,
                            err,
                            &vk_ctx,
                            cost,
                        ),
                    );
                });

                let stream = ReceiverStream::new(rx);
                let mut resp = axum::body::Body::from_stream(stream).into_response();
                resp.headers_mut()
                    .insert("content-type", "text/event-stream".parse().unwrap());
                resp.headers_mut()
                    .insert("cache-control", "no-cache".parse().unwrap());
                rate_limits.inject_anthropic_response_headers(resp.headers_mut());
                resp
            }
            Err(e) => {
                state.metrics.record_error();
                let status = passthrough_error_status(&e);
                log_request(
                    &state.shared,
                    ctx.log_entry_with_attribution(
                        &state.backend_name,
                        peek.model.clone(),
                        status,
                        None,
                        true,
                        Some(e.to_string()),
                        &vk_ctx,
                        None,
                    ),
                );
                passthrough_error_to_response(e)
            }
        }
    } else {
        match client
            .forward(body, &extra_headers, auth_override_ref)
            .await
        {
            Ok((resp_body, rate_limits)) => {
                let mut parsed_resp =
                    serde_json::from_slice::<anthropic::MessageResponse>(&resp_body);

                if let Some(store) = state.active_thinking_repair() {
                    if let Ok(resp) = parsed_resp.as_mut() {
                        let content = std::mem::take(&mut resp.content);
                        let msg_id = resp.id.clone();
                        crate::thinking_repair::record_response(
                            &store,
                            &thinking_repair_namespace,
                            &msg_id,
                            content,
                        )
                        .await;
                    }
                }
                if vk_ctx.is_some() {
                    let anthropic_resp = match parsed_resp {
                        Ok(resp) => resp,
                        Err(e) => {
                            state.metrics.record_error();
                            log_request(
                                &state.shared,
                                ctx.log_entry_with_attribution(
                                    &state.backend_name,
                                    peek.model.clone(),
                                    StatusCode::BAD_GATEWAY.as_u16(),
                                    None,
                                    false,
                                    Some(format!(
                                        "failed to parse upstream usage for virtual key accounting: {e}"
                                    )),
                                    &vk_ctx,
                                    None,
                                ),
                            );
                            return virtual_key_accounting_parse_error();
                        }
                    };
                    state.metrics.record_success();
                    let tokens = (
                        anthropic_resp.usage.input_tokens as u64,
                        anthropic_resp.usage.output_tokens as u64,
                    );
                    let cost = record_virtual_key_usage(
                        &state.shared,
                        &vk_ctx,
                        &anthropic_resp.model,
                        tokens.0,
                        tokens.1,
                    );
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(anthropic_resp.model),
                            200,
                            Some(tokens),
                            false,
                            None,
                            &vk_ctx,
                            Some(cost),
                        ),
                    );
                } else {
                    state.metrics.record_success();
                }
                let mut resp = (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    resp_body,
                )
                    .into_response();
                rate_limits.inject_anthropic_response_headers(resp.headers_mut());
                resp
            }
            Err(e) => {
                state.metrics.record_error();
                let status = passthrough_error_status(&e);
                log_request(
                    &state.shared,
                    ctx.log_entry_with_attribution(
                        &state.backend_name,
                        peek.model.clone(),
                        status,
                        None,
                        false,
                        Some(e.to_string()),
                        &vk_ctx,
                        None,
                    ),
                );
                passthrough_error_to_response(e)
            }
        }
    }
}
