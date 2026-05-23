// Gemini native input handler: POST /v1beta/models/{model}:generateContent
//                               POST /v1beta/models/{model}:streamGenerateContent
//                               POST /v1beta/models/{model}:countTokens
//
// Accepts Gemini generateContent API requests (as sent by the gemini-cli tool),
// translates them to Anthropic format, calls the configured backend, and returns
// Gemini-format responses. This allows the gemini CLI to point at the proxy via
// GEMINI_BASE_URL without any client-side changes.

use crate::backend::anthropic_client::AnthropicClientError;
use crate::backend::bedrock_client::BedrockClientError;
use crate::backend::{find_double_newline, BackendClient, BackendError, MAX_SSE_BUFFER_SIZE};
use crate::server::routes::{
    backend_error_to_response, log_request, record_virtual_key_usage, set_backend_error_kind,
    RequestCtx,
};
use crate::server::state::{AppState, ConcurrencyPermit};
use crate::server::streaming::StreamOutcome;
use anyllm_translate::anthropic;
use anyllm_translate::gemini::request::GenerateContentRequest;
use anyllm_translate::gemini::response::GenerateContentResponse;
use anyllm_translate::mapping::{gemini_message_map, message_map, streaming_map};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
};
use bytes::BytesMut;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

enum GeminiAction {
    Generate,
    Stream,
    CountTokens,
    Unknown,
}

/// Extract model name and action from a `{model}:{action}` path segment.
fn parse_model_action(model_action: &str) -> (&str, GeminiAction) {
    if let Some(model) = model_action.strip_suffix(":streamGenerateContent") {
        (model, GeminiAction::Stream)
    } else if let Some(model) = model_action.strip_suffix(":generateContent") {
        (model, GeminiAction::Generate)
    } else if let Some(model) = model_action.strip_suffix(":countTokens") {
        (model, GeminiAction::CountTokens)
    } else {
        // Unknown action suffix — treat as non-streaming with the full string as model.
        (model_action, GeminiAction::Unknown)
    }
}

/// POST /v1beta/models/{model}:countTokens
///
/// Translates the Gemini request to Anthropic format and counts tokens using
/// the tiktoken o200k_base approximation. Returns `{"totalTokens": N}`.
/// No backend call is made — purely local computation.
async fn gemini_count_tokens(model: &str, gemini_req: GenerateContentRequest) -> Response {
    let anthropic_req = gemini_message_map::gemini_to_anthropic_request(&gemini_req, model);
    match tokio::task::spawn_blocking(move || {
        crate::server::token_counting::count_request_tokens_sync(&anthropic_req)
    })
    .await
    {
        Ok(n) => Json(serde_json::json!({ "totalTokens": n })).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": {"code": 500, "message": "token counting failed", "status": "INTERNAL"}
            })),
        )
            .into_response(),
    }
}

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
                })),
            )
                .into_response()
        }
    }
}

/// Call the backend in non-streaming mode and return an Anthropic MessageResponse.
async fn call_backend_non_streaming(
    state: &AppState,
    req: &anthropic::MessageCreateRequest,
    mapped_model: &str,
) -> Result<anthropic::MessageResponse, BackendError> {
    let original_model = req.model.clone();
    match &state.backend {
        BackendClient::OpenAI(client)
        | BackendClient::AzureOpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let mut openai_req = message_map::anthropic_to_openai_request(req);
            openai_req.model = mapped_model.to_string();
            let (openai_resp, _status, _rate_limits) = client.chat_completion(&openai_req).await?;
            Ok(message_map::openai_to_anthropic_response(
                &openai_resp,
                &original_model,
            ))
        }
        BackendClient::OpenAIResponses(client) => {
            let mut openai_req = message_map::anthropic_to_openai_request(req);
            openai_req.model = mapped_model.to_string();
            let (openai_resp, _status, _rate_limits) = client.chat_completion(&openai_req).await?;
            Ok(message_map::openai_to_anthropic_response(
                &openai_resp,
                &original_model,
            ))
        }
        BackendClient::GeminiNative(client) => {
            // Already have Gemini types; translate Anthropic -> Gemini, call, translate back.
            let gemini_req_out =
                anyllm_translate::mapping::gemini_message_map::anthropic_to_gemini_request(req);
            let gemini_resp = client
                .generate_content(&gemini_req_out, mapped_model)
                .await?;
            Ok(
                anyllm_translate::mapping::gemini_message_map::gemini_to_anthropic_response(
                    &gemini_resp,
                    &original_model,
                ),
            )
        }
        BackendClient::Anthropic(client) => {
            let body = serde_json::to_vec(req).map_err(|e| {
                BackendError::Anthropic(AnthropicClientError::Transport(e.to_string()))
            })?;
            let (resp_bytes, _rate_limits) = client.forward(body.into(), &[]).await?;
            let resp: anthropic::MessageResponse =
                serde_json::from_slice(&resp_bytes).map_err(|e| {
                    BackendError::Anthropic(AnthropicClientError::Transport(e.to_string()))
                })?;
            Ok(resp)
        }
        BackendClient::Bedrock(client) => {
            let body = serde_json::to_vec(req)
                .map_err(|e| BackendError::Bedrock(BedrockClientError::Transport(e.to_string())))?;
            let (resp_bytes, _rate_limits) = client.forward(body.into(), mapped_model).await?;
            let resp: anthropic::MessageResponse = serde_json::from_slice(&resp_bytes)
                .map_err(|e| BackendError::Bedrock(BedrockClientError::Transport(e.to_string())))?;
            Ok(resp)
        }
    }
}

