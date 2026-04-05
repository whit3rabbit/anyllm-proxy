//! SSE streaming translation: reads OpenAI chunks, yields Anthropic [`StreamEvent`]s.

use anyllm_translate::anthropic::streaming::StreamEvent;
use anyllm_translate::mapping;
use anyllm_translate::openai::ChatCompletionChunk;
use bytes::BytesMut;
use futures::{SinkExt, Stream, StreamExt};
use pin_project_lite::pin_project;

use crate::error::ClientError;
use crate::sse::{find_double_newline, SseError, MAX_SSE_BUFFER_SIZE};

pin_project! {
    /// A stream that reads SSE frames from a reqwest response, translates
    /// OpenAI chunks to Anthropic StreamEvents, and yields them.
    pub(crate) struct SseTranslatingStream {
        #[pin]
        inner: futures::channel::mpsc::Receiver<Result<StreamEvent, ClientError>>,
    }
}

impl SseTranslatingStream {
    pub(crate) fn new(response: reqwest::Response, model: String) -> Self {
        let (mut tx, rx) = futures::channel::mpsc::channel(32);

        // Spawn a task to read SSE frames and translate them.
        // Uses send().await instead of try_send() to respect backpressure:
        // try_send drops events silently when the channel is full.
        tokio::spawn(async move {
            let mut translator = mapping::streaming_map::StreamingTranslator::new(model);
            let mut done = false;

            let mut stream = response.bytes_stream();
            let mut buffer = BytesMut::new();
            let mut search_from = 0usize;

            while let Some(chunk_result) = stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(Err(ClientError::Sse(SseError::ReadError(e)))).await;
                        return;
                    }
                };
                buffer.extend_from_slice(&bytes);

                if buffer.len() > MAX_SSE_BUFFER_SIZE {
                    let _ = tx
                        .send(Err(ClientError::Sse(SseError::BufferOverflow)))
                        .await;
                    return;
                }

                while let Some((pos, delim_len)) = find_double_newline(&buffer, search_from) {
                    let events = match std::str::from_utf8(&buffer[..pos]) {
                        Ok(frame_str) => {
                            let mut events = Vec::new();
                            for line in frame_str.lines() {
                                let line = line.trim();
                                if let Some(json_str) = line.strip_prefix("data: ") {
                                    if json_str == "[DONE]" {
                                        done = true;
                                        events.extend(translator.finish());
                                    } else {
                                        match serde_json::from_str::<ChatCompletionChunk>(json_str)
                                        {
                                            Ok(chunk) => {
                                                events.extend(translator.process_chunk(&chunk))
                                            }
                                            Err(e) => {
                                                tracing::debug!(
                                                    "failed to parse streaming chunk: {e}"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            events
                        }
                        Err(e) => {
                            tracing::warn!("skipping non-UTF-8 SSE frame: {e}");
                            vec![]
                        }
                    };
                    let _ = buffer.split_to(pos + delim_len);
                    search_from = 0;

                    for event in events {
                        if tx.send(Ok(event)).await.is_err() {
                            return; // receiver dropped
                        }
                    }
                }
                search_from = buffer.len().saturating_sub(3);
            }

            if !done {
                for event in translator.finish() {
                    if tx.send(Ok(event)).await.is_err() {
                        break;
                    }
                }
            }
        });

        Self { inner: rx }
    }
}

impl Stream for SseTranslatingStream {
    type Item = Result<StreamEvent, ClientError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
    }
}
