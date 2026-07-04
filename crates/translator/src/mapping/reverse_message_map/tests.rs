use super::*;
use serde_json::json;

fn make_basic_request() -> openai::ChatCompletionRequest {
    serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "max_tokens": 100
    }))
    .unwrap()
}

#[test]
fn basic_message_conversion() {
    let req = make_basic_request();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w).unwrap();
    assert_eq!(result.model, "claude-sonnet-4-20250514");
    assert_eq!(result.max_tokens, 100);
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].role, anthropic::Role::User);
}

#[test]
fn system_message_extraction() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [
            {"role": "system", "content": "You are helpful."},
            {"role": "user", "content": "Hi"}
        ],
        "max_tokens": 100
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w).unwrap();
    assert!(
        matches!(result.system, Some(anthropic::System::Text(ref s)) if s == "You are helpful.")
    );
    assert_eq!(result.messages.len(), 1); // system not in messages
}

#[test]
fn developer_role_maps_to_system() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [
            {"role": "developer", "content": "Be concise."},
            {"role": "user", "content": "Hi"}
        ],
        "max_tokens": 100
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w).unwrap();
    assert!(matches!(result.system, Some(anthropic::System::Text(ref s)) if s == "Be concise."));
}

#[test]
fn missing_max_tokens_rejected() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "Hi"}]
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w);
    assert!(result.is_err());
}

#[test]
fn max_completion_tokens_used_as_fallback() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "Hi"}],
        "max_completion_tokens": 200
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w).unwrap();
    assert_eq!(result.max_tokens, 200);
}

#[test]
fn tool_call_conversion() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [
            {"role": "user", "content": "Weather?"},
            {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"loc\":\"NYC\"}"}
                }]
            },
            {"role": "tool", "tool_call_id": "call_1", "content": "Sunny, 72F"}
        ],
        "tools": [{"type": "function", "function": {"name": "get_weather", "parameters": {"type": "object"}}}],
        "max_tokens": 100
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w).unwrap();
    assert_eq!(result.messages.len(), 3);
    assert!(result.tools.is_some());
    // Second message (assistant) should have tool_use block
    match &result.messages[1].content {
        anthropic::Content::Blocks(blocks) => {
            assert!(
                matches!(&blocks[0], anthropic::ContentBlock::ToolUse { name, .. } if name == "get_weather")
            );
        }
        _ => panic!("expected blocks"),
    }
    // Third message (tool result) should be user with tool_result
    assert_eq!(result.messages[2].role, anthropic::Role::User);
}

#[test]
fn thinking_blocks_with_tool_calls_convert_to_signed_anthropic_blocks() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{
            "role": "assistant",
            "content": "I will check.",
            "reasoning_content": "lossy text",
            "thinking_blocks": [
                {"type": "thinking", "thinking": "signed thought", "signature": "sig_123"},
                {"type": "redacted_thinking", "data": "encrypted"}
            ],
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "get_weather", "arguments": "{\"loc\":\"NYC\"}"}
            }]
        }],
        "max_tokens": 100
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w).unwrap();

    let anthropic::Content::Blocks(blocks) = &result.messages[0].content else {
        panic!("expected assistant blocks");
    };
    assert!(matches!(
        &blocks[0],
        anthropic::ContentBlock::Thinking {
            thinking,
            signature: Some(signature),
        } if thinking == "signed thought" && signature == "sig_123"
    ));
    assert!(matches!(
        &blocks[1],
        anthropic::ContentBlock::RedactedThinking { data } if data == "encrypted"
    ));
    assert!(matches!(
        &blocks[2],
        anthropic::ContentBlock::Text { text } if text == "I will check."
    ));
    assert!(matches!(
        &blocks[3],
        anthropic::ContentBlock::ToolUse { id, .. } if id == "call_1"
    ));
}

#[test]
fn reasoning_content_with_tool_calls_does_not_synthesize_unsigned_thinking() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{
            "role": "assistant",
            "reasoning_content": "unsigned thought",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "get_weather", "arguments": "{}"}
            }]
        }],
        "max_tokens": 100
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w).unwrap();

    let anthropic::Content::Blocks(blocks) = &result.messages[0].content else {
        panic!("expected assistant blocks");
    };
    assert!(!blocks
        .iter()
        .any(|block| matches!(block, anthropic::ContentBlock::Thinking { .. })));
    assert!(matches!(
        &blocks[0],
        anthropic::ContentBlock::ToolUse { id, .. } if id == "call_1"
    ));
}

