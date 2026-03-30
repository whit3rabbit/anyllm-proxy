// OpenAI Chat Completions input handler.
//
// Accepts POST /v1/chat/completions in OpenAI format, translates through
// the Anthropic pipeline, returns OpenAI-format responses.

use crate::backend::{find_double_newline, BackendClient, BackendError, MAX_SSE_BUFFER_SIZE};
use crate::cache::{self, CacheBackend, CacheNamespace};
use anyllm_translate::{
    anthropic, mapping, openai, translate_anthropic_to_openai_response,
    translate_openai_to_anthropic_request, ReverseStreamingTranslator, TranslationWarnings,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use bytes::BytesMut;
use futures::StreamExt;

use super::routes::{
    inject_degradation_header, log_request, AppState, ConcurrencyPermit, RequestCtx,
};

/// OpenAI-shaped error response body.
fn openai_error_response(message: &str, error_type: &str, status: StatusCode) -> Response {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": error_type,
            "param": null,
            "code": null
        }
    });
    (status, Json(body)).into_response()
}

/// Convert a BackendError into an OpenAI-shaped error response.
fn backend_error_to_openai_response(error: BackendError) -> Response {
    if let Some((message, status)) = error.api_error_details() {
        let error_type = if status == 429 {
            "rate_limit_error"
        } else if status >= 500 {
            "server_error"
        } else {
            "invalid_request_error"
        };
        let http_status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return openai_error_response(&message, error_type, http_status);
    }
    tracing::error!("backend client error: {error}");
    openai_error_response(
        "An internal error occurred while communicating with the upstream service.",
        "server_error",
        StatusCode::INTERNAL_SERVER_ERROR,
    )
}

