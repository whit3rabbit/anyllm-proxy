use super::*;
use serde_json::json;

fn basic_anthropic_request() -> anthropic::MessageCreateRequest {
    serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 4096,
        "messages": [{"role": "user", "content": "hi"}]
    }))
    .unwrap()
}

#[test]
fn web_search_options_injects_raw_hosted_tool() {
    let openai_req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "search"}],
        "max_tokens": 100,
        "web_search_options": {
            "search_context_size": "high",
            "user_location": {
                "approximate": {"city": "Chicago", "country": "US"}
            }
        }
    }))
    .unwrap();
    let mut anthropic_req = basic_anthropic_request();
    let mut warnings = TranslationWarnings::default();
    let extensions =
        apply_anthropic_chat_extensions(&openai_req, &mut anthropic_req, &mut warnings, false)
            .unwrap();

    assert_eq!(extensions.raw_tools.len(), 1);
    assert_eq!(extensions.raw_tools[0]["type"], "web_search_20250305");
    assert_eq!(extensions.raw_tools[0]["name"], "web_search");
    assert_eq!(extensions.raw_tools[0]["max_uses"], 10);
    assert_eq!(extensions.raw_tools[0]["user_location"]["city"], "Chicago");

    let body = serialize_anthropic_upstream_request(&anthropic_req, &extensions.raw_tools).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["tools"][0]["type"], "web_search_20250305");
}

#[test]
fn beta_headers_merge_without_duplicates() {
    let mut headers = vec![(
        "anthropic-beta".to_string(),
        "existing-beta,fast-mode-2026-02-01".to_string(),
    )];
    merge_anthropic_beta_headers(
        &mut headers,
        &["fast-mode-2026-02-01", "compact-2026-01-12"],
    );
    assert_eq!(
        headers[0].1,
        "existing-beta,fast-mode-2026-02-01,compact-2026-01-12"
    );
}

#[test]
fn reasoning_effort_sets_budget_and_adjusts_default_max_tokens() {
    let openai_req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "think"}],
        "reasoning_effort": {"effort": "high"}
    }))
    .unwrap();
    let mut anthropic_req = basic_anthropic_request();
    let mut warnings = TranslationWarnings::default();
    apply_anthropic_chat_extensions(&openai_req, &mut anthropic_req, &mut warnings, true).unwrap();
    assert_eq!(anthropic_req.max_tokens, 8192);
    assert!(matches!(
        anthropic_req.thinking,
        Some(anthropic::ThinkingConfig::Enabled {
            budget_tokens: 4096
        })
    ));
}

#[test]
fn adaptive_reasoning_effort_sets_adaptive_thinking_and_output_config() {
    let openai_req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-opus-4-8",
        "messages": [{"role": "user", "content": "think"}],
        "reasoning_effort": {"effort": "high"}
    }))
    .unwrap();
    let mut anthropic_req = basic_anthropic_request();
    anthropic_req.model = "claude-opus-4-8".to_string();
    let mut warnings = TranslationWarnings::default();
    apply_anthropic_chat_extensions(&openai_req, &mut anthropic_req, &mut warnings, true).unwrap();

    assert_eq!(anthropic_req.max_tokens, 4096);
    assert!(matches!(
        anthropic_req.thinking,
        Some(anthropic::ThinkingConfig::Adaptive { .. })
    ));
    assert_eq!(
        anthropic_req.extra["output_config"]["effort"],
        serde_json::Value::String("high".to_string())
    );
}

#[test]
fn adaptive_reasoning_effort_maps_minimal_to_low_output_effort() {
    let openai_req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-opus-4-8",
        "messages": [{"role": "user", "content": "think"}],
        "reasoning_effort": "minimal"
    }))
    .unwrap();
    let mut anthropic_req = basic_anthropic_request();
    anthropic_req.model = "claude-opus-4-8".to_string();
    let mut warnings = TranslationWarnings::default();
    apply_anthropic_chat_extensions(&openai_req, &mut anthropic_req, &mut warnings, false).unwrap();

    assert_eq!(
        anthropic_req.extra["output_config"]["effort"],
        serde_json::Value::String("low".to_string())
    );
}

