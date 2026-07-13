use serde_json::json;

use super::*;

fn catalog(json: serde_json::Value) -> ProviderCatalog {
    ProviderCatalog::from_litellm_json(&json.to_string()).unwrap()
}

fn empty_catalog() -> ProviderCatalog {
    ProviderCatalog::from_litellm_json("{}").unwrap()
}

fn request(value: serde_json::Value) -> openai::ChatCompletionRequest {
    serde_json::from_value(value).unwrap()
}

fn ctx<'a>(
    backend_kind: BackendKind,
    provider_id: Option<&'a str>,
    model: &'a str,
    provider_catalog: &'a ProviderCatalog,
) -> OpenAiToolPolicyContext<'a> {
    OpenAiToolPolicyContext {
        backend_kind,
        provider_id,
        model,
        provider_catalog,
    }
}

#[test]
fn openai_tool_policy_mistral_rewrites_ids_and_matching_tool_results() {
    let mut req = request(json!({
        "model": "mistral-large-latest",
        "messages": [
            {
                "role": "assistant",
                "tool_calls": [
                    {"id": "call_alpha", "type": "function", "function": {"name": "a", "arguments": "{}"}},
                    {"id": "call_alpha", "type": "function", "function": {"name": "b", "arguments": "{}"}}
                ]
            },
            {"role": "tool", "tool_call_id": "call_alpha", "content": "first"},
            {"role": "tool", "tool_call_id": "call_alpha", "content": "second"}
        ]
    }));
    let catalog = empty_catalog();
    let mut warnings = TranslationWarnings::default();

    let report = prepare_openai_tool_request(
        &mut req,
        ctx(
            BackendKind::OpenAI,
            Some("mistral"),
            "mistral-large-latest",
            &catalog,
        ),
        &mut warnings,
    )
    .unwrap();

    let calls = req.messages[0].tool_calls.as_ref().unwrap();
    assert_eq!(calls[0].id, "000000000");
    assert_eq!(calls[1].id, "000000001");
    assert_eq!(req.messages[1].tool_call_id.as_deref(), Some("000000000"));
    assert_eq!(req.messages[2].tool_call_id.as_deref(), Some("000000001"));
    assert_eq!(report.duplicate_tool_call_ids, 1);
    assert_eq!(report.remapped_tool_results, 2);
}

#[test]
fn openai_tool_policy_gemini_sanitizes_schema_and_removes_strict() {
    let mut req = request(json!({
        "model": "gemini-2.5-pro",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "search",
                "strict": true,
                "parameters": {
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "q": {"type": "string", "default": ""}
                    }
                }
            }
        }]
    }));
    let catalog = empty_catalog();
    let mut warnings = TranslationWarnings::default();

    prepare_openai_tool_request(
        &mut req,
        ctx(
            BackendKind::Gemini,
            Some("gemini"),
            "gemini-2.5-pro",
            &catalog,
        ),
        &mut warnings,
    )
    .unwrap();

    let tool = req.tools.as_ref().unwrap().first().unwrap();
    assert_eq!(tool.function.strict, None);
    let params = tool.function.parameters.as_ref().unwrap();
    assert!(params.get("$schema").is_none());
    assert!(params.get("additionalProperties").is_none());
    assert!(
        params.pointer("/properties/q/default").is_none(),
        "Gemini rejects JSON Schema default in nested properties"
    );
    assert_eq!(
        warnings.as_header_value().as_deref(),
        Some("tools.function.strict")
    );
}

