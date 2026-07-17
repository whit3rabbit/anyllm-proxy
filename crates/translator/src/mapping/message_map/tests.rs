use super::request::is_o_series_model;
use super::*;
use crate::{anthropic, openai};
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
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::Stop),
            logprobs: None,
        }],
        usage: Some(openai::ChatUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        }),
        created: Some(1700000000),
        system_fingerprint: None,
        service_tier: None,
    }
}

// --- Request translation tests ---

#[test]
fn basic_text_request() {
    let req = basic_request();
    let oai = anthropic_to_openai_request(&req);

    assert_eq!(oai.model, "claude-3-5-sonnet-20241022");
    assert_eq!(oai.max_tokens, Some(1024));
    assert_eq!(oai.max_completion_tokens, Some(1024));
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
    assert_eq!(oai.messages[0].role, openai::ChatRole::System);
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

    assert_eq!(oai.messages[0].role, openai::ChatRole::System);
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
    req.tool_choice = Some(anthropic::ToolChoice::Auto {
        disable_parallel_tool_use: None,
    });
    let oai = anthropic_to_openai_request(&req);
    assert!(matches!(
        oai.tool_choice,
        Some(openai::ChatToolChoice::Simple(ref s)) if s == "auto"
    ));
}

#[test]
fn tool_choice_any_becomes_required() {
    let mut req = basic_request();
    req.tool_choice = Some(anthropic::ToolChoice::Any {
        disable_parallel_tool_use: None,
    });
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
fn disable_parallel_tool_use_sets_parallel_tool_calls_false() {
    let mut req = basic_request();
    req.tool_choice = Some(anthropic::ToolChoice::Auto {
        disable_parallel_tool_use: Some(true),
    });
    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.parallel_tool_calls, Some(false));
}

#[test]
fn disable_parallel_tool_use_false_leaves_parallel_tool_calls_none() {
    let mut req = basic_request();
    req.tool_choice = Some(anthropic::ToolChoice::Auto {
        disable_parallel_tool_use: Some(false),
    });
    let oai = anthropic_to_openai_request(&req);
    assert!(oai.parallel_tool_calls.is_none());
}

