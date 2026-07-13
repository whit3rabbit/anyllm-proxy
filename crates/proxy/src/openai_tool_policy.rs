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
mod tests;
