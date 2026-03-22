// Phase 20e: Anthropic <-> Gemini native message mapping (TASKS.md Phase 20e)
//
// Stateless conversion between Anthropic Messages API and Gemini
// generateContent API. No IO, no async.

use std::collections::HashMap;

use serde_json::json;

use crate::anthropic;
use crate::gemini::generate_content::{
    Candidate, Content, FinishReason, GeminiRole, GenerateContentRequest, GenerateContentResponse,
    GenerationConfig, InlineData, Part,
};
use crate::gemini::generate_content::{FunctionCallData, FunctionResponseData};
use crate::gemini::tools::{
    FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration, Tool, ToolConfig,
};
use crate::mapping::gemini_schema_map::{clean_gemini_schema, enforce_function_limit};
use crate::mapping::message_map;
use crate::util;

/// Convert an Anthropic MessageCreateRequest to a Gemini GenerateContentRequest.
pub fn anthropic_to_gemini_request(
    req: &anthropic::MessageCreateRequest,
) -> GenerateContentRequest {
    // System prompt -> system_instruction
    let system_instruction = req.system.as_ref().map(|sys| {
        let text = message_map::extract_system_text(sys);
        Content {
            role: None,
            parts: vec![Part::Text { text }],
        }
    });

    // Build tool_use_id -> function_name lookup for ToolResult mapping
    let tool_name_map = build_tool_name_map(&req.messages);

    // Messages -> contents with role alternation merging
    let contents = convert_messages(&req.messages, &tool_name_map);

    // Generation config
    let generation_config = Some(GenerationConfig {
        max_output_tokens: Some(req.max_tokens),
        temperature: req.temperature,
        top_p: req.top_p,
        top_k: req.top_k,
        stop_sequences: req.stop_sequences.clone(),
        candidate_count: None,
        seed: None,
        presence_penalty: None,
        frequency_penalty: None,
        response_mime_type: None,
        response_schema: None,
        extra: serde_json::Map::new(),
    });

    // Tools
    let tools = req.tools.as_ref().map(|anthropic_tools| {
        let decls: Vec<FunctionDeclaration> = anthropic_tools
            .iter()
            .map(|t| FunctionDeclaration {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: Some(clean_gemini_schema(&t.input_schema)),
            })
            .collect();
        let decls = enforce_function_limit(decls, super::gemini_schema_map::DEFAULT_FUNCTION_LIMIT);
        vec![Tool {
            function_declarations: Some(decls),
        }]
    });

    // Tool choice
    let tool_config = req.tool_choice.as_ref().map(|tc| {
        let (mode, allowed) = match tc {
            anthropic::ToolChoice::Auto => (FunctionCallingMode::Auto, None),
            anthropic::ToolChoice::Any => (FunctionCallingMode::Any, None),
            anthropic::ToolChoice::None => (FunctionCallingMode::None, None),
            anthropic::ToolChoice::Tool { name } => {
                (FunctionCallingMode::Any, Some(vec![name.clone()]))
            }
        };
        ToolConfig {
            function_calling_config: Some(FunctionCallingConfig {
                mode: Some(mode),
                allowed_function_names: allowed,
            }),
        }
    });

    // Warn on dropped fields
    if req.metadata.is_some() {
        tracing::warn!("metadata dropped: no Gemini equivalent");
    }
    if req.thinking.is_some() {
        tracing::warn!("thinking config dropped: no Gemini equivalent");
    }

    GenerateContentRequest {
        contents,
        system_instruction,
        tools,
        tool_config,
        generation_config,
        safety_settings: None,
        extra: serde_json::Map::new(),
    }
}