/// Streaming path: call the backend and translate events to Gemini SSE format.
async fn gemini_stream(
    state: AppState,
    body: anthropic::MessageCreateRequest,
    ctx: RequestCtx,
    mapped_model: String,
    deployment: Option<std::sync::Arc<crate::config::model_router::Deployment>>,
    concurrency_permit: Option<ConcurrencyPermit>,
    vk_ctx: Option<crate::server::middleware::VirtualKeyContext>,
) -> Response {
    match &state.backend {
        BackendClient::OpenAI(client)
        | BackendClient::AzureOpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let client = client.clone();
            let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(32);
            let metrics = state.metrics.clone();
            let log_shared = state.shared.clone();
            let log_backend_name = state.backend_name.clone();
            let model = body.model.clone();
            let permit = concurrency_permit;
            let cost_model = mapped_model.clone();

            let mut openai_req = message_map::anthropic_to_openai_request(&body);
            openai_req.model = mapped_model.clone();

            tokio::spawn(async move {
                let _permit = permit;
                let _deployment = deployment;
                metrics.record_stream_started();

                let (response, _rate_limits) =
                    match client.chat_completion_stream(&openai_req).await {
                        Ok(v) => v,
                        Err(e) => {
                            let backend_error = BackendError::from(e);
                            metrics.record_error();
                            tracing::error!("gemini input stream backend error: {backend_error}");
                            let mut entry = ctx.log_entry_with_attribution(
                                &log_backend_name,
                                Some(mapped_model),
                                backend_error.status_code(),
                                None,
                                true,
                                Some(backend_error.to_string()),
                                &vk_ctx,
                                None,
                            );
                            set_backend_error_kind(&mut entry, &backend_error);
                            log_request(&log_shared, entry);
                            return;
                        }
                    };

                let mut buffer = BytesMut::new();
                let mut translator = streaming_map::StreamingTranslator::new(model.clone());
                let mut search_from: usize = 0;
                let mut byte_stream = response.bytes_stream();

                let mut outcome = StreamOutcome::Completed;
                let mut done = false;

                'outer: while let Some(chunk) = byte_stream.next().await {
                    let bytes = match chunk {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!("stream read error: {e}");
                            metrics.record_error();
                            outcome = StreamOutcome::UpstreamError;
                            break;
                        }
                    };

                    if buffer.len() + bytes.len() > MAX_SSE_BUFFER_SIZE {
                        tracing::error!("SSE buffer exceeded max, aborting gemini input stream");
                        metrics.record_error();
                        outcome = StreamOutcome::UpstreamError;
                        break;
                    }
                    buffer.extend_from_slice(&bytes);

                    while let Some((pos, delim_len)) = find_double_newline(&buffer, search_from) {
                        if let Ok(frame_str) = std::str::from_utf8(&buffer[..pos]) {
                            for line in frame_str.lines() {
                                let line = line.trim();
                                if let Some(json_str) = line.strip_prefix("data: ") {
                                    let events = if json_str == "[DONE]" {
                                        done = true;
                                        translator.finish()
                                    } else {
                                        match serde_json::from_str(json_str) {
                                            Ok(chunk) => translator.process_chunk(&chunk),
                                            Err(_) => vec![],
                                        }
                                    };
                                    for ev in &events {
                                        if let Some(gemini_chunk) =
                                            anthropic_event_to_gemini_chunk(ev, &model)
                                        {
                                            let data = match serde_json::to_string(&gemini_chunk) {
                                                Ok(s) => s,
                                                Err(_) => continue,
                                            };
                                            if tx
                                                .send(Ok(Event::default().data(data)))
                                                .await
                                                .is_err()
                                            {
                                                outcome = StreamOutcome::ClientDisconnected;
                                                break 'outer;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let _ = buffer.split_to(pos + delim_len);
                        search_from = 0;
                    }
                    search_from = buffer.len().saturating_sub(3);
                }
                if matches!(outcome, StreamOutcome::Completed) && !done {
                    let _ = translator.finish();
                }
                let tokens = translator
                    .usage()
                    .map(|u| (u.input_tokens as u64, u.output_tokens as u64));
                let cost = tokens.map(|(input_t, output_t)| {
                    record_virtual_key_usage(&log_shared, &vk_ctx, &cost_model, input_t, output_t)
                });
                let (status, err) = outcome.record(&metrics);
                log_request(
                    &log_shared,
                    ctx.log_entry_with_attribution(
                        &log_backend_name,
                        Some(mapped_model),
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
            Sse::new(stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }

        // For non-OpenAI backends, fall back to buffered "streaming" (single SSE event).
        _ => {
            match call_backend_non_streaming(&state, &body, &mapped_model).await {
                Ok(anthropic_resp) => {
                    state.metrics.record_success();
                    let tokens = (
                        anthropic_resp.usage.input_tokens as u64,
                        anthropic_resp.usage.output_tokens as u64,
                    );
                    let cost = record_virtual_key_usage(
                        &state.shared,
                        &vk_ctx,
                        &mapped_model,
                        tokens.0,
                        tokens.1,
                    );
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some(tokens),
                            true,
                            None,
                            &vk_ctx,
                            Some(cost),
                        ),
                    );
                    let gemini_resp =
                        gemini_message_map::anthropic_to_gemini_response(&anthropic_resp);
                    let data = serde_json::to_string(&gemini_resp).unwrap_or_default();
                    let permit = concurrency_permit;
                    // Single SSE event containing the full response.
                    let stream = futures::stream::once(async move {
                        let _permit = permit;
                        Ok::<_, std::convert::Infallible>(Event::default().data(data))
                    });
                    Sse::new(stream).into_response()
                }
                Err(e) => {
                    state.metrics.record_error();
                    let mut entry = ctx.log_entry_with_attribution(
                        &state.backend_name,
                        Some(mapped_model),
                        e.status_code(),
                        None,
                        true,
                        Some(e.to_string()),
                        &vk_ctx,
                        None,
                    );
                    set_backend_error_kind(&mut entry, &e);
                    log_request(&state.shared, entry);
                    backend_error_to_response(e)
                }
            }
        }
    }
}

/// Convert a single Anthropic SSE event to a partial Gemini GenerateContentResponse,
/// if it carries content that should be forwarded to the Gemini CLI.
fn anthropic_event_to_gemini_chunk(
    event: &anthropic::StreamEvent,
    model: &str,
) -> Option<GenerateContentResponse> {
    use anyllm_translate::gemini::request::{Content, Part};
    use anyllm_translate::gemini::response::{Candidate, FinishReason, UsageMetadata};

    match event {
        anthropic::StreamEvent::ContentBlockDelta {
            delta: anthropic::Delta::TextDelta { text },
            ..
        } => Some(GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".to_string()),
                    parts: vec![Part::text(text.clone())],
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
            model_version: None,
        }),

        anthropic::StreamEvent::MessageDelta { delta, usage } => {
            let finish_reason = match delta.stop_reason {
                Some(anthropic::StopReason::MaxTokens) => Some(FinishReason::MAX_TOKENS),
                _ => Some(FinishReason::STOP),
            };
            Some(GenerateContentResponse {
                candidates: vec![Candidate {
                    content: Content {
                        role: Some("model".to_string()),
                        parts: vec![],
                    },
                    finish_reason,
                    safety_ratings: None,
                }],
                // Anthropic SSE MessageDelta only carries output token counts;
                // prompt_token_count comes from MessageStart which we don't forward.
                usage_metadata: usage.as_ref().map(|u| UsageMetadata {
                    prompt_token_count: 0,
                    candidates_token_count: u.output_tokens,
                    total_token_count: u.output_tokens,
                    cached_content_token_count: 0,
                }),
                model_version: Some(model.to_string()),
            })
        }

        // All other events (ping, content_block_start/stop, message_start) don't
        // carry content that maps to a Gemini chunk.
        _ => None,
    }
}

/// Map a BackendError to an HTTP status and message for the Gemini error response.
fn gemini_error_from_backend(error: &BackendError) -> (StatusCode, String) {
    if let Some((msg, status)) = error.api_error_details() {
        let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return (code, msg);
    }
    tracing::error!("gemini input backend error: {error}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "An internal error occurred while communicating with the upstream service.".to_string(),
    )
}
