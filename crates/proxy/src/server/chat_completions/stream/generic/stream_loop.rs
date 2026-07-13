use crate::backend::{BackendClient, BackendError, SseFrameBuffer};
use crate::openai_tool_policy::{
    backend_kind_for_policy, parse_openai_chat_completion_chunk, prepare_openai_tool_request,
    OpenAiToolPolicyContext,
};
use crate::server::routes::{inject_degradation_header, log_request, set_backend_error_kind};
use crate::server::state::AppState;
use anyllm_translate::{anthropic, mapping, openai, ReverseStreamingTranslator};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::StreamExt;

use crate::server::chat_completions::helpers::{
    backend_error_to_openai_response, openai_error_response,
};

use super::super::ChatCompletionsStreamMeta;
use super::tool_loop::run_tool_loop_for_stream;

pub async fn generic_chat_completions_stream(
    state: AppState,
    anthropic_req: anthropic::MessageCreateRequest,
    meta: ChatCompletionsStreamMeta,
) -> Response {
    let ChatCompletionsStreamMeta {
        ctx,
        original_model,
        mapped_model: mapped_model_resolved,
        mut warnings,
        safe_headers: _,
        concurrency_permit,
        vk_ctx,
        deployment_accounting,
        ..
    } = meta;

    // Translate to OpenAI format for the backend
    let mut openai_req = mapping::message_map::anthropic_to_openai_request(&anthropic_req);
    crate::server::routes::inject_gemini_thinking(&anthropic_req, &state.backend, &mut openai_req);
    crate::server::routes::inject_glm_thinking(&anthropic_req, &state.backend, &mut openai_req);
    openai_req.model = mapped_model_resolved;
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
        BackendClient::Anthropic(_)
        | BackendClient::Bedrock(_)
        | BackendClient::GeminiNative(_) => {
            return openai_error_response(
                "This backend does not support /v1/chat/completions. Use /v1/messages instead.",
                "invalid_request_error",
                StatusCode::BAD_REQUEST,
            );
        }
    };

    let policy_model = openai_req.model.clone();
    if let Err(err) = prepare_openai_tool_request(
        &mut openai_req,
        OpenAiToolPolicyContext {
            backend_kind: backend_kind_for_policy(&state.backend),
            provider_id: state.provider_id.as_deref(),
            model: &policy_model,
            provider_catalog: &state.provider_catalog,
        },
        &mut warnings,
    ) {
        return openai_error_response(
            err.message(),
            "invalid_request_error",
            StatusCode::BAD_REQUEST,
        );
    }

    let mapped_model = openai_req.model.clone();

    // Opt-in RTK tool-output compression on the streaming translate path.
    // Order: tool-request preparation (above) runs before compression;
    // on the non-streaming path in handler.rs the order is reversed.
    // Both paths are correct because RTK and tool policy touch disjoint
    // fields (text content vs tool_call IDs / tool_choice / tools[]).
    state.apply_rtk_to_openai(&mut openai_req, &mapped_model);

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
            let tool_engine = state.tool_engine.clone();
            let anthropic_req_for_tools = anthropic_req.clone();
            let client_for_tools = client.clone();
            let omit_stream_options_for_tools = state.omit_stream_options;
            let backend_kind_for_tools = backend_kind_for_policy(&state.backend);
            let provider_id_for_tools = state.provider_id.clone();
            let provider_catalog_for_tools = state.provider_catalog.clone();
            let permit = concurrency_permit;
            let mut deployment_accounting = deployment_accounting;

            tokio::spawn(async move {
                let _permit = permit;
                metrics.record_stream_started();
                let mut translator = ReverseStreamingTranslator::new(
                    format!("chatcmpl-{}", uuid::Uuid::new_v4().as_simple()),
                    model_for_translator.clone(),
                );
                let mut stream_translator =
                    mapping::streaming_map::StreamingTranslator::new(model_for_translator.clone());

                let mut byte_stream = resp.bytes_stream();
                let mut buffer = SseFrameBuffer::new();
                let mut timed_out = false;
                // Accumulate tool call fragments for collect-then-execute.
                // Each entry: (id, function_name, arguments_json).
                let mut accumulated_tool_calls: Vec<(String, String, String)> = Vec::new();

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
                        let frames = match buffer.push(&bytes) {
                            Ok(frames) => frames,
                            Err(e) => {
                                tracing::error!(error = %e, "SSE buffer exceeded maximum size");
                                metrics.record_error();
                                metrics.record_stream_failed();
                                return;
                            }
                        };

                        for frame in frames {
                            if let Ok(frame_str) = std::str::from_utf8(&frame) {
                                for line in frame_str.lines() {
                                    let line = line.trim();
                                    if let Some(json_str) = line.strip_prefix("data: ") {
                                        if json_str == "[DONE]" {
                                            // Defer [DONE] until after potential tool execution.
                                            continue;
                                        }
                                        // Parse OpenAI chunk, translate to Anthropic events,
                                        // then reverse-translate to OpenAI chunks
                                        if let Ok(chunk) =
                                            parse_openai_chat_completion_chunk(json_str)
                                        {
                                            // Accumulate tool call fragments from delta.
                                            if let Some(choice) = chunk.choices.first() {
                                                if let Some(ref tc_list) = choice.delta.tool_calls {
                                                    for tc in tc_list {
                                                        let idx = tc.index as usize;
                                                        while accumulated_tool_calls.len() <= idx {
                                                            accumulated_tool_calls.push((
                                                                String::new(),
                                                                String::new(),
                                                                String::new(),
                                                            ));
                                                        }
                                                        if let Some(ref id) = tc.id {
                                                            if !id.is_empty() {
                                                                accumulated_tool_calls[idx].0 =
                                                                    id.clone();
                                                            }
                                                        }
                                                        if let Some(ref func) = tc.function {
                                                            if let Some(ref name) = func.name {
                                                                if !name.is_empty() {
                                                                    accumulated_tool_calls[idx].1 =
                                                                        name.clone();
                                                                }
                                                            }
                                                            if let Some(ref args) = func.arguments {
                                                                accumulated_tool_calls[idx]
                                                                    .2
                                                                    .push_str(args);
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            let anthropic_events =
                                                stream_translator.process_chunk(&chunk);
                                            for event in &anthropic_events {
                                                let oai_chunks = translator.process_event(event);
                                                for oai_chunk in &oai_chunks {
                                                    if let Ok(json) =
                                                        serde_json::to_string(oai_chunk)
                                                    {
                                                        let sse_line =
                                                            format!("data: {}\n\n", json);
                                                        if tx.send(Ok(sse_line)).await.is_err() {
                                                            metrics
                                                                .record_stream_client_disconnected(
                                                                );
                                                            return; // Client disconnected
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
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

                // Collect-then-execute loop: bounded by engine.loop_config.max_iterations.
                // Mirrors the non-streaming `maybe_execute_tools` loop so follow-up tool
                // calls are not silently dropped.
                if !accumulated_tool_calls.is_empty() {
                    if let Some(ref engine) = tool_engine {
                        run_tool_loop_for_stream(
                            accumulated_tool_calls,
                            engine,
                            &anthropic_req_for_tools,
                            &client_for_tools,
                            omit_stream_options_for_tools,
                            &cost_model,
                            &model_for_translator,
                            &tx,
                            &vk_ctx,
                            &log_shared,
                            backend_kind_for_tools.clone(),
                            provider_id_for_tools.clone(),
                            provider_catalog_for_tools.clone(),
                        )
                        .await;
                    }
                }

                // Send final [DONE] after initial stream and any tool execution follow-up.
                let _ = tx.send(Ok("data: [DONE]\n\n".to_string())).await;

                // Extract token counts from the stream translator for cost tracking.
                let usage = stream_translator.usage();
                let tokens = usage.map(|u| (u.input_tokens as u64, u.output_tokens as u64));
                let cost = if let Some((input_t, output_t)) = tokens {
                    Some(crate::server::routes::record_virtual_key_usage(
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
                deployment_accounting.finish();
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
            if state.expose_degradation_warnings {
                inject_degradation_header(response.headers_mut(), &warnings);
            }
            response
        }
        Err(e) => {
            state.metrics.record_error();
            let backend_error = BackendError::from(e);
            let mut entry = ctx.log_entry_with_attribution(
                &state.backend_name,
                Some(mapped_model),
                backend_error.status_code(),
                None,
                true,
                Some(backend_error.to_string()),
                &vk_ctx,
                None,
            );
            set_backend_error_kind(&mut entry, &backend_error);
            log_request(&state.shared, entry);
            backend_error_to_openai_response(backend_error)
        }
    };

    response
}
