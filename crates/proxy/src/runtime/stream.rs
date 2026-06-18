use bytes::BytesMut;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::{ChatCompletionChunkStream, ChatCompletionError};
use crate::backend::MAX_SSE_BUFFER_SIZE;
use anyllm_translate::{anthropic, mapping, openai, ReverseStreamingTranslator};

pub(crate) struct DeploymentLatencyGuard {
    deployment: Option<Arc<crate::config::model_router::Deployment>>,
    start: Option<Instant>,
}

impl DeploymentLatencyGuard {
    pub(crate) fn from_started(
        deployment: Option<Arc<crate::config::model_router::Deployment>>,
        start: Option<Instant>,
    ) -> Self {
        Self { deployment, start }
    }

    pub(crate) fn deployment(&self) -> Option<&Arc<crate::config::model_router::Deployment>> {
        self.deployment.as_ref()
    }

    pub(crate) fn finish(&mut self) {
        if let (Some(deployment), Some(start)) = (&self.deployment, self.start.take()) {
            deployment.record_finish(start.elapsed().as_millis() as u64);
        }
    }
}

impl Drop for DeploymentLatencyGuard {
    fn drop(&mut self) {
        self.finish();
    }
}

pub(crate) fn openai_chunk_stream(
    response: reqwest::Response,
    stream_timeout_secs: u64,
    mut deployment_latency: DeploymentLatencyGuard,
) -> ChatCompletionChunkStream {
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        let fut = async {
            let mut byte_stream = response.bytes_stream();
            let mut buffer = BytesMut::new();
            let mut search_from: usize = 0;

            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = match chunk_result {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        send_stream_error(&tx, ChatCompletionError::StreamRead(e.to_string()))
                            .await;
                        return;
                    }
                };

                if buffer.len() + bytes.len() > MAX_SSE_BUFFER_SIZE {
                    send_stream_error(&tx, ChatCompletionError::StreamBufferOverflow).await;
                    return;
                }
                buffer.extend_from_slice(&bytes);

                while let Some((pos, delim_len)) =
                    anyllm_client::find_double_newline(&buffer, search_from)
                {
                    let frame = match std::str::from_utf8(&buffer[..pos]) {
                        Ok(frame) => frame,
                        Err(e) => {
                            send_stream_error(&tx, ChatCompletionError::StreamParse(e.to_string()))
                                .await;
                            return;
                        }
                    };

                    for line in frame.lines() {
                        let line = line.trim();
                        let Some(json_str) = line.strip_prefix("data: ") else {
                            continue;
                        };
                        if json_str == "[DONE]" {
                            continue;
                        }
                        let parsed = serde_json::from_str::<openai::ChatCompletionChunk>(json_str);
                        match parsed {
                            Ok(chunk) => {
                                if let (Some(deployment), Some(ref usage)) =
                                    (deployment_latency.deployment(), &chunk.usage)
                                {
                                    deployment.record_tokens(usage.total_tokens as u64);
                                }
                                if tx.send(Ok(chunk)).await.is_err() {
                                    return;
                                }
                            }
                            Err(e) => {
                                send_stream_error(
                                    &tx,
                                    ChatCompletionError::StreamParse(e.to_string()),
                                )
                                .await;
                                return;
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
            if tokio::time::timeout(std::time::Duration::from_secs(stream_timeout_secs), fut)
                .await
                .is_err()
            {
                send_stream_error(&tx, ChatCompletionError::StreamTimeout).await;
            }
        } else {
            fut.await;
        }
        deployment_latency.finish();
    });

    Box::pin(ReceiverStream::new(rx))
}

pub(crate) fn responses_chunk_stream(
    response: reqwest::Response,
    model: String,
    stream_timeout_secs: u64,
    mut deployment_latency: DeploymentLatencyGuard,
) -> ChatCompletionChunkStream {
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        let fut = async {
            let mut responses_translator =
                mapping::responses_streaming_map::ResponsesStreamingTranslator::new(model.clone());
            let mut reverse_translator = ReverseStreamingTranslator::new(
                format!("chatcmpl-{}", uuid::Uuid::new_v4().as_simple()),
                model,
            );
            let mut byte_stream = response.bytes_stream();
            let mut buffer = BytesMut::new();
            let mut search_from: usize = 0;

            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = match chunk_result {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        send_stream_error(&tx, ChatCompletionError::StreamRead(e.to_string()))
                            .await;
                        return;
                    }
                };

                if buffer.len() + bytes.len() > MAX_SSE_BUFFER_SIZE {
                    send_stream_error(&tx, ChatCompletionError::StreamBufferOverflow).await;
                    return;
                }
                buffer.extend_from_slice(&bytes);

                while let Some((pos, delim_len)) =
                    anyllm_client::find_double_newline(&buffer, search_from)
                {
                    let frame = match std::str::from_utf8(&buffer[..pos]) {
                        Ok(frame) => frame,
                        Err(e) => {
                            send_stream_error(&tx, ChatCompletionError::StreamParse(e.to_string()))
                                .await;
                            return;
                        }
                    };

                    for line in frame.lines() {
                        let line = line.trim();
                        let Some(json_str) = line.strip_prefix("data: ") else {
                            continue;
                        };
                        let parsed = serde_json::from_str::<
                            mapping::responses_streaming_map::ResponsesStreamEvent,
                        >(json_str);
                        match parsed {
                            Ok(event) => {
                                let anthropic_events = responses_translator.process_event(&event);
                                if !send_translated_chunks(
                                    &tx,
                                    &mut reverse_translator,
                                    &anthropic_events,
                                    deployment_latency.deployment(),
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            Err(e) => {
                                send_stream_error(
                                    &tx,
                                    ChatCompletionError::StreamParse(e.to_string()),
                                )
                                .await;
                                return;
                            }
                        }
                    }

                    let _ = buffer.split_to(pos + delim_len);
                    search_from = 0;
                }
                search_from = buffer.len().saturating_sub(3);
            }

            let final_events = responses_translator.finish();
            let _ = send_translated_chunks(
                &tx,
                &mut reverse_translator,
                &final_events,
                deployment_latency.deployment(),
            )
            .await;
        };

        if stream_timeout_secs > 0 {
            if tokio::time::timeout(std::time::Duration::from_secs(stream_timeout_secs), fut)
                .await
                .is_err()
            {
                send_stream_error(&tx, ChatCompletionError::StreamTimeout).await;
            }
        } else {
            fut.await;
        }
        deployment_latency.finish();
    });

    Box::pin(ReceiverStream::new(rx))
}

async fn send_translated_chunks(
    tx: &mpsc::Sender<Result<openai::ChatCompletionChunk, ChatCompletionError>>,
    reverse_translator: &mut ReverseStreamingTranslator,
    anthropic_events: &[anthropic::StreamEvent],
    deployment: Option<&Arc<crate::config::model_router::Deployment>>,
) -> bool {
    for event in anthropic_events {
        let chunks = reverse_translator.process_event(event);
        for chunk in chunks {
            if let (Some(deployment), Some(usage)) = (deployment, &chunk.usage) {
                deployment.record_tokens(usage.total_tokens as u64);
            }
            if tx.send(Ok(chunk)).await.is_err() {
                return false;
            }
        }
    }
    true
}

async fn send_stream_error(
    tx: &mpsc::Sender<Result<openai::ChatCompletionChunk, ChatCompletionError>>,
    error: ChatCompletionError,
) {
    let _ = tx.send(Err(error)).await;
}