#[test]
fn no_tool_choice_leaves_parallel_tool_calls_none() {
    let req = basic_request();
    let oai = anthropic_to_openai_request(&req);
    assert!(oai.parallel_tool_calls.is_none());
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
fn empty_stop_sequences_becomes_none() {
    let mut req = basic_request();
    req.stop_sequences = Some(vec![]);

    let oai = anthropic_to_openai_request(&req);

    assert!(
        oai.stop.is_none(),
        "empty stop_sequences should map to None, not Stop::Multiple([])"
    );
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
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::ToolCalls),
            logprobs: None,
        }],
        usage: Some(openai::ChatUsage {
            prompt_tokens: 20,
            completion_tokens: 10,
            total_tokens: 30,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        }),
        created: None,
        system_fingerprint: None,
        service_tier: None,
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
                    refusal: None,
                    reasoning_content: None,
                    thinking_blocks: None,
                },
                finish_reason: Some(oai_reason),
                logprobs: None,
            }],
            usage: None,
            created: None,
            system_fingerprint: None,
            service_tier: None,
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
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::Stop),
            logprobs: None,
        }],
        usage: None,
        created: None,
        system_fingerprint: None,
        service_tier: None,
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
        service_tier: None,
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
        service_tier: None,
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
    // Multiple text blocks are now preserved as separate parts (not concatenated)
    // to support mixed text+image content in tool results.
    match &oai.messages[0].content {
        Some(openai::ChatContent::Parts(parts)) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[0], openai::ChatContentPart::Text { text } if text == "part1"));
            assert!(matches!(&parts[1], openai::ChatContentPart::Text { text } if text == "part2"));
        }
        other => panic!("expected Parts with 2 text entries, got {:?}", other),
    }
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

    // Should produce two messages: tool result first (must follow assistant),
    // then user text.
    assert_eq!(oai.messages.len(), 2);
    assert_eq!(oai.messages[0].role, openai::ChatRole::Tool);
    assert_eq!(oai.messages[1].role, openai::ChatRole::User);
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
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::ToolCalls),
            logprobs: None,
        }],
        usage: Some(openai::ChatUsage {
            prompt_tokens: 5,
            completion_tokens: 3,
            total_tokens: 8,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        }),
        created: None,
        system_fingerprint: None,
        service_tier: None,
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
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::ToolCalls),
            logprobs: None,
        }],
        usage: None,
        created: None,
        system_fingerprint: None,
        service_tier: None,
    };

    let anth = openai_to_anthropic_response(&resp, "m");
    match &anth.content[0] {
        anthropic::ContentBlock::ToolUse { input, .. } => {
            // parse_tool_arguments wraps invalid JSON in an object
            assert_eq!(input, &json!({"_raw_error": "not json"}));
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
        top_k: None,
        stop_sequences: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
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

#[test]
fn created_timestamp_preserved_from_openai() {
    let resp = basic_openai_response();
    assert_eq!(resp.created, Some(1700000000));
    let anth = openai_to_anthropic_response(&resp, "claude-sonnet-4-6");
    assert_eq!(anth.created, Some(1700000000));
}

#[test]
fn created_timestamp_none_when_absent() {
    let mut resp = basic_openai_response();
    resp.created = None;
    let anth = openai_to_anthropic_response(&resp, "claude-sonnet-4-6");
    assert_eq!(anth.created, None);
    // Verify None created is omitted from JSON
    let json = serde_json::to_string(&anth).unwrap();
    assert!(!json.contains("\"created\""));
}

#[test]
fn thinking_config_stripped_in_translation() {
    let mut req = basic_request();
    req.thinking = Some(anthropic::ThinkingConfig::Enabled {
        budget_tokens: 4096,
    });
    let oai = anthropic_to_openai_request(&req);
    // Thinking has no OpenAI equivalent; verify translation succeeds
    // and the OpenAI request has no thinking field (it's not in the struct)
    assert_eq!(oai.max_completion_tokens, Some(1024));
}

#[test]
fn thinking_block_mapped_to_reasoning_content_in_assistant_translation() {
    let mut req = basic_request();
    req.messages = vec![anthropic::InputMessage {
        role: anthropic::Role::Assistant,
        content: anthropic::Content::Blocks(vec![
            anthropic::ContentBlock::Thinking {
                thinking: "Let me reason...".into(),
                signature: Some("sig_abc".into()),
            },
            anthropic::ContentBlock::Text {
                text: "Here is my answer.".into(),
            },
        ]),
    }];

    let oai = anthropic_to_openai_request(&req);

    // Thinking block mapped to reasoning_content, text block preserved
    assert_eq!(oai.messages.len(), 1);
    assert_eq!(
        oai.messages[0].reasoning_content.as_deref(),
        Some("Let me reason...")
    );
    assert_eq!(
        oai.messages[0].thinking_blocks.as_ref(),
        Some(&vec![openai::ThinkingBlock::Thinking {
            thinking: "Let me reason...".into(),
            signature: Some("sig_abc".into()),
        }])
    );
    assert!(matches!(
        &oai.messages[0].content,
        Some(openai::ChatContent::Text(t)) if t == "Here is my answer."
    ));
}

#[test]
fn redacted_thinking_block_preserved_in_assistant_translation() {
    let mut req = basic_request();
    req.messages = vec![anthropic::InputMessage {
        role: anthropic::Role::Assistant,
        content: anthropic::Content::Blocks(vec![
            anthropic::ContentBlock::RedactedThinking {
                data: "encrypted_data".into(),
            },
            anthropic::ContentBlock::Text {
                text: "My answer.".into(),
            },
        ]),
    }];

    let oai = anthropic_to_openai_request(&req);

    assert_eq!(oai.messages.len(), 1);
    assert_eq!(oai.messages[0].reasoning_content, None);
    assert_eq!(
        oai.messages[0].thinking_blocks.as_ref(),
        Some(&vec![openai::ThinkingBlock::RedactedThinking {
            data: "encrypted_data".into(),
        }])
    );
    assert!(matches!(
        &oai.messages[0].content,
        Some(openai::ChatContent::Text(t)) if t == "My answer."
    ));
}

#[test]
fn temperature_clamped_to_zero_one() {
    let mut req = basic_request();
    req.temperature = Some(1.5);
    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.temperature, Some(1.0));

    req.temperature = Some(0.5);
    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.temperature, Some(0.5));

    req.temperature = Some(-0.1);
    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.temperature, Some(0.0));

    req.temperature = None;
    let oai = anthropic_to_openai_request(&req);
    assert!(oai.temperature.is_none());
}

