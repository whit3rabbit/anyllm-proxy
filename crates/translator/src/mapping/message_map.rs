// Anthropic <-> OpenAI message mapping
// PLAN.md lines 765-793, 964-977

use crate::anthropic;
use crate::mapping::{streaming_map, tools_map, usage_map};
use crate::openai;
use crate::util;

/// Convert an Anthropic MessageCreateRequest to an OpenAI ChatCompletionRequest.
pub fn anthropic_to_openai_request(
    req: &anthropic::MessageCreateRequest,
) -> openai::ChatCompletionRequest {
    let mut messages = Vec::new();

    if let Some(ref system) = req.system {
        let text = match system {
            anthropic::System::Text(s) => s.clone(),
            anthropic::System::Blocks(blocks) => blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        };
        messages.push(openai::ChatMessage {
            role: openai::ChatRole::Developer,
            content: Some(openai::ChatContent::Text(text)),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for msg in &req.messages {
        convert_anthropic_message(msg, &mut messages);
    }

    let tools = req
        .tools
        .as_ref()
        .map(|t| tools_map::anthropic_tools_to_openai(t));

    let tool_choice = req
        .tool_choice
        .as_ref()
        .map(tools_map::anthropic_tool_choice_to_openai);

    // OpenAI caps stop sequences at 4
    let stop = req.stop_sequences.as_ref().map(|seqs| {
        let capped: Vec<String> = seqs.iter().take(4).cloned().collect();
        if capped.len() == 1 {
            openai::Stop::Single(capped.into_iter().next().unwrap())
        } else {
            openai::Stop::Multiple(capped)
        }
    });

    openai::ChatCompletionRequest {
        model: req.model.clone(),
        messages,
        max_tokens: Some(req.max_tokens),
        temperature: req.temperature,
        top_p: req.top_p,
        stop,
        tools,
        tool_choice,
        stream: req.stream,
        stream_options: if req.stream == Some(true) {
            Some(openai::StreamOptions {
                include_usage: true,
            })
        } else {
            None
        },
        presence_penalty: None,
        frequency_penalty: None,
        response_format: None,
        extra: serde_json::Map::new(),
    }
}

/// Convert a single Anthropic InputMessage into one or more OpenAI ChatMessages.
/// An assistant message with tool_use blocks produces tool_calls.
/// A user message with tool_result blocks produces OpenAI tool-role messages.
fn convert_anthropic_message(msg: &anthropic::InputMessage, out: &mut Vec<openai::ChatMessage>) {
    let role = match msg.role {
        anthropic::Role::User => openai::ChatRole::User,
        anthropic::Role::Assistant => openai::ChatRole::Assistant,
    };

    match &msg.content {
        anthropic::Content::Text(text) => {
            out.push(openai::ChatMessage {
                role,
                content: Some(openai::ChatContent::Text(text.clone())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }
        anthropic::Content::Blocks(blocks) => {
            if msg.role == anthropic::Role::Assistant {
                convert_assistant_blocks(blocks, out);
            } else {
                convert_user_blocks(blocks, out);
            }
        }
    }
}

/// Assistant blocks: text parts become content, tool_use blocks become tool_calls.
fn convert_assistant_blocks(
    blocks: &[anthropic::ContentBlock],
    out: &mut Vec<openai::ChatMessage>,
) {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in blocks {
        match block {
            anthropic::ContentBlock::Text { text } => {
                text_parts.push(text.clone());
            }
            anthropic::ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(openai::ToolCall {
                    id: id.clone(),
                    call_type: "function".to_string(),
                    function: openai::FunctionCall {
                        name: name.clone(),
                        arguments: util::json::value_to_json_string(input),
                    },
                });
            }
            _ => {} // Other block types not expected in assistant messages
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(openai::ChatContent::Text(text_parts.join("")))
    };

    out.push(openai::ChatMessage {
        role: openai::ChatRole::Assistant,
        content,
        name: None,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        tool_call_id: None,
    });
}

/// User blocks: text/image parts become content, tool_result blocks become
/// separate OpenAI tool-role messages.
fn convert_user_blocks(blocks: &[anthropic::ContentBlock], out: &mut Vec<openai::ChatMessage>) {
    let mut content_parts: Vec<openai::ChatContentPart> = Vec::new();
    let mut tool_results: Vec<(String, String)> = Vec::new();

    for block in blocks {
        match block {
            anthropic::ContentBlock::Text { text } => {
                content_parts.push(openai::ChatContentPart::Text { text: text.clone() });
            }
            anthropic::ContentBlock::Image { source } => {
                let url = if let Some(ref url) = source.url {
                    url.clone()
                } else if let Some(ref data) = source.data {
                    let mt = source.media_type.as_deref().unwrap_or("image/png");
                    format!("data:{};base64,{}", mt, data)
                } else {
                    continue;
                };
                content_parts.push(openai::ChatContentPart::ImageUrl {
                    image_url: openai::chat_completions::ImageUrl { url, detail: None },
                });
            }
            anthropic::ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let text = match content {
                    Some(anthropic::messages::ToolResultContent::Text(s)) => s.clone(),
                    Some(anthropic::messages::ToolResultContent::Blocks(inner)) => inner
                        .iter()
                        .filter_map(|b| match b {
                            anthropic::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(""),
                    None => String::new(),
                };
                let text = if *is_error == Some(true) {
                    format!("Error: {}", text)
                } else {
                    text
                };
                tool_results.push((tool_use_id.clone(), text));
            }
            anthropic::ContentBlock::Document { source, title } => {
                // OpenAI Chat Completions doesn't support inline documents.
                // Convert to a text note describing the document.
                // Full support would require OpenAI Responses API with input_file.file_data.
                let label = title.as_deref().unwrap_or("document");
                let note = format!(
                    "[Attached {}: {} ({} bytes base64)]",
                    label,
                    source.media_type,
                    source.data.len()
                );
                content_parts.push(openai::ChatContentPart::Text { text: note });
            }
            // ToolUse blocks don't appear in user messages; ignore if present
            anthropic::ContentBlock::ToolUse { .. } => {}
        }
    }

    // Emit user content message if there are text/image parts
    if !content_parts.is_empty() {
        let content = if content_parts.len() == 1 {
            if let openai::ChatContentPart::Text { ref text } = content_parts[0] {
                Some(openai::ChatContent::Text(text.clone()))
            } else {
                Some(openai::ChatContent::Parts(content_parts))
            }
        } else {
            Some(openai::ChatContent::Parts(content_parts))
        };
        out.push(openai::ChatMessage {
            role: openai::ChatRole::User,
            content,
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // Emit tool result messages
    for (tool_call_id, text) in tool_results {
        out.push(openai::ChatMessage {
            role: openai::ChatRole::Tool,
            content: Some(openai::ChatContent::Text(text)),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
        });
    }
}

/// Convert an OpenAI ChatCompletionResponse back to an Anthropic MessageResponse.
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

        // Map tool calls
        if let Some(ref tool_calls) = choice.message.tool_calls {
            for tc in tool_calls {
                content.push(anthropic::ContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    input: util::json::parse_json_lenient(&tc.function.arguments),
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- Helper: build a minimal Anthropic request ---

    fn basic_request() -> anthropic::MessageCreateRequest {
        anthropic::MessageCreateRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            max_tokens: 1024,
            messages: vec![anthropic::InputMessage {
                role: anthropic::Role::User,
                content: anthropic::Content::Text("Hello".to_string()),
            }],
            system: None,
            temperature: None,
            top_p: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            stream: None,
            extra: serde_json::Map::new(),
        }
    }

    fn basic_openai_response() -> openai::ChatCompletionResponse {
        openai::ChatCompletionResponse {
            id: "chatcmpl-abc123".to_string(),
            object: "chat.completion".to_string(),
            model: "gpt-4o".to_string(),
            choices: vec![openai::Choice {
                index: 0,
                message: openai::ChatMessage {
                    role: openai::ChatRole::Assistant,
                    content: Some(openai::ChatContent::Text("Hi there!".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some(openai::FinishReason::Stop),
            }],
            usage: Some(openai::ChatUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            created: Some(1700000000),
            system_fingerprint: None,
        }
    }

    // --- Request translation tests ---

    #[test]
    fn basic_text_request() {
        let req = basic_request();
        let oai = anthropic_to_openai_request(&req);

        assert_eq!(oai.model, "claude-3-5-sonnet-20241022");
        assert_eq!(oai.max_tokens, Some(1024));
        assert_eq!(oai.messages.len(), 1);
        assert_eq!(oai.messages[0].role, openai::ChatRole::User);
        assert!(matches!(
            &oai.messages[0].content,
            Some(openai::ChatContent::Text(t)) if t == "Hello"
        ));
        assert!(oai.tools.is_none());
        assert!(oai.tool_choice.is_none());
        assert!(oai.stream_options.is_none());
    }

    #[test]
    fn system_prompt_string_becomes_developer_message() {
        let mut req = basic_request();
        req.system = Some(anthropic::System::Text(
            "You are a helpful assistant.".to_string(),
        ));

        let oai = anthropic_to_openai_request(&req);

        assert_eq!(oai.messages.len(), 2);
        assert_eq!(oai.messages[0].role, openai::ChatRole::Developer);
        assert!(matches!(
            &oai.messages[0].content,
            Some(openai::ChatContent::Text(t)) if t == "You are a helpful assistant."
        ));
    }

    #[test]
    fn system_prompt_blocks_concatenated_into_developer_message() {
        let mut req = basic_request();
        req.system = Some(anthropic::System::Blocks(vec![
            anthropic::messages::SystemBlock {
                block_type: "text".to_string(),
                text: "Be concise.".to_string(),
                cache_control: None,
            },
            anthropic::messages::SystemBlock {
                block_type: "text".to_string(),
                text: "Respond in JSON.".to_string(),
                cache_control: None,
            },
        ]));

        let oai = anthropic_to_openai_request(&req);

        assert_eq!(oai.messages[0].role, openai::ChatRole::Developer);
        assert!(matches!(
            &oai.messages[0].content,
            Some(openai::ChatContent::Text(t)) if t == "Be concise.\nRespond in JSON."
        ));
    }

    #[test]
    fn tool_definitions_mapped() {
        let schema = json!({
            "type": "object",
            "properties": {"location": {"type": "string"}},
            "required": ["location"]
        });

        let mut req = basic_request();
        req.tools = Some(vec![anthropic::Tool {
            name: "get_weather".to_string(),
            description: Some("Get weather for a location".to_string()),
            input_schema: schema.clone(),
        }]);

        let oai = anthropic_to_openai_request(&req);

        let tools = oai.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_type, "function");
        assert_eq!(tools[0].function.name, "get_weather");
        assert_eq!(
            tools[0].function.description.as_deref(),
            Some("Get weather for a location")
        );
        assert_eq!(tools[0].function.parameters, Some(schema));
    }

    #[test]
    fn tool_choice_auto() {
        let mut req = basic_request();
        req.tool_choice = Some(anthropic::ToolChoice::Auto);
        let oai = anthropic_to_openai_request(&req);
        assert!(matches!(
            oai.tool_choice,
            Some(openai::ChatToolChoice::Simple(ref s)) if s == "auto"
        ));
    }

    #[test]
    fn tool_choice_any_becomes_required() {
        let mut req = basic_request();
        req.tool_choice = Some(anthropic::ToolChoice::Any);
        let oai = anthropic_to_openai_request(&req);
        assert!(matches!(
            oai.tool_choice,
            Some(openai::ChatToolChoice::Simple(ref s)) if s == "required"
        ));
    }

    #[test]
    fn tool_choice_none() {
        let mut req = basic_request();
        req.tool_choice = Some(anthropic::ToolChoice::None);
        let oai = anthropic_to_openai_request(&req);
        assert!(matches!(
            oai.tool_choice,
            Some(openai::ChatToolChoice::Simple(ref s)) if s == "none"
        ));
    }

    #[test]
    fn tool_choice_specific_tool() {
        let mut req = basic_request();
        req.tool_choice = Some(anthropic::ToolChoice::Tool {
            name: "get_weather".to_string(),
        });
        let oai = anthropic_to_openai_request(&req);
        match oai.tool_choice {
            Some(openai::ChatToolChoice::Named(ref n)) => {
                assert_eq!(n.choice_type, "function");
                assert_eq!(n.function.name, "get_weather");
            }
            other => panic!("expected Named tool choice, got {:?}", other),
        }
    }

    #[test]
    fn stop_sequences_capped_at_four() {
        let mut req = basic_request();
        req.stop_sequences = Some(vec![
            "a".into(),
            "b".into(),
            "c".into(),
            "d".into(),
            "e".into(),
        ]);

        let oai = anthropic_to_openai_request(&req);

        match oai.stop {
            Some(openai::Stop::Multiple(ref v)) => assert_eq!(v.len(), 4),
            other => panic!("expected Multiple stop, got {:?}", other),
        }
    }

    #[test]
    fn single_stop_sequence_is_single() {
        let mut req = basic_request();
        req.stop_sequences = Some(vec!["END".into()]);

        let oai = anthropic_to_openai_request(&req);

        assert!(matches!(
            oai.stop,
            Some(openai::Stop::Single(ref s)) if s == "END"
        ));
    }

    #[test]
    fn streaming_sets_stream_options() {
        let mut req = basic_request();
        req.stream = Some(true);

        let oai = anthropic_to_openai_request(&req);

        assert_eq!(oai.stream, Some(true));
        assert!(oai.stream_options.as_ref().unwrap().include_usage);
    }

    #[test]
    fn conversation_with_tool_use_and_tool_result() {
        let mut req = basic_request();
        req.messages = vec![
            // User asks
            anthropic::InputMessage {
                role: anthropic::Role::User,
                content: anthropic::Content::Text("What is the weather in NYC?".to_string()),
            },
            // Assistant calls tool
            anthropic::InputMessage {
                role: anthropic::Role::Assistant,
                content: anthropic::Content::Blocks(vec![
                    anthropic::ContentBlock::Text {
                        text: "Let me check.".to_string(),
                    },
                    anthropic::ContentBlock::ToolUse {
                        id: "call_001".to_string(),
                        name: "get_weather".to_string(),
                        input: json!({"location": "NYC"}),
                    },
                ]),
            },
            // User provides tool result
            anthropic::InputMessage {
                role: anthropic::Role::User,
                content: anthropic::Content::Blocks(vec![anthropic::ContentBlock::ToolResult {
                    tool_use_id: "call_001".to_string(),
                    content: Some(anthropic::messages::ToolResultContent::Text(
                        "72F, sunny".to_string(),
                    )),
                    is_error: None,
                }]),
            },
        ];

        let oai = anthropic_to_openai_request(&req);

        // msg 0: user text
        assert_eq!(oai.messages[0].role, openai::ChatRole::User);
        assert!(matches!(
            &oai.messages[0].content,
            Some(openai::ChatContent::Text(t)) if t == "What is the weather in NYC?"
        ));

        // msg 1: assistant with text + tool_calls
        assert_eq!(oai.messages[1].role, openai::ChatRole::Assistant);
        assert!(matches!(
            &oai.messages[1].content,
            Some(openai::ChatContent::Text(t)) if t == "Let me check."
        ));
        let tc = oai.messages[1].tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "call_001");
        assert_eq!(tc[0].function.name, "get_weather");
        assert_eq!(tc[0].function.arguments, r#"{"location":"NYC"}"#);

        // msg 2: tool result
        assert_eq!(oai.messages[2].role, openai::ChatRole::Tool);
        assert_eq!(oai.messages[2].tool_call_id.as_deref(), Some("call_001"));
        assert!(matches!(
            &oai.messages[2].content,
            Some(openai::ChatContent::Text(t)) if t == "72F, sunny"
        ));
    }

    #[test]
    fn tool_result_error_prefixed() {
        let mut req = basic_request();
        req.messages = vec![anthropic::InputMessage {
            role: anthropic::Role::User,
            content: anthropic::Content::Blocks(vec![anthropic::ContentBlock::ToolResult {
                tool_use_id: "call_err".to_string(),
                content: Some(anthropic::messages::ToolResultContent::Text(
                    "not found".to_string(),
                )),
                is_error: Some(true),
            }]),
        }];

        let oai = anthropic_to_openai_request(&req);

        assert_eq!(oai.messages[0].role, openai::ChatRole::Tool);
        assert!(matches!(
            &oai.messages[0].content,
            Some(openai::ChatContent::Text(t)) if t == "Error: not found"
        ));
    }

    #[test]
    fn image_block_to_image_url_part() {
        let mut req = basic_request();
        req.messages = vec![anthropic::InputMessage {
            role: anthropic::Role::User,
            content: anthropic::Content::Blocks(vec![
                anthropic::ContentBlock::Text {
                    text: "Describe this".to_string(),
                },
                anthropic::ContentBlock::Image {
                    source: anthropic::messages::ImageSource {
                        source_type: "base64".to_string(),
                        media_type: Some("image/jpeg".to_string()),
                        data: Some("abc123".to_string()),
                        url: None,
                    },
                },
            ]),
        }];

        let oai = anthropic_to_openai_request(&req);

        assert_eq!(oai.messages.len(), 1);
        match &oai.messages[0].content {
            Some(openai::ChatContent::Parts(parts)) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(
                    &parts[0],
                    openai::ChatContentPart::Text { text } if text == "Describe this"
                ));
                match &parts[1] {
                    openai::ChatContentPart::ImageUrl { image_url } => {
                        assert_eq!(image_url.url, "data:image/jpeg;base64,abc123");
                    }
                    other => panic!("expected ImageUrl, got {:?}", other),
                }
            }
            other => panic!("expected Parts, got {:?}", other),
        }
    }

    #[test]
    fn image_block_with_url_uses_url_directly() {
        let mut req = basic_request();
        req.messages = vec![anthropic::InputMessage {
            role: anthropic::Role::User,
            content: anthropic::Content::Blocks(vec![anthropic::ContentBlock::Image {
                source: anthropic::messages::ImageSource {
                    source_type: "url".to_string(),
                    media_type: None,
                    data: None,
                    url: Some("https://example.com/img.png".to_string()),
                },
            }]),
        }];

        let oai = anthropic_to_openai_request(&req);

        // Single image part still wrapped in Parts (not a text shortcut)
        match &oai.messages[0].content {
            Some(openai::ChatContent::Parts(parts)) => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    openai::ChatContentPart::ImageUrl { image_url } => {
                        assert_eq!(image_url.url, "https://example.com/img.png");
                    }
                    other => panic!("expected ImageUrl, got {:?}", other),
                }
            }
            other => panic!("expected Parts, got {:?}", other),
        }
    }

    #[test]
    fn single_text_block_user_message_flattened() {
        // A single text block in user content should produce ChatContent::Text, not Parts.
        let mut req = basic_request();
        req.messages = vec![anthropic::InputMessage {
            role: anthropic::Role::User,
            content: anthropic::Content::Blocks(vec![anthropic::ContentBlock::Text {
                text: "just text".to_string(),
            }]),
        }];

        let oai = anthropic_to_openai_request(&req);

        assert!(matches!(
            &oai.messages[0].content,
            Some(openai::ChatContent::Text(t)) if t == "just text"
        ));
    }

    // --- Response translation tests ---

    #[test]
    fn openai_text_response_to_anthropic() {
        let resp = basic_openai_response();
        let anth = openai_to_anthropic_response(&resp, "claude-3-5-sonnet-20241022");

        assert!(anth.id.starts_with("msg_"));
        assert_eq!(anth.response_type, "message");
        assert_eq!(anth.role, anthropic::Role::Assistant);
        assert_eq!(anth.model, "claude-3-5-sonnet-20241022");
        assert_eq!(anth.content.len(), 1);
        assert!(matches!(
            &anth.content[0],
            anthropic::ContentBlock::Text { text } if text == "Hi there!"
        ));
        assert_eq!(anth.stop_reason, Some(anthropic::StopReason::EndTurn));
        assert!(anth.stop_sequence.is_none());
        assert_eq!(anth.usage.input_tokens, 10);
        assert_eq!(anth.usage.output_tokens, 5);
    }

    #[test]
    fn openai_tool_calls_response_to_anthropic() {
        let resp = openai::ChatCompletionResponse {
            id: "chatcmpl-xyz".to_string(),
            object: "chat.completion".to_string(),
            model: "gpt-4o".to_string(),
            choices: vec![openai::Choice {
                index: 0,
                message: openai::ChatMessage {
                    role: openai::ChatRole::Assistant,
                    content: None,
                    name: None,
                    tool_calls: Some(vec![openai::ToolCall {
                        id: "call_abc".to_string(),
                        call_type: "function".to_string(),
                        function: openai::FunctionCall {
                            name: "get_weather".to_string(),
                            arguments: r#"{"location":"NYC"}"#.to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some(openai::FinishReason::ToolCalls),
            }],
            usage: Some(openai::ChatUsage {
                prompt_tokens: 20,
                completion_tokens: 10,
                total_tokens: 30,
            }),
            created: None,
            system_fingerprint: None,
        };

        let anth = openai_to_anthropic_response(&resp, "claude-3-5-sonnet-20241022");

        assert_eq!(anth.stop_reason, Some(anthropic::StopReason::ToolUse));
        assert_eq!(anth.content.len(), 1);
        match &anth.content[0] {
            anthropic::ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_abc");
                assert_eq!(name, "get_weather");
                assert_eq!(input, &json!({"location": "NYC"}));
            }
            other => panic!("expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn stop_reason_mapping() {
        let cases = vec![
            (openai::FinishReason::Stop, anthropic::StopReason::EndTurn),
            (
                openai::FinishReason::Length,
                anthropic::StopReason::MaxTokens,
            ),
            (
                openai::FinishReason::ToolCalls,
                anthropic::StopReason::ToolUse,
            ),
            (
                openai::FinishReason::ContentFilter,
                anthropic::StopReason::EndTurn,
            ),
            (
                openai::FinishReason::FunctionCall,
                anthropic::StopReason::ToolUse,
            ),
        ];

        for (oai_reason, expected) in cases {
            let resp = openai::ChatCompletionResponse {
                id: "x".into(),
                object: "chat.completion".into(),
                model: "gpt-4o".into(),
                choices: vec![openai::Choice {
                    index: 0,
                    message: openai::ChatMessage {
                        role: openai::ChatRole::Assistant,
                        content: Some(openai::ChatContent::Text("ok".into())),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    },
                    finish_reason: Some(oai_reason),
                }],
                usage: None,
                created: None,
                system_fingerprint: None,
            };
            let anth = openai_to_anthropic_response(&resp, "m");
            assert_eq!(anth.stop_reason, Some(expected));
        }
    }

    #[test]
    fn empty_content_response() {
        let resp = openai::ChatCompletionResponse {
            id: "x".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![openai::Choice {
                index: 0,
                message: openai::ChatMessage {
                    role: openai::ChatRole::Assistant,
                    content: Some(openai::ChatContent::Text(String::new())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some(openai::FinishReason::Stop),
            }],
            usage: None,
            created: None,
            system_fingerprint: None,
        };

        let anth = openai_to_anthropic_response(&resp, "m");
        // Empty text is not added to content blocks
        assert!(anth.content.is_empty());
    }

    #[test]
    fn no_choices_produces_default_response() {
        let resp = openai::ChatCompletionResponse {
            id: "x".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![],
            usage: None,
            created: None,
            system_fingerprint: None,
        };

        let anth = openai_to_anthropic_response(&resp, "m");
        assert!(anth.content.is_empty());
        // Default stop_reason when no choice
        assert_eq!(anth.stop_reason, Some(anthropic::StopReason::EndTurn));
    }

    #[test]
    fn missing_usage_produces_defaults() {
        let resp = openai::ChatCompletionResponse {
            id: "x".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![],
            usage: None,
            created: None,
            system_fingerprint: None,
        };

        let anth = openai_to_anthropic_response(&resp, "m");
        assert_eq!(anth.usage.input_tokens, 0);
        assert_eq!(anth.usage.output_tokens, 0);
    }

    #[test]
    fn tool_result_blocks_content_concatenated() {
        let mut req = basic_request();
        req.messages = vec![anthropic::InputMessage {
            role: anthropic::Role::User,
            content: anthropic::Content::Blocks(vec![anthropic::ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: Some(anthropic::messages::ToolResultContent::Blocks(vec![
                    anthropic::ContentBlock::Text {
                        text: "part1".to_string(),
                    },
                    anthropic::ContentBlock::Text {
                        text: "part2".to_string(),
                    },
                ])),
                is_error: None,
            }]),
        }];

        let oai = anthropic_to_openai_request(&req);

        assert_eq!(oai.messages[0].role, openai::ChatRole::Tool);
        assert!(matches!(
            &oai.messages[0].content,
            Some(openai::ChatContent::Text(t)) if t == "part1part2"
        ));
    }

    #[test]
    fn tool_result_none_content_becomes_empty_string() {
        let mut req = basic_request();
        req.messages = vec![anthropic::InputMessage {
            role: anthropic::Role::User,
            content: anthropic::Content::Blocks(vec![anthropic::ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: None,
                is_error: None,
            }]),
        }];

        let oai = anthropic_to_openai_request(&req);

        assert!(matches!(
            &oai.messages[0].content,
            Some(openai::ChatContent::Text(t)) if t.is_empty()
        ));
    }

    #[test]
    fn mixed_user_content_and_tool_results_produces_multiple_messages() {
        let mut req = basic_request();
        req.messages = vec![anthropic::InputMessage {
            role: anthropic::Role::User,
            content: anthropic::Content::Blocks(vec![
                anthropic::ContentBlock::Text {
                    text: "Here are the results".to_string(),
                },
                anthropic::ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: Some(anthropic::messages::ToolResultContent::Text(
                        "result1".to_string(),
                    )),
                    is_error: None,
                },
            ]),
        }];

        let oai = anthropic_to_openai_request(&req);

        // Should produce two messages: one user text, one tool result
        assert_eq!(oai.messages.len(), 2);
        assert_eq!(oai.messages[0].role, openai::ChatRole::User);
        assert_eq!(oai.messages[1].role, openai::ChatRole::Tool);
    }

    #[test]
    fn assistant_text_and_tool_use_combined() {
        // Assistant message with both text and tool_use should produce a single
        // OpenAI message with content + tool_calls.
        let mut req = basic_request();
        req.messages = vec![anthropic::InputMessage {
            role: anthropic::Role::Assistant,
            content: anthropic::Content::Blocks(vec![
                anthropic::ContentBlock::Text {
                    text: "Thinking...".to_string(),
                },
                anthropic::ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "search".to_string(),
                    input: json!({"q": "rust"}),
                },
            ]),
        }];

        let oai = anthropic_to_openai_request(&req);

        assert_eq!(oai.messages.len(), 1);
        assert_eq!(oai.messages[0].role, openai::ChatRole::Assistant);
        assert!(matches!(
            &oai.messages[0].content,
            Some(openai::ChatContent::Text(t)) if t == "Thinking..."
        ));
        let tc = oai.messages[0].tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "call_1");
    }

    #[test]
    fn openai_response_with_text_and_tool_calls() {
        // OpenAI can return both content and tool_calls in a single choice.
        let resp = openai::ChatCompletionResponse {
            id: "x".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![openai::Choice {
                index: 0,
                message: openai::ChatMessage {
                    role: openai::ChatRole::Assistant,
                    content: Some(openai::ChatContent::Text("Let me check.".into())),
                    name: None,
                    tool_calls: Some(vec![openai::ToolCall {
                        id: "call_1".into(),
                        call_type: "function".into(),
                        function: openai::FunctionCall {
                            name: "lookup".into(),
                            arguments: "{}".into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some(openai::FinishReason::ToolCalls),
            }],
            usage: Some(openai::ChatUsage {
                prompt_tokens: 5,
                completion_tokens: 3,
                total_tokens: 8,
            }),
            created: None,
            system_fingerprint: None,
        };

        let anth = openai_to_anthropic_response(&resp, "m");

        assert_eq!(anth.content.len(), 2);
        assert!(matches!(
            &anth.content[0],
            anthropic::ContentBlock::Text { text } if text == "Let me check."
        ));
        assert!(matches!(
            &anth.content[1],
            anthropic::ContentBlock::ToolUse { id, name, .. } if id == "call_1" && name == "lookup"
        ));
    }

    #[test]
    fn malformed_tool_arguments_handled() {
        // If OpenAI returns invalid JSON in arguments, parse_json_lenient wraps it as a string.
        let resp = openai::ChatCompletionResponse {
            id: "x".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![openai::Choice {
                index: 0,
                message: openai::ChatMessage {
                    role: openai::ChatRole::Assistant,
                    content: None,
                    name: None,
                    tool_calls: Some(vec![openai::ToolCall {
                        id: "call_bad".into(),
                        call_type: "function".into(),
                        function: openai::FunctionCall {
                            name: "broken".into(),
                            arguments: "not json".into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some(openai::FinishReason::ToolCalls),
            }],
            usage: None,
            created: None,
            system_fingerprint: None,
        };

        let anth = openai_to_anthropic_response(&resp, "m");
        match &anth.content[0] {
            anthropic::ContentBlock::ToolUse { input, .. } => {
                // Lenient parse wraps bad JSON as Value::String
                assert_eq!(input, &json!("not json"));
            }
            other => panic!("expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn document_block_converted_to_text_note() {
        let req = anthropic::MessageCreateRequest {
            model: "claude-opus-4-6".into(),
            max_tokens: 1024,
            messages: vec![anthropic::InputMessage {
                role: anthropic::Role::User,
                content: anthropic::Content::Blocks(vec![
                    anthropic::ContentBlock::Text {
                        text: "Summarize this PDF".into(),
                    },
                    anthropic::ContentBlock::Document {
                        source: anthropic::messages::DocumentSource {
                            source_type: "base64".into(),
                            media_type: "application/pdf".into(),
                            data: "AAAA".into(),
                        },
                        title: Some("report.pdf".into()),
                    },
                ]),
            }],
            system: None,
            temperature: None,
            top_p: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            stream: None,
            extra: serde_json::Map::new(),
        };

        let openai_req = anthropic_to_openai_request(&req);
        // Should produce a single user message with multipart content
        assert_eq!(openai_req.messages.len(), 1);
        match &openai_req.messages[0].content {
            Some(openai::ChatContent::Parts(parts)) => {
                assert_eq!(parts.len(), 2);
                // Second part should be the document note
                if let openai::ChatContentPart::Text { text } = &parts[1] {
                    assert!(text.contains("report.pdf"));
                    assert!(text.contains("application/pdf"));
                } else {
                    panic!("expected text part for document");
                }
            }
            other => panic!("expected Parts, got {:?}", other),
        }
    }
}