/// Convert a Gemini GenerateContentResponse to an Anthropic MessageResponse.
pub fn gemini_to_anthropic_response(
    resp: &GenerateContentResponse,
    model: &str,
) -> anthropic::MessageResponse {
    // Handle safety-blocked (no candidates)
    let candidate = resp.candidates.as_ref().and_then(|c| c.first());

    if candidate.is_none() {
        if let Some(ref feedback) = resp.prompt_feedback {
            if let Some(ref reason) = feedback.block_reason {
                tracing::warn!(reason, "Prompt blocked by Gemini safety filter");
            }
        }
        return anthropic::MessageResponse {
            id: util::ids::generate_message_id(),
            response_type: "message".to_string(),
            role: anthropic::Role::Assistant,
            content: vec![],
            model: model.to_string(),
            stop_reason: Some(anthropic::StopReason::EndTurn),
            stop_sequence: None,
            usage: anthropic::Usage::default(),
            created: None,
        };
    }
    let candidate = candidate.unwrap();

    // Convert parts -> content blocks
    let content = convert_parts_to_content_blocks(candidate);
    let has_tool_use = content
        .iter()
        .any(|b| matches!(b, anthropic::ContentBlock::ToolUse { .. }));

    // Finish reason
    let stop_reason = if has_tool_use {
        Some(anthropic::StopReason::ToolUse)
    } else {
        Some(map_finish_reason(candidate.finish_reason.as_ref()))
    };

    // Usage
    let usage = resp
        .usage_metadata
        .as_ref()
        .map(|u| anthropic::Usage {
            input_tokens: u.prompt_token_count.unwrap_or(0),
            output_tokens: u.candidates_token_count.unwrap_or(0),
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        })
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
        created: None,
    }
}

// -- internal helpers --

/// Build a map from tool_use_id to function name by scanning all messages.
/// Gemini's FunctionResponse.name requires the actual function name, but
/// Anthropic's ToolResult only carries tool_use_id (an opaque ID).
fn build_tool_name_map(messages: &[anthropic::InputMessage]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for msg in messages {
        if let anthropic::Content::Blocks(blocks) = &msg.content {
            for block in blocks {
                if let anthropic::ContentBlock::ToolUse { id, name, .. } = block {
                    map.insert(id.clone(), name.clone());
                }
            }
        }
    }
    map
}

/// Convert Anthropic messages to Gemini contents, merging consecutive same-role turns.
fn convert_messages(
    messages: &[anthropic::InputMessage],
    tool_name_map: &HashMap<String, String>,
) -> Vec<Content> {
    let mut contents: Vec<Content> = Vec::new();

    for msg in messages {
        let role = match msg.role {
            anthropic::Role::User => GeminiRole::User,
            anthropic::Role::Assistant => GeminiRole::Model,
        };
        let parts = content_to_parts(&msg.content, tool_name_map);

        // Merge with previous if same role (Gemini requires strict alternation)
        if let Some(last) = contents.last_mut() {
            if last.role.as_ref() == Some(&role) {
                tracing::warn!(
                    role = ?role,
                    "Merging consecutive same-role turns for Gemini role alternation"
                );
                last.parts.extend(parts);
                continue;
            }
        }

        contents.push(Content {
            role: Some(role),
            parts,
        });
    }

    contents
}

/// Convert Anthropic content (string or blocks) to Gemini parts.
fn content_to_parts(
    content: &anthropic::Content,
    tool_name_map: &HashMap<String, String>,
) -> Vec<Part> {
    match content {
        anthropic::Content::Text(s) => vec![Part::Text { text: s.clone() }],
        anthropic::Content::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| content_block_to_part(b, tool_name_map))
            .collect(),
    }
}

