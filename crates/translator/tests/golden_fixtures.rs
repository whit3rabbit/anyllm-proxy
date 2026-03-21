// Golden-file tests: validate that fixture JSON files can be deserialized
// and that translation between formats produces the expected shapes.

use anthropic_openai_translate::{anthropic, mapping, openai};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
}

#[test]
fn anthropic_basic_request_fixture_deserializes() {
    let path = fixtures_dir().join("anthropic/messages_basic.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let req: anthropic::MessageCreateRequest =
        serde_json::from_value(fixture["request"].clone()).unwrap();
    assert_eq!(req.model, "claude-opus-4-6");
    assert_eq!(req.max_tokens, 256);

    let resp: anthropic::MessageResponse =
        serde_json::from_value(fixture["response"].clone()).unwrap();
    assert_eq!(resp.stop_reason, Some(anthropic::StopReason::EndTurn));
}

#[test]
fn anthropic_tool_use_fixture_deserializes() {
    let path = fixtures_dir().join("anthropic/messages_tool_use.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let req: anthropic::MessageCreateRequest =
        serde_json::from_value(fixture["request"].clone()).unwrap();
    assert!(req.tools.is_some());
    assert_eq!(req.tools.as_ref().unwrap().len(), 1);

    let resp: anthropic::MessageResponse =
        serde_json::from_value(fixture["response"].clone()).unwrap();
    assert_eq!(resp.stop_reason, Some(anthropic::StopReason::ToolUse));
    match &resp.content[0] {
        anthropic::ContentBlock::ToolUse { name, .. } => {
            assert_eq!(name, "get_stock_price");
        }
        other => panic!("expected ToolUse, got {:?}", other),
    }
}

#[test]
fn openai_basic_response_fixture_deserializes() {
    let path = fixtures_dir().join("openai/chat_completion_basic.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let resp: openai::ChatCompletionResponse =
        serde_json::from_value(fixture["response"].clone()).unwrap();
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(
        resp.choices[0].finish_reason,
        Some(openai::FinishReason::Stop)
    );
}

#[test]
fn openai_tool_call_response_fixture_deserializes() {
    let path = fixtures_dir().join("openai/chat_completion_tool_call.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let resp: openai::ChatCompletionResponse =
        serde_json::from_value(fixture["response"].clone()).unwrap();
    assert_eq!(
        resp.choices[0].finish_reason,
        Some(openai::FinishReason::ToolCalls)
    );
    let tc = resp.choices[0]
        .message
        .tool_calls
        .as_ref()
        .expect("should have tool_calls");
    assert_eq!(tc[0].function.name, "get_stock_price");
}

#[test]
fn translate_anthropic_fixture_to_openai_request() {
    let path = fixtures_dir().join("anthropic/messages_basic.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let req: anthropic::MessageCreateRequest =
        serde_json::from_value(fixture["request"].clone()).unwrap();
    let openai_req = mapping::message_map::anthropic_to_openai_request(&req);

    // System prompt should become a developer message
    assert_eq!(openai_req.messages[0].role, openai::ChatRole::Developer);
    // User message should follow
    assert_eq!(openai_req.messages[1].role, openai::ChatRole::User);
    assert_eq!(openai_req.model, "claude-opus-4-6");
}

#[test]
fn translate_openai_response_to_anthropic() {
    let path = fixtures_dir().join("openai/chat_completion_basic.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let resp: openai::ChatCompletionResponse =
        serde_json::from_value(fixture["response"].clone()).unwrap();
    let anthropic_resp =
        mapping::message_map::openai_to_anthropic_response(&resp, "claude-opus-4-6");

    assert_eq!(anthropic_resp.model, "claude-opus-4-6");
    assert_eq!(
        anthropic_resp.stop_reason,
        Some(anthropic::StopReason::EndTurn)
    );
    assert!(!anthropic_resp.content.is_empty());
    assert_eq!(anthropic_resp.usage.input_tokens, 25);
    assert_eq!(anthropic_resp.usage.output_tokens, 30);
}

#[test]
fn translate_openai_tool_call_response_to_anthropic() {
    let path = fixtures_dir().join("openai/chat_completion_tool_call.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let resp: openai::ChatCompletionResponse =
        serde_json::from_value(fixture["response"].clone()).unwrap();
    let anthropic_resp =
        mapping::message_map::openai_to_anthropic_response(&resp, "claude-opus-4-6");

    assert_eq!(
        anthropic_resp.stop_reason,
        Some(anthropic::StopReason::ToolUse)
    );
    match &anthropic_resp.content[0] {
        anthropic::ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_xyz789");
            assert_eq!(name, "get_stock_price");
            assert_eq!(input["ticker"], "^GSPC");
        }
        other => panic!("expected ToolUse, got {:?}", other),
    }
}