#[test]
fn openai_tool_policy_applies_gemini_policy_by_protocol_for_managed_backend() {
    // A managed/custom backend can run BackendKind::OpenAI while its provider_id
    // resolves to the Gemini OpenAI shim in the catalog. The policy must key off
    // the provider protocol, not a hardcoded "gemini" id, so strict-stripping
    // still applies.
    let catalog = ProviderCatalog::bundled();
    let mut req = request(json!({
        "model": "gemini-2.5-pro",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "search",
                "strict": true,
                "parameters": {"type": "object", "additionalProperties": false}
            }
        }]
    }));
    let mut warnings = TranslationWarnings::default();

    prepare_openai_tool_request(
        &mut req,
        ctx(
            BackendKind::OpenAI,
            Some("gemini"),
            "gemini-2.5-pro",
            &catalog,
        ),
        &mut warnings,
    )
    .unwrap();

    let tool = req.tools.as_ref().unwrap().first().unwrap();
    assert_eq!(tool.function.strict, None);
    assert!(tool
        .function
        .parameters
        .as_ref()
        .unwrap()
        .get("additionalProperties")
        .is_none());
}

#[test]
fn openai_tool_policy_gemini_strips_parallel_false_with_multiple_tools() {
    let mut req = request(json!({
        "model": "gemini-2.5-pro",
        "messages": [{"role": "user", "content": "hi"}],
        "parallel_tool_calls": false,
        "tools": [
            {"type": "function", "function": {"name": "a", "parameters": {"type": "object"}}},
            {"type": "function", "function": {"name": "b", "parameters": {"type": "object"}}}
        ]
    }));
    let catalog = empty_catalog();
    let mut warnings = TranslationWarnings::default();

    prepare_openai_tool_request(
        &mut req,
        ctx(
            BackendKind::Gemini,
            Some("gemini"),
            "gemini-2.5-pro",
            &catalog,
        ),
        &mut warnings,
    )
    .unwrap();

    // Gemini cannot honor parallel_tool_calls; the field is stripped and the
    // degradation is reported rather than the request being rejected.
    assert!(req.parallel_tool_calls.is_none());
    assert_eq!(
        warnings.as_header_value().as_deref(),
        Some("parallel_tool_calls")
    );
}

#[test]
fn openai_tool_policy_rejects_required_tool_choice_when_model_lacks_it() {
    let catalog = catalog(json!({
        "demo-model": {
            "litellm_provider": "demo",
            "mode": "chat",
            "supports_function_calling": true,
            "supports_tool_choice": false
        }
    }));
    let mut req = request(json!({
        "model": "demo-model",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "a", "parameters": {"type": "object"}}}],
        "tool_choice": "required"
    }));
    let mut warnings = TranslationWarnings::default();

    let err = prepare_openai_tool_request(
        &mut req,
        ctx(BackendKind::OpenAI, Some("demo"), "demo-model", &catalog),
        &mut warnings,
    )
    .unwrap_err();

    assert!(err.message().contains("tool_choice=required"));
}

#[test]
fn openai_tool_policy_drops_auto_tool_choice_when_model_lacks_it() {
    let catalog = catalog(json!({
        "demo-model": {
            "litellm_provider": "demo",
            "mode": "chat",
            "supports_function_calling": true,
            "supports_tool_choice": false
        }
    }));
    let mut req = request(json!({
        "model": "demo-model",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "a", "parameters": {"type": "object"}}}],
        "tool_choice": "auto"
    }));
    let mut warnings = TranslationWarnings::default();

    prepare_openai_tool_request(
        &mut req,
        ctx(BackendKind::OpenAI, Some("demo"), "demo-model", &catalog),
        &mut warnings,
    )
    .unwrap();

    assert!(req.tool_choice.is_none());
    assert!(req.tools.is_some());
    assert_eq!(warnings.as_header_value().as_deref(), Some("tool_choice"));
}

#[test]
fn openai_tool_policy_drops_none_tool_choice_and_tools_without_continuation() {
    let catalog = catalog(json!({
        "demo-model": {
            "litellm_provider": "demo",
            "mode": "chat",
            "supports_function_calling": true,
            "supports_tool_choice": false
        }
    }));
    let mut req = request(json!({
        "model": "demo-model",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "a", "parameters": {"type": "object"}}}],
        "tool_choice": "none"
    }));
    let mut warnings = TranslationWarnings::default();

    prepare_openai_tool_request(
        &mut req,
        ctx(BackendKind::OpenAI, Some("demo"), "demo-model", &catalog),
        &mut warnings,
    )
    .unwrap();

    assert!(req.tool_choice.is_none());
    assert!(req.tools.is_none());
    assert_eq!(
        warnings.as_header_value().as_deref(),
        Some("tool_choice, tools")
    );
}

