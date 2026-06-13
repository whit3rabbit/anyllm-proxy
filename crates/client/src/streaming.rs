//! SSE streaming: translation stream (OpenAI → Anthropic) and passthrough stream
//! (raw Anthropic SSE frames → [`StreamEvent`]).

use anyllm_translate::anthropic::streaming::StreamEvent;
use anyllm_translate::mapping;
use anyllm_translate::openai::ChatCompletionChunk;
use bytes::BytesMut;
use futures::{SinkExt, Stream, StreamExt};
use pin_project_lite::pin_project;

use crate::error::ClientError;
use crate::sse::{find_double_newline, SseError, MAX_SSE_BUFFER_SIZE};

/// Argument to the [`run_sse_task`] handler. Either a parsed UTF-8 SSE frame
/// or the end-of-stream signal (bytes exhausted without a transport error).
enum SseEvent<'a> {
    Frame(&'a str),
    End,
}

/// Core SSE loop shared by both stream types.
///
/// Reads bytes from `response`, finds double-newline-delimited SSE frames,
/// decodes UTF-8, and calls `handler` once per frame and once at stream end.
/// The handler returns a Vec of events which are forwarded to `tx` in order;
/// returning early when `tx` is closed (receiver dropped).
async fn run_sse_task(
    response: reqwest::Response,
    mut tx: futures::channel::mpsc::Sender<Result<StreamEvent, ClientError>>,
    mut handler: impl FnMut(SseEvent<'_>) -> Vec<Result<StreamEvent, ClientError>>,
) {
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
                Ok(frame_str) => handler(SseEvent::Frame(frame_str)),
                Err(e) => {
                    tracing::warn!("skipping non-UTF-8 SSE frame: {e}");
                    vec![]
                }
            };
            let _ = buffer.split_to(pos + delim_len);
            search_from = 0;

            for event in events {
                if tx.send(event).await.is_err() {
                    return; // receiver dropped
                }
            }
        }
        search_from = buffer.len().saturating_sub(3);
    }

    for event in handler(SseEvent::End) {
        if tx.send(event).await.is_err() {
            break;
        }
    }
}

pin_project! {
    /// A stream that reads SSE frames from a reqwest response, translates
    /// OpenAI chunks to Anthropic StreamEvents, and yields them.
    pub(crate) struct SseTranslatingStream {
        #[pin]
        inner: futures::channel::mpsc::Receiver<Result<StreamEvent, ClientError>>,
    }
}

impl SseTranslatingStream {
    /// Spawn a background task that reads SSE frames from `response` and translates
    /// OpenAI chunks to Anthropic `StreamEvent`s via a bounded channel (capacity 32).
    pub(crate) fn new(response: reqwest::Response, model: String) -> Self {
        let (tx, rx) = futures::channel::mpsc::channel(32);

        // Spawn a task to read SSE frames and translate them.
        // Uses send().await instead of try_send() to respect backpressure:
        // try_send drops events silently when the channel is full.
        tokio::spawn(async move {
            let mut translator = mapping::streaming_map::StreamingTranslator::new(model);
            let mut done = false;

            run_sse_task(response, tx, |ev| match ev {
                SseEvent::Frame(frame_str) => {
                    let mut events = Vec::new();
                    for line in frame_str.lines() {
                        let line = line.trim();
                        if let Some(json_str) = line.strip_prefix("data: ") {
                            if json_str == "[DONE]" {
                                done = true;
                                events.extend(translator.finish().into_iter().map(Ok));
                            } else {
                                match serde_json::from_str::<ChatCompletionChunk>(json_str) {
                                    Ok(chunk) => {
                                        events.extend(
                                            translator.process_chunk(&chunk).into_iter().map(Ok),
                                        )
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
                SseEvent::End => {
                    if !done {
                        translator.finish().into_iter().map(Ok).collect()
                    } else {
                        vec![]
                    }
                }
            })
            .await;
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

pin_project! {
    /// A stream that reads raw Anthropic SSE frames from a reqwest response and
    /// deserializes each `data:` line as a [`StreamEvent`].
    ///
    /// No translation: the events are Anthropic-native (message_start,
    /// content_block_delta, message_stop, etc.). Unknown `type` values land in
    /// [`StreamEvent::Unknown`] so the stream never errors on new event types.
    pub(crate) struct SsePassthroughStream {
        #[pin]
        inner: futures::channel::mpsc::Receiver<Result<StreamEvent, ClientError>>,
    }
}

impl SsePassthroughStream {
    /// Spawn a background task that reads SSE frames from `response` and parses
    /// each `data:` line as a [`StreamEvent`] via a bounded channel (capacity 32).
    pub(crate) fn new(response: reqwest::Response) -> Self {
        let (tx, rx) = futures::channel::mpsc::channel(32);

        tokio::spawn(async move {
            run_sse_task(response, tx, |ev| match ev {
                SseEvent::Frame(frame_str) => {
                    let mut events = Vec::new();
                    for line in frame_str.lines() {
                        let line = line.trim();
                        if let Some(json_str) = line.strip_prefix("data: ") {
                            match serde_json::from_str::<StreamEvent>(json_str) {
                                Ok(ev) => events.push(Ok(ev)),
                                Err(e) => {
                                    // Log and skip unparseable events rather than
                                    // terminating the stream. Unknown event types
                                    // land in StreamEvent::Unknown by design.
                                    tracing::debug!(
                                        "failed to parse Anthropic SSE event: {e}"
                                    );
                                }
                            }
                        }
                    }
                    events
                }
                SseEvent::End => vec![],
            })
            .await;
        });

        Self { inner: rx }
    }
}

impl Stream for SsePassthroughStream {
    type Item = Result<StreamEvent, ClientError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
    }
}
