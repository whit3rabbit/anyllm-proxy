// Phase 22: Anthropic <-> OpenAI Responses API message mapping
//
// Pure translation between Anthropic Messages API and OpenAI Responses API.
// The Responses API uses `input` (text or items) instead of `messages[]`,
// `instructions` instead of system messages, and `output[]` instead of `choices[]`.

use crate::anthropic;
use crate::mapping::message_map::extract_system_text;
use crate::openai::responses::{ResponsesInput, ResponsesRequest, ResponsesResponse};
use crate::util;
use serde_json::{json, Value};

/// Convert an Anthropic MessageCreateRequest to an OpenAI Responses API request.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
/// OpenAI Responses: <https://platform.openai.com/docs/api-reference/responses/create>
pub fn anthropic_to_responses_request(req: &anthropic::MessageCreateRequest) -> ResponsesRequest {
    let instructions = req.system.as_ref().map(extract_system_text);

    let input = build_input_items(&req.messages);

    let tools = req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| {
                let mut tool = json!({
                    "type": "function",
                    "name": t.name,
                    "parameters": t.input_schema,
                    "strict": false,
                });
                if let Some(ref desc) = t.description {
                    tool["description"] = json!(desc);
                }
                tool
            })
            .collect()
    });

    if req.top_k.is_some() {
        tracing::warn!("top_k parameter dropped: no OpenAI equivalent");
    }
    if req.thinking.is_some() {
        tracing::warn!("thinking config stripped: no OpenAI equivalent");
    }
    if req.metadata.is_some() {
        tracing::warn!("metadata dropped: no direct OpenAI Responses API equivalent");
    }

    let mut extra = serde_json::Map::new();
    if let Some(top_p) = req.top_p {
        extra.insert("top_p".into(), json!(top_p));
    }
    if let Some(ref tc) = req.tool_choice {
        let mapped = match tc {
            anthropic::ToolChoice::Auto { .. } => json!("auto"),
            anthropic::ToolChoice::Any { .. } => json!("required"),
            anthropic::ToolChoice::None => json!("none"),
            anthropic::ToolChoice::Tool { name } => json!({
                "type": "function",
                "name": name,
            }),
        };
        extra.insert("tool_choice".into(), mapped);
    }

    if let Some(ref seqs) = req.stop_sequences {
        if seqs.len() > 4 {
            tracing::warn!(
                count = seqs.len(),
                "stop_sequences truncated from {} to 4 (OpenAI limit)",
                seqs.len()
            );
        }
        let capped: Vec<&str> = seqs.iter().take(4).map(|s| s.as_str()).collect();
        extra.insert("stop".into(), json!(capped));
    }

    ResponsesRequest {
        model: req.model.clone(),
        input,
        instructions,
        max_output_tokens: Some(req.max_tokens),
        temperature: req.temperature.map(|t| t.clamp(0.0, 1.0)),
        tools,
        stream: req.stream,
        extra,
    }
}

/// Build Responses API input items from Anthropic messages.
///
/// The Responses API models tool calls as first-class items in the input
/// array, not as message content blocks. Anthropic tool_use blocks become
/// function_call items at the root level; tool_result blocks become
/// function_call_output items. This flattened structure is required by the
/// Responses API schema.
fn build_input_items(messages: &[anthropic::InputMessage]) -> ResponsesInput {
    let mut items: Vec<Value> = Vec::new();

    for msg in messages {
        let role = match msg.role {
            anthropic::Role::User => "user",
            anthropic::Role::Assistant => "assistant",
        };

        match &msg.content {
            anthropic::Content::Text(text) => {
                items.push(json!({
                    "type": "message",
                    "role": role,
                    "content": [{"type": "input_text", "text": text}],
                }));
            }
            anthropic::Content::Blocks(blocks) => {
                convert_blocks_to_items(blocks, role, &mut items);
            }
        }
    }

    ResponsesInput::Items(items)
}

/// Convert Anthropic content blocks into Responses API input items.
///
/// Text/image blocks are grouped into a message item.
/// Tool results become separate `function_call_output` items.
/// Tool use blocks (in assistant messages) become `function_call` items.
fn convert_blocks_to_items(blocks: &[anthropic::ContentBlock], role: &str, items: &mut Vec<Value>) {
    let mut content_parts: Vec<Value> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut tool_results: Vec<Value> = Vec::new();

    for block in blocks {
        match block {
            anthropic::ContentBlock::Text { text } => {
                content_parts.push(json!({"type": "input_text", "text": text}));
            }
            anthropic::ContentBlock::Image { source } => {
                if let Some(ref url) = source.url {
                    content_parts.push(json!({
                        "type": "input_image",
                        "image_url": url,
                    }));
                } else if let Some(ref data) = source.data {
                    let mt = source.media_type.as_deref().unwrap_or("image/png");
                    content_parts.push(json!({
                        "type": "input_image",
                        "image_url": format!("data:{};base64,{}", mt, data),
                    }));
                }
            }
            anthropic::ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(json!({
                    "type": "function_call",
                    "call_id": id,
                    "name": name,
                    "arguments": util::json::value_to_json_string(input),
                }));
            }
            anthropic::ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => {
                let output = tool_result_to_string(content.as_ref());
                tool_results.push(json!({
                    "type": "function_call_output",
                    "call_id": tool_use_id,
                    "output": output,
                }));
            }
            anthropic::ContentBlock::Document { .. } => {
                tracing::warn!("document block degraded to text note in Responses API translation");
                content_parts.push(json!({
                    "type": "input_text",
                    "text": "[Document content not supported in translation]",
                }));
            }
            anthropic::ContentBlock::Thinking { .. }
            | anthropic::ContentBlock::RedactedThinking { .. } => {
                // Silently dropped, same as Chat Completions path
            }
            _ => {}
        }
    }

    if !content_parts.is_empty() {
        items.push(json!({
            "type": "message",
            "role": role,
            "content": content_parts,
        }));
    }

    items.extend(tool_calls);
    items.extend(tool_results);
}

