// SSE streaming infrastructure and the messages_stream handler.

use crate::backend::{find_double_newline, BackendClient, RateLimitHeaders, SseFrameBuffer};
use crate::metrics::Metrics;
use crate::openai_tool_policy::{
    backend_kind_for_policy, parse_openai_chat_completion_chunk, prepare_openai_tool_request,
    tool_policy_error_to_backend_error, OpenAiToolPolicyContext,
};
use anyllm_translate::{anthropic, mapping, TranslationWarnings};
use axum::response::sse::{Event, KeepAlive, Sse};
use bytes::BytesMut;
use futures::stream::Stream;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::routes::{log_request, set_backend_error_kind, RequestCtx};
use super::state::AppState;

pub(crate) struct StreamDeploymentAccounting {
    deployment: Option<Arc<crate::config::model_router::Deployment>>,
    start: Option<Instant>,
}

impl StreamDeploymentAccounting {
    pub(crate) fn start(deployment: Option<Arc<crate::config::model_router::Deployment>>) -> Self {
        if let Some(deployment) = &deployment {
            deployment.record_start();
            Self {
                deployment: Some(deployment.clone()),
                start: Some(Instant::now()),
            }
        } else {
            Self {
                deployment: None,
                start: None,
            }
        }
    }

    pub(crate) fn finish(&mut self) {
        if let (Some(deployment), Some(start)) = (&self.deployment, self.start.take()) {
            deployment.record_finish(start.elapsed().as_millis() as u64);
        }
    }
}

