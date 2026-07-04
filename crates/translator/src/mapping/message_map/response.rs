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
    if resp.choices.is_empty() {
        tracing::warn!(
            "openai_to_anthropic_response: response has no choices; \
             the translated MessageResponse will have empty content. \
             This may indicate a misconfigured backend or wrong API format."
        );
    }
    let choice = resp.choices.first();

    let mut content = Vec::new();
    let mut stop_reason = Some(anthropic::StopReason::EndTurn);

    if let Some(choice) = choice {
        stop_reason = choice
            .finish_reason
            .as_ref()
            .map(streaming_map::map_finish_reason);

        // Prefer exact LiteLLM/Anthropic thinking blocks when present. The text-only
        // fallback has no signature and is unsafe for tool-result continuations.
        // Only treat thinking_blocks as authoritative if they actually yield a
        // block; an empty or all-`Unknown` array must not suppress reasoning_content.
        let mut pushed_thinking = false;
        if let Some(ref thinking_blocks) = choice.message.thinking_blocks {
            for block in thinking_blocks {
                if let Some(block) = super::super::openai_thinking_block_to_anthropic(block) {
                    content.push(block);
                    pushed_thinking = true;
                }
            }
        }
        // NOTE: reverse_message_map::convert_assistant_to_anthropic has a
        // similar-looking fallback but additionally suppresses unsigned
        // reasoning_content when tool_calls are present -- that guard exists
        // because that function's output gets REPLAYED to a real Anthropic
        // backend, which rejects unsigned thinking blocks in tool-result
        // continuations. This function instead builds a response handed TO
        // the client from a non-Anthropic backend; nothing here re-validates
        // a signature, so there is no unsafe replay to guard against, and
        // suppressing it would just silently drop the model's reasoning.
        if !pushed_thinking {
            if let Some(ref reasoning) = choice.message.reasoning_content {
                if !reasoning.is_empty() {
                    content.push(anthropic::ContentBlock::Thinking {
                        thinking: reasoning.clone(),
                        signature: None,
                    });
                }
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
