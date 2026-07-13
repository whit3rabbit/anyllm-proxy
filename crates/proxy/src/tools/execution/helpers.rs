//! Parsers/converters between Anthropic message types and `ToolCall`/`ToolResult`.

use super::{ToolCall, ToolResult};
use crate::tools::trace::ToolOutcome;

/// Extract ToolCall structs from an Anthropic MessageResponse.
pub fn extract_tool_calls(
    response: &anyllm_translate::anthropic::MessageResponse,
) -> Vec<ToolCall> {
    response
        .content
        .iter()
        .filter_map(|block| {
            if let anyllm_translate::anthropic::ContentBlock::ToolUse { id, name, input } = block {
                Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Convert tool execution results to an Anthropic user message with ToolResult blocks.
pub fn tool_results_to_user_message(
    results: &[ToolResult],
) -> anyllm_translate::anthropic::InputMessage {
    let blocks: Vec<anyllm_translate::anthropic::ContentBlock> = results
        .iter()
        .map(|r| {
            let (content_text, is_error) = match &r.outcome {
                ToolOutcome::Success(v) => (serde_json::to_string(v).unwrap_or_default(), false),
                ToolOutcome::Error { message, .. } => (message.clone(), true),
                ToolOutcome::Timeout => ("Tool execution timed out".to_string(), true),
            };
            anyllm_translate::anthropic::ContentBlock::ToolResult {
                tool_use_id: r.tool_use_id.clone(),
                content: Some(anyllm_translate::anthropic::ToolResultContent::Text(
                    content_text,
                )),
                is_error: Some(is_error),
            }
        })
        .collect();

    anyllm_translate::anthropic::InputMessage {
        role: anyllm_translate::anthropic::Role::User,
        content: anyllm_translate::anthropic::Content::Blocks(blocks),
    }
}

/// Convert a MessageResponse's content into an assistant InputMessage for conversation history.
pub fn response_to_assistant_message(
    response: &anyllm_translate::anthropic::MessageResponse,
) -> anyllm_translate::anthropic::InputMessage {
    anyllm_translate::anthropic::InputMessage {
        role: anyllm_translate::anthropic::Role::Assistant,
        content: anyllm_translate::anthropic::Content::Blocks(response.content.clone()),
    }
}
