// OpenAI Chat Completions input handler.
//
// Accepts POST /v1/chat/completions in OpenAI format, translates through
// the Anthropic pipeline, returns OpenAI-format responses.

use crate::backend::{find_double_newline, BackendClient, BackendError, MAX_SSE_BUFFER_SIZE};
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
        return openai_error_response(message, error_type, http_status);
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
    body: Result<Json<openai::ChatCompletionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
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
        return chat_completions_stream(
            state,
            anthropic_req,
            ctx,
            original_model,
            warnings,
            permit,
        )
        .await;
    }

    // Non-streaming path
    match &state.backend {
        BackendClient::OpenAI(client)
        | BackendClient::AzureOpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&anthropic_req);
            super::routes::inject_gemini_thinking(&anthropic_req, &state.backend, &mut openai_req);
            if state.omit_stream_options {
                openai_req.stream_options = None;
            }
            openai_req.model = state.map_model(&openai_req.model);
            let mapped_model = openai_req.model.clone();

            match client.chat_completion(&openai_req).await {
                Ok((openai_resp, _status, rate_limits)) => {
                    state.metrics.record_success();
                    // Translate Anthropic response back to OpenAI format
                    let anthropic_resp = mapping::message_map::openai_to_anthropic_response(
                        &openai_resp,
                        &original_model,
                    );
                    let oai_response =
                        translate_anthropic_to_openai_response(&anthropic_resp, &original_model);
                    log_request(
                        &state.shared,
                        ctx.log_entry(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                        ),
                    );
                    let mut response = (StatusCode::OK, Json(oai_response)).into_response();
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
                    inject_degradation_header(response.headers_mut(), &warnings);
                    response
                }
                Err(e) => {
                    state.metrics.record_error();
                    let status = e.status_code();
                    log_request(
                        &state.shared,
                        ctx.log_entry(
                            &state.backend_name,
                            Some(mapped_model),
                            status,
                            None,
                            false,
                            Some(e.to_string()),
                        ),
                    );
                    backend_error_to_openai_response(BackendError::from(e))
                }
            }
        }
        BackendClient::OpenAIResponses(client) => {
            let mut responses_req =
                mapping::responses_message_map::anthropic_to_responses_request(&anthropic_req);
            responses_req.model = state.map_model(&responses_req.model);
            let mapped_model = responses_req.model.clone();

            match client.responses(&responses_req).await {
                Ok((resp, _status, rate_limits)) => {
                    state.metrics.record_success();
                    let anthropic_resp =
                        mapping::responses_message_map::responses_to_anthropic_response(
                            &resp,
                            &original_model,
                        );
                    let oai_response =
                        translate_anthropic_to_openai_response(&anthropic_resp, &original_model);
                    log_request(
                        &state.shared,
                        ctx.log_entry(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                        ),
                    );
                    let mut response = (StatusCode::OK, Json(oai_response)).into_response();
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
                    inject_degradation_header(response.headers_mut(), &warnings);
                    response
                }
                Err(e) => {
                    state.metrics.record_error();
                    let status = e.status_code();
                    log_request(
                        &state.shared,
                        ctx.log_entry(
                            &state.backend_name,
                            Some(mapped_model),
                            status,
                            None,
                            false,
                            Some(e.to_string()),
                        ),
                    );
                    backend_error_to_openai_response(BackendError::from(e))
                }
            }
        }
        BackendClient::Anthropic(_) | BackendClient::Bedrock(_) => openai_error_response(
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
) -> Response {
    // Translate to OpenAI format for the backend
    let mut openai_req = mapping::message_map::anthropic_to_openai_request(&anthropic_req);
    super::routes::inject_gemini_thinking(&anthropic_req, &state.backend, &mut openai_req);
    openai_req.model = state.map_model(&openai_req.model);
    openai_req.stream = Some(true);
    if !state.omit_stream_options {
        openai_req.stream_options = Some(openai::StreamOptions {
            include_usage: true,
        });
    }

    let client = match &state.backend {
        BackendClient::OpenAI(c)
        | BackendClient::AzureOpenAI(c)
        | BackendClient::Vertex(c)
        | BackendClient::GeminiOpenAI(c)
        | BackendClient::OpenAIResponses(c) => c.clone(),
        BackendClient::Anthropic(_) | BackendClient::Bedrock(_) => {
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
            let _permit = concurrency_permit;

            tokio::spawn(async move {
                let mut translator = ReverseStreamingTranslator::new(
                    format!("chatcmpl-{}", uuid::Uuid::new_v4().as_simple()),
                    model_for_translator.clone(),
                );
                let mut stream_translator =
                    mapping::streaming_map::StreamingTranslator::new(model_for_translator.clone());

                let mut byte_stream = resp.bytes_stream();
                let mut buffer = BytesMut::new();
                let mut search_from: usize = 0;

                while let Some(chunk_result) = byte_stream.next().await {
                    let bytes = match chunk_result {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!("stream read error: {e}");
                            metrics.record_error();
                            break;
                        }
                    };
                    buffer.extend_from_slice(&bytes);

                    if buffer.len() > MAX_SSE_BUFFER_SIZE {
                        tracing::error!("SSE buffer exceeded maximum size");
                        metrics.record_error();
                        break;
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

                metrics.record_success();
                log_request(
                    &log_shared,
                    ctx.log_entry(
                        &log_backend_name,
                        Some(mapped_model),
                        200,
                        None, // Token counts come from usage chunk, hard to capture here
                        true,
                        None,
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
                ctx.log_entry(
                    &state.backend_name,
                    Some(mapped_model),
                    e.status_code(),
                    None,
                    true,
                    Some(e.to_string()),
                ),
            );
            backend_error_to_openai_response(BackendError::from(e))
        }
    };

    response
}
