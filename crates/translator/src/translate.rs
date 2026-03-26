// Phase 21a: Convenience wrappers for the translation layer.
//
// Thin functions combining TranslationConfig with the stateless mapping functions.
// The raw mapping API (crate::mapping::*) remains available for advanced use.

use crate::anthropic::{MessageCreateRequest, MessageResponse};
use crate::config::TranslationConfig;
use crate::error::TranslateError;
use crate::mapping::{
    message_map, responses_message_map, responses_streaming_map, reverse_message_map,
    reverse_streaming_map, streaming_map,
};
use crate::openai::responses::{ResponsesRequest, ResponsesResponse};
use crate::openai::{ChatCompletionRequest, ChatCompletionResponse};
pub use crate::mapping::warnings::TranslationWarnings;

/// Compute degradation warnings for a request — features that will be dropped in translation.
///
/// Call this before translating; inject the result as `x-anyllm-degradation` header.
pub fn compute_request_warnings(req: &MessageCreateRequest) -> TranslationWarnings {
    message_map::compute_request_warnings(req)
}

/// Translate an Anthropic request to an OpenAI Chat Completions request.
///
/// Applies model mapping from config to the resulting request's `model` field.
pub fn translate_request(
    req: &MessageCreateRequest,
    config: &TranslationConfig,
) -> Result<ChatCompletionRequest, TranslateError> {
    let mut openai_req = message_map::anthropic_to_openai_request(req);
    openai_req.model = config.map_model(&openai_req.model)?;
    Ok(openai_req)
}

/// Translate an OpenAI Chat Completions response back to an Anthropic response.
///
/// `original_model` is the Anthropic model name from the original request,
/// used in the response's `model` field.
pub fn translate_response(resp: &ChatCompletionResponse, original_model: &str) -> MessageResponse {
    message_map::openai_to_anthropic_response(resp, original_model)
}

/// Create a new streaming translator for OpenAI Chat Completions chunks.
///
/// The returned translator is stateful: feed chunks via `process_chunk()`,
/// then call `finish()` to get the final events.
pub fn new_stream_translator(model: String) -> streaming_map::StreamingTranslator {
    streaming_map::StreamingTranslator::new(model)
}

/// Translate an Anthropic request to an OpenAI Responses API request.
///
/// Applies model mapping from config to the resulting request's `model` field.
pub fn translate_request_responses(
    req: &MessageCreateRequest,
    config: &TranslationConfig,
) -> Result<ResponsesRequest, TranslateError> {
    let mut responses_req = responses_message_map::anthropic_to_responses_request(req);
    responses_req.model = config.map_model(&responses_req.model)?;
    Ok(responses_req)
}

/// Translate an OpenAI Responses API response back to an Anthropic response.
///
/// `original_model` is the Anthropic model name from the original request.
pub fn translate_response_responses(
    resp: &ResponsesResponse,
    original_model: &str,
) -> MessageResponse {
    responses_message_map::responses_to_anthropic_response(resp, original_model)
}

/// Translate an OpenAI Chat Completions request to an Anthropic request.
///
/// Returns an error if `max_tokens` / `max_completion_tokens` is absent.
/// Populates `warnings` with features dropped during translation.
pub fn translate_openai_to_anthropic_request(
    req: &ChatCompletionRequest,
    warnings: &mut TranslationWarnings,
) -> Result<MessageCreateRequest, TranslateError> {
    reverse_message_map::openai_to_anthropic_request(req, warnings)
}

/// Translate an Anthropic response to an OpenAI Chat Completions response.
///
/// `model` is used as the response's `model` field.
pub fn translate_anthropic_to_openai_response(
    resp: &MessageResponse,
    model: &str,
) -> ChatCompletionResponse {
    reverse_message_map::anthropic_to_openai_response(resp, model)
}

