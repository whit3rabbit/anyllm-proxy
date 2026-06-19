use crate::backend::{find_double_newline, BackendClient, BackendError, MAX_SSE_BUFFER_SIZE};
use crate::server::routes::{
    backend_error_to_response, log_request, record_virtual_key_usage, set_backend_error_kind,
    RequestCtx,
};
use crate::server::state::{AppState, ConcurrencyPermit};
use crate::server::streaming::StreamOutcome;
use anyllm_translate::anthropic;
use anyllm_translate::gemini::response::GenerateContentResponse;
use anyllm_translate::mapping::{message_map, streaming_map};
use axum::response::{
    sse::{Event, KeepAlive, Sse},
    IntoResponse, Response,
};
use bytes::BytesMut;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::non_streaming::call_backend_non_streaming;

/// Streaming path: call the backend and translate events to Gemini SSE format.
pub(super) async fn gemini_stream(
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
                        anyllm_translate::mapping::gemini_message_map::anthropic_to_gemini_response(
                            &anthropic_resp,
                        );
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