/// Convert a single Anthropic content block to a Gemini part.
fn content_block_to_part(
    block: &anthropic::ContentBlock,
    tool_name_map: &HashMap<String, String>,
) -> Option<Part> {
    match block {
        anthropic::ContentBlock::Text { text } => Some(Part::Text { text: text.clone() }),
        anthropic::ContentBlock::Image { source } => {
            if source.source_type == "url" {
                tracing::warn!(
                    "Image URL not natively supported by Gemini, converting to text fallback"
                );
                let url = source.url.as_deref().unwrap_or("unknown");
                return Some(Part::Text {
                    text: format!("[image: {url}]"),
                });
            }
            Some(Part::InlineData {
                inline_data: InlineData {
                    mime_type: source.media_type.clone().unwrap_or_default(),
                    data: source.data.clone().unwrap_or_default(),
                },
            })
        }
        anthropic::ContentBlock::Document { source, .. } => Some(Part::InlineData {
            inline_data: InlineData {
                mime_type: source.media_type.clone(),
                data: source.data.clone(),
            },
        }),
        anthropic::ContentBlock::ToolUse { name, input, .. } => Some(Part::FunctionCall {
            function_call: FunctionCallData {
                name: name.clone(),
                args: input.clone(),
            },
        }),
        anthropic::ContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => {
            // Resolve the actual function name from the tool_use_id
            let fn_name = tool_name_map.get(tool_use_id).cloned().unwrap_or_else(|| {
                tracing::warn!(
                    tool_use_id,
                    "No matching ToolUse block found for tool_use_id, using id as fallback"
                );
                tool_use_id.clone()
            });

            let response = match content {
                Some(anthropic::ToolResultContent::Text(s)) => {
                    json!({"result": s})
                }
                Some(anthropic::ToolResultContent::Blocks(blocks)) => {
                    let text: String = blocks
                        .iter()
                        .filter_map(|b| match b {
                            anthropic::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    json!({"result": text})
                }
                None => json!({"result": ""}),
            };
            Some(Part::FunctionResponse {
                function_response: FunctionResponseData {
                    name: fn_name,
                    response,
                },
            })
        }
        anthropic::ContentBlock::Thinking { .. } => {
            tracing::warn!("Thinking block dropped: no Gemini equivalent");
            None
        }
    }
}

/// Convert Gemini candidate parts to Anthropic content blocks.
fn convert_parts_to_content_blocks(candidate: &Candidate) -> Vec<anthropic::ContentBlock> {
    let parts = match candidate.content.as_ref() {
        Some(content) => &content.parts,
        None => return vec![],
    };

    parts
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(anthropic::ContentBlock::Text {
                text: text.clone(),
            }),
            Part::FunctionCall { function_call } => Some(anthropic::ContentBlock::ToolUse {
                id: util::ids::generate_tool_use_id(),
                name: function_call.name.clone(),
                input: function_call.args.clone(),
            }),
            Part::InlineData { inline_data } => Some(anthropic::ContentBlock::Image {
                source: anthropic::ImageSource {
                    source_type: "base64".to_string(),
                    media_type: Some(inline_data.mime_type.clone()),
                    data: Some(inline_data.data.clone()),
                    url: None,
                },
            }),
            Part::FileData { file_data } => {
                tracing::warn!(uri = %file_data.file_uri, "FileData has no direct Anthropic equivalent, converting to text");
                Some(anthropic::ContentBlock::Text {
                    text: format!("[file: {}]", file_data.file_uri),
                })
            }
            Part::FunctionResponse { .. } => {
                // Shouldn't appear in model output
                None
            }
        })
        .collect()
}