/// Extract text from an Anthropic tool result content.
fn tool_result_to_string(content: Option<&anthropic::messages::ToolResultContent>) -> String {
    match content {
        None => String::new(),
        Some(anthropic::messages::ToolResultContent::Text(t)) => t.clone(),
        Some(anthropic::messages::ToolResultContent::Blocks(blocks)) => {
            let mut parts = Vec::new();
            for b in blocks {
                if let anthropic::ContentBlock::Text { text } = b {
                    parts.push(text.as_str());
                }
            }
            parts.join("\n")
        }
    }
}

/// Convert an OpenAI Responses API response to an Anthropic MessageResponse.
///
/// OpenAI Responses: <https://platform.openai.com/docs/api-reference/responses/object>
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub fn responses_to_anthropic_response(
    resp: &ResponsesResponse,
    original_model: &str,
) -> anthropic::MessageResponse {
    let mut content = Vec::new();

    let stop_reason = match resp.status.as_str() {
        "completed" => Some(anthropic::StopReason::EndTurn),
        "incomplete" => Some(anthropic::StopReason::MaxTokens),
        "failed" => Some(anthropic::StopReason::EndTurn),
        other => {
            tracing::warn!(
                status = other,
                "unknown Responses API status, defaulting to end_turn"
            );
            Some(anthropic::StopReason::EndTurn)
        }
    };

    for item in &resp.output {
        extract_output_item(item, &mut content);
    }

    if content.is_empty() {
        content.push(anthropic::ContentBlock::Text {
            text: String::new(),
        });
    }

    let usage = resp
        .usage
        .as_ref()
        .map_or_else(anthropic::Usage::default, |u| {
            let cache_read_input_tokens =
                super::usage_map::extract_cached_tokens(u.input_token_details.as_ref());
            anthropic::Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_creation_input_tokens: None,
                cache_read_input_tokens,
                ..Default::default()
            }
        });

    anthropic::MessageResponse {
        id: util::ids::generate_message_id(),
        response_type: "message".to_string(),
        role: anthropic::Role::Assistant,
        content,
        model: original_model.to_string(),
        stop_reason,
        stop_sequence: None,
        usage,
        created: None,
    }
}

/// Extract content blocks from a single output item (JSON value).
fn extract_output_item(item: &Value, content: &mut Vec<anthropic::ContentBlock>) {
    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match item_type {
        "message" => {
            // OutputMessage: has content[] array
            if let Some(parts) = item.get("content").and_then(|v| v.as_array()) {
                for part in parts {
                    let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match part_type {
                        "output_text" => {
                            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                content.push(anthropic::ContentBlock::Text {
                                    text: text.to_string(),
                                });
                            }
                        }
                        "refusal" => {
                            if let Some(refusal) = part.get("refusal").and_then(|v| v.as_str()) {
                                content.push(anthropic::ContentBlock::Text {
                                    text: super::format_refusal(refusal),
                                });
                            }
                        }
                        _ => {
                            tracing::debug!(
                                part_type = part_type,
                                "unknown output content part type, skipped"
                            );
                        }
                    }
                }
            }
        }
        "function_call" => {
            // FunctionToolCall output item -> tool_use content block
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = item
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");

            if name.is_empty() {
                tracing::warn!("skipping function_call output with empty name");
                return;
            }

            let id = if call_id.is_empty() {
                let synthetic = util::ids::generate_tool_use_id();
                tracing::warn!(
                    name = name,
                    synthetic_id = synthetic,
                    "function_call had empty call_id; generated synthetic toolu_ ID"
                );
                synthetic
            } else {
                call_id.to_string()
            };

            let input = util::json::parse_tool_arguments(arguments);

            content.push(anthropic::ContentBlock::ToolUse {
                id,
                name: name.to_string(),
                input,
            });
        }
        _ => {
            tracing::debug!(item_type = item_type, "unknown output item type, skipped");
        }
    }
}

#[cfg(test)]
mod tests;
