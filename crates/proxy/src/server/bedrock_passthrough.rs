// Bedrock passthrough handler: forwards Anthropic-format requests to AWS Bedrock
// with SigV4 signing and AWS Event Stream decoding for streaming.

use crate::backend::bedrock_client::{eventstream, BedrockClientError};
use crate::backend::BackendClient;
use crate::openai_tool_policy::{
    backend_kind_for_policy, validate_anthropic_tool_request, OpenAiToolPolicyContext,
};
use crate::server::routes::{log_request, record_virtual_key_usage, RequestCtx};
use crate::server::state::ConcurrencyPermit;
use crate::server::streaming::{AnthropicStreamUsage, StreamOutcome};
use anyllm_translate::{anthropic, mapping};
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use bytes::BytesMut;
use futures::StreamExt;

use super::state::AppState;

/// Bedrock passthrough handler for POST /v1/messages.
/// Strips the `model` field from the body (Bedrock uses it in the URL),
/// adds `anthropic_version`, and handles AWS Event Stream binary framing
/// for streaming responses.
pub(crate) async fn bedrock_passthrough(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    permit: Option<axum::Extension<ConcurrencyPermit>>,
    vk_ctx: Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>,
    body: Bytes,
) -> Response {
    let permit = permit.map(|axum::Extension(p)| p);
    let vk_ctx = vk_ctx.map(|axum::Extension(c)| c);
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

    // Enforce model allowlist policy for virtual keys.
    if let Some(ref ctx) = vk_ctx {
        if !crate::server::policy::is_model_allowed(&model_id, &ctx.allowed_models) {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::PermissionError,
                format!("Model '{}' is not allowed for this API key.", model_id),
                None,
            );
            return (StatusCode::FORBIDDEN, Json(err)).into_response();
        }
    }

    // Map model name through model router or runtime config
    let mapped_model = match state.resolve_model(&model_id) {
        super::state::ResolvedModel::Routed { model, .. } => model,
        super::state::ResolvedModel::AllAtLimit => {
            let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                anyllm_translate::anthropic::ErrorType::RateLimitError,
                "all deployments for this model are at their RPM limit".to_string(),
                None,
            );
            return (StatusCode::TOO_MANY_REQUESTS, Json(err)).into_response();
        }
        super::state::ResolvedModel::UnknownModel => {
            let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                anyllm_translate::anthropic::ErrorType::InvalidRequestError,
                format!("model '{}' is not configured in model_list", model_id),
                None,
            );
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }
        super::state::ResolvedModel::Legacy(m) => m,
    };

    let is_stream = parsed
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let ctx = super::routes::RequestCtx {
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string(),
        start: std::time::Instant::now(),
        model_requested: model_id.clone(),
    };

    if let Ok(parsed_req) =
        serde_json::from_value::<anthropic::MessageCreateRequest>(parsed.clone())
    {
        if let Err(err) = validate_anthropic_tool_request(
            &parsed_req,
            OpenAiToolPolicyContext {
                backend_kind: backend_kind_for_policy(&state.backend),
                provider_id: state.provider_id.as_deref(),
                model: &mapped_model,
                provider_catalog: &state.provider_catalog,
            },
        ) {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::InvalidRequestError,
                err.to_string(),
                None,
            );
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }
    }

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
    let bedrock_body = match super::secret_redaction::redact_body_with_content_type(
        state.redact_secrets(),
        Some("application/json"),
        bedrock_body,
    )
    .await
    {
        Ok(body) => body,
        Err(err) => return super::secret_redaction::error_response(err),
    };

    if is_stream {
        bedrock_stream(
            state,
            &client,
            bedrock_body,
            &mapped_model,
            ctx,
            permit,
            vk_ctx,
        )
        .await
    } else {
        bedrock_non_stream(state, &client, bedrock_body, &mapped_model, ctx, vk_ctx).await
    }
}