#[test]
fn metadata_user_id_maps_to_openai_user() {
    let mut req = basic_request();
    req.metadata = Some(anthropic::messages::Metadata {
        user_id: Some("u-abc123".into()),
    });
    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.user.as_deref(), Some("u-abc123"));

    // No metadata: user is None
    req.metadata = None;
    let oai = anthropic_to_openai_request(&req);
    assert!(oai.user.is_none());
}

// --- Claude Code parallel tool use ---

#[test]
fn claude_code_parallel_tool_use_request() {
    // Assistant message with 2 tool_use blocks -> OpenAI message with 2 tool_calls
    let mut req = basic_request();
    req.messages = vec![
        anthropic::InputMessage {
            role: anthropic::Role::User,
            content: anthropic::Content::Text("Read config and list tests.".into()),
        },
        anthropic::InputMessage {
            role: anthropic::Role::Assistant,
            content: anthropic::Content::Blocks(vec![
                anthropic::ContentBlock::Text {
                    text: "I'll do both.".into(),
                },
                anthropic::ContentBlock::ToolUse {
                    id: "toolu_01A".into(),
                    name: "Read".into(),
                    input: json!({"file_path": "/config.toml"}),
                },
                anthropic::ContentBlock::ToolUse {
                    id: "toolu_01B".into(),
                    name: "Glob".into(),
                    input: json!({"pattern": "**/*test*"}),
                },
            ]),
        },
    ];
    let oai = anthropic_to_openai_request(&req);
    // Should produce: user msg, assistant msg with tool_calls
    let assistant_msg = &oai.messages[1];
    assert_eq!(assistant_msg.role, openai::ChatRole::Assistant);
    match assistant_msg.content.as_ref().unwrap() {
        openai::ChatContent::Text(t) => assert_eq!(t, "I'll do both."),
        other => panic!("expected Text content, got {:?}", other),
    }
    let tool_calls = assistant_msg.tool_calls.as_ref().unwrap();
    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_calls[0].id, "toolu_01A");
    assert_eq!(tool_calls[0].function.name, "Read");
    assert_eq!(tool_calls[1].id, "toolu_01B");
    assert_eq!(tool_calls[1].function.name, "Glob");
}

#[test]
fn claude_code_tool_result_request() {
    // User message with tool_result blocks -> OpenAI tool-role messages
    let mut req = basic_request();
    req.messages = vec![anthropic::InputMessage {
        role: anthropic::Role::User,
        content: anthropic::Content::Blocks(vec![
            anthropic::ContentBlock::ToolResult {
                tool_use_id: "toolu_01A".into(),
                content: Some(anthropic::messages::ToolResultContent::Text(
                    "file contents here".into(),
                )),
                is_error: Some(false),
            },
            anthropic::ContentBlock::ToolResult {
                tool_use_id: "toolu_01B".into(),
                content: Some(anthropic::messages::ToolResultContent::Text(
                    "test1.rs\ntest2.rs".into(),
                )),
                is_error: Some(false),
            },
        ]),
    }];
    let oai = anthropic_to_openai_request(&req);
    // Should produce 2 tool-role messages
    assert_eq!(oai.messages.len(), 2);
    assert_eq!(oai.messages[0].role, openai::ChatRole::Tool);
    assert_eq!(oai.messages[0].tool_call_id.as_deref(), Some("toolu_01A"));
    assert_eq!(oai.messages[1].role, openai::ChatRole::Tool);
    assert_eq!(oai.messages[1].tool_call_id.as_deref(), Some("toolu_01B"));
}

