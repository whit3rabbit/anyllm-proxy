use std::borrow::Cow;

use anyllm_providers::{requires_numeric_tool_call_ids, ProviderCatalog, ProviderProtocol};
use anyllm_translate::{
    anthropic,
    mapping::{tools_map, warnings::TranslationWarnings},
    openai::{
        self,
        tool_normalization::{
            normalize_chat_completion_chunk_value, normalize_request_tool_call_ids,
            ToolCallIdStrategy, ToolCallNormalizationReport,
        },
        ChatRole, ChatToolChoice,
    },
};

use crate::backend::{openai_client::OpenAIClientError, BackendClient, BackendError};
use crate::config::BackendKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPolicyError {
    message: String,
}

impl ToolPolicyError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for ToolPolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ToolPolicyError {}

pub struct OpenAiToolPolicyContext<'a> {
    pub backend_kind: BackendKind,
    pub provider_id: Option<&'a str>,
    pub model: &'a str,
    pub provider_catalog: &'a ProviderCatalog,
}

pub fn prepare_openai_tool_request(
    req: &mut openai::ChatCompletionRequest,
    ctx: OpenAiToolPolicyContext<'_>,
    warnings: &mut TranslationWarnings,
) -> Result<ToolCallNormalizationReport, ToolPolicyError> {
    let provider_id = effective_provider_id(&ctx);
    let strategy = if requires_numeric_tool_call_ids(provider_id.as_ref()) {
        ToolCallIdStrategy::NineDigitSequential
    } else {
        ToolCallIdStrategy::Preserve
    };
    let report = normalize_request_tool_call_ids(req, strategy);

    if uses_gemini_openai_policy(
        ctx.provider_catalog,
        &ctx.backend_kind,
        provider_id.as_ref(),
    ) {
        sanitize_tools_for_gemini(req, warnings);
        enforce_parallel_tool_calls(req, warnings);
    }

    if let Some(caps) = tool_capabilities(ctx.provider_catalog, provider_id.as_ref(), ctx.model) {
        if !caps.tool_use && request_contains_tool_semantics(req) {
            return Err(ToolPolicyError::invalid(format!(
                "model '{}' for provider '{}' does not support tools",
                ctx.model, provider_id
            )));
        }
        if !caps.tool_choice {
            enforce_tool_choice(req, warnings)?;
        }
    }

    Ok(report)
}

pub fn parse_openai_chat_completion_chunk(
    json: &str,
) -> Result<openai::ChatCompletionChunk, serde_json::Error> {
    let mut value: serde_json::Value = serde_json::from_str(json)?;
    normalize_chat_completion_chunk_value(&mut value);
    serde_json::from_value(value)
}

pub fn validate_anthropic_tool_request(
    req: &anthropic::MessageCreateRequest,
    ctx: OpenAiToolPolicyContext<'_>,
) -> Result<(), ToolPolicyError> {
    let provider_id = effective_provider_id(&ctx);
    let Some(caps) = tool_capabilities(ctx.provider_catalog, provider_id.as_ref(), ctx.model)
    else {
        return Ok(());
    };

    if !caps.tool_use && anthropic_request_contains_tool_semantics(req) {
        return Err(ToolPolicyError::invalid(format!(
            "model '{}' for provider '{}' does not support tools",
            ctx.model, provider_id
        )));
    }

    if !caps.tool_choice && anthropic_request_requires_tool_choice(req) {
        return Err(ToolPolicyError::invalid(
            "forced tool_choice is not supported by this provider/model",
        ));
    }

    Ok(())
}

pub fn backend_kind_for_policy(backend: &BackendClient) -> BackendKind {
    match backend {
        BackendClient::OpenAI(_) | BackendClient::OpenAIResponses(_) => BackendKind::OpenAI,
        BackendClient::AzureOpenAI(_) => BackendKind::AzureOpenAI,
        BackendClient::Vertex(_) => BackendKind::Vertex,
        BackendClient::GeminiOpenAI(_) => BackendKind::Gemini,
        BackendClient::Anthropic(_) => BackendKind::Anthropic,
        BackendClient::Bedrock(_) => BackendKind::Bedrock,
        BackendClient::GeminiNative(_) => BackendKind::Gemini,
    }
}

