// Response direction: Gemini -> Anthropic.

use crate::anthropic::messages as anthropic;
use crate::gemini::request as gemini;
use crate::gemini::response as gemini_resp;
use crate::util::ids::{generate_message_id, generate_tool_use_id};

/// Convert a Gemini `GenerateContentResponse` into an Anthropic `MessageResponse`.
///
/// Uses only the first candidate. Synthesizes Anthropic-format tool IDs for any
/// function calls.
pub fn gemini_to_anthropic_response(
    resp: &gemini_resp::GenerateContentResponse,
    model: &str,
) -> anthropic::MessageResponse {
    let candidate = resp.candidates.first();

    let content = candidate
        .map(|c| {
            c.content
                .parts
                .iter()
                .filter_map(gemini_part_to_content_block)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let has_function_call = content
        .iter()
        .any(|b| matches!(b, anthropic::ContentBlock::ToolUse { .. }));

    let stop_reason = candidate
        .and_then(|c| c.finish_reason.as_ref())
        .map(|fr| match fr {
            gemini_resp::FinishReason::STOP if has_function_call => anthropic::StopReason::ToolUse,
            gemini_resp::FinishReason::STOP => anthropic::StopReason::EndTurn,
            gemini_resp::FinishReason::MAX_TOKENS => anthropic::StopReason::MaxTokens,
            // SAFETY, RECITATION, LANGUAGE, OTHER, Unknown all map to EndTurn.
            _ => anthropic::StopReason::EndTurn,
        })
        // No finish_reason at all (e.g. empty candidates) -> EndTurn.
        .or(if candidate.is_some() {
            Some(anthropic::StopReason::EndTurn)
        } else {
            None
        });

    let usage = resp
        .usage_metadata
        .as_ref()
        .map(|u| anthropic::Usage {
            input_tokens: u.prompt_token_count,
            output_tokens: u.candidates_token_count,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            ..Default::default()
        })
        .unwrap_or_default();

    anthropic::MessageResponse {
        id: generate_message_id(),
        response_type: "message".into(),
        role: anthropic::Role::Assistant,
        content,
        model: model.to_string(),
        stop_reason,
        stop_sequence: None,
        usage,
        created: None,
    }
}

/// Convert a single Gemini Part to an Anthropic ContentBlock, or None if not mappable.
fn gemini_part_to_content_block(part: &gemini::Part) -> Option<anthropic::ContentBlock> {
    // Thought parts from thinking models map to Anthropic thinking blocks.
    if part.thought == Some(true) {
        return part
            .text
            .as_ref()
            .map(|text| anthropic::ContentBlock::Thinking {
                thinking: text.clone(),
                signature: None,
            });
    }
    if let Some(text) = &part.text {
        return Some(anthropic::ContentBlock::Text { text: text.clone() });
    }
    if let Some(fc) = &part.function_call {
        return Some(anthropic::ContentBlock::ToolUse {
            id: generate_tool_use_id(),
            name: fc.name.clone(),
            input: fc.args.clone(),
        });
    }
    // inline_data, file_data, function_response: not expected in model output,
    // or have no Anthropic equivalent. Drop.
    None
}
