use crate::backend::anthropic_client::AnthropicClientError;
use crate::backend::bedrock_client::BedrockClientError;
use crate::backend::{BackendClient, BackendError};
use crate::server::state::AppState;
use anyllm_translate::anthropic;
use anyllm_translate::mapping::message_map;
use axum::http::StatusCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeminiAction {
    Generate,
    Stream,
    CountTokens,
    Unknown,
}

/// Extract model name and action from a `{model}:{action}` path segment.
pub fn parse_model_action(model_action: &str) -> (&str, GeminiAction) {
    if let Some(model) = model_action.strip_suffix(":streamGenerateContent") {
        (model, GeminiAction::Stream)
    } else if let Some(model) = model_action.strip_suffix(":generateContent") {
        (model, GeminiAction::Generate)
    } else if let Some(model) = model_action.strip_suffix(":countTokens") {
        (model, GeminiAction::CountTokens)
    } else {
        // Unknown action suffix — treat as non-streaming with the full string as model.
        (model_action, GeminiAction::Unknown)
    }
}

/// Call the backend in non-streaming mode and return an Anthropic MessageResponse.
pub(super) async fn call_backend_non_streaming(
    state: &AppState,
    req: &anthropic::MessageCreateRequest,
    mapped_model: &str,
) -> Result<anthropic::MessageResponse, BackendError> {
    let original_model = req.model.clone();
    match &state.backend {
        BackendClient::OpenAI(client)
        | BackendClient::AzureOpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let mut openai_req = message_map::anthropic_to_openai_request(req);
            openai_req.model = mapped_model.to_string();
            let (openai_resp, _status, _rate_limits) = client.chat_completion(&openai_req).await?;
            Ok(message_map::openai_to_anthropic_response(
                &openai_resp,
                &original_model,
            ))
        }
        BackendClient::OpenAIResponses(client) => {
            let mut openai_req = message_map::anthropic_to_openai_request(req);
            openai_req.model = mapped_model.to_string();
            let (openai_resp, _status, _rate_limits) = client.chat_completion(&openai_req).await?;
            Ok(message_map::openai_to_anthropic_response(
                &openai_resp,
                &original_model,
            ))
        }
        BackendClient::GeminiNative(client) => {
            // Already have Gemini types; translate Anthropic -> Gemini, call, translate back.
            let gemini_req_out =
                anyllm_translate::mapping::gemini_message_map::anthropic_to_gemini_request(req);
            let gemini_resp = client
                .generate_content(&gemini_req_out, mapped_model)
                .await?;
            Ok(
                anyllm_translate::mapping::gemini_message_map::gemini_to_anthropic_response(
                    &gemini_resp,
                    &original_model,
                ),
            )
        }
        BackendClient::Anthropic(client) => {
            let body = serde_json::to_vec(req).map_err(|e| {
                BackendError::Anthropic(AnthropicClientError::Transport(e.to_string()))
            })?;
            let (resp_bytes, _rate_limits) = client.forward(body.into(), &[]).await?;
            let resp: anthropic::MessageResponse =
                serde_json::from_slice(&resp_bytes).map_err(|e| {
                    BackendError::Anthropic(AnthropicClientError::Transport(e.to_string()))
                })?;
            Ok(resp)
        }
        BackendClient::Bedrock(client) => {
            let body = serde_json::to_vec(req)
                .map_err(|e| BackendError::Bedrock(BedrockClientError::Transport(e.to_string())))?;
            let (resp_bytes, _rate_limits) = client.forward(body.into(), mapped_model).await?;
            let resp: anthropic::MessageResponse = serde_json::from_slice(&resp_bytes)
                .map_err(|e| BackendError::Bedrock(BedrockClientError::Transport(e.to_string())))?;
            Ok(resp)
        }
    }
}

/// Map a BackendError to an HTTP status and message for the Gemini error response.
pub(super) fn gemini_error_from_backend(error: &BackendError) -> (StatusCode, String) {
    if let Some((msg, status)) = error.api_error_details() {
        let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return (code, msg);
    }
    tracing::error!("gemini input backend error: {error}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "An internal error occurred while communicating with the upstream service.".to_string(),
    )
}
