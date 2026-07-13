use crate::backend::{find_double_newline, SseFrameBuffer};
use crate::metrics::Metrics;
use anyllm_translate::anthropic;
use axum::response::sse::Event;
use bytes::BytesMut;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

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

pub(crate) async fn send_events(
    tx: &mpsc::Sender<Result<Event, std::convert::Infallible>>,
    events: &[anthropic::StreamEvent],
) -> bool {
    for ev in events {
        match crate::server::sse::stream_event_to_sse(ev) {
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

#[derive(Debug, Clone, Copy)]
pub(crate) enum StreamOutcome {
    Completed,
    ClientDisconnected,
    UpstreamError,
}

impl StreamOutcome {
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

pub(crate) async fn read_sse_frames<F>(
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
    let mut buffer = SseFrameBuffer::new();
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