/// Handler for POST /v1/chat/completions (non-streaming and streaming).
pub(crate) async fn chat_completions(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    permit: Option<axum::Extension<ConcurrencyPermit>>,
    vk_ctx: Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>,
    body: Result<Json<openai::ChatCompletionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let vk_ctx = vk_ctx.map(|axum::Extension(c)| c);
    let body = match body {
        Ok(Json(b)) => b,
        Err(e) => {
            return openai_error_response(
                &e.body_text(),
                "invalid_request_error",
                StatusCode::BAD_REQUEST,
            );
        }
    };

    let permit = permit.map(|axum::Extension(p)| p);
    let ctx = RequestCtx {
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string(),
        start: std::time::Instant::now(),
        model_requested: body.model.clone(),
    };
    state.metrics.record_request();

    // Enforce model allowlist policy for virtual keys.
    if let Some(ref ctx) = vk_ctx {
        if !crate::server::policy::is_model_allowed(&body.model, &ctx.allowed_models) {
            return openai_error_response(
                &format!("Model '{}' is not allowed for this API key.", body.model),
                "permission_error",
                axum::http::StatusCode::FORBIDDEN,
            );
        }
    }

    // Translate OpenAI request -> Anthropic request
    let mut warnings = TranslationWarnings::default();
    let anthropic_req = match translate_openai_to_anthropic_request(&body, &mut warnings) {
        Ok(req) => req,
        Err(e) => {
            return openai_error_response(
                &e.to_string(),
                "invalid_request_error",
                StatusCode::BAD_REQUEST,
            );
        }
    };

    if anthropic_req.messages.is_empty() {
        return openai_error_response(
            "messages array must not be empty",
            "invalid_request_error",
            StatusCode::BAD_REQUEST,
        );
    }

    let is_streaming = body.stream == Some(true);
    let original_model = body.model.clone();

    if is_streaming {
        let mut response = chat_completions_stream(
            state,
            anthropic_req,
            ctx,
            original_model,
            warnings,
            permit,
            vk_ctx,
        )
        .await;
        response.headers_mut().insert(
            "x-anyllm-cache",
            axum::http::HeaderValue::from_static("bypass"),
        );
        return response;
    }

    // Non-streaming path: check cache before calling backend.
    let body_value = serde_json::to_value(&body).unwrap_or_default();
    let cache_ttl = match cache::parse_cache_ttl(&body_value) {
        Ok(ttl) => ttl,
        Err(msg) => {
            return openai_error_response(&msg, "invalid_request_error", StatusCode::BAD_REQUEST);
        }
    };
    let bypass_cache = cache_ttl == Some(0);
    let cache_key = if !bypass_cache {
        Some(cache::cache_key_for_request(
            &body_value,
            CacheNamespace::OpenAI,
        ))
    } else {
        None
    };

    // Check cache on non-bypass requests
    if let (Some(ref key), Some(ref c)) = (&cache_key, &state.cache) {
        if let Some(entry) = c.get(key).await {
            tracing::debug!(cache_key = %key, "cache hit for /v1/chat/completions");
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("x-anyllm-cache", "hit")
                .body(axum::body::Body::from(entry.response_body))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            inject_degradation_header(response.headers_mut(), &warnings);
            return response;
        }
    }

    // Resolve model routing (may switch to a different backend).
    let (mapped_model, effective, deployment) = match state.resolve_model_and_state(&original_model)
    {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if let Some(ref d) = deployment {
        d.record_start();
    }
    let backend_start = std::time::Instant::now();

    // Non-streaming path
    match &effective.backend {
        BackendClient::OpenAI(client)
        | BackendClient::AzureOpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&anthropic_req);
            super::routes::inject_gemini_thinking(
                &anthropic_req,
                &effective.backend,
                &mut openai_req,
            );
            // Gemini/Vertex rejects standard JSON Schema keywords; sanitize tool schemas.
            if matches!(
                effective.backend,
                BackendClient::GeminiOpenAI(_) | BackendClient::Vertex(_)
            ) {
                if let Some(tools) = openai_req.tools.take() {
                    openai_req.tools = Some(
                        tools
                            .into_iter()
                            .map(|mut t| {
                                if let Some(params) = t.function.parameters.take() {
                                    t.function.parameters = Some(
                                        mapping::tools_map::sanitize_schema_for_gemini(params),
                                    );
                                }
                                t
                            })
                            .collect(),
                    );
                }
            }
            if effective.omit_stream_options {
                openai_req.stream_options = None;
            }
            openai_req.model = mapped_model.clone();
            let mapped_model = openai_req.model.clone();

            match client.chat_completion(&openai_req).await {
                Ok((openai_resp, _status, rate_limits)) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_success();
                    // Translate Anthropic response back to OpenAI format
                    let anthropic_resp = mapping::message_map::openai_to_anthropic_response(
                        &openai_resp,
                        &original_model,
                    );
                    let oai_response =
                        translate_anthropic_to_openai_response(&anthropic_resp, &original_model);
                    super::routes::record_vk_tpm(&vk_ctx, anthropic_resp.usage.output_tokens);
                    let cost = crate::cost::record_cost(
                        &state.shared,
                        &vk_ctx,
                        &mapped_model,
                        anthropic_resp.usage.input_tokens as u64,
                        anthropic_resp.usage.output_tokens as u64,
                    );
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                            &vk_ctx,
                            Some(cost),
                        ),
                    );
                    super::routes::try_cache_response(
                        &cache_key,
                        &state.cache,
                        cache_ttl,
                        &oai_response,
                        original_model.clone(),
                    )
                    .await;

                    let cache_hv = super::routes::cache_header_value(bypass_cache);
                    let mut response = (StatusCode::OK, Json(oai_response)).into_response();
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
                    inject_degradation_header(response.headers_mut(), &warnings);
                    response.headers_mut().insert("x-anyllm-cache", cache_hv);
                    response
                }
                Err(e) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_error();
                    let status = e.status_code();
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            status,
                            None,
                            false,
                            Some(e.to_string()),
                            &vk_ctx,
                            None,
                        ),
                    );
                    backend_error_to_openai_response(BackendError::from(e))
                }
            }
        }
        BackendClient::OpenAIResponses(client) => {
            let mut responses_req =
                mapping::responses_message_map::anthropic_to_responses_request(&anthropic_req);
            responses_req.model = mapped_model.clone();
            let mapped_model = responses_req.model.clone();

            match client.responses(&responses_req).await {
                Ok((resp, _status, rate_limits)) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_success();
                    let anthropic_resp =
                        mapping::responses_message_map::responses_to_anthropic_response(
                            &resp,
                            &original_model,
                        );
                    let oai_response =
                        translate_anthropic_to_openai_response(&anthropic_resp, &original_model);
                    super::routes::record_vk_tpm(&vk_ctx, anthropic_resp.usage.output_tokens);
                    let cost = crate::cost::record_cost(
                        &state.shared,
                        &vk_ctx,
                        &mapped_model,
                        anthropic_resp.usage.input_tokens as u64,
                        anthropic_resp.usage.output_tokens as u64,
                    );
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                            &vk_ctx,
                            Some(cost),
                        ),
                    );

                    super::routes::try_cache_response(
                        &cache_key,
                        &state.cache,
                        cache_ttl,
                        &oai_response,
                        original_model.clone(),
                    )
                    .await;

                    let cache_hv = super::routes::cache_header_value(bypass_cache);
                    let mut response = (StatusCode::OK, Json(oai_response)).into_response();
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
                    inject_degradation_header(response.headers_mut(), &warnings);
                    response.headers_mut().insert("x-anyllm-cache", cache_hv);
                    response
                }
                Err(e) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_error();
                    let status = e.status_code();
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            status,
                            None,
                            false,
                            Some(e.to_string()),
                            &vk_ctx,
                            None,
                        ),
                    );
                    backend_error_to_openai_response(BackendError::from(e))
                }
            }
        }
        BackendClient::Anthropic(_) | BackendClient::Bedrock(_) | BackendClient::GeminiNative(_) => openai_error_response(
            "This backend does not support /v1/chat/completions. Use /v1/messages instead.",
            "invalid_request_error",
            StatusCode::BAD_REQUEST,
        ),
    }
}

