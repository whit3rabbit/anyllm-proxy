// Bedrock passthrough handler: forwards Anthropic-format requests to AWS Bedrock
// with SigV4 signing and AWS Event Stream decoding for streaming.

use crate::backend::bedrock_client::{eventstream, BedrockClientError};
use crate::backend::BackendClient;
use anyllm_translate::{anthropic, mapping};
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use bytes::BytesMut;
use futures::StreamExt;

use super::routes::AppState;

/// Bedrock passthrough handler for POST /v1/messages.
/// Strips the `model` field from the body (Bedrock uses it in the URL),
/// adds `anthropic_version`, and handles AWS Event Stream binary framing
/// for streaming responses.
pub(crate) async fn bedrock_passthrough(State(state): State<AppState>, body: Bytes) -> Response {
    state.metrics.record_request();

    let client = match &state.backend {
        BackendClient::Bedrock(c) => c.clone(),
        _ => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "Backend is not configured as bedrock".to_string(),
                None,
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response();
        }
    };

    // Parse the body to extract model and stream fields, then rebuild for Bedrock.
    let mut parsed: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::InvalidRequestError,
                format!("invalid JSON: {e}"),
                None,
            );
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }
    };

    // Extract model for the URL
    let model_id = parsed
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if model_id.is_empty() {
        let err = mapping::errors_map::create_anthropic_error(
            anthropic::ErrorType::InvalidRequestError,
            "model is required".to_string(),
            None,
        );
        return (StatusCode::BAD_REQUEST, Json(err)).into_response();
    }

    // Map model name through model router or runtime config
    let mapped_model = match state.resolve_model(&model_id) {
        super::routes::ResolvedModel::Routed { model, .. } => model,
        super::routes::ResolvedModel::AllAtLimit => {
            let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                anyllm_translate::anthropic::ErrorType::RateLimitError,
                "all deployments for this model are at their RPM limit".to_string(),
                None,
            );
            return (StatusCode::TOO_MANY_REQUESTS, Json(err)).into_response();
        }
        super::routes::ResolvedModel::Legacy(m) => m,
    };

    let is_stream = parsed
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Bedrock: model goes in URL, not body. Add anthropic_version.
    if let Some(obj) = parsed.as_object_mut() {
        obj.remove("model");
        obj.insert(
            "anthropic_version".to_string(),
            serde_json::Value::String("bedrock-2023-05-31".to_string()),
        );
    }

    let bedrock_body = match serde_json::to_vec(&parsed) {
        Ok(b) => bytes::Bytes::from(b),
        Err(e) => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                format!("failed to serialize request: {e}"),
                None,
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response();
        }
    };

    if is_stream {
        bedrock_stream(state, &client, bedrock_body, &mapped_model).await
    } else {
        bedrock_non_stream(state, &client, bedrock_body, &mapped_model).await
    }
}

/// Non-streaming Bedrock request.
async fn bedrock_non_stream(
    state: AppState,
    client: &crate::backend::bedrock_client::BedrockClient,
    body: bytes::Bytes,
    model_id: &str,
) -> Response {
    match client.forward(body, model_id).await {
        Ok((resp_body, rate_limits)) => {
            state.metrics.record_success();
            let mut resp = (
                StatusCode::OK,
                [("content-type", "application/json")],
                resp_body,
            )
                .into_response();
            rate_limits.inject_anthropic_response_headers(resp.headers_mut());
            resp
        }
        Err(e) => {
            state.metrics.record_error();
            bedrock_error_to_response(e)
        }
    }
}