pub fn tool_policy_error_to_backend_error(err: ToolPolicyError) -> BackendError {
    BackendError::OpenAI(OpenAIClientError::ApiError {
        status: 400,
        error: openai::errors::ErrorResponse {
            error: openai::errors::ErrorDetail {
                message: err.to_string(),
                error_type: "invalid_request_error".to_string(),
                param: None,
                code: None,
            },
        },
    })
}

#[derive(Debug, Clone, Copy)]
struct ToolCapabilities {
    tool_use: bool,
    tool_choice: bool,
}

fn effective_provider_id<'a>(ctx: &OpenAiToolPolicyContext<'a>) -> Cow<'a, str> {
    if let Some(provider_id) = ctx.provider_id.filter(|id| !id.trim().is_empty()) {
        return Cow::Borrowed(provider_id);
    }

    Cow::Borrowed(match ctx.backend_kind {
        BackendKind::OpenAI => "openai",
        BackendKind::AzureOpenAI => "azure",
        BackendKind::Vertex => "vertex_ai",
        BackendKind::Gemini => "gemini",
        BackendKind::Anthropic => "anthropic",
        BackendKind::Bedrock => "bedrock",
    })
}

fn uses_gemini_openai_policy(
    catalog: &ProviderCatalog,
    backend_kind: &BackendKind,
    provider_id: &str,
) -> bool {
    // The Gemini/Vertex OpenAI shim is identified by protocol in the catalog
    // (the single source of provider wire behavior) rather than a hardcoded
    // provider-id list, so any provider on that shim -- not just "gemini" /
    // "vertex_ai" -- gets the schema/strict/parallel sanitization.
    matches!(backend_kind, BackendKind::Gemini | BackendKind::Vertex)
        || catalog.get_provider(provider_id).is_some_and(|provider| {
            matches!(
                provider.protocol,
                ProviderProtocol::GeminiOpenAI | ProviderProtocol::VertexAI
            )
        })
}

fn sanitize_tools_for_gemini(
    req: &mut openai::ChatCompletionRequest,
    warnings: &mut TranslationWarnings,
) {
    let Some(tools) = req.tools.as_mut() else {
        return;
    };

    let mut removed_strict = false;
    for tool in tools {
        if tool.function.strict.take().is_some() {
            removed_strict = true;
        }
        if let Some(params) = tool.function.parameters.take() {
            tool.function.parameters = Some(tools_map::sanitize_schema_for_gemini(params));
        }
    }
    if removed_strict {
        warnings.add("tools.function.strict");
    }
}

fn enforce_parallel_tool_calls(
    req: &mut openai::ChatCompletionRequest,
    warnings: &mut TranslationWarnings,
) {
    if req.parallel_tool_calls.is_none() {
        return;
    }

    // Gemini/Vertex's OpenAI shim does not accept parallel_tool_calls. We cannot
    // enforce parallel_tool_calls=false (the model controls parallelism), but
    // failing the request is worse than degrading: strip the field and report the
    // degradation via the warnings header rather than returning a 400.
    req.parallel_tool_calls = None;
    warnings.add("parallel_tool_calls");
}

fn enforce_tool_choice(
    req: &mut openai::ChatCompletionRequest,
    warnings: &mut TranslationWarnings,
) -> Result<(), ToolPolicyError> {
    let Some(choice) = req.tool_choice.clone() else {
        return Ok(());
    };

    match choice {
        ChatToolChoice::Simple(value) if value == "auto" => {
            req.tool_choice = None;
            warnings.add("tool_choice");
            Ok(())
        }
        ChatToolChoice::Simple(value) if value == "none" => {
            if has_active_tool_continuation(req) {
                return Err(ToolPolicyError::invalid(
                    "tool_choice=none cannot be rewritten during an active tool-call continuation",
                ));
            }
            req.tool_choice = None;
            warnings.add("tool_choice");
            if req.tools.as_ref().is_some_and(|tools| !tools.is_empty()) {
                req.tools = None;
                warnings.add("tools");
            }
            Ok(())
        }
        ChatToolChoice::Simple(value) => Err(ToolPolicyError::invalid(format!(
            "tool_choice={value} is not supported by this provider/model"
        ))),
        ChatToolChoice::Named(_) => Err(ToolPolicyError::invalid(
            "named tool_choice is not supported by this provider/model",
        )),
    }
}

