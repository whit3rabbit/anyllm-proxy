// SSE streaming infrastructure and the messages_stream handler.

use crate::backend::{find_double_newline, BackendClient, RateLimitHeaders, MAX_SSE_BUFFER_SIZE};
use crate::metrics::Metrics;
use anyllm_translate::{anthropic, mapping, openai};
use axum::response::sse::{Event, KeepAlive, Sse};
use bytes::BytesMut;
use futures::stream::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::routes::{log_request, set_backend_error_kind, RequestCtx};
use super::state::AppState;

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
pub(super) enum StreamOutcome {
    /// Backend stream completed normally.
    Completed,
    /// Downstream client disconnected before the stream finished.
    ClientDisconnected,
    /// Backend stream failed (error already recorded in metrics).
    UpstreamError,
}

impl StreamOutcome {
    /// Record metrics and return (HTTP status, error message) for logging.
    fn record(&self, metrics: &Metrics) -> (u16, Option<String>) {
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
    // BytesMut (not String) because TCP chunks may split mid-UTF-8 character.
    // String::from_utf8_lossy would permanently replace partial trailing bytes
    // with U+FFFD, corrupting the JSON payload.
    let mut buffer = BytesMut::new();
    // Reuse a single events buffer across all frames to avoid per-frame allocation
    let mut frame_events: Vec<anthropic::StreamEvent> = Vec::new();
    // Track where to start the next delimiter search so we don't rescan
    // already-inspected bytes when a large SSE event spans many TCP chunks.
    let mut search_from: usize = 0;

    while let Some(chunk_result) = stream.next().await {
        let bytes = match chunk_result {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("stream read error: {e}");
                metrics.record_error();
                return StreamOutcome::UpstreamError;
            }
        };
        buffer.extend_from_slice(&bytes);

        // Guard against unbounded buffer growth from a misbehaving backend.
        if buffer.len() > MAX_SSE_BUFFER_SIZE {
            tracing::error!(
                buffer_len = buffer.len(),
                "SSE buffer exceeded maximum size, aborting stream"
            );
            metrics.record_error();
            return StreamOutcome::UpstreamError;
        }

        while let Some((pos, delim_len)) = find_double_newline(&buffer, search_from) {
            frame_events.clear();
            // Convert the complete frame bytes to UTF-8. A frame ending at
            // a double-newline boundary should always be valid UTF-8; if not,
            // skip the malformed frame rather than injecting replacement chars.
            match std::str::from_utf8(&buffer[..pos]) {
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
            let _ = buffer.split_to(pos + delim_len);
            // split_to shifted the buffer; restart search at the beginning
            search_from = 0;

            if !send_events(tx, &frame_events).await {
                tracing::debug!("client disconnected during stream");
                return StreamOutcome::ClientDisconnected;
            }
        }
        // Next chunk: resume scanning 3 bytes back from the end. The 4-byte
        // delimiter \r\n\r\n could straddle the chunk boundary (e.g., \r\n at
        // end of this chunk, \r\n at start of the next).
        search_from = buffer.len().saturating_sub(3);
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
            // Strip Gemini-incompatible JSON Schema keywords from tool parameters.
            if matches!(
                state.backend,
                crate::backend::BackendClient::GeminiOpenAI(_)
                    | crate::backend::BackendClient::Vertex(_)
            ) {
                if let Some(tools) = openai_req.tools.take() {
                    openai_req.tools = Some(
                        tools
                            .into_iter()
                            .map(|mut t| {
                                if let Some(params) = t.function.parameters.take() {
                                    t.function.parameters = Some(
                                        anyllm_translate::mapping::tools_map::sanitize_schema_for_gemini(params),
                                    );
                                }
                                t
                            })
                            .collect(),
                    );
                }
            }
            if state.omit_stream_options {
                openai_req.stream_options = None;
            }
            openai_req.model = mapped_model.clone();
            let model = body.model.clone();
            let permit = concurrency_permit.clone();

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
                            match serde_json::from_str::<openai::ChatCompletionChunk>(json_str) {
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
                        // Record cost for virtual key spend tracking.
                        let cost = if let Some((input_t, output_t)) = tokens {
                            Some(crate::cost::record_cost(
                                &log_shared,
                                &vk_ctx,
                                &mapped_model,
                                input_t,
                                output_t,
                            ))
                        } else {
                            None
                        };
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
                        let cost = if let Some((input_t, output_t)) = tokens {
                            Some(crate::cost::record_cost(
                                &log_shared,
                                &vk_ctx,
                                &mapped_model,
                                input_t,
                                output_t,
                            ))
                        } else {
                            None
                        };
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