#[test]
fn context_translation_sanitizes_tool_names_and_restores_response_names() {
    let long_name = format!("{}!", "x".repeat(130));
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [
            {"role": "user", "content": "Use tools"},
            {
                "role": "assistant",
                "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "bad.name", "arguments": "{}"}},
                    {"id": "call_2", "type": "function", "function": {"name": "bad/name", "arguments": "{}"}},
                    {"id": "call_3", "type": "function", "function": {"name": long_name, "arguments": "{}"}}
                ]
            }
        ],
        "tools": [
            {"type": "function", "function": {"name": "bad.name", "parameters": {"type": "object"}}},
            {"type": "function", "function": {"name": "bad/name", "parameters": {"type": "string"}}},
            {"type": "function", "function": {"name": long_name, "parameters": null}}
        ],
        "tool_choice": {"type": "function", "function": {"name": "bad.name"}},
        "max_tokens": 100
    }))
    .unwrap();

    let mut w = TranslationWarnings::default();
    let (anthropic_req, context) = openai_to_anthropic_request_with_context(&req, &mut w).unwrap();
    let tools = anthropic_req.tools.as_ref().unwrap();
    assert_eq!(tools[0].name, "bad_name");
    assert_eq!(tools[1].name, "bad_name_2");
    assert_eq!(tools[2].name.len(), 128);
    assert_eq!(tools[1].input_schema["type"], "object");
    assert_eq!(tools[1].input_schema["properties"], json!({}));
    assert!(matches!(
        anthropic_req.tool_choice,
        Some(anthropic::ToolChoice::Tool { ref name }) if name == "bad_name"
    ));

    let resp = anthropic::MessageResponse {
        id: "msg_tools".to_string(),
        response_type: "message".to_string(),
        role: anthropic::Role::Assistant,
        content: vec![anthropic::ContentBlock::ServerToolUse {
            id: "call_1".to_string(),
            name: "bad_name".to_string(),
            input: json!({}),
        }],
        model: "claude-sonnet-4-20250514".to_string(),
        stop_reason: Some(anthropic::StopReason::ToolUse),
        stop_sequence: None,
        usage: anthropic::Usage::default(),
        created: None,
    };
    let result =
        anthropic_to_openai_response_with_context(&resp, "claude-sonnet-4-20250514", &context);
    assert_eq!(
        result.choices[0].message.tool_calls.as_ref().unwrap()[0]
            .function
            .name,
        "bad.name"
    );
}

#[test]
fn lossy_fields_generate_warnings() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "Hi"}],
        "max_tokens": 100,
        "presence_penalty": 0.5,
        "frequency_penalty": 0.3,
        "logprobs": true,
        "seed": 42
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    openai_to_anthropic_request(&req, &mut w).unwrap();
    let header = w.as_header_value().unwrap();
    assert!(header.contains("presence_penalty"));
    assert!(header.contains("frequency_penalty"));
    assert!(header.contains("logprobs"));
    assert!(header.contains("seed"));
}

#[test]
fn stop_sequences_mapping() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "Hi"}],
        "max_tokens": 100,
        "stop": ["END", "STOP"]
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w).unwrap();
    assert_eq!(
        result.stop_sequences,
        Some(vec!["END".into(), "STOP".into()])
    );
}

// --- Response tests ---

#[test]
fn basic_response_conversion() {
    let resp = anthropic::MessageResponse {
        id: "msg_123".to_string(),
        response_type: "message".to_string(),
        role: anthropic::Role::Assistant,
        content: vec![anthropic::ContentBlock::Text {
            text: "Hello!".to_string(),
        }],
        model: "claude-sonnet-4-20250514".to_string(),
        stop_reason: Some(anthropic::StopReason::EndTurn),
        stop_sequence: None,
        usage: anthropic::Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            ..Default::default()
        },
        created: Some(1700000000),
    };
    let result = anthropic_to_openai_response(&resp, "claude-sonnet-4-20250514");
    assert_eq!(result.object, "chat.completion");
    assert!(result.id.starts_with("chatcmpl-"));
    assert_eq!(result.choices.len(), 1);
    match &result.choices[0].message.content {
        Some(openai::ChatContent::Text(s)) => assert_eq!(s, "Hello!"),
        other => panic!("expected Text, got {:?}", other),
    }
    assert_eq!(
        result.choices[0].finish_reason,
        Some(openai::FinishReason::Stop)
    );
    let usage = result.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 10);
    assert_eq!(usage.completion_tokens, 5);
}