/// Non-streaming Bedrock request.
async fn bedrock_non_stream(
    state: AppState,
    client: &crate::backend::bedrock_client::BedrockClient,
    body: bytes::Bytes,
    model_id: &str,
    ctx: RequestCtx,
    vk_ctx: Option<crate::server::middleware::VirtualKeyContext>,
) -> Response {
    match client.forward(body, model_id).await {
        Ok((resp_body, rate_limits)) => {
            if vk_ctx.is_some() {
                let parsed = serde_json::from_slice::<anthropic::MessageResponse>(&resp_body);
                let anthropic_resp = match parsed {
                    Ok(resp) => resp,
                    Err(e) => {
                        state.metrics.record_error();
                        log_request(
                            &state.shared,
                            ctx.log_entry_with_attribution(
                                &state.backend_name,
                                Some(model_id.to_string()),
                                StatusCode::BAD_GATEWAY.as_u16(),
                                None,
                                false,
                                Some(format!(
                                    "failed to parse upstream usage for virtual key accounting: {e}"
                                )),
                                &vk_ctx,
                                None,
                            ),
                        );
                        return virtual_key_accounting_parse_error();
                    }
                };
                state.metrics.record_success();
                let tokens = (
                    anthropic_resp.usage.input_tokens as u64,
                    anthropic_resp.usage.output_tokens as u64,
                );
                let cost =
                    record_virtual_key_usage(&state.shared, &vk_ctx, model_id, tokens.0, tokens.1);
                log_request(
                    &state.shared,
                    ctx.log_entry_with_attribution(
                        &state.backend_name,
                        Some(model_id.to_string()),
                        200,
                        Some(tokens),
                        false,
                        None,
                        &vk_ctx,
                        Some(cost),
                    ),
                );
            } else {
                state.metrics.record_success();
            }
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
            log_request(
                &state.shared,
                ctx.log_entry_with_attribution(
                    &state.backend_name,
                    Some(model_id.to_string()),
                    bedrock_error_status(&e),
                    None,
                    false,
                    Some(e.to_string()),
                    &vk_ctx,
                    None,
                ),
            );
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
    ctx: RequestCtx,
    concurrency_permit: Option<ConcurrencyPermit>,
    vk_ctx: Option<crate::server::middleware::VirtualKeyContext>,
) -> Response {
    let (response, rate_limits) = match client.forward_stream(body, model_id).await {
        Ok(r) => r,
        Err(e) => {
            state.metrics.record_error();
            log_request(
                &state.shared,
                ctx.log_entry_with_attribution(
                    &state.backend_name,
                    Some(model_id.to_string()),
                    bedrock_error_status(&e),
                    None,
                    true,
                    Some(e.to_string()),
                    &vk_ctx,
                    None,
                ),
            );
            return bedrock_error_to_response(e);
        }
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::convert::Infallible>>(32);
    let metrics = state.metrics.clone();
    let log_shared = state.shared.clone();
    let log_backend_name = state.backend_name.clone();
    let cost_model = model_id.to_string();

    tokio::spawn(async move {
        let _permit = concurrency_permit;
        metrics.record_stream_started();
        let mut byte_stream = response.bytes_stream();
        let mut event_buf = BytesMut::new();
        let mut usage = AnthropicStreamUsage::default();
        let mut outcome = StreamOutcome::Completed;

        while let Some(chunk_result) = byte_stream.next().await {
            let bytes = match chunk_result {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!("Bedrock stream read error: {e}");
                    metrics.record_error();
                    outcome = StreamOutcome::UpstreamError;
                    break;
                }
            };

            if event_buf.len() + bytes.len() > crate::backend::MAX_SSE_BUFFER_SIZE {
                tracing::error!(
                    buffer_len = event_buf.len(),
                    "Bedrock event stream buffer exceeded maximum size, aborting"
                );
                metrics.record_error();
                outcome = StreamOutcome::UpstreamError;
                break;
            }
            event_buf.extend_from_slice(&bytes);

            // Decode all complete frames in the buffer
            loop {
                match eventstream::decode_frame(&mut event_buf) {
                    Err(e) => {
                        // On prelude CRC failure the decoder does NOT advance the buffer
                        // (total_len is untrustworthy), so continuing the inner loop
                        // would call decode_frame on the same bytes indefinitely.
                        // On message CRC failure the buffer is advanced, but the frame
                        // is corrupt regardless. Either way, close the connection per
                        // the decoder's contract ("caller closes the connection").
                        tracing::error!(error = %e, "Bedrock event stream CRC error; closing connection");
                        metrics.record_error();
                        outcome = StreamOutcome::UpstreamError;
                        break;
                    }
                    Ok(None) => break, // no complete frame yet
                    Ok(Some(payload)) => {
                        if let Some(event_json) = eventstream::extract_event_from_payload(&payload)
                        {
                            usage.observe_data(&event_json);
                            // Re-emit as SSE: "event: <type>\ndata: <json>\n\n"
                            // Bedrock events are raw Anthropic JSON; detect the event type.
                            let event_type = detect_event_type(&event_json);
                            let sse_line = format!("event: {event_type}\ndata: {event_json}\n\n");
                            if tx.send(Ok(sse_line)).await.is_err() {
                                outcome = StreamOutcome::ClientDisconnected;
                                break;
                            }
                        }
                    }
                }
                if !matches!(outcome, StreamOutcome::Completed) {
                    break;
                }
            }
            if !matches!(outcome, StreamOutcome::Completed) {
                break;
            }
        }
        let tokens = usage.tokens();
        let cost = tokens.map(|(input_t, output_t)| {
            record_virtual_key_usage(&log_shared, &vk_ctx, &cost_model, input_t, output_t)
        });
        let (status, err) = outcome.record(&metrics);
        log_request(
            &log_shared,
            ctx.log_entry_with_attribution(
                &log_backend_name,
                Some(cost_model),
                status,
                tokens,
                true,
                err,
                &vk_ctx,
                cost,
            ),
        );
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

fn bedrock_error_status(error: &BedrockClientError) -> u16 {
    match error {
        BedrockClientError::ApiError { status, .. } => *status,
        BedrockClientError::Transport(_) => StatusCode::BAD_GATEWAY.as_u16(),
        BedrockClientError::Signing(_) => StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
    }
}

fn virtual_key_accounting_parse_error() -> Response {
    let err = mapping::errors_map::create_anthropic_error(
        anthropic::ErrorType::ApiError,
        "Upstream response could not be accounted for this virtual API key.".to_string(),
        None,
    );
    (StatusCode::BAD_GATEWAY, Json(err)).into_response()
}

#[cfg(test)]
mod tests {
    use super::detect_event_type;

    #[test]
    fn detect_message_start() {
        assert_eq!(
            detect_event_type(r#"{"type":"message_start","message":{"id":"msg-1"}}"#),
            "message_start"
        );
    }

    #[test]
    fn detect_content_block_delta() {
        assert_eq!(
            detect_event_type(
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#
            ),
            "content_block_delta"
        );
    }

    #[test]
    fn detect_falls_back_for_unknown_type() {
        assert_eq!(
            detect_event_type(r#"{"type":"some_future_event"}"#),
            "message"
        );
    }

    #[test]
    fn detect_falls_back_on_malformed_json() {
        assert_eq!(detect_event_type("not json at all"), "message");
    }

    #[test]
    fn detect_handles_spaced_json() {
        assert_eq!(
            detect_event_type(r#"{ "type" : "message_stop" }"#),
            "message_stop"
        );
    }

    #[test]
    fn detect_ignores_nested_type_field() {
        // Top-level type is content_block_delta; nested delta.type is text_delta.
        let json = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}"#;
        assert_eq!(detect_event_type(json), "content_block_delta");
    }
}
