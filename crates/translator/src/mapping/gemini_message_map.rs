// Anthropic Messages API <-> Gemini generateContent API message mapping.
//
// Pure translation functions, no IO. Converts Anthropic request types into
// Gemini request types and Gemini response types back into Anthropic responses.

use std::collections::HashMap;

use crate::anthropic::messages as anthropic;
use crate::gemini::request as gemini;
use crate::gemini::response as gemini_resp;
use crate::mapping::tools_map::sanitize_schema_for_gemini;
use crate::util::ids::{generate_message_id, generate_tool_use_id};

// ---------------------------------------------------------------------------
// Request direction: Anthropic -> Gemini
// ---------------------------------------------------------------------------

/// Compute degradation warnings for a Gemini-bound request.
///
/// Call before translating to surface features that are silently dropped during
/// Anthropic → Gemini translation. Emit the result as an `x-anyllm-degradation`
/// response header so clients can detect lossy translations.
pub fn compute_gemini_request_warnings(
    req: &anthropic::MessageCreateRequest,
) -> crate::mapping::warnings::TranslationWarnings {
    use crate::mapping::warnings::TranslationWarnings;
    let mut w = TranslationWarnings::default();

    // Single pass: collect all per-block warning flags at once.
    let mut has_thinking = false;
    let mut has_document = false;
    let mut has_url_image = false;
    for msg in &req.messages {
        if let anthropic::Content::Blocks(blocks) = &msg.content {
            for b in blocks {
                match b {
                    // Thinking/RedactedThinking: no Gemini Content equivalent.
                    anthropic::ContentBlock::Thinking { .. }
                    | anthropic::ContentBlock::RedactedThinking { .. } => has_thinking = true,
                    // Document blocks have no Gemini equivalent.
                    anthropic::ContentBlock::Document { .. } => has_document = true,
                    // URL-type images: Gemini only accepts inline base64 data.
                    anthropic::ContentBlock::Image { source } if source.source_type != "base64" => {
                        has_url_image = true
                    }
                    _ => {}
                }
            }
        }
    }
    if has_thinking {
        w.add("thinking_blocks");
    }
    if has_document {
        w.add("document_blocks");
    }
    if has_url_image {
        w.add("url_images");
    }

    // cache_control on system blocks is dropped; Gemini has no prompt-caching API.
    if let Some(anthropic::System::Blocks(blocks)) = &req.system {
        if blocks.iter().any(|b| b.cache_control.is_some()) {
            w.add("cache_control");
        }
    }

    w
}

/// Convert an Anthropic `MessageCreateRequest` into a Gemini `GenerateContentRequest`.
///
/// Maps `thinking_config` to `generationConfig.thinkingConfig` for Gemini 2.5
/// thinking models. Drops unsupported features (thinking content blocks in prior
/// messages, document blocks, cache_control) and merges consecutive same-role
/// messages to satisfy Gemini's strict alternation requirement.
pub fn anthropic_to_gemini_request(
    req: &anthropic::MessageCreateRequest,
) -> gemini::GenerateContentRequest {
    let tool_id_map = build_tool_id_map(&req.messages);

    // System instruction
    let system_instruction = req.system.as_ref().map(|sys| {
        let text = match sys {
            anthropic::System::Text(s) => s.clone(),
            anthropic::System::Blocks(blocks) => blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        };
        gemini::Content {
            role: None,
            parts: vec![gemini::Part::text(text)],
        }
    });

    // Convert messages
    let mut contents: Vec<gemini::Content> = Vec::new();
    for msg in &req.messages {
        let role = match msg.role {
            anthropic::Role::User => "user",
            anthropic::Role::Assistant => "model",
        };
        let parts = content_blocks_to_parts(&msg.content, &tool_id_map);
        if !parts.is_empty() {
            contents.push(gemini::Content {
                role: Some(role.to_string()),
                parts,
            });
        }
    }
    contents = merge_consecutive_roles(contents);

    // Tools
    let tools = req.tools.as_ref().map(|tools| {
        vec![gemini::Tool {
            function_declarations: tools
                .iter()
                .map(|t| gemini::FunctionDeclaration {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: Some(sanitize_schema_for_gemini(t.input_schema.clone())),
                })
                .collect(),
        }]
    });

    // Tool config
    let tool_config = req.tool_choice.as_ref().map(|tc| {
        if matches!(
            tc,
            anthropic::ToolChoice::Auto {
                disable_parallel_tool_use: Some(true)
            } | anthropic::ToolChoice::Any {
                disable_parallel_tool_use: Some(true)
            }
        ) {
            tracing::warn!(
                "disable_parallel_tool_use=true is not supported by Gemini; \
                 parallel tool calls may still occur"
            );
        }
        let (mode, allowed) = match tc {
            anthropic::ToolChoice::Auto { .. } => ("AUTO", None),
            anthropic::ToolChoice::Any { .. } => ("ANY", None),
            anthropic::ToolChoice::None => ("NONE", None),
            // Gemini ANY + allowedFunctionNames restricts to a specific tool.
            anthropic::ToolChoice::Tool { name, .. } => ("ANY", Some(vec![name.clone()])),
        };
        gemini::ToolConfig {
            function_calling_config: gemini::FunctionCallingConfig {
                mode: mode.to_string(),
                allowed_function_names: allowed,
            },
        }
    });

    // Generation config
    let generation_config = {
        let thinking_config =
            if let Some(anthropic::ThinkingConfig::Enabled { budget_tokens }) = &req.thinking {
                Some(gemini::ThinkingConfig {
                    thinking_budget: *budget_tokens,
                    include_thoughts: Some(true),
                })
            } else {
                None
            };
        let gc = gemini::GenerationConfig {
            max_output_tokens: Some(req.max_tokens),
            temperature: req.temperature,
            top_p: req.top_p,
            top_k: req.top_k,
            stop_sequences: req.stop_sequences.clone(),
            thinking_config,
            ..Default::default()
        };
        Some(gc)
    };

    gemini::GenerateContentRequest {
        contents,
        system_instruction,
        generation_config,
        tools,
        tool_config,
        safety_settings: None,
    }
}