/// Streaming handler for POST /v1/chat/completions with stream: true.
///
/// Translates the Anthropic request to OpenAI, streams the backend response,
/// then uses ReverseStreamingTranslator to convert Anthropic SSE events back
/// to OpenAI ChatCompletionChunk SSE format.
async fn chat_completions_stream(
    state: AppState,
    anthropic_req: anthropic::MessageCreateRequest,
    ctx: RequestCtx,
    original_model: String,
    warnings: TranslationWarnings,
    concurrency_permit: Option<ConcurrencyPermit>,
    vk_ctx: Option<crate::server::middleware::VirtualKeyContext>,
) -> Response {
    // Resolve model routing (may switch to a different backend).
    let (mapped_model_resolved, effective, _deployment) =
        match state.resolve_model_and_state(&original_model) {
            Ok(v) => v,
            Err(resp) => return resp,
        };

    // Translate to OpenAI format for the backend
    let mut openai_req = mapping::message_map::anthropic_to_openai_request(&anthropic_req);
    super::routes::inject_gemini_thinking(&anthropic_req, &effective.backend, &mut openai_req);
    // Gemini/Vertex rejects standard JSON Schema keywords; sanitize tool schemas.
    if matches!(
        effective.backend,
        BackendClient::GeminiOpenAI(_) | BackendClient::Vertex(_)
    ) {
        if let Some(tools) = openai_req.tools.take() {
            openai_req.tools = Some(
                tools
                    .into_iter()
                    .map(|mut t| {
                        if let Some(params) = t.function.parameters.take() {
                            t.function.parameters = Some(
                                mapping::tools_map::sanitize_schema_for_gemini(params),
                            );
                        }
                        t
                    })
                    .collect(),
            );
        }
    }
    openai_req.model = mapped_model_resolved;
    openai_req.stream = Some(true);
    if !effective.omit_stream_options {
        openai_req.stream_options = Some(openai::StreamOptions {
            include_usage: true,
        });
    }

    let client = match &effective.backend {
        BackendClient::OpenAI(c)
        | BackendClient::AzureOpenAI(c)
        | BackendClient::Vertex(c)
        | BackendClient::GeminiOpenAI(c)
        | BackendClient::OpenAIResponses(c) => c.clone(),
        BackendClient::Anthropic(_) | BackendClient::Bedrock(_) | BackendClient::GeminiNative(_) => {
            return openai_error_response(
                "This backend does not support /v1/chat/completions. Use /v1/messages instead.",
                "invalid_request_error",
                StatusCode::BAD_REQUEST,
            );
        }
    };

    let mapped_model = openai_req.model.clone();

    // Start the backend request
    let response = match client.chat_completion_stream(&openai_req).await {
        Ok((resp, rate_limits)) => {
            // Build the SSE response with OpenAI chunk format
            let (tx, rx) =
                tokio::sync::mpsc::channel::<Result<String, std::convert::Infallible>>(32);
            let metrics = state.metrics.clone();
            let log_shared = state.shared.clone();
            let log_backend_name = state.backend_name.clone();
            let model_for_translator = original_model.clone();
            let cost_model = mapped_model.clone();
            let stream_timeout_secs = state.stream_timeout_secs;
            let _permit = concurrency_permit;

            tokio::spawn(async move {
                metrics.record_stream_started();
                let mut translator = ReverseStreamingTranslator::new(
                    format!("chatcmpl-{}", uuid::Uuid::new_v4().as_simple()),
                    model_for_translator.clone(),
                );
                let mut stream_translator =
                    mapping::streaming_map::StreamingTranslator::new(model_for_translator.clone());

                let mut byte_stream = resp.bytes_stream();
                let mut buffer = BytesMut::new();
                let mut search_from: usize = 0;
                let mut timed_out = false;

                let stream_loop = async {
                    while let Some(chunk_result) = byte_stream.next().await {
                        let bytes = match chunk_result {
                            Ok(b) => b,
                            Err(e) => {
                                tracing::error!("stream read error: {e}");
                                metrics.record_error();
                                metrics.record_stream_failed();
                                return;
                            }
                        };
                        buffer.extend_from_slice(&bytes);

                        if buffer.len() > MAX_SSE_BUFFER_SIZE {
                            tracing::error!("SSE buffer exceeded maximum size");
                            metrics.record_error();
                            metrics.record_stream_failed();
                            return;
                        }

                        while let Some((pos, delim_len)) = find_double_newline(&buffer, search_from) {
                            if let Ok(frame_str) = std::str::from_utf8(&buffer[..pos]) {
                                for line in frame_str.lines() {
                                    let line = line.trim();
                                    if let Some(json_str) = line.strip_prefix("data: ") {
                                        if json_str == "[DONE]" {
                                            // Emit [DONE] for OpenAI clients
                                            let _ = tx.send(Ok("data: [DONE]\n\n".to_string())).await;
                                            continue;
                                        }
                                        // Parse OpenAI chunk, translate to Anthropic events,
                                        // then reverse-translate to OpenAI chunks
                                        if let Ok(chunk) =
                                            serde_json::from_str::<openai::ChatCompletionChunk>(
                                                json_str,
                                            )
                                        {
                                            let anthropic_events =
                                                stream_translator.process_chunk(&chunk);
                                            for event in &anthropic_events {
                                                let oai_chunks = translator.process_event(event);
                                                for oai_chunk in &oai_chunks {
                                                    if let Ok(json) = serde_json::to_string(oai_chunk) {
                                                        let sse_line = format!("data: {}\n\n", json);
                                                        if tx.send(Ok(sse_line)).await.is_err() {
                                                            metrics.record_stream_client_disconnected();
                                                            return; // Client disconnected
                                                        }
                                                    }
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
                };

                if stream_timeout_secs > 0 {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(stream_timeout_secs),
                        stream_loop,
                    )
                    .await
                    {
                        Ok(()) => {}
                        Err(_) => {
                            tracing::warn!(
                                timeout_secs = stream_timeout_secs,
                                "chat_completions streaming response exceeded wall-clock timeout"
                            );
                            metrics.record_error();
                            metrics.record_stream_failed();
                            timed_out = true;
                        }
                    }
                } else {
                    stream_loop.await;
                }

                if timed_out {
                    // Log and exit without emitting finish events on timeout.
                    log_request(
                        &log_shared,
                        ctx.log_entry_with_attribution(
                            &log_backend_name,
                            Some(mapped_model),
                            504,
                            None,
                            true,
                            Some("stream timeout".into()),
                            &vk_ctx,
                            None,
                        ),
                    );
                    return;
                }

                // Emit any remaining finish events
                let finish_events = stream_translator.finish();
                for event in &finish_events {
                    let oai_chunks = translator.process_event(event);
                    for oai_chunk in &oai_chunks {
                        if let Ok(json) = serde_json::to_string(oai_chunk) {
                            let _ = tx.send(Ok(format!("data: {}\n\n", json))).await;
                        }
                    }
                }

                if !translator.is_done() {
                    let _ = tx.send(Ok("data: [DONE]\n\n".to_string())).await;
                }

                // Extract token counts from the stream translator for cost tracking.
                let usage = stream_translator.usage();
                let tokens = usage.map(|u| (u.input_tokens as u64, u.output_tokens as u64));
                let cost = if let Some((input_t, output_t)) = tokens {
                    Some(crate::cost::record_cost(
                        &log_shared,
                        &vk_ctx,
                        &cost_model,
                        input_t,
                        output_t,
                    ))
                } else {
                    None
                };

                metrics.record_success();
                metrics.record_stream_completed();
                log_request(
                    &log_shared,
                    ctx.log_entry_with_attribution(
                        &log_backend_name,
                        Some(mapped_model),
                        200,
                        tokens,
                        true,
                        None,
                        &vk_ctx,
                        cost,
                    ),
                );
            });

            // Build the SSE response using raw text/event-stream
            let body_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
            let body = axum::body::Body::from_stream(body_stream);
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .header("connection", "keep-alive")
                .body(body)
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            rate_limits.inject_anthropic_response_headers(response.headers_mut());
            inject_degradation_header(response.headers_mut(), &warnings);
            response
        }
        Err(e) => {
            state.metrics.record_error();
            log_request(
                &state.shared,
                ctx.log_entry_with_attribution(
                    &state.backend_name,
                    Some(mapped_model),
                    e.status_code(),
                    None,
                    true,
                    Some(e.to_string()),
                    &vk_ctx,
                    None,
                ),
            );
            backend_error_to_openai_response(BackendError::from(e))
        }
    };

    response
}
