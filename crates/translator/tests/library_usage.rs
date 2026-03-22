//! Integration test: verify the crate works as a standalone library without the proxy.

use anthropic_openai_translate::anthropic::{MessageCreateRequest, MessageResponse, Usage};
use anthropic_openai_translate::openai::ChatCompletionResponse;
use anthropic_openai_translate::{
    translate_request, translate_response, TranslateError, TranslationConfig,
};

#[test]
fn standalone_translate_request() {
    let config = TranslationConfig::builder()
        .model_map("haiku", "gpt-4o-mini")
        .model_map("sonnet", "gpt-4o")
        .build();

    let req: MessageCreateRequest = serde_json::from_str(
        r#"{
        "model": "claude-sonnet-4-6",
        "max_tokens": 100,
        "messages": [{"role": "user", "content": "Hello"}]
    }"#,
    )
    .unwrap();

    let openai_req = translate_request(&req, &config).unwrap();
    assert_eq!(openai_req.model, "gpt-4o");
    assert_eq!(openai_req.max_completion_tokens, Some(100));
}

#[test]
fn standalone_translate_response() {
    let openai_resp: ChatCompletionResponse = serde_json::from_str(
        r#"{
        "id": "chatcmpl-abc",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "Hello!"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7}
    }"#,
    )
    .unwrap();

    let resp: MessageResponse = translate_response(&openai_resp, "claude-sonnet-4-6");
    assert_eq!(resp.model, "claude-sonnet-4-6");
    assert_eq!(
        resp.usage,
        Usage {
            input_tokens: 5,
            output_tokens: 2,
            ..Default::default()
        }
    );
}

#[test]
fn standalone_strict_mode_rejects_unknown() {
    let config = TranslationConfig::builder()
        .model_map("sonnet", "gpt-4o")
        .passthrough_unknown_models(false)
        .build();

    let req: MessageCreateRequest = serde_json::from_str(
        r#"{
        "model": "unknown-model",
        "max_tokens": 10,
        "messages": [{"role": "user", "content": "Hi"}]
    }"#,
    )
    .unwrap();

    let err = translate_request(&req, &config).unwrap_err();
    assert!(matches!(err, TranslateError::UnknownModel(_)));
    assert!(err.to_string().contains("unknown-model"));
}

#[test]
fn default_config_passthrough() {
    let config = TranslationConfig::default();

    let req: MessageCreateRequest = serde_json::from_str(
        r#"{
        "model": "any-model-name",
        "max_tokens": 10,
        "messages": [{"role": "user", "content": "test"}]
    }"#,
    )
    .unwrap();

    let openai_req = translate_request(&req, &config).unwrap();
    assert_eq!(openai_req.model, "any-model-name");
}
