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

// --- Claude Code fixture tests ---

#[test]
fn claude_code_tool_use_fixture_deserializes() {
    let path = fixtures_dir().join("anthropic/claude_code_tool_use.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let req: anthropic::MessageCreateRequest =
        serde_json::from_value(fixture["request"].clone()).unwrap();
    assert_eq!(req.tools.as_ref().unwrap().len(), 6);
    assert_eq!(req.tools.as_ref().unwrap()[0].name, "Read");
    assert_eq!(req.tools.as_ref().unwrap()[1].name, "Bash");

    let resp: anthropic::MessageResponse =
        serde_json::from_value(fixture["response"].clone()).unwrap();
    assert_eq!(resp.stop_reason, Some(anthropic::StopReason::ToolUse));
    // Response has text + 2 parallel tool_use blocks
    assert_eq!(resp.content.len(), 3);
    match &resp.content[1] {
        anthropic::ContentBlock::ToolUse { name, .. } => assert_eq!(name, "Read"),
        other => panic!("expected ToolUse, got {:?}", other),
    }
    match &resp.content[2] {
        anthropic::ContentBlock::ToolUse { name, .. } => assert_eq!(name, "Glob"),
        other => panic!("expected ToolUse, got {:?}", other),
    }
}

#[test]
fn claude_code_tool_call_fixture_deserializes() {
    let path = fixtures_dir().join("openai/claude_code_tool_call.json");
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
    assert_eq!(tc.len(), 2);
    assert_eq!(tc[0].function.name, "Read");
    assert_eq!(tc[1].function.name, "Glob");
}

#[test]
fn claude_code_request_translates_to_openai() {
    let path = fixtures_dir().join("anthropic/claude_code_tool_use.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let req: anthropic::MessageCreateRequest =
        serde_json::from_value(fixture["request"].clone()).unwrap();
    let oai_req = mapping::message_map::anthropic_to_openai_request(&req);

    // 6 Anthropic tools -> 6 OpenAI function tools
    let tools = oai_req.tools.as_ref().unwrap();
    assert_eq!(tools.len(), 6);
    assert_eq!(tools[0].tool_type, "function");
    assert_eq!(tools[0].function.name, "Read");
    assert_eq!(tools[1].function.name, "Bash");

    // tool_choice auto preserved
    match oai_req.tool_choice.as_ref().unwrap() {
        openai::ChatToolChoice::Simple(s) => assert_eq!(s, "auto"),
        other => panic!("expected Simple(auto), got {:?}", other),
    }
}

#[test]
fn claude_code_openai_response_translates_back() {
    // Load the OpenAI fixture response, translate to Anthropic, verify parallel tool_use
    let path = fixtures_dir().join("openai/claude_code_tool_call.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let resp: openai::ChatCompletionResponse =
        serde_json::from_value(fixture["response"].clone()).unwrap();
    let anth =
        mapping::message_map::openai_to_anthropic_response(&resp, "claude-sonnet-4-20250514");

    assert_eq!(anth.stop_reason, Some(anthropic::StopReason::ToolUse));
    // Text content + 2 tool_use blocks
    assert_eq!(anth.content.len(), 3);
    match &anth.content[0] {
        anthropic::ContentBlock::Text { text } => {
            assert!(text.contains("parallel"));
        }
        other => panic!("expected Text, got {:?}", other),
    }
    match &anth.content[1] {
        anthropic::ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_read_001");
            assert_eq!(name, "Read");
            assert_eq!(input["file_path"], "/home/user/project/config.toml");
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

#[test]
fn claude_code_tool_result_cycle_translates() {
    // Full cycle: Anthropic request with tool_results -> OpenAI messages
    let path = fixtures_dir().join("anthropic/claude_code_tool_result.json");
    let content = std::fs::read_to_string(&path).expect("fixture file should exist");
    let fixture: serde_json::Value = serde_json::from_str(&content).unwrap();

    let req: anthropic::MessageCreateRequest =
        serde_json::from_value(fixture["request"].clone()).unwrap();
    let oai = mapping::message_map::anthropic_to_openai_request(&req);

    // Expected: user, assistant (with tool_calls), tool, tool
    assert_eq!(oai.messages.len(), 4);
    assert_eq!(oai.messages[0].role, openai::ChatRole::User);
    assert_eq!(oai.messages[1].role, openai::ChatRole::Assistant);
    assert_eq!(oai.messages[2].role, openai::ChatRole::Tool);
    assert_eq!(oai.messages[3].role, openai::ChatRole::Tool);

    // Assistant message has 2 tool_calls
    let tc = oai.messages[1].tool_calls.as_ref().unwrap();
    assert_eq!(tc.len(), 2);
    assert_eq!(tc[0].id, "toolu_01UDAtfZkgcGYMBq7Ns84vfN");
    assert_eq!(tc[0].function.name, "Read");
    assert_eq!(tc[1].id, "toolu_01J3KzMqBf9tXyQFhVw2NxRG");
    assert_eq!(tc[1].function.name, "Glob");

    // Tool result messages have correct IDs
    assert_eq!(
        oai.messages[2].tool_call_id.as_deref(),
        Some("toolu_01UDAtfZkgcGYMBq7Ns84vfN")
    );
    assert_eq!(
        oai.messages[3].tool_call_id.as_deref(),
        Some("toolu_01J3KzMqBf9tXyQFhVw2NxRG")
    );
}