/// Build a map from Anthropic tool_use IDs to tool names.
///
/// Scans all messages for `ToolUse` blocks so that `ToolResult` translation can
/// look up the function name Gemini expects.
pub fn build_tool_id_map(messages: &[anthropic::InputMessage]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for msg in messages {
        let blocks = match &msg.content {
            anthropic::Content::Text(_) => continue,
            anthropic::Content::Blocks(b) => b,
        };
        for block in blocks {
            if let anthropic::ContentBlock::ToolUse { id, name, .. } = block {
                map.insert(id.clone(), name.clone());
            }
        }
    }
    map
}

/// Merge consecutive same-role `Content` entries by concatenating their parts.
///
/// Gemini requires strict user/model role alternation. When the Anthropic
/// conversation has two consecutive user (or model) turns, this merges them
/// into a single turn.
pub fn merge_consecutive_roles(contents: Vec<gemini::Content>) -> Vec<gemini::Content> {
    let mut merged: Vec<gemini::Content> = Vec::with_capacity(contents.len());
    for c in contents {
        if let Some(last) = merged.last_mut() {
            if last.role == c.role {
                last.parts.extend(c.parts);
                continue;
            }
        }
        merged.push(c);
    }

    // Gemini requires the first content turn to have role "user". An Anthropic
    // client may legally send an assistant-first conversation (for few-shot
    // prompting). Prepend a dummy user turn so Gemini does not return a 400.
    if merged.first().and_then(|c| c.role.as_deref()) == Some("model") {
        merged.insert(
            0,
            gemini::Content {
                role: Some("user".to_string()),
                parts: vec![gemini::Part::text(String::new())],
            },
        );
    }

    merged
}

/// Convert Anthropic message content into a vec of Gemini Parts.
fn content_blocks_to_parts(
    content: &anthropic::Content,
    tool_id_map: &HashMap<String, String>,
) -> Vec<gemini::Part> {
    match content {
        anthropic::Content::Text(s) => vec![gemini::Part::text(s.clone())],
        anthropic::Content::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| content_block_to_part(block, tool_id_map))
            .collect(),
    }
}

/// Convert a single Anthropic ContentBlock to a Gemini Part, or None if dropped.
fn content_block_to_part(
    block: &anthropic::ContentBlock,
    tool_id_map: &HashMap<String, String>,
) -> Option<gemini::Part> {
    match block {
        anthropic::ContentBlock::Text { text } => Some(gemini::Part::text(text.clone())),

        anthropic::ContentBlock::Image { source } => {
            // Gemini only supports inline base64 data, not URLs.
            if source.source_type == "base64" {
                let mime = source
                    .media_type
                    .clone()
                    .unwrap_or_else(|| "image/png".into());
                let data = source.data.clone().unwrap_or_default();
                Some(gemini::Part::inline_data(mime, data))
            } else {
                // URL-type images cannot be sent as inline_data; drop.
                None
            }
        }

        anthropic::ContentBlock::ToolUse { name, input, .. } => {
            // Strip the Anthropic tool_use id; Gemini uses name-based correlation.
            Some(gemini::Part::function_call(name.clone(), input.clone()))
        }

        anthropic::ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let Some(name) = tool_id_map.get(tool_use_id).cloned() else {
                // Gemini requires the function name to match a declared FunctionDeclaration.
                // Emitting an unknown name causes a 400; drop the result instead.
                tracing::warn!(
                    tool_use_id,
                    "dropping ToolResult: tool_use_id not found in tool_id_map"
                );
                return None;
            };

            let response_value = tool_result_to_json(content, *is_error);
            Some(gemini::Part::function_response(name, response_value))
        }

        // Thinking, RedactedThinking, Document: not supported by Gemini, drop.
        anthropic::ContentBlock::Thinking { .. }
        | anthropic::ContentBlock::RedactedThinking { .. }
        | anthropic::ContentBlock::Document { .. } => None,
        _ => None,
    }
}

/// Convert Anthropic ToolResult content into a JSON value for Gemini FunctionResponse.
fn tool_result_to_json(
    content: &Option<anthropic::ToolResultContent>,
    is_error: Option<bool>,
) -> serde_json::Value {
    let text = match content {
        Some(anthropic::ToolResultContent::Text(s)) => s.clone(),
        Some(anthropic::ToolResultContent::Blocks(blocks)) => {
            // Concatenate text blocks; other block types (e.g., images) cannot be
            // represented in Gemini FunctionResponse and are replaced with a placeholder.
            blocks
                .iter()
                .map(|b| match b {
                    anthropic::ContentBlock::Text { text } => text.clone(),
                    _ => {
                        tracing::warn!(
                            "tool_result contains non-text block; \
                             replacing with \"[non-text]\" placeholder for Gemini"
                        );
                        "[non-text]".into()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        None => String::new(),
    };

    if is_error == Some(true) {
        serde_json::json!({ "error": text })
    } else {
        serde_json::json!({ "result": text })
    }
}

// ---------------------------------------------------------------------------
// Response direction: Gemini -> Anthropic
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Input direction: Gemini CLI -> Anthropic (for accepting Gemini-format input)
// ---------------------------------------------------------------------------

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

#[cfg(test)]
mod tests;