#[test]
fn claude_code_tool_response_roundtrip() {
    // OpenAI tool_call response -> Anthropic tool_use, verify fields survive
    let resp = openai::ChatCompletionResponse {
        id: "chatcmpl-llama001".into(),
        object: "chat.completion".into(),
        model: "llama-3.3-70b".into(),
        choices: vec![openai::Choice {
            index: 0,
            message: openai::ChatMessage {
                role: openai::ChatRole::Assistant,
                content: Some(openai::ChatContent::Text("Reading file.".into())),
                name: None,
                tool_calls: Some(vec![
                    openai::ToolCall {
                        id: "call_read_001".into(),
                        call_type: "function".into(),
                        function: openai::FunctionCall {
                            name: "Read".into(),
                            arguments: r#"{"file_path":"/config.toml"}"#.into(),
                        },
                    },
                    openai::ToolCall {
                        id: "call_glob_001".into(),
                        call_type: "function".into(),
                        function: openai::FunctionCall {
                            name: "Glob".into(),
                            arguments: r#"{"pattern":"**/*test*"}"#.into(),
                        },
                    },
                ]),
                tool_call_id: None,
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::ToolCalls),
            logprobs: None,
        }],
        usage: Some(openai::ChatUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        }),
        created: None,
        system_fingerprint: None,
        service_tier: None,
    };

    let anth = openai_to_anthropic_response(&resp, "claude-sonnet-4-20250514");
    assert_eq!(anth.stop_reason, Some(anthropic::StopReason::ToolUse));
    // First block is text
    match &anth.content[0] {
        anthropic::ContentBlock::Text { text } => assert_eq!(text, "Reading file."),
        other => panic!("expected Text, got {:?}", other),
    }
    // Second and third blocks are tool_use
    match &anth.content[1] {
        anthropic::ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_read_001");
            assert_eq!(name, "Read");
            assert_eq!(input["file_path"], "/config.toml");
        }
        other => panic!("expected ToolUse, got {:?}", other),
    }
    match &anth.content[2] {
        anthropic::ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_glob_001");
            assert_eq!(name, "Glob");
            assert_eq!(input["pattern"], "**/*test*");
        }
        other => panic!("expected ToolUse, got {:?}", other),
    }
}

// --- Local LLM robustness ---

#[test]
fn tool_call_empty_id_gets_synthetic_id() {
    let resp = openai::ChatCompletionResponse {
        id: "x".into(),
        object: "chat.completion".into(),
        model: "llama".into(),
        choices: vec![openai::Choice {
            index: 0,
            message: openai::ChatMessage {
                role: openai::ChatRole::Assistant,
                content: None,
                name: None,
                tool_calls: Some(vec![openai::ToolCall {
                    id: "".into(), // empty ID from local LLM
                    call_type: "function".into(),
                    function: openai::FunctionCall {
                        name: "Read".into(),
                        arguments: r#"{"file_path":"/tmp/x"}"#.into(),
                    },
                }]),
                tool_call_id: None,
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::ToolCalls),
            logprobs: None,
        }],
        usage: None,
        created: None,
        system_fingerprint: None,
        service_tier: None,
    };

    let anth = openai_to_anthropic_response(&resp, "m");
    match &anth.content[0] {
        anthropic::ContentBlock::ToolUse { id, name, .. } => {
            assert!(
                id.starts_with("toolu_"),
                "expected synthetic toolu_ ID, got: {}",
                id
            );
            assert_eq!(name, "Read");
        }
        other => panic!("expected ToolUse, got {:?}", other),
    }
}

#[test]
fn tool_call_empty_arguments_becomes_empty_object() {
    let resp = openai::ChatCompletionResponse {
        id: "x".into(),
        object: "chat.completion".into(),
        model: "llama".into(),
        choices: vec![openai::Choice {
            index: 0,
            message: openai::ChatMessage {
                role: openai::ChatRole::Assistant,
                content: None,
                name: None,
                tool_calls: Some(vec![openai::ToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: openai::FunctionCall {
                        name: "Bash".into(),
                        arguments: "".into(), // empty args from local LLM
                    },
                }]),
                tool_call_id: None,
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::ToolCalls),
            logprobs: None,
        }],
        usage: None,
        created: None,
        system_fingerprint: None,
        service_tier: None,
    };

    let anth = openai_to_anthropic_response(&resp, "m");
    match &anth.content[0] {
        anthropic::ContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &json!({}));
        }
        other => panic!("expected ToolUse, got {:?}", other),
    }
}

