// Reverse message mapping: OpenAI Chat Completions -> Anthropic Messages
//
// Converts OpenAI-format requests to Anthropic format (for accepting OpenAI
// input) and Anthropic responses back to OpenAI format.

use crate::anthropic;
use crate::error::TranslateError;
use crate::mapping::{tools_map, usage_map, warnings::TranslationWarnings};
use crate::openai;
use crate::util;
use std::collections::BTreeMap;

/// Request-local metadata needed to round-trip Anthropic-compatible tool names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnthropicTranslationContext {
    original_to_sanitized_tool_names: BTreeMap<String, String>,
    sanitized_to_original_tool_names: BTreeMap<String, String>,
}

impl AnthropicTranslationContext {
    pub fn from_openai_request(req: &openai::ChatCompletionRequest) -> Self {
        let mut ctx = Self::default();

        if let Some(tools) = &req.tools {
            for tool in tools {
                ctx.register_tool_name(&tool.function.name);
            }
        }
        if let Some(openai::ChatToolChoice::Named(named)) = &req.tool_choice {
            ctx.register_tool_name(&named.function.name);
        }
        for message in &req.messages {
            if let Some(tool_calls) = &message.tool_calls {
                for tool_call in tool_calls {
                    ctx.register_tool_name(&tool_call.function.name);
                }
            }
        }

        ctx
    }