#[test]
fn tool_use_response_conversion() {
    let resp = anthropic::MessageResponse {
        id: "msg_456".to_string(),
        response_type: "message".to_string(),
        role: anthropic::Role::Assistant,
        content: vec![anthropic::ContentBlock::ToolUse {
            id: "call_1".to_string(),
            name: "get_weather".to_string(),
            input: json!({"location": "NYC"}),
        }],
        model: "claude-sonnet-4-20250514".to_string(),
        stop_reason: Some(anthropic::StopReason::ToolUse),
        stop_sequence: None,
        usage: anthropic::Usage::default(),
        created: None,
    };
    let result = anthropic_to_openai_response(&resp, "claude-sonnet-4-20250514");
    let tc = result.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tc.len(), 1);
    assert_eq!(tc[0].id, "call_1");
    assert_eq!(tc[0].function.name, "get_weather");
    assert_eq!(
        result.choices[0].finish_reason,
        Some(openai::FinishReason::ToolCalls)
    );
}

#[test]
fn thinking_block_maps_to_reasoning_content() {
    let resp = anthropic::MessageResponse {
        id: "msg_789".to_string(),
        response_type: "message".to_string(),
        role: anthropic::Role::Assistant,
        content: vec![
            anthropic::ContentBlock::Thinking {
                thinking: "Let me think...".to_string(),
                signature: None,
            },
            anthropic::ContentBlock::Text {
                text: "The answer is 4.".to_string(),
            },
        ],
        model: "claude-sonnet-4-20250514".to_string(),
        stop_reason: Some(anthropic::StopReason::EndTurn),
        stop_sequence: None,
        usage: anthropic::Usage::default(),
        created: None,
    };
    let result = anthropic_to_openai_response(&resp, "claude-sonnet-4-20250514");
    assert_eq!(
        result.choices[0].message.reasoning_content.as_deref(),
        Some("Let me think...")
    );
    match &result.choices[0].message.content {
        Some(openai::ChatContent::Text(s)) => assert_eq!(s, "The answer is 4."),
        other => panic!("expected Text, got {:?}", other),
    }
}

#[test]
fn stop_reason_mapping() {
    assert_eq!(
        anthropic_stop_reason_to_openai(&anthropic::StopReason::EndTurn),
        openai::FinishReason::Stop
    );
    assert_eq!(
        anthropic_stop_reason_to_openai(&anthropic::StopReason::MaxTokens),
        openai::FinishReason::Length
    );
    assert_eq!(
        anthropic_stop_reason_to_openai(&anthropic::StopReason::ToolUse),
        openai::FinishReason::ToolCalls
    );
    assert_eq!(
        anthropic_stop_reason_to_openai(&anthropic::StopReason::StopSequence),
        openai::FinishReason::Stop
    );
    assert_eq!(
        anthropic_stop_reason_to_openai(&anthropic::StopReason::PauseTurn),
        openai::FinishReason::Stop
    );
    assert_eq!(
        anthropic_stop_reason_to_openai(&anthropic::StopReason::Refusal),
        openai::FinishReason::ContentFilter
    );
}

#[test]
fn data_uri_image_parsing() {
    let source = url_to_image_source("data:image/png;base64,iVBORw0KGgo=");
    assert_eq!(source.source_type, "base64");
    assert_eq!(source.media_type.as_deref(), Some("image/png"));
    assert_eq!(source.data.as_deref(), Some("iVBORw0KGgo="));
    assert!(source.url.is_none());
}

#[test]
fn regular_url_image_source() {
    let source = url_to_image_source("https://example.com/img.png");
    assert_eq!(source.source_type, "url");
    assert_eq!(source.url.as_deref(), Some("https://example.com/img.png"));
    assert!(source.data.is_none());
}

#[test]
fn user_field_maps_to_metadata() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "Hi"}],
        "max_tokens": 100,
        "user": "user-123"
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w).unwrap();
    assert_eq!(
        result.metadata.as_ref().and_then(|m| m.user_id.as_deref()),
        Some("user-123")
    );
}

#[test]
fn parallel_tool_calls_false_maps_to_disable() {
    let req: openai::ChatCompletionRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "Hi"}],
        "max_tokens": 100,
        "tools": [{"type": "function", "function": {"name": "test", "parameters": {"type": "object"}}}],
        "tool_choice": "auto",
        "parallel_tool_calls": false
    }))
    .unwrap();
    let mut w = TranslationWarnings::default();
    let result = openai_to_anthropic_request(&req, &mut w).unwrap();
    assert!(matches!(
        result.tool_choice,
        Some(anthropic::ToolChoice::Auto {
            disable_parallel_tool_use: Some(true)
        })
    ));
}
