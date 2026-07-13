// Reverse directions:
//   - Gemini CLI request -> Anthropic request (for accepting Gemini-format input)
//   - Anthropic response -> Gemini response (for returning Gemini-format output)

use std::collections::HashMap;

use crate::anthropic::messages as anthropic;
use crate::gemini::request as gemini;
use crate::gemini::response as gemini_resp;
use crate::util::ids::generate_tool_use_id;

/// Convert a Gemini CLI `GenerateContentRequest` into an Anthropic `MessageCreateRequest`.
///
/// `model` is the model name extracted from the URL path (e.g. `gemini-2.5-pro` from
/// `POST /v1beta/models/gemini-2.5-pro:generateContent`). All generation config fields
/// map directly; unsupported Gemini features (safety settings, response_schema) are dropped.
pub fn gemini_to_anthropic_request(
    req: &gemini::GenerateContentRequest,
    model: &str,
) -> anthropic::MessageCreateRequest {
    // Build name->id map so that function_response parts reference the same
    // synthetic tool_use_id as the corresponding function_call part.
    let mut name_to_id: HashMap<String, String> = HashMap::new();
    for content in &req.contents {
        for part in &content.parts {
            if let Some(ref fc) = part.function_call {
                name_to_id
                    .entry(fc.name.clone())
                    .or_insert_with(generate_tool_use_id);
            }
        }
    }

    // System instruction -> system
    let system = req.system_instruction.as_ref().map(|si| {
        let text = si
            .parts
            .iter()
            .filter_map(|p| p.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");
        anthropic::System::Text(text)
    });

    // Contents -> messages
    let messages: Vec<anthropic::InputMessage> = req
        .contents
        .iter()
        .filter_map(|c| gemini_content_to_input_message(c, &name_to_id))
        .collect();

    // Generation config
    let gc = req.generation_config.as_ref();
    let max_tokens = gc.and_then(|g| g.max_output_tokens).unwrap_or(8192);
    let temperature = gc.and_then(|g| g.temperature);
    let top_p = gc.and_then(|g| g.top_p);
    let top_k = gc.and_then(|g| g.top_k);
    let stop_sequences = gc
        .and_then(|g| g.stop_sequences.clone())
        .filter(|v| !v.is_empty());

    // Tools
    let tools = req.tools.as_ref().map(|ts| {
        ts.iter()
            .flat_map(|t| t.function_declarations.iter())
            .map(|fd| anthropic::Tool {
                name: fd.name.clone(),
                description: fd.description.clone(),
                input_schema: fd
                    .parameters
                    .clone()
                    .unwrap_or_else(|| serde_json::json!({"type": "object"})),
            })
            .collect::<Vec<_>>()
    });

    // Tool choice
    let tool_choice = req.tool_config.as_ref().map(|tc| {
        match tc.function_calling_config.mode.as_str() {
            "NONE" => anthropic::ToolChoice::None,
            "ANY" => match tc.function_calling_config.allowed_function_names.as_deref() {
                Some([name]) => anthropic::ToolChoice::Tool { name: name.clone() },
                _ => anthropic::ToolChoice::Any {
                    disable_parallel_tool_use: None,
                },
            },
            // AUTO or anything else
            _ => anthropic::ToolChoice::Auto {
                disable_parallel_tool_use: None,
            },
        }
    });

    anthropic::MessageCreateRequest {
        model: model.to_string(),
        max_tokens,
        messages,
        system,
        temperature,
        top_p,
        top_k,
        stop_sequences,
        tools,
        tool_choice,
        metadata: None,
        thinking: None,
        stream: None,
        extra: Default::default(),
    }
}

/// Convert a single Gemini `Content` turn into an Anthropic `InputMessage`.
/// Returns `None` if the turn produces no content blocks (e.g. all parts were dropped).
fn gemini_content_to_input_message(
    content: &gemini::Content,
    name_to_id: &HashMap<String, String>,
) -> Option<anthropic::InputMessage> {
    let role = match content.role.as_deref() {
        Some("model") => anthropic::Role::Assistant,
        // "user", None, or anything unrecognised -> user.
        _ => anthropic::Role::User,
    };
    let blocks: Vec<anthropic::ContentBlock> = content
        .parts
        .iter()
        .filter_map(|p| gemini_input_part_to_block(p, name_to_id))
        .collect();
    if blocks.is_empty() {
        return None;
    }
    Some(anthropic::InputMessage {
        role,
        content: anthropic::Content::Blocks(blocks),
    })
}

/// Convert a single Gemini `Part` from a user/model message into an Anthropic `ContentBlock`.
fn gemini_input_part_to_block(
    part: &gemini::Part,
    name_to_id: &HashMap<String, String>,
) -> Option<anthropic::ContentBlock> {
    if let Some(ref text) = part.text {
        return Some(anthropic::ContentBlock::Text { text: text.clone() });
    }
    if let Some(ref fc) = part.function_call {
        let id = name_to_id
            .get(&fc.name)
            .cloned()
            .unwrap_or_else(generate_tool_use_id);
        return Some(anthropic::ContentBlock::ToolUse {
            id,
            name: fc.name.clone(),
            input: fc.args.clone(),
        });
    }
    if let Some(ref fr) = part.function_response {
        let tool_use_id = name_to_id
            .get(&fr.name)
            .cloned()
            .unwrap_or_else(generate_tool_use_id);
        return Some(anthropic::ContentBlock::ToolResult {
            tool_use_id,
            content: Some(anthropic::ToolResultContent::Text(
                serde_json::to_string(&fr.response).unwrap_or_default(),
            )),
            is_error: None,
        });
    }
    if let Some(ref data) = part.inline_data {
        if data.mime_type.starts_with("image/") {
            return Some(anthropic::ContentBlock::Image {
                source: anthropic::ImageSource {
                    source_type: "base64".to_string(),
                    media_type: Some(data.mime_type.clone()),
                    data: Some(data.data.clone()),
                    url: None,
                },
            });
        }
        // Non-image inline data (audio, video, etc.) has no Anthropic equivalent — drop.
        tracing::warn!(
            mime_type = %data.mime_type,
            "dropping inline_data part with non-image mime type (no Anthropic equivalent)"
        );
    }
    None
}

/// Convert an Anthropic `MessageResponse` into a Gemini `GenerateContentResponse`.
///
/// Used when the proxy accepts Gemini CLI input and must return Gemini-format output.
/// This is the inverse of `gemini_to_anthropic_response`.
pub fn anthropic_to_gemini_response(
    resp: &anthropic::MessageResponse,
) -> gemini_resp::GenerateContentResponse {
    let parts: Vec<gemini::Part> = resp
        .content
        .iter()
        .filter_map(|block| match block {
            anthropic::ContentBlock::Text { text } => Some(gemini::Part::text(text.clone())),
            anthropic::ContentBlock::ToolUse { name, input, .. } => {
                Some(gemini::Part::function_call(name.clone(), input.clone()))
            }
            anthropic::ContentBlock::Thinking { thinking, .. } => Some(gemini::Part {
                thought: Some(true),
                text: Some(thinking.clone()),
                ..Default::default()
            }),
            // ToolResult, Image, Document, RedactedThinking: not expected in model output.
            _ => None,
        })
        .collect();

    let finish_reason = resp.stop_reason.as_ref().map(|sr| match sr {
        anthropic::StopReason::EndTurn | anthropic::StopReason::ToolUse => {
            gemini_resp::FinishReason::STOP
        }
        anthropic::StopReason::MaxTokens => gemini_resp::FinishReason::MAX_TOKENS,
        anthropic::StopReason::StopSequence => gemini_resp::FinishReason::STOP,
        _ => gemini_resp::FinishReason::STOP,
    });

    let candidate = gemini_resp::Candidate {
        content: gemini::Content {
            role: Some("model".to_string()),
            parts,
        },
        finish_reason,
        safety_ratings: None,
    };

    gemini_resp::GenerateContentResponse {
        candidates: vec![candidate],
        usage_metadata: Some(gemini_resp::UsageMetadata {
            prompt_token_count: resp.usage.input_tokens,
            candidates_token_count: resp.usage.output_tokens,
            total_token_count: resp.usage.input_tokens + resp.usage.output_tokens,
            cached_content_token_count: 0,
        }),
        model_version: Some(resp.model.clone()),
    }
}
