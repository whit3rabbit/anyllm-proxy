// Request direction: Anthropic -> Gemini.

use std::collections::HashMap;

use super::helpers::{build_tool_id_map, merge_consecutive_roles};
use crate::anthropic::messages as anthropic;
use crate::gemini::request as gemini;
use crate::mapping::tools_map::sanitize_schema_for_gemini;

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