#[test]
fn tool_call_missing_name_skipped() {
    let resp = openai::ChatCompletionResponse {
        id: "x".into(),
        object: "chat.completion".into(),
        model: "llama".into(),
        choices: vec![openai::Choice {
            index: 0,
            message: openai::ChatMessage {
                role: openai::ChatRole::Assistant,
                content: Some(openai::ChatContent::Text("text".into())),
                name: None,
                tool_calls: Some(vec![
                    openai::ToolCall {
                        id: "call_1".into(),
                        call_type: "function".into(),
                        function: openai::FunctionCall {
                            name: "".into(), // empty name
                            arguments: "{}".into(),
                        },
                    },
                    openai::ToolCall {
                        id: "call_2".into(),
                        call_type: "function".into(),
                        function: openai::FunctionCall {
                            name: "Read".into(),
                            arguments: r#"{"file_path":"/x"}"#.into(),
                        },
                    },
                ]),
                tool_call_id: None,
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::ToolCalls),
            logprobs: None,
        }],
        usage: None,
        created: None,
        system_fingerprint: None,
        service_tier: None,
    };

    let anth = openai_to_anthropic_response(&resp, "m");
    // Empty-name tool call skipped; text + valid tool call remain
    assert_eq!(anth.content.len(), 2);
    match &anth.content[0] {
        anthropic::ContentBlock::Text { text } => assert_eq!(text, "text"),
        other => panic!("expected Text, got {:?}", other),
    }
    match &anth.content[1] {
        anthropic::ContentBlock::ToolUse { name, .. } => assert_eq!(name, "Read"),
        other => panic!("expected ToolUse, got {:?}", other),
    }
}

#[test]
fn refusal_mapped_to_text_block() {
    let resp = openai::ChatCompletionResponse {
        id: "chatcmpl-1".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![openai::Choice {
            index: 0,
            message: openai::ChatMessage {
                role: openai::ChatRole::Assistant,
                content: None,
                name: None,
                tool_calls: None,
                tool_call_id: None,
                refusal: Some("I cannot help with that request.".into()),
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::ContentFilter),
            logprobs: None,
        }],
        usage: None,
        created: None,
        system_fingerprint: None,
        service_tier: None,
    };

    let anth = openai_to_anthropic_response(&resp, "m");
    assert_eq!(anth.content.len(), 1);
    match &anth.content[0] {
        anthropic::ContentBlock::Text { text } => {
            assert!(text.contains("Refusal"));
            assert!(text.contains("I cannot help with that request."));
        }
        other => panic!("expected Text with refusal, got {:?}", other),
    }
}

#[test]
fn extra_fields_forwarded_to_openai_request() {
    let mut req = basic_request();
    req.extra
        .insert("seed".into(), serde_json::Value::Number(42.into()));
    req.extra
        .insert("logprobs".into(), serde_json::Value::Bool(true));

    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.extra.get("seed"), Some(&json!(42)));
    assert_eq!(oai.extra.get("logprobs"), Some(&json!(true)));
}

#[test]
fn n_parameter_stripped_from_extra() {
    let mut req = basic_request();
    req.extra.insert("n".into(), json!(4));
    req.extra.insert("seed".into(), json!(42));
    let oai = anthropic_to_openai_request(&req);
    assert!(oai.extra.get("n").is_none());
    assert_eq!(oai.extra.get("seed"), Some(&json!(42)));
}

#[test]
fn n_parameter_one_stripped_silently() {
    let mut req = basic_request();
    req.extra.insert("n".into(), json!(1));
    let oai = anthropic_to_openai_request(&req);
    assert!(oai.extra.get("n").is_none());
}

#[test]
fn reasoning_content_mapped_to_thinking_block_in_response() {
    let oai_resp = openai::ChatCompletionResponse {
        id: "chatcmpl-1".into(),
        object: "chat.completion".into(),
        model: "deepseek-reasoner".into(),
        choices: vec![openai::Choice {
            index: 0,
            message: openai::ChatMessage {
                role: openai::ChatRole::Assistant,
                content: Some(openai::ChatContent::Text("The answer is 4.".into())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                refusal: None,
                reasoning_content: Some("Let me think... 2+2=4".into()),
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::Stop),
            logprobs: None,
        }],
        usage: Some(openai::ChatUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        }),
        created: None,
        system_fingerprint: None,
        service_tier: None,
    };
    let resp = openai_to_anthropic_response(&oai_resp, "deepseek-reasoner");
    // First block should be thinking, second should be text
    assert_eq!(resp.content.len(), 2);
    match &resp.content[0] {
        anthropic::ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "Let me think... 2+2=4");
            assert!(signature.is_none());
        }
        other => panic!("expected Thinking block, got {:?}", other),
    }
    match &resp.content[1] {
        anthropic::ContentBlock::Text { text } => {
            assert_eq!(text, "The answer is 4.");
        }
        other => panic!("expected Text block, got {:?}", other),
    }
}

