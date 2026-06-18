use super::*;
use serde_json::json;

fn simple_request() -> anthropic::MessageCreateRequest {
    serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    }))
    .unwrap()
}

#[test]
fn basic_text_request() {
    let req = simple_request();
    let responses_req = anthropic_to_responses_request(&req);

    assert_eq!(responses_req.model, "claude-sonnet-4-6");
    assert_eq!(responses_req.max_output_tokens, Some(1024));
    assert!(responses_req.instructions.is_none());

    match &responses_req.input {
        ResponsesInput::Items(items) => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0]["type"], "message");
            assert_eq!(items[0]["role"], "user");
            assert_eq!(items[0]["content"][0]["type"], "input_text");
            assert_eq!(items[0]["content"][0]["text"], "Hello");
        }
        _ => panic!("expected Items input"),
    }
}

#[test]
fn system_prompt_to_instructions() {
    let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 100,
        "system": "You are helpful",
        "messages": [{"role": "user", "content": "Hi"}]
    }))
    .unwrap();

    let responses_req = anthropic_to_responses_request(&req);
    assert_eq!(responses_req.instructions, Some("You are helpful".into()));
}

#[test]
fn multi_turn_conversation() {
    let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 100,
        "messages": [
            {"role": "user", "content": "What is 2+2?"},
            {"role": "assistant", "content": "4"},
            {"role": "user", "content": "And 3+3?"}
        ]
    }))
    .unwrap();

    let responses_req = anthropic_to_responses_request(&req);
    match &responses_req.input {
        ResponsesInput::Items(items) => {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0]["role"], "user");
            assert_eq!(items[1]["role"], "assistant");
            assert_eq!(items[2]["role"], "user");
        }
        _ => panic!("expected Items input"),
    }
}

#[test]
fn tool_definitions_mapping() {
    let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 100,
        "tools": [{
            "name": "get_weather",
            "description": "Get weather for a city",
            "input_schema": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }
        }],
        "messages": [{"role": "user", "content": "Weather in NYC?"}]
    }))
    .unwrap();

    let responses_req = anthropic_to_responses_request(&req);
    let tools = responses_req.tools.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["name"], "get_weather");
    assert_eq!(tools[0]["description"], "Get weather for a city");
    assert!(tools[0]["parameters"]["properties"]["city"].is_object());
}

#[test]
fn tool_use_in_assistant_message() {
    let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 100,
        "messages": [
            {"role": "user", "content": "Weather?"},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_123", "name": "get_weather", "input": {"city": "NYC"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_123", "content": "72F sunny"}
            ]}
        ]
    }))
    .unwrap();

    let responses_req = anthropic_to_responses_request(&req);
    match &responses_req.input {
        ResponsesInput::Items(items) => {
            // user message, function_call item, function_call_output item
            assert_eq!(items.len(), 3);
            assert_eq!(items[0]["type"], "message");
            assert_eq!(items[1]["type"], "function_call");
            assert_eq!(items[1]["call_id"], "toolu_123");
            assert_eq!(items[1]["name"], "get_weather");
            assert_eq!(items[2]["type"], "function_call_output");
            assert_eq!(items[2]["call_id"], "toolu_123");
            assert_eq!(items[2]["output"], "72F sunny");
        }
        _ => panic!("expected Items input"),
    }
}

#[test]
fn temperature_clamped() {
    let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 100,
        "temperature": 1.5,
        "messages": [{"role": "user", "content": "Hi"}]
    }))
    .unwrap();

    let responses_req = anthropic_to_responses_request(&req);
    assert_eq!(responses_req.temperature, Some(1.0));
}

#[test]
fn stop_sequences_truncated() {
    let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 100,
        "stop_sequences": ["a", "b", "c", "d", "e"],
        "messages": [{"role": "user", "content": "Hi"}]
    }))
    .unwrap();

    let responses_req = anthropic_to_responses_request(&req);
    let stop = responses_req.extra.get("stop").unwrap().as_array().unwrap();
    assert_eq!(stop.len(), 4);
}

#[test]
fn basic_text_response() {
    let resp: ResponsesResponse = serde_json::from_value(json!({
        "id": "resp_abc",
        "type": "response",
        "model": "gpt-4o",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "Hello!"}],
            "id": "msg_1",
            "status": "completed"
        }],
        "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15},
        "status": "completed"
    }))
    .unwrap();

    let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
    assert_eq!(anthropic_resp.model, "claude-sonnet-4-6");
    assert_eq!(anthropic_resp.role, anthropic::Role::Assistant);
    assert_eq!(
        anthropic_resp.stop_reason,
        Some(anthropic::StopReason::EndTurn)
    );
    assert_eq!(anthropic_resp.usage.input_tokens, 10);
    assert_eq!(anthropic_resp.usage.output_tokens, 5);

    match &anthropic_resp.content[0] {
        anthropic::ContentBlock::Text { text } => assert_eq!(text, "Hello!"),
        _ => panic!("expected text block"),
    }
}