/// Create a new reverse streaming translator (Anthropic SSE -> OpenAI chunks).
///
/// The returned translator is stateful: feed Anthropic StreamEvent items via
/// `process_event()`, which returns OpenAI ChatCompletionChunk objects.
pub fn new_reverse_stream_translator(
    id: String,
    model: String,
) -> reverse_streaming_map::ReverseStreamingTranslator {
    reverse_streaming_map::ReverseStreamingTranslator::new(id, model)
}

/// Create a new streaming translator for OpenAI Responses API events.
///
/// Same stateful pattern as `new_stream_translator`.
pub fn new_responses_stream_translator(
    model: String,
) -> responses_streaming_map::ResponsesStreamingTranslator {
    responses_streaming_map::ResponsesStreamingTranslator::new(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LossyBehavior;

    fn sample_request() -> MessageCreateRequest {
        serde_json::from_str(
            r#"{
                "model": "claude-sonnet-4-6",
                "max_tokens": 100,
                "messages": [{"role": "user", "content": "Hello"}]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn translate_request_with_default_config() {
        let config = TranslationConfig::default();
        let req = sample_request();
        let openai_req = translate_request(&req, &config).unwrap();
        // Default config: empty model_map, passthrough
        assert_eq!(openai_req.model, "claude-sonnet-4-6");
        assert_eq!(openai_req.max_completion_tokens, Some(100));
    }

    #[test]
    fn translate_request_with_model_mapping() {
        let config = TranslationConfig::builder()
            .model_map("haiku", "gpt-4o-mini")
            .model_map("sonnet", "gpt-4o")
            .build();

        let req = sample_request();
        let openai_req = translate_request(&req, &config).unwrap();
        assert_eq!(openai_req.model, "gpt-4o");
    }

    #[test]
    fn translate_request_unknown_model_passthrough() {
        let config = TranslationConfig::builder()
            .model_map("sonnet", "gpt-4o")
            .build();

        let req: MessageCreateRequest = serde_json::from_str(
            r#"{
                "model": "custom-model",
                "max_tokens": 50,
                "messages": [{"role": "user", "content": "Hi"}]
            }"#,
        )
        .unwrap();

        let openai_req = translate_request(&req, &config).unwrap();
        assert_eq!(openai_req.model, "custom-model");
    }

    #[test]
    fn translate_request_unknown_model_strict() {
        let config = TranslationConfig::builder()
            .model_map("sonnet", "gpt-4o")
            .passthrough_unknown_models(false)
            .build();

        let req: MessageCreateRequest = serde_json::from_str(
            r#"{
                "model": "custom-model",
                "max_tokens": 50,
                "messages": [{"role": "user", "content": "Hi"}]
            }"#,
        )
        .unwrap();

        let err = translate_request(&req, &config).unwrap_err();
        assert!(matches!(err, TranslateError::UnknownModel(_)));
    }

    #[test]
    fn translate_response_roundtrip() {
        let openai_resp: ChatCompletionResponse = serde_json::from_str(
            r#"{
                "id": "chatcmpl-123",
                "object": "chat.completion",
                "created": 1700000000,
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hi there!"},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
            }"#,
        )
        .unwrap();

        let anthropic_resp = translate_response(&openai_resp, "claude-sonnet-4-6");
        assert_eq!(anthropic_resp.model, "claude-sonnet-4-6");
        assert_eq!(anthropic_resp.usage.input_tokens, 10);
        assert_eq!(anthropic_resp.usage.output_tokens, 5);
    }

    #[test]
    fn builder_ergonomics() {
        let config = TranslationConfig::builder()
            .model_map("haiku", "gemini-2.5-flash")
            .model_map("sonnet", "gemini-2.5-pro")
            .model_map("opus", "gemini-2.5-pro")
            .lossy_behavior(LossyBehavior::Silent)
            .passthrough_unknown_models(false)
            .build();

        assert_eq!(config.model_map.len(), 3);
        assert_eq!(config.lossy_behavior, LossyBehavior::Silent);
        assert!(!config.passthrough_unknown_models);
    }
}