#[test]
fn thinking_blocks_preferred_over_reasoning_content_in_response() {
    let oai_resp = openai::ChatCompletionResponse {
        id: "chatcmpl-1".into(),
        object: "chat.completion".into(),
        model: "claude-sonnet-4-20250514".into(),
        choices: vec![openai::Choice {
            index: 0,
            message: openai::ChatMessage {
                role: openai::ChatRole::Assistant,
                content: Some(openai::ChatContent::Text("Use the tool.".into())),
                name: None,
                tool_calls: Some(vec![openai::ToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: openai::FunctionCall {
                        name: "get_weather".into(),
                        arguments: "{\"location\":\"NYC\"}".into(),
                    },
                }]),
                tool_call_id: None,
                refusal: None,
                reasoning_content: Some("lossy text".into()),
                thinking_blocks: Some(vec![
                    openai::ThinkingBlock::Thinking {
                        thinking: "signed thought".into(),
                        signature: Some("sig_123".into()),
                    },
                    openai::ThinkingBlock::RedactedThinking {
                        data: "encrypted".into(),
                    },
                ]),
            },
            finish_reason: Some(openai::FinishReason::ToolCalls),
            logprobs: None,
        }],
        usage: None,
        created: None,
        system_fingerprint: None,
        service_tier: None,
    };

    let resp = openai_to_anthropic_response(&oai_resp, "claude-sonnet-4-20250514");

    assert!(matches!(
        &resp.content[0],
        anthropic::ContentBlock::Thinking {
            thinking,
            signature: Some(signature),
        } if thinking == "signed thought" && signature == "sig_123"
    ));
    assert!(matches!(
        &resp.content[1],
        anthropic::ContentBlock::RedactedThinking { data } if data == "encrypted"
    ));
    assert!(matches!(
        &resp.content[3],
        anthropic::ContentBlock::ToolUse { id, .. } if id == "call_1"
    ));
}

#[test]
fn thinking_block_mapped_to_reasoning_content_in_request() {
    let mut req = basic_request();
    req.messages = vec![anthropic::InputMessage {
        role: anthropic::Role::Assistant,
        content: anthropic::Content::Blocks(vec![
            anthropic::ContentBlock::Thinking {
                thinking: "Let me reason...".into(),
                signature: Some("sig_abc".into()),
            },
            anthropic::ContentBlock::Text {
                text: "Here is my answer.".into(),
            },
        ]),
    }];

    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.messages.len(), 1);
    assert_eq!(
        oai.messages[0].reasoning_content.as_deref(),
        Some("Let me reason...")
    );
    assert_eq!(
        oai.messages[0].thinking_blocks.as_ref(),
        Some(&vec![openai::ThinkingBlock::Thinking {
            thinking: "Let me reason...".into(),
            signature: Some("sig_abc".into()),
        }])
    );
    assert!(matches!(
        &oai.messages[0].content,
        Some(openai::ChatContent::Text(t)) if t == "Here is my answer."
    ));
}