/// Map Gemini finish reason to Anthropic stop reason.
pub fn map_finish_reason(reason: Option<&FinishReason>) -> anthropic::StopReason {
    match reason {
        Some(FinishReason::Stop) => anthropic::StopReason::EndTurn,
        Some(FinishReason::MaxTokens) => anthropic::StopReason::MaxTokens,
        Some(FinishReason::Safety) => {
            tracing::warn!("Safety-blocked response mapped to end_turn");
            anthropic::StopReason::EndTurn
        }
        Some(FinishReason::Recitation) => {
            tracing::warn!("Recitation finish reason mapped to end_turn");
            anthropic::StopReason::EndTurn
        }
        Some(FinishReason::MalformedFunctionCall) => {
            tracing::warn!("MalformedFunctionCall finish reason mapped to end_turn");
            anthropic::StopReason::EndTurn
        }
        Some(other) => {
            tracing::warn!(reason = ?other, "Unexpected Gemini finish reason mapped to end_turn");
            anthropic::StopReason::EndTurn
        }
        None => anthropic::StopReason::EndTurn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gemini::generate_content::UsageMetadata;
    use serde_json::json;

    // -- Helper builders --

    fn text_msg(role: anthropic::Role, text: &str) -> anthropic::InputMessage {
        anthropic::InputMessage {
            role,
            content: anthropic::Content::Text(text.to_string()),
        }
    }

    fn blocks_msg(
        role: anthropic::Role,
        blocks: Vec<anthropic::ContentBlock>,
    ) -> anthropic::InputMessage {
        anthropic::InputMessage {
            role,
            content: anthropic::Content::Blocks(blocks),
        }
    }

    fn base_request(messages: Vec<anthropic::InputMessage>) -> anthropic::MessageCreateRequest {
        anthropic::MessageCreateRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 1024,
            messages,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            stream: None,
            extra: serde_json::Map::new(),
        }
    }

    fn gemini_text_response(text: &str) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some(GeminiRole::Model),
                    parts: vec![Part::Text {
                        text: text.to_string(),
                    }],
                }),
                finish_reason: Some(FinishReason::Stop),
                safety_ratings: None,
                citation_metadata: None,
                index: Some(0),
            }]),
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: Some(10),
                candidates_token_count: Some(20),
                total_token_count: Some(30),
            }),
            prompt_feedback: None,
        }
    }

    // -- Request mapping tests --

    #[test]
    fn basic_text_message() {
        let req = base_request(vec![text_msg(anthropic::Role::User, "Hello")]);
        let result = anthropic_to_gemini_request(&req);

        assert_eq!(result.contents.len(), 1);
        assert_eq!(result.contents[0].role, Some(GeminiRole::User));
        match &result.contents[0].parts[0] {
            Part::Text { text } => assert_eq!(text, "Hello"),
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn system_prompt_string() {
        let mut req = base_request(vec![text_msg(anthropic::Role::User, "Hi")]);
        req.system = Some(anthropic::System::Text("Be helpful.".to_string()));

        let result = anthropic_to_gemini_request(&req);
        let sys = result.system_instruction.unwrap();
        assert!(sys.role.is_none());
        match &sys.parts[0] {
            Part::Text { text } => assert_eq!(text, "Be helpful."),
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn system_prompt_blocks() {
        let mut req = base_request(vec![text_msg(anthropic::Role::User, "Hi")]);
        req.system = Some(anthropic::System::Blocks(vec![
            anthropic::SystemBlock {
                block_type: "text".to_string(),
                text: "First.".to_string(),
                cache_control: Some(anthropic::CacheControl {
                    cache_type: "ephemeral".to_string(),
                }),
            },
            anthropic::SystemBlock {
                block_type: "text".to_string(),
                text: "Second.".to_string(),
                cache_control: None,
            },
        ]));

        let result = anthropic_to_gemini_request(&req);
        let sys = result.system_instruction.unwrap();
        match &sys.parts[0] {
            Part::Text { text } => assert_eq!(text, "First.\nSecond."),
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn role_mapping() {
        let req = base_request(vec![
            text_msg(anthropic::Role::User, "Hello"),
            text_msg(anthropic::Role::Assistant, "Hi there"),
        ]);
        let result = anthropic_to_gemini_request(&req);

        assert_eq!(result.contents[0].role, Some(GeminiRole::User));
        assert_eq!(result.contents[1].role, Some(GeminiRole::Model));
    }

    #[test]
    fn role_alternation_merge() {
        let req = base_request(vec![
            text_msg(anthropic::Role::User, "First"),
            text_msg(anthropic::Role::User, "Second"),
            text_msg(anthropic::Role::Assistant, "Reply"),
        ]);
        let result = anthropic_to_gemini_request(&req);

        // Two user messages merged into one
        assert_eq!(result.contents.len(), 2);
        assert_eq!(result.contents[0].parts.len(), 2);
        match &result.contents[0].parts[1] {
            Part::Text { text } => assert_eq!(text, "Second"),
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn image_base64() {
        let req = base_request(vec![blocks_msg(
            anthropic::Role::User,
            vec![anthropic::ContentBlock::Image {
                source: anthropic::ImageSource {
                    source_type: "base64".to_string(),
                    media_type: Some("image/png".to_string()),
                    data: Some("iVBORw0KGgo=".to_string()),
                    url: None,
                },
            }],
        )]);
        let result = anthropic_to_gemini_request(&req);

        match &result.contents[0].parts[0] {
            Part::InlineData { inline_data } => {
                assert_eq!(inline_data.mime_type, "image/png");
                assert_eq!(inline_data.data, "iVBORw0KGgo=");
            }
            _ => panic!("expected InlineData"),
        }
    }

    #[test]
    fn document_block() {
        let req = base_request(vec![blocks_msg(
            anthropic::Role::User,
            vec![anthropic::ContentBlock::Document {
                source: anthropic::DocumentSource {
                    source_type: "base64".to_string(),
                    media_type: "application/pdf".to_string(),
                    data: "JVBERi0=".to_string(),
                },
                title: Some("test.pdf".to_string()),
            }],
        )]);
        let result = anthropic_to_gemini_request(&req);

        match &result.contents[0].parts[0] {
            Part::InlineData { inline_data } => {
                assert_eq!(inline_data.mime_type, "application/pdf");
                assert_eq!(inline_data.data, "JVBERi0=");
            }
            _ => panic!("expected InlineData"),
        }
    }

    #[test]
    fn tool_use_block() {
        let req = base_request(vec![blocks_msg(
            anthropic::Role::Assistant,
            vec![anthropic::ContentBlock::ToolUse {
                id: "toolu_123".to_string(),
                name: "get_weather".to_string(),
                input: json!({"location": "NYC"}),
            }],
        )]);
        let result = anthropic_to_gemini_request(&req);

        match &result.contents[0].parts[0] {
            Part::FunctionCall { function_call } => {
                assert_eq!(function_call.name, "get_weather");
                assert_eq!(function_call.args["location"], "NYC");
            }
            _ => panic!("expected FunctionCall"),
        }
    }

    #[test]
    fn tool_result_block() {
        // ToolResult must be preceded by ToolUse so the function name can be resolved
        let req = base_request(vec![
            blocks_msg(
                anthropic::Role::Assistant,
                vec![anthropic::ContentBlock::ToolUse {
                    id: "toolu_123".to_string(),
                    name: "get_weather".to_string(),
                    input: json!({"location": "NYC"}),
                }],
            ),
            blocks_msg(
                anthropic::Role::User,
                vec![anthropic::ContentBlock::ToolResult {
                    tool_use_id: "toolu_123".to_string(),
                    content: Some(anthropic::ToolResultContent::Text("72 degrees".to_string())),
                    is_error: None,
                }],
            ),
        ]);
        let result = anthropic_to_gemini_request(&req);

        // ToolResult is in the second content (user turn)
        match &result.contents[1].parts[0] {
            Part::FunctionResponse { function_response } => {
                // Gemini needs the actual function name, not the tool_use_id
                assert_eq!(function_response.name, "get_weather");
                assert_eq!(function_response.response["result"], "72 degrees");
            }
            _ => panic!("expected FunctionResponse"),
        }
    }

    #[test]
    fn tool_result_without_prior_tool_use_falls_back() {
        // When no matching ToolUse exists, falls back to tool_use_id with a warning
        let req = base_request(vec![blocks_msg(
            anthropic::Role::User,
            vec![anthropic::ContentBlock::ToolResult {
                tool_use_id: "toolu_orphan".to_string(),
                content: Some(anthropic::ToolResultContent::Text("result".to_string())),
                is_error: None,
            }],
        )]);
        let result = anthropic_to_gemini_request(&req);

        match &result.contents[0].parts[0] {
            Part::FunctionResponse { function_response } => {
                assert_eq!(function_response.name, "toolu_orphan");
            }
            _ => panic!("expected FunctionResponse"),
        }
    }

    #[test]
    fn tool_definitions_with_schema_sanitization() {
        let mut req = base_request(vec![text_msg(anthropic::Role::User, "Hi")]);
        req.tools = Some(vec![anthropic::Tool {
            name: "search".to_string(),
            description: Some("Search things".to_string()),
            input_schema: json!({
                "type": "object",
                "$schema": "http://json-schema.org/draft-07/schema#",
                "properties": {
                    "query": {"type": "string"}
                }
            }),
        }]);

        let result = anthropic_to_gemini_request(&req);
        let tools = result.tools.unwrap();
        let decls = tools[0].function_declarations.as_ref().unwrap();
        assert_eq!(decls[0].name, "search");
        // $schema should be stripped by clean_gemini_schema
        let params = decls[0].parameters.as_ref().unwrap();
        assert!(params.get("$schema").is_none());
        assert_eq!(params["type"], "object");
    }

    #[test]
    fn tool_choice_variants() {
        for (choice, expected_mode, expected_names) in [
            (anthropic::ToolChoice::Auto, FunctionCallingMode::Auto, None),
            (anthropic::ToolChoice::Any, FunctionCallingMode::Any, None),
            (anthropic::ToolChoice::None, FunctionCallingMode::None, None),
            (
                anthropic::ToolChoice::Tool {
                    name: "search".to_string(),
                },
                FunctionCallingMode::Any,
                Some(vec!["search".to_string()]),
            ),
        ] {
            let mut req = base_request(vec![text_msg(anthropic::Role::User, "Hi")]);
            req.tool_choice = Some(choice);

            let result = anthropic_to_gemini_request(&req);
            let fcc = result.tool_config.unwrap().function_calling_config.unwrap();
            assert_eq!(fcc.mode, Some(expected_mode));
            assert_eq!(fcc.allowed_function_names, expected_names);
        }
    }

    #[test]
    fn generation_config_mapping() {
        let mut req = base_request(vec![text_msg(anthropic::Role::User, "Hi")]);
        req.temperature = Some(0.7);
        req.top_p = Some(0.9);
        req.top_k = Some(40);
        req.stop_sequences = Some(vec!["END".to_string()]);

        let result = anthropic_to_gemini_request(&req);
        let gc = result.generation_config.unwrap();
        assert_eq!(gc.max_output_tokens, Some(1024));
        assert_eq!(gc.temperature, Some(0.7));
        assert_eq!(gc.top_p, Some(0.9));
        assert_eq!(gc.top_k, Some(40));
        assert_eq!(gc.stop_sequences, Some(vec!["END".to_string()]));
    }

    #[test]
    fn dropped_fields_warned() {
        let mut req = base_request(vec![text_msg(anthropic::Role::User, "Hi")]);
        req.metadata = Some(anthropic::Metadata {
            user_id: Some("user_1".to_string()),
        });
        req.thinking = Some(anthropic::ThinkingConfig::Enabled {
            budget_tokens: 1000,
        });

        // Should not panic; warnings are logged
        let result = anthropic_to_gemini_request(&req);
        assert!(result.system_instruction.is_none());
    }

    // -- Response mapping tests --

    #[test]
    fn response_text() {
        let resp = gemini_text_response("Hello there!");
        let result = gemini_to_anthropic_response(&resp, "gemini-2.5-pro");

        assert!(result.id.starts_with("msg_"));
        assert_eq!(result.response_type, "message");
        assert_eq!(result.role, anthropic::Role::Assistant);
        assert_eq!(result.model, "gemini-2.5-pro");
        assert_eq!(result.stop_reason, Some(anthropic::StopReason::EndTurn));
        assert!(result.stop_sequence.is_none());
        assert!(result.created.is_none());

        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            anthropic::ContentBlock::Text { text } => assert_eq!(text, "Hello there!"),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn response_tool_call() {
        let resp = GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some(GeminiRole::Model),
                    parts: vec![Part::FunctionCall {
                        function_call: FunctionCallData {
                            name: "search".to_string(),
                            args: json!({"query": "rust"}),
                        },
                    }],
                }),
                finish_reason: Some(FinishReason::Stop),
                safety_ratings: None,
                citation_metadata: None,
                index: Some(0),
            }]),
            usage_metadata: None,
            prompt_feedback: None,
        };

        let result = gemini_to_anthropic_response(&resp, "gemini-2.5-pro");
        assert_eq!(result.stop_reason, Some(anthropic::StopReason::ToolUse));
        match &result.content[0] {
            anthropic::ContentBlock::ToolUse { id, name, input } => {
                assert!(id.starts_with("toolu_"));
                assert_eq!(name, "search");
                assert_eq!(input["query"], "rust");
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    #[test]
    fn finish_reason_mapping() {
        assert_eq!(
            map_finish_reason(Some(&FinishReason::Stop)),
            anthropic::StopReason::EndTurn
        );
        assert_eq!(
            map_finish_reason(Some(&FinishReason::MaxTokens)),
            anthropic::StopReason::MaxTokens
        );
        assert_eq!(
            map_finish_reason(Some(&FinishReason::Safety)),
            anthropic::StopReason::EndTurn
        );
        assert_eq!(
            map_finish_reason(Some(&FinishReason::MalformedFunctionCall)),
            anthropic::StopReason::EndTurn
        );
        assert_eq!(map_finish_reason(None), anthropic::StopReason::EndTurn);
    }

    #[test]
    fn usage_mapping() {
        let resp = gemini_text_response("Hi");
        let result = gemini_to_anthropic_response(&resp, "test");

        assert_eq!(result.usage.input_tokens, 10);
        assert_eq!(result.usage.output_tokens, 20);
    }

    #[test]
    fn usage_default_when_missing() {
        let resp = GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some(GeminiRole::Model),
                    parts: vec![Part::Text {
                        text: "Hi".to_string(),
                    }],
                }),
                finish_reason: Some(FinishReason::Stop),
                safety_ratings: None,
                citation_metadata: None,
                index: None,
            }]),
            usage_metadata: None,
            prompt_feedback: None,
        };
        let result = gemini_to_anthropic_response(&resp, "test");
        assert_eq!(result.usage.input_tokens, 0);
        assert_eq!(result.usage.output_tokens, 0);
    }

    #[test]
    fn safety_blocked_response() {
        let resp = GenerateContentResponse {
            candidates: None,
            usage_metadata: None,
            prompt_feedback: Some(crate::gemini::generate_content::PromptFeedback {
                block_reason: Some("SAFETY".to_string()),
                safety_ratings: None,
            }),
        };
        let result = gemini_to_anthropic_response(&resp, "test");

        assert!(result.content.is_empty());
        assert_eq!(result.stop_reason, Some(anthropic::StopReason::EndTurn));
    }

    #[test]
    fn thinking_block_dropped() {
        let req = base_request(vec![blocks_msg(
            anthropic::Role::Assistant,
            vec![
                anthropic::ContentBlock::Thinking {
                    thinking: "Let me think...".to_string(),
                    signature: None,
                },
                anthropic::ContentBlock::Text {
                    text: "Result".to_string(),
                },
            ],
        )]);
        let result = anthropic_to_gemini_request(&req);

        // Thinking block dropped, only text remains
        assert_eq!(result.contents[0].parts.len(), 1);
        match &result.contents[0].parts[0] {
            Part::Text { text } => assert_eq!(text, "Result"),
            _ => panic!("expected Text part"),
        }
    }
}
