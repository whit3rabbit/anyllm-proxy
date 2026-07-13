// Anthropic Messages response -> OpenAI Chat Completions response.

use super::context::AnthropicTranslationContext;
use crate::anthropic;
use crate::mapping::usage_map;
use crate::openai;
use crate::util;

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
    let mut thinking_blocks = Vec::new();

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
            anthropic::ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                thinking_blocks.push(openai::ThinkingBlock::Thinking {
                    thinking: thinking.clone(),
                    signature: signature.clone(),
                });
                match &mut reasoning_content {
                    Some(existing) => {
                        existing.push_str(thinking);
                    }
                    None => {
                        reasoning_content = Some(thinking.clone());
                    }
                }
            }
            anthropic::ContentBlock::RedactedThinking { data } => {
                thinking_blocks
                    .push(openai::ThinkingBlock::RedactedThinking { data: data.clone() });
            }
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
                thinking_blocks: if thinking_blocks.is_empty() {
                    None
                } else {
                    Some(thinking_blocks)
                },
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