#[test]
fn unknown_finish_reason_maps_to_end_turn() {
    let oai_resp = openai::ChatCompletionResponse {
        id: "chatcmpl-1".into(),
        object: "chat.completion".into(),
        model: "deepseek-chat".into(),
        choices: vec![openai::Choice {
            index: 0,
            message: openai::ChatMessage {
                role: openai::ChatRole::Assistant,
                content: Some(openai::ChatContent::Text("Sorry".into())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                refusal: None,
                reasoning_content: None,
                thinking_blocks: None,
            },
            finish_reason: Some(openai::FinishReason::Unknown),
            logprobs: None,
        }],
        usage: None,
        created: None,
        system_fingerprint: None,
        service_tier: None,
    };
    let resp = openai_to_anthropic_response(&oai_resp, "deepseek-chat");
    assert_eq!(resp.stop_reason, Some(anthropic::StopReason::EndTurn));
}

#[test]
fn is_o_series_model_matches() {
    assert!(is_o_series_model("o1"));
    assert!(is_o_series_model("o3"));
    assert!(is_o_series_model("o4"));
    assert!(is_o_series_model("o1-mini"));
    assert!(is_o_series_model("o1-preview"));
    assert!(is_o_series_model("o3-mini"));
    assert!(is_o_series_model("o4-mini"));
    assert!(is_o_series_model("O1")); // case-insensitive
    assert!(is_o_series_model("O3-Mini"));
}

#[test]
fn is_o_series_model_rejects() {
    assert!(!is_o_series_model("gpt-4o"));
    assert!(!is_o_series_model("gpt-4o-mini"));
    assert!(!is_o_series_model("gpt-4"));
    assert!(!is_o_series_model("claude-3-opus"));
}

#[test]
fn is_o_series_model_future_models() {
    assert!(is_o_series_model("o2"), "o2 must match");
    assert!(is_o_series_model("o5"), "o5 must match");
    assert!(is_o_series_model("o10"), "o10 (multi-digit) must match");
    assert!(is_o_series_model("o2-mini"), "o2-mini must match");
    assert!(
        !is_o_series_model("o-preview"),
        "bare 'o' with no digit must not match"
    );
    assert!(
        !is_o_series_model("openai-o1"),
        "o1 not at start must not match"
    );
}

fn make_request(model: &str, system: Option<&str>) -> anthropic::MessageCreateRequest {
    anthropic::MessageCreateRequest {
        model: model.into(),
        max_tokens: 1024,
        messages: vec![],
        system: system.map(|s| anthropic::System::Text(s.into())),
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

#[test]
fn o_series_model_gets_only_max_completion_tokens() {
    let req = make_request("o1-mini", Some("You are helpful."));
    let oai = anthropic_to_openai_request(&req);
    assert!(
        oai.max_tokens.is_none(),
        "o-series should not set max_tokens"
    );
    assert_eq!(oai.max_completion_tokens, Some(1024));
    // System role should be converted to Developer for o-series.
    assert_eq!(oai.messages[0].role, openai::ChatRole::Developer);
}

#[test]
fn non_o_series_model_gets_both_max_tokens() {
    let req = make_request("gpt-4o", Some("You are helpful."));
    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.max_tokens, Some(1024));
    assert_eq!(oai.max_completion_tokens, Some(1024));
    // System role should remain System for non-o-series.
    assert_eq!(oai.messages[0].role, openai::ChatRole::System);
}

#[test]
fn o_series_ga_strips_temperature() {
    // All o-series models (including GA variants like o3-mini) reject temperature.
    let mut req = make_request("o3-mini", None);
    req.temperature = Some(0.7);
    let oai = anthropic_to_openai_request(&req);
    assert!(
        oai.temperature.is_none(),
        "o3-mini should strip temperature (all o-series reject it)"
    );
}

#[test]
fn o_series_preview_strips_temperature() {
    // All o-series models including early previews do not support temperature.
    let mut req = make_request("o1-preview", None);
    req.temperature = Some(0.7);
    let oai = anthropic_to_openai_request(&req);
    assert!(
        oai.temperature.is_none(),
        "o1-preview should strip temperature"
    );
}

#[test]
fn o_series_preview_strips_top_p() {
    let mut req = make_request("o1-preview", None);
    req.top_p = Some(0.9);
    let oai = anthropic_to_openai_request(&req);
    assert!(oai.top_p.is_none(), "o1-preview should strip top_p");
}

#[test]
fn o1_mini_strips_top_p() {
    let mut req = make_request("o1-mini", None);
    req.top_p = Some(0.9);
    let oai = anthropic_to_openai_request(&req);
    assert!(oai.top_p.is_none(), "o1-mini should strip top_p");
}

#[test]
fn non_o_series_preserves_temperature() {
    let mut req = make_request("gpt-4o", None);
    req.temperature = Some(0.7);
    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.temperature, Some(0.7));
}

// --- compute_request_warnings ---

#[test]
fn warnings_empty_for_plain_request() {
    let req = basic_request();
    let w = compute_request_warnings(&req);
    assert!(w.is_empty());
    assert!(w.as_header_value().is_none());
}

#[test]
fn warnings_top_k() {
    let mut req = basic_request();
    req.top_k = Some(40);
    let w = compute_request_warnings(&req);
    assert_eq!(w.as_header_value().unwrap(), "top_k");
}

#[test]
fn warnings_thinking_config() {
    let mut req = basic_request();
    req.thinking = Some(anthropic::ThinkingConfig::Enabled {
        budget_tokens: 5000,
    });
    let w = compute_request_warnings(&req);
    assert_eq!(w.as_header_value().unwrap(), "thinking_config");
}

#[test]
fn warnings_stop_sequences_truncated_at_5() {
    let mut req = basic_request();
    req.stop_sequences = Some(vec![
        "a".to_string(),
        "b".to_string(),
        "c".to_string(),
        "d".to_string(),
        "e".to_string(),
    ]);
    let w = compute_request_warnings(&req);
    assert_eq!(w.as_header_value().unwrap(), "stop_sequences_truncated");
}

#[test]
fn warnings_stop_sequences_4_is_fine() {
    let mut req = basic_request();
    req.stop_sequences = Some(vec![
        "a".to_string(),
        "b".to_string(),
        "c".to_string(),
        "d".to_string(),
    ]);
    let w = compute_request_warnings(&req);
    assert!(w.is_empty());
}

#[test]
fn warnings_cache_control_on_system() {
    let mut req = basic_request();
    req.system = Some(anthropic::System::Blocks(vec![anthropic::SystemBlock {
        block_type: "text".to_string(),
        text: "You are helpful.".to_string(),
        cache_control: Some(anthropic::CacheControl {
            cache_type: "ephemeral".to_string(),
        }),
    }]));
    let w = compute_request_warnings(&req);
    assert_eq!(w.as_header_value().unwrap(), "cache_control");
}

#[test]
fn warnings_document_blocks() {
    let mut req = basic_request();
    req.messages = vec![anthropic::InputMessage {
        role: anthropic::Role::User,
        content: anthropic::Content::Blocks(vec![anthropic::ContentBlock::Document {
            source: anthropic::DocumentSource {
                source_type: "base64".to_string(),
                media_type: "application/pdf".to_string(),
                data: "dGVzdA==".to_string(),
            },
            title: None,
        }]),
    }];
    let w = compute_request_warnings(&req);
    assert_eq!(w.as_header_value().unwrap(), "document_blocks");
}

#[test]
fn warnings_multiple_combined() {
    let mut req = basic_request();
    req.top_k = Some(10);
    req.thinking = Some(anthropic::ThinkingConfig::Enabled {
        budget_tokens: 1000,
    });
    let w = compute_request_warnings(&req);
    let val = w.as_header_value().unwrap();
    assert!(val.contains("top_k"), "missing top_k in: {val}");
    assert!(
        val.contains("thinking_config"),
        "missing thinking_config in: {val}"
    );
}

#[test]
fn forced_tool_choice_enables_strict_mode_in_openai_request() {
    let anthropic_req: anthropic::MessageCreateRequest =
        serde_json::from_value(serde_json::json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Extract the data"}],
            "tools": [{
                "name": "extract_data",
                "description": "Extract structured data",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "value": {"type": "integer"}
                    }
                }
            }],
            "tool_choice": {"type": "tool", "name": "extract_data"}
        }))
        .unwrap();

    let openai_req = anthropic_to_openai_request(&anthropic_req);

    let tools = openai_req.tools.expect("tools should be present");
    assert_eq!(tools.len(), 1);

    // The tool should have strict: true.
    assert_eq!(tools[0].function.strict, Some(true));

    // The schema should have additionalProperties: false.
    let params = tools[0]
        .function
        .parameters
        .as_ref()
        .expect("parameters should be present");
    assert_eq!(params["additionalProperties"], serde_json::json!(false));

    // required should include both properties.
    let required = params["required"]
        .as_array()
        .expect("required should be present");
    assert!(required.iter().any(|v| v == "name"));
    assert!(required.iter().any(|v| v == "value"));
}

#[test]
fn omit_max_tokens_marker_clears_and_is_stripped() {
    // Without the marker: max_tokens is round-tripped from the Anthropic request.
    let mut req = basic_request();
    req.max_tokens = 1024;
    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.max_tokens, Some(1024));
    assert_eq!(oai.max_completion_tokens, Some(1024));

    // With the marker: both cleared so the backend applies its own default,
    // and the marker itself is stripped so it never leaks upstream.
    let mut req = basic_request();
    req.extra
        .insert(OMIT_MAX_TOKENS_MARKER.to_string(), json!(true));
    let oai = anthropic_to_openai_request(&req);
    assert_eq!(oai.max_tokens, None);
    assert_eq!(oai.max_completion_tokens, None);
    assert!(!oai.extra.contains_key(OMIT_MAX_TOKENS_MARKER));
}
