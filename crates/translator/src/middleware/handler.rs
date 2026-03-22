// Core handler logic for the Anthropic compatibility middleware.
// Shared by both the Router factory and the Tower Layer.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::anthropic::streaming::StreamEvent;
use crate::anthropic::{self, MessageCreateRequest};
use crate::mapping::{errors_map, streaming_map};
use crate::translate;

use super::client::ForwardingError;
use super::MiddlewareState;

/// Handle a POST /v1/messages request (streaming or non-streaming).
pub(crate) async fn handle_messages(
    state: Arc<MiddlewareState>,
    body: MessageCreateRequest,
) -> Response {
    if body.stream == Some(true) {
        return handle_streaming(state, body).await.into_response();
    }
    handle_non_streaming(state, body).await
}

async fn handle_non_streaming(state: Arc<MiddlewareState>, body: MessageCreateRequest) -> Response {
    let original_model = body.model.clone();

    let openai_req = match translate::translate_request(&body, &state.config.translation) {
        Ok(req) => req,
        Err(e) => return translation_error_response(&e.to_string()),
    };

    match state.client.chat_completion(&openai_req).await {
        Ok((openai_resp, _status)) => {
            let anthropic_resp = translate::translate_response(&openai_resp, &original_model);
            (StatusCode::OK, Json(anthropic_resp)).into_response()
        }
        Err(e) => forwarding_error_response(e),
    }
}

async fn handle_streaming(
    state: Arc<MiddlewareState>,
    body: MessageCreateRequest,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);

    let original_model = body.model.clone();

    let openai_req = match translate::translate_request(&body, &state.config.translation) {
        Ok(req) => req,
        Err(e) => {
            // Send error as SSE event, then close
            let _ = tx.send(Ok(error_to_sse_event(&e.to_string()))).await;
            return Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default());
        }
    };

    tokio::spawn(async move {
        match state.client.chat_completion_stream(&openai_req).await {
            Ok(response) => {
                let mut translator = streaming_map::StreamingTranslator::new(original_model);
                let mut done = false;

                let completed = read_sse_frames(response, &tx, |json_str| {
                    if json_str == "[DONE]" {
                        done = true;
                        return Some(translator.finish());
                    }
                    if let Ok(chunk) =
                        serde_json::from_str::<crate::openai::ChatCompletionChunk>(json_str)
                    {
                        return Some(translator.process_chunk(&chunk));
                    }
                    None
                })
                .await;

                if completed && !done {
                    let events = translator.finish();
                    send_events(&tx, &events).await;
                }
            }
            Err(e) => {
                let _ = tx.send(Ok(error_to_sse_event(&e.to_string()))).await;
            }
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

// --- SSE helpers ---

/// Format a StreamEvent as an axum SSE Event with the Anthropic event type name.
pub fn stream_event_to_sse(event: &StreamEvent) -> Result<Event, serde_json::Error> {
    let event_type = match event {
        StreamEvent::MessageStart { .. } => "message_start",
        StreamEvent::ContentBlockStart { .. } => "content_block_start",
        StreamEvent::ContentBlockDelta { .. } => "content_block_delta",
        StreamEvent::ContentBlockStop { .. } => "content_block_stop",
        StreamEvent::MessageDelta { .. } => "message_delta",
        StreamEvent::MessageStop { .. } => "message_stop",
        StreamEvent::Ping { .. } => "ping",
        StreamEvent::Error { .. } => "error",
    };
    let data = serde_json::to_string(event)?;
    Ok(Event::default().event(event_type).data(data))
}

fn error_to_sse_event(message: &str) -> Event {
    let event = StreamEvent::Error {
        error: crate::anthropic::streaming::StreamError {
            error_type: "api_error".to_string(),
            message: message.to_string(),
        },
    };
    // Best-effort; if serialization fails, send a plain text error
    stream_event_to_sse(&event).unwrap_or_else(|_| {
        Event::default().event("error").data(format!(
            r#"{{"type":"error","error":{{"type":"api_error","message":"{message}"}}}}"#
        ))
    })
}

/// Read SSE frames from a response, parse data lines, call `on_data` for each.
/// Returns true if stream completed normally.
async fn read_sse_frames<F>(
    response: reqwest::Response,
    tx: &mpsc::Sender<Result<Event, Infallible>>,
    mut on_data: F,
) -> bool
where
    F: FnMut(&str) -> Option<Vec<StreamEvent>>,
{
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut frame_events: Vec<StreamEvent> = Vec::new();

    while let Some(chunk_result) = stream.next().await {
        let bytes = match chunk_result {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("stream read error: {e}");
                return false;
            }
        };
        match std::str::from_utf8(&bytes) {
            Ok(s) => buffer.push_str(s),
            Err(_) => buffer.push_str(&String::from_utf8_lossy(&bytes)),
        }

        while let Some(pos) = buffer.find("\n\n") {
            frame_events.clear();
            for line in buffer[..pos].lines() {
                let line = line.trim();
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if let Some(mut events) = on_data(json_str) {
                        frame_events.append(&mut events);
                    }
                }
            }
            buffer.drain(..pos + 2);

            if !send_events(tx, &frame_events).await {
                return false;
            }
        }
    }

    true
}

/// Send translated events through the channel. Returns false if receiver is gone.
async fn send_events(tx: &mpsc::Sender<Result<Event, Infallible>>, events: &[StreamEvent]) -> bool {
    for event in events {
        if let Ok(sse_event) = stream_event_to_sse(event) {
            if tx.send(Ok(sse_event)).await.is_err() {
                return false;
            }
        }
    }
    true
}

// --- Error response helpers ---

fn translation_error_response(message: &str) -> Response {
    let err = errors_map::create_anthropic_error(
        anthropic::ErrorType::InvalidRequestError,
        message.to_string(),
        None,
    );
    (StatusCode::BAD_REQUEST, Json(err)).into_response()
}

fn forwarding_error_response(error: ForwardingError) -> Response {
    if let Some((body, status)) = error.api_error_details() {
        // Try to extract a message from the backend's JSON error body
        let message = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| body.to_string());

        let anthropic_err = errors_map::status_to_anthropic_error(status, &message, None);
        let http_status = StatusCode::from_u16(errors_map::anthropic_error_type_to_status(
            &anthropic_err.error.error_type,
        ))
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return (http_status, Json(anthropic_err)).into_response();
    }

    let err = errors_map::create_anthropic_error(
        anthropic::ErrorType::ApiError,
        format!("Upstream error: {error}"),
        None,
    );
    (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
}
