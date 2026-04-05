// Gemini native handler: POST /v1/messages -> Gemini generateContent/streamGenerateContent.
//
// Translates Anthropic requests to Gemini native format (not OpenAI-compat),
// calls the Gemini API, and translates responses back to Anthropic format.

use crate::backend::{BackendClient, BackendError};
use crate::server::state::{AnthropicJson, AppState};
use crate::server::streaming::{read_sse_frames, send_events, StreamOutcome};
use anyllm_translate::anthropic;
use anyllm_translate::gemini::response::GenerateContentResponse;
use anyllm_translate::mapping::gemini_message_map::{
    anthropic_to_gemini_request, compute_gemini_request_warnings, gemini_to_anthropic_response,
};
use anyllm_translate::mapping::gemini_streaming_map::GeminiStreamingTranslator;
use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Json, Response},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// POST /v1/messages — Gemini native path.
pub(crate) async fn gemini_native_handler(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>,
    AnthropicJson(body): AnthropicJson<anthropic::MessageCreateRequest>,
) -> Response {
    let client = match &state.backend {
        BackendClient::GeminiNative(c) => c.clone(),
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    anyllm_translate::mapping::errors_map::create_anthropic_error(
                        anthropic::ErrorType::ApiError,
                        "gemini_native_handler called with non-native backend".to_string(),
                        None,
                    ),
                ),
            )
                .into_response();
        }
    };

    state.metrics.record_request();

    // Enforce model allowlist for virtual keys.
    if let Some(axum::Extension(ref ctx)) = vk_ctx {
        if !crate::server::policy::is_model_allowed(&body.model, &ctx.allowed_models) {
            let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::PermissionError,
                format!("Model '{}' is not allowed for this API key.", body.model),
                None,
            );
            return (axum::http::StatusCode::FORBIDDEN, axum::response::Json(err)).into_response();
        }
    }

    let model = client.map_model(&body.model);
    let gemini_req = anthropic_to_gemini_request(&body);
    let original_model = body.model.clone();

    if body.stream == Some(true) {
        let metrics = state.metrics.clone();
        let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(32);

        tokio::spawn(async move {
            let resp = match client.generate_content_stream(&gemini_req, &model).await {
                Ok(r) => r,
                Err(e) => {
                    metrics.record_error();
                    let be = BackendError::from(e);
                    // Send a synthetic error event so the client knows the stream failed.
                    let err = anyllm_translate::mapping::errors_map::create_anthropic_error(
                        anthropic::ErrorType::ApiError,
                        be.to_string(),
                        None,
                    );
                    let event = anthropic::StreamEvent::Error {
                        error: anthropic::streaming::StreamError {
                            error_type: "api_error".to_string(),
                            message: err.error.message.clone(),
                        },
                    };
                    let _ = send_events(&tx, &[event]).await;
                    return;
                }
            };

            let mut translator = GeminiStreamingTranslator::new(original_model.clone());

            let outcome = read_sse_frames(resp, &tx, &metrics, |data| {
                if data == "[DONE]" {
                    return None;
                }
                match serde_json::from_str::<GenerateContentResponse>(data) {
                    Ok(gresp) => {
                        let events = translator.process_response(&gresp);
                        if events.is_empty() {
                            None
                        } else {
                            Some(events)
                        }
                    }
                    Err(e) => {
                        tracing::warn!("failed to parse Gemini SSE frame: {e}");
                        None
                    }
                }
            })
            .await;

            // If the stream ended without a finishReason, flush the translator.
            if matches!(outcome, StreamOutcome::Completed) && !translator.is_finished() {
                let final_events = translator.finish();
                send_events(&tx, &final_events).await;
            }

            match outcome {
                StreamOutcome::Completed => {
                    metrics.record_success();
                    metrics.record_stream_completed();
                }
                StreamOutcome::ClientDisconnected => {
                    metrics.record_stream_client_disconnected();
                }
                StreamOutcome::UpstreamError => {
                    metrics.record_stream_failed();
                }
            }
        });

        let stream = ReceiverStream::new(rx);
        Sse::new(stream)
            .keep_alive(KeepAlive::default())
            .into_response()
    } else {
        match client.generate_content(&gemini_req, &model).await {
            Ok(gresp) => {
                state.metrics.record_success();
                let anthropic_resp = gemini_to_anthropic_response(&gresp, &original_model);
                let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
                if state.expose_degradation_warnings {
                    let warnings = compute_gemini_request_warnings(&body);
                    crate::server::routes::inject_degradation_header(
                        response.headers_mut(),
                        &warnings,
                    );
                }
                response
            }
            Err(e) => {
                state.metrics.record_error();
                super::routes::backend_error_to_response(BackendError::from(e))
            }
        }
    }
}