impl Drop for StreamDeploymentAccounting {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Send translated stream events over the SSE channel. Returns false if client disconnected.
pub(super) async fn send_events(
    tx: &mpsc::Sender<Result<Event, std::convert::Infallible>>,
    events: &[anthropic::StreamEvent],
) -> bool {
    for ev in events {
        match super::sse::stream_event_to_sse(ev) {
            Ok(sse) => {
                if tx.send(Ok(sse)).await.is_err() {
                    return false;
                }
            }
            Err(e) => {
                tracing::warn!("failed to serialize stream event: {e}");
            }
        }
    }
    true
}

/// Why the SSE stream ended.
#[derive(Debug, Clone, Copy)]
pub(crate) enum StreamOutcome {
    /// Backend stream completed normally.
    Completed,
    /// Downstream client disconnected before the stream finished.
    ClientDisconnected,
    /// Backend stream failed (error already recorded in metrics).
    UpstreamError,
}

impl StreamOutcome {
    /// Record metrics and return (HTTP status, error message) for logging.
    pub(crate) fn record(&self, metrics: &Metrics) -> (u16, Option<String>) {
        match self {
            Self::Completed => {
                metrics.record_success();
                metrics.record_stream_completed();
                (200, None)
            }
            Self::ClientDisconnected => {
                metrics.record_stream_client_disconnected();
                (499, Some("client disconnected".into()))
            }
            Self::UpstreamError => {
                metrics.record_stream_failed();
                (502, Some("stream interrupted".into()))
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct AnthropicStreamUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

impl AnthropicStreamUsage {
    pub(crate) fn observe_data(&mut self, data: &str) {
        if data == "[DONE]" {
            return;
        }
        let Ok(event) = serde_json::from_str::<anthropic::StreamEvent>(data) else {
            return;
        };
        match event {
            anthropic::StreamEvent::MessageStart { message } => {
                self.input_tokens = Some(message.usage.input_tokens as u64);
            }
            anthropic::StreamEvent::MessageDelta {
                usage: Some(usage), ..
            } => {
                self.output_tokens = Some(usage.output_tokens as u64);
            }
            _ => {}
        }
    }

    pub(crate) fn tokens(&self) -> Option<(u64, u64)> {
        match (self.input_tokens, self.output_tokens) {
            (Some(input), Some(output)) => Some((input, output)),
            (Some(input), None) => Some((input, 0)),
            (None, Some(output)) => Some((0, output)),
            (None, None) => None,
        }
    }
}

/// Parse buffered SSE frames, updating token usage and (when `recorder` is
/// `Some`) accumulating content blocks for the thinking-block repair store.
/// Completed messages (from `message_stop`) are pushed onto `ready` for the
/// caller to commit — accumulation here is synchronous, but committing to
/// the store is async, so it can't happen inline in this loop.
pub(crate) fn observe_anthropic_sse_frames(
    buffer: &mut BytesMut,
    search_from: &mut usize,
    usage: &mut AnthropicStreamUsage,
    mut recorder: Option<&mut crate::thinking_repair::ThinkingRecorder>,
    ready: &mut Vec<(String, Vec<anthropic::ContentBlock>)>,
) {
    while let Some((pos, delim_len)) = find_double_newline(buffer, *search_from) {
        if let Ok(frame_str) = std::str::from_utf8(&buffer[..pos]) {
            for line in frame_str.lines() {
                let line = line.trim();
                if let Some(json_str) = line.strip_prefix("data: ") {
                    usage.observe_data(json_str);
                    if let Some(rec) = recorder.as_deref_mut() {
                        if let Some(done) = rec.observe_json(json_str) {
                            ready.push(done);
                        }
                    }
                }
            }
        }
        let _ = buffer.split_to(pos + delim_len);
        *search_from = 0;
    }
    *search_from = buffer.len().saturating_sub(3);
}

/// Read SSE bytes from a response, parse frames, and call `on_data` for each data line.
pub(super) async fn read_sse_frames<F>(
    response: reqwest::Response,
    tx: &mpsc::Sender<Result<Event, std::convert::Infallible>>,
    metrics: &Metrics,
    mut on_data: F,
) -> StreamOutcome
where
    F: FnMut(&str) -> Option<Vec<anthropic::StreamEvent>>,
{
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    // Buffer bytes (not String) because TCP chunks may split mid-UTF-8 character.
    // String::from_utf8_lossy would permanently replace partial trailing bytes
    // with U+FFFD, corrupting the JSON payload.
    let mut buffer = SseFrameBuffer::new();
    // Reuse a single events buffer across all frames to avoid per-frame allocation
    let mut frame_events: Vec<anthropic::StreamEvent> = Vec::new();

    while let Some(chunk_result) = stream.next().await {
        let bytes = match chunk_result {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("stream read error: {e}");
                metrics.record_error();
                return StreamOutcome::UpstreamError;
            }
        };
        let frames = match buffer.push(&bytes) {
            Ok(frames) => frames,
            Err(e) => {
                tracing::error!(error = %e, "SSE buffer exceeded maximum size, aborting stream");
                metrics.record_error();
                return StreamOutcome::UpstreamError;
            }
        };

        for frame in frames {
            frame_events.clear();
            // Convert the complete frame bytes to UTF-8. A frame ending at
            // a double-newline boundary should always be valid UTF-8; if not,
            // skip the malformed frame rather than injecting replacement chars.
            match std::str::from_utf8(&frame) {
                Ok(frame_str) => {
                    for line in frame_str.lines() {
                        let line = line.trim();
                        if let Some(json_str) = line.strip_prefix("data: ") {
                            if let Some(mut events) = on_data(json_str) {
                                frame_events.append(&mut events);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("skipping non-UTF-8 SSE frame: {e}");
                }
            }

            if !send_events(tx, &frame_events).await {
                tracing::debug!("client disconnected during stream");
                return StreamOutcome::ClientDisconnected;
            }
        }
    }

    StreamOutcome::Completed
}

/// Build an SSE response that streams Anthropic events translated from backend chunks.
/// Returns rate limit headers alongside the SSE stream so the caller can inject them.
/// Pre-stream backend errors (e.g., 401, 429, 500 before any data) are returned as
/// `Err(BackendError)` so the caller can respond with a proper HTTP status code.
/// Logging is deferred: each spawned task logs after the stream completes with actual
/// latency, status, and token counts.
pub(crate) async fn messages_stream(
    state: AppState,
    body: anthropic::MessageCreateRequest,
    ctx: RequestCtx,
    mapped_model: String,
    concurrency_permit: Option<super::state::ConcurrencyPermit>,
    vk_ctx: Option<crate::server::middleware::VirtualKeyContext>,
    deployment_accounting: StreamDeploymentAccounting,
) -> Result<
    (
        RateLimitHeaders,
        Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>,
    ),
    crate::backend::BackendError,
> {
    let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(32);
    let (rl_tx, rl_rx) =
        tokio::sync::oneshot::channel::<Result<RateLimitHeaders, crate::backend::BackendError>>();

    let metrics = state.metrics.clone();
    let log_shared = state.shared.clone();
    let log_backend_name = state.backend_name.clone();
    let stream_timeout_secs = state.stream_timeout_secs;

    match &state.backend {
        BackendClient::OpenAI(client)
        | BackendClient::AzureOpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let client = client.clone();
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
            super::routes::inject_gemini_thinking(&body, &state.backend, &mut openai_req);
            super::routes::inject_glm_thinking(&body, &state.backend, &mut openai_req);
            if state.omit_stream_options {
                openai_req.stream_options = None;
            }
            openai_req.model = mapped_model.clone();
            let policy_model = openai_req.model.clone();
            let mut policy_warnings = TranslationWarnings::default();
            if let Err(err) = prepare_openai_tool_request(
                &mut openai_req,
                OpenAiToolPolicyContext {
                    backend_kind: backend_kind_for_policy(&state.backend),
                    provider_id: state.provider_id.as_deref(),
                    model: &policy_model,
                    provider_catalog: &state.provider_catalog,
                },
                &mut policy_warnings,
            ) {
                return Err(tool_policy_error_to_backend_error(err));
            }
            let model = body.model.clone();
            let permit = concurrency_permit.clone();
            let mut deployment_accounting = deployment_accounting;

            tokio::spawn(async move {
                // Hold concurrency permit until the stream completes, not just
                // until headers are sent, so the semaphore accurately bounds
                // concurrent streaming connections.
                let _permit = permit;
                metrics.record_stream_started();
                match client.chat_completion_stream(&openai_req).await {
                    Ok((response, rate_limits)) => {
                        rl_tx.send(Ok(rate_limits)).ok();
                        let mut translator =
                            mapping::streaming_map::StreamingTranslator::new(model);
                        let mut done = false;

                        let stream_future = read_sse_frames(response, &tx, &metrics, |json_str| {
                            if json_str == "[DONE]" {
                                done = true;
                                let events = translator.finish();
                                return Some(events);
                            }
                            match parse_openai_chat_completion_chunk(json_str) {
                                Ok(chunk) => Some(translator.process_chunk(&chunk)),
                                Err(e) => {
                                    tracing::debug!("failed to parse OpenAI streaming chunk: {e}");
                                    None
                                }
                            }
                        });
                        let outcome = if stream_timeout_secs > 0 {
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(stream_timeout_secs),
                                stream_future,
                            )
                            .await
                            {
                                Ok(o) => o,
                                Err(_) => {
                                    tracing::warn!(
                                        timeout_secs = stream_timeout_secs,
                                        "streaming response exceeded wall-clock timeout"
                                    );
                                    StreamOutcome::UpstreamError
                                }
                            }
                        } else {
                            stream_future.await
                        };

                        if matches!(outcome, StreamOutcome::Completed) && !done {
                            let events = translator.finish();
                            send_events(&tx, &events).await;
                        }
                        let usage = translator.usage();
                        let tokens = usage.map(|u| (u.input_tokens as u64, u.output_tokens as u64));
                        let cost = tokens.map(|(input_t, output_t)| {
                            super::routes::record_virtual_key_usage(
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
                    }
                    Err(e) => {
                        let backend_error = crate::backend::BackendError::from(e);
                        metrics.record_error();
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
                        // Send the error through the oneshot so the caller can
                        // return a proper HTTP error response instead of 200 OK.
                        let _ = rl_tx.send(Err(backend_error));
                        deployment_accounting.finish();
                    }
                }
            });
        }
        BackendClient::OpenAIResponses(client) => {
            let client = client.clone();
            let mut responses_req =
                mapping::responses_message_map::anthropic_to_responses_request(&body);
            responses_req.model = mapped_model.clone();
            responses_req.stream = Some(true);
            let model = body.model.clone();
            let permit = concurrency_permit;
            let mut deployment_accounting = deployment_accounting;

            tokio::spawn(async move {
                let _permit = permit;
                metrics.record_stream_started();
                match client.responses_stream(&responses_req).await {
                    Ok((response, rate_limits)) => {
                        rl_tx.send(Ok(rate_limits)).ok();
                        let mut translator =
                            mapping::responses_streaming_map::ResponsesStreamingTranslator::new(
                                model,
                            );

                        let stream_future = read_sse_frames(response, &tx, &metrics, |json_str| {
                            match serde_json::from_str::<
                                mapping::responses_streaming_map::ResponsesStreamEvent,
                            >(json_str)
                            {
                                Ok(event) => Some(translator.process_event(&event)),
                                Err(e) => {
                                    tracing::debug!(
                                        "failed to parse Responses API streaming event: {e}"
                                    );
                                    None
                                }
                            }
                        });
                        let outcome = if stream_timeout_secs > 0 {
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(stream_timeout_secs),
                                stream_future,
                            )
                            .await
                            {
                                Ok(o) => o,
                                Err(_) => {
                                    tracing::warn!(
                                        timeout_secs = stream_timeout_secs,
                                        "streaming response exceeded wall-clock timeout"
                                    );
                                    StreamOutcome::UpstreamError
                                }
                            }
                        } else {
                            stream_future.await
                        };

                        if matches!(outcome, StreamOutcome::Completed) {
                            let events = translator.finish();
                            send_events(&tx, &events).await;
                        }
                        let usage = translator.usage();
                        let tokens = usage.map(|u| (u.input_tokens as u64, u.output_tokens as u64));
                        let cost = tokens.map(|(input_t, output_t)| {
                            super::routes::record_virtual_key_usage(
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
                    }
                    Err(e) => {
                        let backend_error = crate::backend::BackendError::from(e);
                        metrics.record_error();
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
                        let _ = rl_tx.send(Err(backend_error));
                        deployment_accounting.finish();
                    }
                }
            });
        }
        BackendClient::Anthropic(_)
        | BackendClient::Bedrock(_)
        | BackendClient::GeminiNative(_) => {
            drop(rl_tx);
            let _ = tx
                .send(Ok(Event::default().data(
                    r#"{"error":"this backend does not use the translation streaming handler"}"#,
                )))
                .await;
        }
    }

    match rl_rx.await {
        Ok(Ok(rate_limits)) => Ok((
            rate_limits,
            Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()),
        )),
        Ok(Err(backend_err)) => Err(backend_err),
        // Sender dropped without sending (e.g., Anthropic passthrough branch or task panic).
        // Default to empty rate limits and let the stream deliver whatever it has.
        Err(_) => Ok((
            RateLimitHeaders::default(),
            Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()),
        )),
    }
}
