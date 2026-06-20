use crate::backend::{BackendError, SseFrameBuffer};
use crate::openai_tool_policy::{
    backend_kind_for_policy, validate_anthropic_tool_request, OpenAiToolPolicyContext,
};
use crate::server::routes::{inject_degradation_header, log_request, set_backend_error_kind};
use crate::server::state::AppState;
use crate::server::streaming::{AnthropicStreamUsage, StreamOutcome};
use anyllm_translate::{anthropic, ReverseStreamingTranslator};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::StreamExt;

use crate::server::chat_completions::extensions::serialize_anthropic_upstream_request;
use crate::server::chat_completions::helpers::{
    backend_error_to_openai_response, header_refs, openai_error_response,
};

use super::ChatCompletionsStreamMeta;

/// Streaming handler for POST /v1/chat/completions with stream: true.
///
/// Translates the Anthropic request to OpenAI, streams the backend response,
/// then uses ReverseStreamingTranslator to convert Anthropic SSE events back
/// to OpenAI ChatCompletionChunk SSE format.
pub(super) async fn anthropic_chat_completions_stream(
    state: AppState,
    client: crate::backend::anthropic_client::AnthropicClient,
    mut anthropic_req: anthropic::MessageCreateRequest,
    meta: ChatCompletionsStreamMeta,
) -> Response {
    let ChatCompletionsStreamMeta {
        ctx,
        original_model,
        mapped_model,
        warnings,
        safe_headers,
        raw_anthropic_tools,
        tool_context,
        concurrency_permit,
        vk_ctx,
        deployment_accounting,
    } = meta;

    anthropic_req.model = mapped_model.clone();
    anthropic_req.stream = Some(true);
    if let Err(err) = validate_anthropic_tool_request(
        &anthropic_req,
        OpenAiToolPolicyContext {
            backend_kind: backend_kind_for_policy(&state.backend),
            provider_id: state.provider_id.as_deref(),
            model: &mapped_model,
            provider_catalog: &state.provider_catalog,
        },
    ) {
        return openai_error_response(
            err.message(),
            "invalid_request_error",
            StatusCode::BAD_REQUEST,
        );
    }
    let body = match serialize_anthropic_upstream_request(&anthropic_req, &raw_anthropic_tools) {
        Ok(body) => body,
        Err(e) => {
            return openai_error_response(
                &format!("failed to serialize Anthropic request: {e}"),
                "server_error",
                StatusCode::INTERNAL_SERVER_ERROR,
            );
        }
    };
    let refs = header_refs(&safe_headers);

    let (response, rate_limits) = match client.forward_stream(body, &refs).await {
        Ok(result) => result,
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
            return backend_error_to_openai_response(backend_error);
        }
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::convert::Infallible>>(32);
    let metrics = state.metrics.clone();
    let log_shared = state.shared.clone();
    let log_backend_name = state.backend_name.clone();
    let stream_timeout_secs = state.stream_timeout_secs;
    let permit = concurrency_permit;
    let mut deployment_accounting = deployment_accounting;

    tokio::spawn(async move {
        let _permit = permit;
        metrics.record_stream_started();
        let mut translator = ReverseStreamingTranslator::with_context(
            format!("chatcmpl-{}", uuid::Uuid::new_v4().as_simple()),
            original_model,
            tool_context,
        );
        let mut usage = AnthropicStreamUsage::default();
        let mut byte_stream = response.bytes_stream();
        let mut buffer = SseFrameBuffer::new();
        let mut emitted_done = false;

        let stream_loop = async {
            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = match chunk_result {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::error!("Anthropic chat-completions stream read error: {e}");
                        metrics.record_error();
                        return StreamOutcome::UpstreamError;
                    }
                };

                let frames = match buffer.push(&bytes) {
                    Ok(frames) => frames,
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            "Anthropic chat-completions SSE buffer exceeded maximum size"
                        );
                        metrics.record_error();
                        return StreamOutcome::UpstreamError;
                    }
                };

                for frame in frames {
                    if let Ok(frame_str) = std::str::from_utf8(&frame) {
                        for line in frame_str.lines() {
                            let line = line.trim();
                            let Some(json_str) = line.strip_prefix("data: ") else {
                                continue;
                            };
                            usage.observe_data(json_str);
                            let event =
                                match serde_json::from_str::<anthropic::StreamEvent>(json_str) {
                                    Ok(event) => event,
                                    Err(e) => {
                                        tracing::debug!(
                                            "failed to parse Anthropic streaming event: {e}"
                                        );
                                        continue;
                                    }
                                };
                            let chunks = translator.process_event(&event);
                            for chunk in chunks {
                                let Ok(json) = serde_json::to_string(&chunk) else {
                                    continue;
                                };
                                if tx.send(Ok(format!("data: {json}\n\n"))).await.is_err() {
                                    return StreamOutcome::ClientDisconnected;
                                }
                            }
                            if translator.is_done() && !emitted_done {
                                emitted_done = true;
                                if tx.send(Ok("data: [DONE]\n\n".to_string())).await.is_err() {
                                    return StreamOutcome::ClientDisconnected;
                                }
                            }
                        }
                    }
                }
            }
            StreamOutcome::Completed
        };

        let outcome = if stream_timeout_secs > 0 {
            match tokio::time::timeout(
                std::time::Duration::from_secs(stream_timeout_secs),
                stream_loop,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(_) => {
                    tracing::warn!(
                        timeout_secs = stream_timeout_secs,
                        "Anthropic chat-completions stream exceeded wall-clock timeout"
                    );
                    metrics.record_error();
                    StreamOutcome::UpstreamError
                }
            }
        } else {
            stream_loop.await
        };

        if matches!(outcome, StreamOutcome::Completed) && !emitted_done {
            let _ = tx.send(Ok("data: [DONE]\n\n".to_string())).await;
        }

        let tokens = usage.tokens();
        let cost = tokens.map(|(input_t, output_t)| {
            crate::server::routes::record_virtual_key_usage(
                &log_shared,
                &vk_ctx,
                &mapped_model,
                input_t,
                output_t,
            )
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
        deployment_accounting.finish();
    });

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