#[test]
fn openai_tool_policy_rejects_tools_when_known_model_lacks_tool_use() {
    let catalog = catalog(json!({
        "demo-model": {
            "litellm_provider": "demo",
            "mode": "chat",
            "supports_function_calling": false,
            "supports_tool_choice": false
        }
    }));
    let mut req = request(json!({
        "model": "demo-model",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "a", "parameters": {"type": "object"}}}]
    }));
    let mut warnings = TranslationWarnings::default();

    let err = prepare_openai_tool_request(
        &mut req,
        ctx(BackendKind::OpenAI, Some("demo"), "demo-model", &catalog),
        &mut warnings,
    )
    .unwrap_err();

    assert!(err.message().contains("does not support tools"));
}

#[test]
fn openai_tool_policy_allows_forced_tool_choice_for_self_hosted_provider() {
    // vllm/lm_studio/llamafile/triton stubs advertise tool_use but a
    // conservative provider-level tool_choice:false and carry no per-model
    // metadata. Forced tool_choice must pass through (the backend decides),
    // not 400 -- the provider-level flag is not authoritative for an unknown
    // self-hosted model.
    let catalog = ProviderCatalog::bundled();
    assert!(
        catalog.list_models("vllm").is_empty(),
        "test assumes vllm has no per-model metadata"
    );
    let mut req = request(json!({
        "model": "Qwen/Qwen2.5-7B-Instruct",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "a", "parameters": {"type": "object"}}}],
        "tool_choice": "required"
    }));
    let mut warnings = TranslationWarnings::default();

    prepare_openai_tool_request(
        &mut req,
        ctx(
            BackendKind::OpenAI,
            Some("vllm"),
            "Qwen/Qwen2.5-7B-Instruct",
            &catalog,
        ),
        &mut warnings,
    )
    .unwrap();

    assert!(matches!(
        req.tool_choice,
        Some(ChatToolChoice::Simple(ref v)) if v == "required"
    ));
}

#[test]
fn openai_tool_policy_rejects_native_anthropic_tools_when_known_model_lacks_tool_use() {
    let catalog = catalog(json!({
        "demo-model": {
            "litellm_provider": "bedrock",
            "mode": "chat",
            "supports_function_calling": false,
            "supports_tool_choice": false
        }
    }));
    let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
        "model": "demo-model",
        "max_tokens": 64,
        "tools": [{
            "name": "lookup",
            "input_schema": {"type": "object"}
        }],
        "messages": [
            {"role": "user", "content": "hi"}
        ]
    }))
    .unwrap();

    let err = validate_anthropic_tool_request(
        &req,
        ctx(
            BackendKind::Bedrock,
            Some("bedrock"),
            "demo-model",
            &catalog,
        ),
    )
    .unwrap_err();

    assert!(err.message().contains("does not support tools"));
}

#[test]
fn openai_tool_policy_normalizes_streaming_top_level_tool_call_delta() {
    let chunk = parse_openai_chat_completion_chunk(
        r#"{
            "id": "chatcmpl_1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "local",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "name": "lookup",
                        "arguments": {"q":"x"}
                    }]
                }
            }]
        }"#,
    )
    .unwrap();

    let tool_call = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tool_call.call_type.as_deref(), Some("function"));
    assert_eq!(
        tool_call.function.as_ref().unwrap().name.as_deref(),
        Some("lookup")
    );
    assert_eq!(
        tool_call.function.as_ref().unwrap().arguments.as_deref(),
        Some(r#"{"q":"x"}"#)
    );
}
