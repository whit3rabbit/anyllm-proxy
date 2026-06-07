use crate::anthropic;
use crate::mapping::{streaming_map, usage_map};
use crate::openai;
use crate::util;

/// Convert an OpenAI ChatCompletionResponse back to an Anthropic MessageResponse.
///
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/object>
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub fn openai_to_anthropic_response(
    resp: &openai::ChatCompletionResponse,
    model: &str,
) -> anthropic::MessageResponse {
    let choice = resp.choices.first();

    let mut content = Vec::new();
    let mut stop_reason = Some(anthropic::StopReason::EndTurn);

    if let Some(choice) = choice {
        stop_reason = choice
            .finish_reason
            .as_ref()
            .map(streaming_map::map_finish_reason);

        // Map reasoning_content (DeepSeek/Qwen thinking) to Anthropic thinking block.
        // Thinking blocks precede text content in Anthropic responses.
        if let Some(ref reasoning) = choice.message.reasoning_content {
            if !reasoning.is_empty() {
                content.push(anthropic::ContentBlock::Thinking {
                    thinking: reasoning.clone(),
                    signature: None,
                });
            }
        }

        // Map content
        if let Some(ref chat_content) = choice.message.content {
            match chat_content {
                openai::ChatContent::Text(text) => {
                    if !text.is_empty() {
                        content.push(anthropic::ContentBlock::Text { text: text.clone() });
                    }
                }
                openai::ChatContent::Parts(parts) => {
                    for part in parts {
                        if let openai::ChatContentPart::Text { text } = part {
                            content.push(anthropic::ContentBlock::Text { text: text.clone() });
                        }
                    }
                }
            }
        }

        // Map refusal to text block (same pattern as Responses API path)
        if let Some(ref refusal) = choice.message.refusal {
            if !refusal.is_empty() {
                content.push(anthropic::ContentBlock::Text {
                    text: super::super::format_refusal(refusal),
                });
            }
        }

        // Map tool calls with robustness for local LLMs (llama-server, ollama)
        // that may produce empty IDs, empty names, or malformed arguments.
        if let Some(ref tool_calls) = choice.message.tool_calls {
            for tc in tool_calls {
                if tc.function.name.is_empty() {
                    tracing::warn!(id = tc.id, "skipping tool call with empty function name");
                    continue;
                }
                let id = if tc.id.is_empty() {
                    let synthetic = util::ids::generate_tool_use_id();
                    tracing::warn!(
                        name = tc.function.name,
                        synthetic_id = synthetic,
                        "tool call had empty ID; generated synthetic toolu_ ID"
                    );
                    synthetic
                } else {
                    tc.id.clone()
                };
                content.push(anthropic::ContentBlock::ToolUse {
                    id,
                    name: tc.function.name.clone(),
                    input: util::json::parse_tool_arguments(&tc.function.arguments),
                });
            }
        }
    }

    let usage = resp
        .usage
        .as_ref()
        .map(usage_map::openai_to_anthropic_usage)
        .unwrap_or_default();

    anthropic::MessageResponse {
        id: util::ids::generate_message_id(),
        response_type: "message".to_string(),
        role: anthropic::Role::Assistant,
        content,
        model: model.to_string(),
        stop_reason,
        stop_sequence: None,
        usage,
        created: resp.created,
    }
}