/// Streaming Bedrock request. Decodes AWS Event Stream binary frames into
/// Anthropic SSE events and re-emits them as standard SSE.
async fn bedrock_stream(
    state: AppState,
    client: &crate::backend::bedrock_client::BedrockClient,
    body: bytes::Bytes,
    model_id: &str,
) -> Response {
    let (response, rate_limits) = match client.forward_stream(body, model_id).await {
        Ok(r) => r,
        Err(e) => {
            state.metrics.record_error();
            return bedrock_error_to_response(e);
        }
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::convert::Infallible>>(32);
    let metrics = state.metrics.clone();

    tokio::spawn(async move {
        let mut byte_stream = response.bytes_stream();
        let mut event_buf = BytesMut::new();

        while let Some(chunk_result) = byte_stream.next().await {
            let bytes = match chunk_result {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!("Bedrock stream read error: {e}");
                    metrics.record_error();
                    return;
                }
            };
            event_buf.extend_from_slice(&bytes);

            if event_buf.len() > crate::backend::MAX_SSE_BUFFER_SIZE {
                tracing::error!(
                    buffer_len = event_buf.len(),
                    "Bedrock event stream buffer exceeded maximum size, aborting"
                );
                metrics.record_error();
                return;
            }

            // Decode all complete frames in the buffer
            loop {
                match eventstream::decode_frame(&mut event_buf) {
                    Err(e) => {
                        tracing::warn!(error = %e, "Bedrock event stream CRC mismatch, dropping frame");
                        // Buffer was already advanced past the bad frame; continue.
                    }
                    Ok(None) => break, // no complete frame yet
                    Ok(Some(payload)) => {
                        if let Some(event_json) = eventstream::extract_event_from_payload(&payload) {
                            // Re-emit as SSE: "event: <type>\ndata: <json>\n\n"
                            // Bedrock events are raw Anthropic JSON; detect the event type.
                            let event_type = detect_event_type(&event_json);
                            let sse_line = format!("event: {event_type}\ndata: {event_json}\n\n");
                            if tx.send(Ok(sse_line)).await.is_err() {
                                return; // client disconnected
                            }
                        }
                    }
                }
            }
        }
        metrics.record_success();
    });

    let body_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let body = axum::body::Body::from_stream(body_stream);
    let mut resp = axum::http::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    rate_limits.inject_anthropic_response_headers(resp.headers_mut());
    resp
}

/// Extract the Anthropic SSE event type from a JSON string.
/// Parses only the top-level `type` field via serde_json to avoid brittle
/// substring matching that fails on whitespace-formatted JSON or nested fields.
/// Falls back to "message" on any parse failure or unrecognized event type.
fn detect_event_type(json: &str) -> &'static str {
    #[derive(serde::Deserialize)]
    struct EventType<'a> {
        #[serde(rename = "type")]
        event_type: &'a str,
    }
    let parsed: Result<EventType<'_>, _> = serde_json::from_str(json);
    match parsed.as_ref().map(|e| e.event_type) {
        Ok("message_start") => "message_start",
        Ok("content_block_start") => "content_block_start",
        Ok("content_block_delta") => "content_block_delta",
        Ok("content_block_stop") => "content_block_stop",
        Ok("message_delta") => "message_delta",
        Ok("message_stop") => "message_stop",
        Ok("ping") => "ping",
        _ => "message",
    }
}

/// Convert a BedrockClientError into a Response.
fn bedrock_error_to_response(error: BedrockClientError) -> Response {
    match error {
        BedrockClientError::ApiError { status, body } => {
            let http_status =
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            // Try to return body as-is (Bedrock may return JSON error)
            (http_status, [("content-type", "application/json")], body).into_response()
        }
        BedrockClientError::Transport(msg) => {
            tracing::error!("Bedrock transport error: {msg}");
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "An internal error occurred while communicating with the upstream service."
                    .to_string(),
                None,
            );
            (StatusCode::BAD_GATEWAY, Json(err)).into_response()
        }
        BedrockClientError::Signing(msg) => {
            tracing::error!("Bedrock signing error: {msg}");
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "Failed to sign request for AWS Bedrock.".to_string(),
                None,
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::detect_event_type;

    #[test]
    fn detect_message_start() {
        assert_eq!(detect_event_type(r#"{"type":"message_start","message":{"id":"msg-1"}}"#), "message_start");
    }

    #[test]
    fn detect_content_block_delta() {
        assert_eq!(
            detect_event_type(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#),
            "content_block_delta"
        );
    }

    #[test]
    fn detect_falls_back_for_unknown_type() {
        assert_eq!(detect_event_type(r#"{"type":"some_future_event"}"#), "message");
    }

    #[test]
    fn detect_falls_back_on_malformed_json() {
        assert_eq!(detect_event_type("not json at all"), "message");
    }

    #[test]
    fn detect_handles_spaced_json() {
        assert_eq!(detect_event_type(r#"{ "type" : "message_stop" }"#), "message_stop");
    }

    #[test]
    fn detect_ignores_nested_type_field() {
        // Top-level type is content_block_delta; nested delta.type is text_delta.
        let json = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}"#;
        assert_eq!(detect_event_type(json), "content_block_delta");
    }
}