#[test]
fn response_with_function_call() {
    let resp: ResponsesResponse = serde_json::from_value(json!({
        "id": "resp_abc",
        "type": "response",
        "model": "gpt-4o",
        "output": [
            {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_123",
                "name": "get_weather",
                "arguments": "{\"city\":\"NYC\"}",
                "status": "completed"
            }
        ],
        "usage": {"input_tokens": 20, "output_tokens": 15, "total_tokens": 35},
        "status": "completed"
    }))
    .unwrap();

    let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
    match &anthropic_resp.content[0] {
        anthropic::ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_123");
            assert_eq!(name, "get_weather");
            assert_eq!(input["city"], "NYC");
        }
        _ => panic!("expected tool_use block"),
    }
}

#[test]
fn response_incomplete_status() {
    let resp: ResponsesResponse = serde_json::from_value(json!({
        "id": "resp_abc",
        "type": "response",
        "model": "gpt-4o",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "partial..."}],
            "id": "msg_1",
            "status": "incomplete"
        }],
        "usage": {"input_tokens": 10, "output_tokens": 100, "total_tokens": 110},
        "status": "incomplete"
    }))
    .unwrap();

    let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
    assert_eq!(
        anthropic_resp.stop_reason,
        Some(anthropic::StopReason::MaxTokens)
    );
}

#[test]
fn response_no_usage() {
    let resp: ResponsesResponse = serde_json::from_value(json!({
        "id": "resp_abc",
        "type": "response",
        "model": "gpt-4o",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "Hi"}],
            "id": "msg_1",
            "status": "completed"
        }],
        "status": "completed"
    }))
    .unwrap();

    let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
    assert_eq!(anthropic_resp.usage.input_tokens, 0);
    assert_eq!(anthropic_resp.usage.output_tokens, 0);
}

#[test]
fn response_mixed_text_and_tool_calls() {
    let resp: ResponsesResponse = serde_json::from_value(json!({
        "id": "resp_abc",
        "type": "response",
        "model": "gpt-4o",
        "output": [
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Let me check."}],
                "id": "msg_1",
                "status": "completed"
            },
            {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_456",
                "name": "search",
                "arguments": "{\"q\":\"weather\"}",
                "status": "completed"
            }
        ],
        "usage": {"input_tokens": 15, "output_tokens": 20, "total_tokens": 35},
        "status": "completed"
    }))
    .unwrap();

    let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
    assert_eq!(anthropic_resp.content.len(), 2);
    assert!(
        matches!(&anthropic_resp.content[0], anthropic::ContentBlock::Text { text } if text == "Let me check.")
    );
    assert!(
        matches!(&anthropic_resp.content[1], anthropic::ContentBlock::ToolUse { name, .. } if name == "search")
    );
}

#[test]
fn empty_output_gets_empty_text() {
    let resp: ResponsesResponse = serde_json::from_value(json!({
        "id": "resp_abc",
        "type": "response",
        "model": "gpt-4o",
        "output": [],
        "status": "completed"
    }))
    .unwrap();

    let anthropic_resp = responses_to_anthropic_response(&resp, "claude-sonnet-4-6");
    assert_eq!(anthropic_resp.content.len(), 1);
    assert!(
        matches!(&anthropic_resp.content[0], anthropic::ContentBlock::Text { text } if text.is_empty())
    );
}

#[test]
fn tool_choice_mapping() {
    let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 100,
        "tool_choice": {"type": "any"},
        "tools": [{"name": "f", "input_schema": {"type": "object"}}],
        "messages": [{"role": "user", "content": "Hi"}]
    }))
    .unwrap();

    let responses_req = anthropic_to_responses_request(&req);
    assert_eq!(responses_req.extra["tool_choice"], "required");
}

#[test]
fn image_block_mapping() {
    let req: anthropic::MessageCreateRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "What is this?"},
                {"type": "image", "source": {"type": "url", "url": "https://example.com/img.png"}}
            ]
        }]
    }))
    .unwrap();

    let responses_req = anthropic_to_responses_request(&req);
    match &responses_req.input {
        ResponsesInput::Items(items) => {
            let content = items[0]["content"].as_array().unwrap();
            assert_eq!(content.len(), 2);
            assert_eq!(content[0]["type"], "input_text");
            assert_eq!(content[1]["type"], "input_image");
            assert_eq!(content[1]["image_url"], "https://example.com/img.png");
        }
        _ => panic!("expected Items input"),
    }
}