    pub fn sanitized_tool_name(&self, name: &str) -> String {
        self.original_to_sanitized_tool_names
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    pub fn original_tool_name(&self, name: &str) -> String {
        self.sanitized_to_original_tool_names
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    fn register_tool_name(&mut self, original: &str) -> String {
        if let Some(existing) = self.original_to_sanitized_tool_names.get(original) {
            return existing.clone();
        }

        let base = basic_sanitize_anthropic_tool_name(original);
        let mut candidate = base.clone();
        let mut suffix_index = 2usize;
        while self
            .sanitized_to_original_tool_names
            .contains_key(&candidate)
        {
            let suffix = format!("_{suffix_index}");
            let keep = 128usize.saturating_sub(suffix.len());
            candidate = format!("{}{}", &base[..base.len().min(keep)], suffix);
            suffix_index += 1;
        }

        self.original_to_sanitized_tool_names
            .insert(original.to_string(), candidate.clone());
        self.sanitized_to_original_tool_names
            .insert(candidate.clone(), original.to_string());
        candidate
    }
}

fn basic_sanitize_anthropic_tool_name(original: &str) -> String {
    let mut sanitized: String = original
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(128)
        .collect();

    if sanitized.is_empty() {
        sanitized = "tool".to_string();
    }
    sanitized
}

/// Convert an OpenAI ChatCompletionRequest to an Anthropic MessageCreateRequest.
///
/// Returns an error if `max_tokens` and `max_completion_tokens` are both absent
/// (Anthropic requires `max_tokens`).
pub fn openai_to_anthropic_request(
    req: &openai::ChatCompletionRequest,
    warnings: &mut TranslationWarnings,
) -> Result<anthropic::MessageCreateRequest, TranslateError> {
    openai_to_anthropic_request_inner(req, warnings, &AnthropicTranslationContext::default())
}

/// Convert an OpenAI request and return request-local translation context.
pub fn openai_to_anthropic_request_with_context(
    req: &openai::ChatCompletionRequest,
    warnings: &mut TranslationWarnings,
) -> Result<(anthropic::MessageCreateRequest, AnthropicTranslationContext), TranslateError> {
    let context = AnthropicTranslationContext::from_openai_request(req);
    let req = openai_to_anthropic_request_inner(req, warnings, &context)?;
    Ok((req, context))
}

fn openai_to_anthropic_request_inner(
    req: &openai::ChatCompletionRequest,
    warnings: &mut TranslationWarnings,
    context: &AnthropicTranslationContext,
) -> Result<anthropic::MessageCreateRequest, TranslateError> {
    // max_tokens is required in Anthropic; reject if absent.
    // NOT A BUG: Anthropic has no server-side default for max_tokens — the field
    // is mandatory per the API spec. Injecting a silent default would mask
    // misconfigured clients. Standard OpenAI SDKs that omit max_tokens are not
    // supported by the Anthropic API regardless, so rejecting with 400 is correct.
    let max_tokens = req
        .max_completion_tokens
        .or(req.max_tokens)
        .ok_or_else(|| {
            TranslateError::MissingField("max_tokens or max_completion_tokens is required".into())
        })?;

    let mut system: Option<anthropic::System> = None;
    let mut messages = Vec::new();

    for msg in &req.messages {
        match msg.role {
            openai::ChatRole::System | openai::ChatRole::Developer => {
                // Extract system messages into the Anthropic system field.
                // Multiple system messages are concatenated.
                let text = extract_text_content(&msg.content);
                if !text.is_empty() {
                    match &mut system {
                        Some(anthropic::System::Text(existing)) => {
                            existing.push('\n');
                            existing.push_str(&text);
                        }
                        None => {
                            system = Some(anthropic::System::Text(text));
                        }
                        _ => {}
                    }
                }
            }
            openai::ChatRole::User => {
                let content = convert_openai_content_to_anthropic(&msg.content);
                messages.push(anthropic::InputMessage {
                    role: anthropic::Role::User,
                    content,
                });
            }
            openai::ChatRole::Assistant => {
                let content = convert_assistant_to_anthropic(msg, context);
                messages.push(anthropic::InputMessage {
                    role: anthropic::Role::Assistant,
                    content,
                });
            }
            openai::ChatRole::Tool => {
                // Tool role messages become Anthropic tool_result blocks
                // on a user message (Anthropic requires tool results in user turn)
                let text = extract_text_content(&msg.content);
                let tool_use_id = msg.tool_call_id.clone().unwrap_or_default();
                let content_block = anthropic::ContentBlock::ToolResult {
                    tool_use_id,
                    content: if text.is_empty() {
                        None
                    } else {
                        Some(anthropic::ToolResultContent::Text(text))
                    },
                    is_error: None,
                };
                messages.push(anthropic::InputMessage {
                    role: anthropic::Role::User,
                    content: anthropic::Content::Blocks(vec![content_block]),
                });
            }
            openai::ChatRole::Function => {
                // Deprecated function role: treat as tool
                let text = extract_text_content(&msg.content);
                let tool_use_id = msg.name.clone().unwrap_or_default();
                let content_block = anthropic::ContentBlock::ToolResult {
                    tool_use_id,
                    content: if text.is_empty() {
                        None
                    } else {
                        Some(anthropic::ToolResultContent::Text(text))
                    },
                    is_error: None,
                };
                messages.push(anthropic::InputMessage {
                    role: anthropic::Role::User,
                    content: anthropic::Content::Blocks(vec![content_block]),
                });
            }
        }
    }

    let tools = req.tools.as_ref().map(|t| {
        let mut tools = tools_map::openai_tools_to_anthropic(t);
        for tool in &mut tools {
            tool.name = context.sanitized_tool_name(&tool.name);
        }
        tools
    });

    let mut tool_choice = req
        .tool_choice
        .as_ref()
        .map(tools_map::openai_tool_choice_to_anthropic);
    if let Some(anthropic::ToolChoice::Tool { name }) = &mut tool_choice {
        *name = context.sanitized_tool_name(name);
    }

    let stop_sequences = req.stop.as_ref().map(|s| match s {
        openai::Stop::Single(s) => vec![s.clone()],
        openai::Stop::Multiple(v) => v.clone(),
    });

    let metadata = req.user.as_ref().map(|u| anthropic::Metadata {
        user_id: Some(u.clone()),
    });

    if req.presence_penalty.is_some() {
        warnings.add("presence_penalty");
    }
    if req.frequency_penalty.is_some() {
        warnings.add("frequency_penalty");
    }
    if req.response_format.is_some() {
        warnings.add("response_format");
    }
    if req.extra.contains_key("logprobs") {
        warnings.add("logprobs");
    }
    if req.extra.contains_key("n") {
        warnings.add("n");
    }
    if req.extra.contains_key("seed") {
        warnings.add("seed");
    }
    if req.stream_options.is_some() {
        warnings.add("stream_options");
    }

    let tool_choice = match (tool_choice, req.parallel_tool_calls) {
        (Some(anthropic::ToolChoice::Auto { .. }), Some(false)) => {
            Some(anthropic::ToolChoice::Auto {
                disable_parallel_tool_use: Some(true),
            })
        }
        (Some(anthropic::ToolChoice::Any { .. }), Some(false)) => {
            Some(anthropic::ToolChoice::Any {
                disable_parallel_tool_use: Some(true),
            })
        }
        (tc, _) => tc,
    };

    Ok(anthropic::MessageCreateRequest {
        model: req.model.clone(),
        max_tokens,
        messages,
        system,
        temperature: req.temperature,
        top_p: req.top_p,
        top_k: None,
        stop_sequences,
        tools,
        tool_choice,
        metadata,
        thinking: None,
        stream: req.stream,
        extra: serde_json::Map::new(),
    })
}

/// Convert an Anthropic MessageResponse to an OpenAI ChatCompletionResponse.
pub fn anthropic_to_openai_response(
    resp: &anthropic::MessageResponse,
    model: &str,
) -> openai::ChatCompletionResponse {
    anthropic_to_openai_response_with_context(resp, model, &AnthropicTranslationContext::default())
}

/// Convert an Anthropic MessageResponse using request-local translation context.
pub fn anthropic_to_openai_response_with_context(
    resp: &anthropic::MessageResponse,
    model: &str,
    context: &AnthropicTranslationContext,
) -> openai::ChatCompletionResponse {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut reasoning_content: Option<String> = None;

    for block in &resp.content {
        match block {
            anthropic::ContentBlock::Text { text } => {
                text_parts.push(text.clone());
            }
            anthropic::ContentBlock::ToolUse { id, name, input }
            | anthropic::ContentBlock::ServerToolUse { id, name, input } => {
                tool_calls.push(openai::ToolCall {
                    id: id.clone(),
                    call_type: "function".to_string(),
                    function: openai::FunctionCall {
                        name: context.original_tool_name(name),
                        arguments: util::json::value_to_json_string(input),
                    },
                });
            }
            anthropic::ContentBlock::Thinking { thinking, .. } => match &mut reasoning_content {
                Some(existing) => {
                    existing.push_str(thinking);
                }
                None => {
                    reasoning_content = Some(thinking.clone());
                }
            },
            _ => {}
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(openai::ChatContent::Text(text_parts.join("")))
    };

    let finish_reason = resp
        .stop_reason
        .as_ref()
        .map(anthropic_stop_reason_to_openai);

    let usage = usage_map::anthropic_to_openai_usage(&resp.usage);

    let id = format!("chatcmpl-{}", util::ids::generate_uuid());

    openai::ChatCompletionResponse {
        id,
        object: "chat.completion".to_string(),
        model: model.to_string(),
        choices: vec![openai::Choice {
            index: 0,
            message: openai::ChatMessage {
                role: openai::ChatRole::Assistant,
                content,
                name: None,
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
                tool_call_id: None,
                refusal: None,
                reasoning_content,
            },
            finish_reason,
            logprobs: None,
        }],
        usage: Some(usage),
        created: resp.created,
        system_fingerprint: None,
        service_tier: None,
    }
}

/// Map Anthropic stop_reason to OpenAI finish_reason.
pub fn anthropic_stop_reason_to_openai(
    stop_reason: &anthropic::StopReason,
) -> openai::FinishReason {
    match stop_reason {
        anthropic::StopReason::EndTurn => openai::FinishReason::Stop,
        anthropic::StopReason::MaxTokens => openai::FinishReason::Length,
        anthropic::StopReason::ToolUse => openai::FinishReason::ToolCalls,
        anthropic::StopReason::StopSequence => openai::FinishReason::Stop,
        anthropic::StopReason::PauseTurn => openai::FinishReason::Stop,
        anthropic::StopReason::Refusal => openai::FinishReason::ContentFilter,
        anthropic::StopReason::Unknown => openai::FinishReason::Unknown,
    }
}

/// Compute warnings for an OpenAI request about features that will be dropped.
pub fn compute_openai_request_warnings(req: &openai::ChatCompletionRequest) -> TranslationWarnings {
    let mut w = TranslationWarnings::default();
    openai_to_anthropic_request(req, &mut w).ok();
    w
}

// --- Helper functions ---

fn extract_text_content(content: &Option<openai::ChatContent>) -> String {
    match content {
        Some(openai::ChatContent::Text(s)) => s.clone(),
        Some(openai::ChatContent::Parts(parts)) => {
            let mut had_non_text = false;
            let text = parts
                .iter()
                .filter_map(|p| match p {
                    openai::ChatContentPart::Text { text } => Some(text.as_str()),
                    _ => {
                        had_non_text = true;
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("");
            if had_non_text {
                tracing::warn!(
                    "message contains non-text content parts (image/file); \
                     only text parts are extracted as plain text"
                );
            }
            text
        }
        None => String::new(),
    }
}

fn convert_openai_content_to_anthropic(
    content: &Option<openai::ChatContent>,
) -> anthropic::Content {
    match content {
        Some(openai::ChatContent::Text(s)) => anthropic::Content::Text(s.clone()),
        Some(openai::ChatContent::Parts(parts)) => {
            let mut blocks = Vec::new();
            for part in parts {
                match part {
                    openai::ChatContentPart::Text { text } => {
                        blocks.push(anthropic::ContentBlock::Text { text: text.clone() });
                    }
                    openai::ChatContentPart::ImageUrl { image_url } => {
                        // Parse data URIs back to base64 + media_type
                        let source = url_to_image_source(&image_url.url);
                        blocks.push(anthropic::ContentBlock::Image { source });
                    }
                    // InputAudio and File have no Anthropic equivalent; drop them
                    _ => {}
                }
            }
            if blocks.is_empty() {
                anthropic::Content::Text(String::new())
            } else {
                anthropic::Content::Blocks(blocks)
            }
        }
        None => anthropic::Content::Text(String::new()),
    }
}

fn convert_assistant_to_anthropic(
    msg: &openai::ChatMessage,
    context: &AnthropicTranslationContext,
) -> anthropic::Content {
    let mut blocks = Vec::new();

    // Map reasoning_content to thinking block.
    // signature is always None because OpenAI does not emit Anthropic-style
    // cryptographic signatures. Anthropic will reject thinking blocks passed
    // back in tool-result continuations without a valid signature — callers
    // must strip thinking blocks from history when using the reverse path.
    if let Some(ref reasoning) = msg.reasoning_content {
        if !reasoning.is_empty() {
            blocks.push(anthropic::ContentBlock::Thinking {
                thinking: reasoning.clone(),
                signature: None,
            });
        }
    }

    // Map text content
    match &msg.content {
        Some(openai::ChatContent::Text(text)) if !text.is_empty() => {
            blocks.push(anthropic::ContentBlock::Text { text: text.clone() });
        }
        Some(openai::ChatContent::Text(_)) => {}
        Some(openai::ChatContent::Parts(parts)) => {
            for part in parts {
                if let openai::ChatContentPart::Text { text } = part {
                    blocks.push(anthropic::ContentBlock::Text { text: text.clone() });
                }
            }
        }
        None => {}
    }

    // Map tool calls to tool_use blocks
    if let Some(ref tool_calls) = msg.tool_calls {
        for tc in tool_calls {
            blocks.push(anthropic::ContentBlock::ToolUse {
                id: tc.id.clone(),
                name: context.sanitized_tool_name(&tc.function.name),
                input: util::json::parse_tool_arguments(&tc.function.arguments),
            });
        }
    }

    if blocks.is_empty() {
        anthropic::Content::Text(String::new())
    } else if blocks.len() == 1 {
        if let anthropic::ContentBlock::Text { ref text } = blocks[0] {
            return anthropic::Content::Text(text.clone());
        }
        anthropic::Content::Blocks(blocks)
    } else {
        anthropic::Content::Blocks(blocks)
    }
}

/// Parse a URL string into an Anthropic ImageSource.
/// Handles both data URIs (data:image/png;base64,...) and regular URLs.
fn url_to_image_source(url: &str) -> anthropic::ImageSource {
    if let Some(rest) = url.strip_prefix("data:") {
        // Parse data URI: data:media_type;base64,data
        if let Some((meta, data)) = rest.split_once(',') {
            let media_type = meta.strip_suffix(";base64").unwrap_or(meta);
            return anthropic::ImageSource {
                source_type: "base64".to_string(),
                media_type: Some(media_type.to_string()),
                data: Some(data.to_string()),
                url: None,
            };
        }
    }
    // Regular URL
    anthropic::ImageSource {
        source_type: "url".to_string(),
        media_type: None,
        data: None,
        url: Some(url.to_string()),
    }
}

#[cfg(test)]
mod tests;