#[test]
fn adaptive_reasoning_effort_rejects_unsupported_xhigh() {
    let openai_req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "messages": [{"role": "user", "content": "think"}],
        "reasoning_effort": "xhigh"
    }))
    .unwrap();
    let mut anthropic_req = basic_anthropic_request();
    anthropic_req.model = "claude-sonnet-4-6".to_string();
    let mut warnings = TranslationWarnings::default();
    let err =
        apply_anthropic_chat_extensions(&openai_req, &mut anthropic_req, &mut warnings, false)
            .unwrap_err();

    assert!(err.contains("does not support reasoning_effort xhigh"));
}

#[test]
fn reasoning_effort_rejects_too_small_explicit_max_tokens() {
    let openai_req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "think"}],
        "max_tokens": 1024,
        "reasoning_effort": "low"
    }))
    .unwrap();
    let mut anthropic_req = basic_anthropic_request();
    anthropic_req.max_tokens = 1024;
    let mut warnings = TranslationWarnings::default();
    let err =
        apply_anthropic_chat_extensions(&openai_req, &mut anthropic_req, &mut warnings, false)
            .unwrap_err();
    assert!(err.contains("max_tokens must be greater"));
}

#[test]
fn native_thinking_rejects_too_small_explicit_max_tokens() {
    let openai_req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "think"}],
        "max_tokens": 4096,
        "thinking": {"type": "enabled", "budget_tokens": 16000}
    }))
    .unwrap();
    let mut anthropic_req = basic_anthropic_request();
    anthropic_req.max_tokens = 4096;
    let mut warnings = TranslationWarnings::default();
    let err =
        apply_anthropic_chat_extensions(&openai_req, &mut anthropic_req, &mut warnings, false)
            .unwrap_err();
    assert!(err.contains("max_tokens must be greater"));
}

#[test]
fn native_thinking_rejects_budget_tokens_on_adaptive_only_model() {
    let openai_req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-opus-4-8",
        "messages": [{"role": "user", "content": "think"}],
        "max_tokens": 16000,
        "thinking": {"type": "enabled", "budget_tokens": 4096}
    }))
    .unwrap();
    let mut anthropic_req = basic_anthropic_request();
    anthropic_req.model = "claude-opus-4-8".to_string();
    anthropic_req.max_tokens = 16000;
    let mut warnings = TranslationWarnings::default();
    let err =
        apply_anthropic_chat_extensions(&openai_req, &mut anthropic_req, &mut warnings, false)
            .unwrap_err();
    assert!(err.contains("only supports adaptive thinking"));
}

#[test]
fn native_thinking_rejects_explicit_disabled_on_fable_5() {
    let openai_req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-fable-5",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 16000,
        "thinking": {"type": "disabled"}
    }))
    .unwrap();
    let mut anthropic_req = basic_anthropic_request();
    anthropic_req.model = "claude-fable-5".to_string();
    let mut warnings = TranslationWarnings::default();
    let err =
        apply_anthropic_chat_extensions(&openai_req, &mut anthropic_req, &mut warnings, false)
            .unwrap_err();
    assert!(err.contains("rejects an explicit thinking"));
}

#[test]
fn native_thinking_null_reasoning_effort_is_not_a_conflict() {
    let openai_req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "think"}],
        "thinking": {"type": "enabled", "budget_tokens": 1024},
        "reasoning_effort": null
    }))
    .unwrap();
    let mut anthropic_req = basic_anthropic_request();
    let mut warnings = TranslationWarnings::default();
    apply_anthropic_chat_extensions(&openai_req, &mut anthropic_req, &mut warnings, false)
        .expect("an explicit null reasoning_effort must not conflict with native thinking");
    assert!(matches!(
        anthropic_req.thinking,
        Some(anthropic::ThinkingConfig::Enabled {
            budget_tokens: 1024
        })
    ));
}

#[test]
fn structured_output_schema_is_filtered() {
    let schema = sanitize_anthropic_output_schema(json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "minItems": 1,
                "items": {"type": "string", "minLength": 2}
            }
        }
    }));
    assert_eq!(schema["additionalProperties"], false);
    assert!(schema["properties"]["items"].get("minItems").is_none());
    assert!(schema["properties"]["items"]["items"]
        .get("minLength")
        .is_none());
    assert!(schema["properties"]["items"]["description"]
        .as_str()
        .unwrap()
        .contains("minimum number of items"));
}
