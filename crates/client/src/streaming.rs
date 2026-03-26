//! SSE streaming translation: reads OpenAI chunks, yields Anthropic [`StreamEvent`]s.

use anyllm_translate::anthropic::streaming::StreamEvent;
use anyllm_translate::mapping;
use anyllm_translate::openai::ChatCompletionChunk;
use futures::Stream;
use pin_project_lite::pin_project;

use crate::error::ClientError;

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
        tokio::spawn(async move {
            let mut translator = mapping::streaming_map::StreamingTranslator::new(model);
            let mut done = false;

            let result = crate::sse::read_sse_stream(
                response,
                |json_str| {
                    if json_str == "[DONE]" {
                        done = true;
                        return Some(translator.finish());
                    }
                    match serde_json::from_str::<ChatCompletionChunk>(json_str) {
                        Ok(chunk) => Some(translator.process_chunk(&chunk)),
                        Err(e) => {
                            tracing::debug!("failed to parse streaming chunk: {e}");
                            None
                        }
                    }
                },
                |events| {
                    for event in events {
                        // Block on send; if receiver is dropped, stop.
                        if tx.try_send(Ok(event.clone())).is_err() {
                            return false;
                        }
                    }
                    true
                },
            )
            .await;

            if let Err(e) = result {
                let _ = tx.try_send(Err(ClientError::Sse(e)));
            } else if !done {
                // Stream ended without [DONE]; flush remaining events.
                let events = translator.finish();
                for event in events {
                    if tx.try_send(Ok(event)).is_err() {
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