fn tool_capabilities(
    catalog: &ProviderCatalog,
    provider_id: &str,
    model: &str,
) -> Option<ToolCapabilities> {
    let model_id = model_id_for_catalog(provider_id, model);
    if let Some(model_def) = catalog.get_model(provider_id, model_id.as_ref()) {
        return Some(ToolCapabilities {
            tool_use: model_def.capabilities.tool_use,
            tool_choice: model_def.capabilities.tool_choice,
        });
    }

    // If a provider has no model metadata at all, provider-level endpoint capability
    // is the best available signal. If models exist but this model is unknown,
    // do not reject solely from missing metadata.
    if catalog.list_models(provider_id).is_empty() {
        let provider = catalog.get_provider(provider_id)?;
        return Some(ToolCapabilities {
            tool_use: provider.capabilities.tool_use,
            // Provider-level tool_choice is a conservative endpoint default, not
            // per-model truth. For an unknown model on a generic OpenAI-compat
            // endpoint (vllm / lm_studio / llamafile / triton / etc.) we must not
            // hard-reject forced tool_choice from it -- let the backend decide.
            // Only a known model's per-model tool_choice (above) is authoritative.
            tool_choice: provider.capabilities.tool_choice || provider.capabilities.tool_use,
        });
    }

    None
}

fn model_id_for_catalog<'a>(provider_id: &str, model: &'a str) -> Cow<'a, str> {
    if let Some(stripped) = model.strip_prefix(&format!("{provider_id}/")) {
        Cow::Owned(stripped.to_string())
    } else {
        Cow::Borrowed(model)
    }
}

fn request_contains_tool_semantics(req: &openai::ChatCompletionRequest) -> bool {
    req.tools.as_ref().is_some_and(|tools| !tools.is_empty())
        || has_active_tool_continuation(req)
        || req.tool_choice.as_ref().is_some_and(|choice| match choice {
            ChatToolChoice::Simple(value) => value != "auto" && value != "none",
            ChatToolChoice::Named(_) => true,
        })
}

fn has_active_tool_continuation(req: &openai::ChatCompletionRequest) -> bool {
    req.messages.iter().any(|message| {
        matches!(message.role, ChatRole::Tool)
            || (matches!(message.role, ChatRole::Assistant)
                && message
                    .tool_calls
                    .as_ref()
                    .is_some_and(|calls| !calls.is_empty()))
    })
}

fn anthropic_request_contains_tool_semantics(req: &anthropic::MessageCreateRequest) -> bool {
    req.tools.as_ref().is_some_and(|tools| !tools.is_empty())
        || req.tool_choice.is_some()
        || req.messages.iter().any(|message| match &message.content {
            anthropic::Content::Text(_) => false,
            anthropic::Content::Blocks(blocks) => blocks.iter().any(|block| {
                matches!(
                    block,
                    anthropic::ContentBlock::ToolUse { .. }
                        | anthropic::ContentBlock::ToolResult { .. }
                        | anthropic::ContentBlock::ServerToolUse { .. }
                        | anthropic::ContentBlock::WebSearchToolResult { .. }
                        | anthropic::ContentBlock::WebFetchToolResult { .. }
                )
            }),
        })
}

fn anthropic_request_requires_tool_choice(req: &anthropic::MessageCreateRequest) -> bool {
    matches!(
        req.tool_choice,
        Some(anthropic::ToolChoice::Any { .. }) | Some(anthropic::ToolChoice::Tool { .. })
    )
}

#[cfg(test)]
mod tests {
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
}
