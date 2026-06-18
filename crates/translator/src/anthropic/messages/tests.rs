use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn deserialize_basic_text_request() {
    let j = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "Hello, world"}
        ]
    });
    let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
    assert_eq!(req.model, "claude-3-5-sonnet-20241022");
    assert_eq!(req.max_tokens, 1024);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, Role::User);
    match &req.messages[0].content {
        Content::Text(s) => assert_eq!(s, "Hello, world"),
        _ => panic!("expected Content::Text"),
    }
}

#[test]
fn deserialize_system_as_string() {
    let j = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 256,
        "messages": [],
        "system": "You are a helpful assistant."
    });
    let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
    match req.system.unwrap() {
        System::Text(s) => assert_eq!(s, "You are a helpful assistant."),
        _ => panic!("expected System::Text"),
    }
}

#[test]
fn deserialize_system_as_blocks() {
    let j = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 256,
        "messages": [],
        "system": [
            {"type": "text", "text": "Be concise."},
            {"type": "text", "text": "Respond in JSON.", "cache_control": {"type": "ephemeral"}}
        ]
    });
    let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
    match req.system.unwrap() {
        System::Blocks(blocks) => {
            assert_eq!(blocks.len(), 2);
            assert_eq!(blocks[0].text, "Be concise.");
            assert!(blocks[0].cache_control.is_none());
            assert_eq!(blocks[1].text, "Respond in JSON.");
            assert_eq!(
                blocks[1].cache_control.as_ref().unwrap().cache_type,
                "ephemeral"
            );
        }
        _ => panic!("expected System::Blocks"),
    }
}

#[test]
fn deserialize_tools_and_tool_choice() {
    let j = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "What is the weather?"}],
        "tools": [{
            "name": "get_weather",
            "description": "Get weather for a location",
            "input_schema": {
                "type": "object",
                "properties": {"location": {"type": "string"}},
                "required": ["location"]
            }
        }],
        "tool_choice": {"type": "auto"}
    });
    let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
    let tools = req.tools.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "get_weather");
    assert!(tools[0].description.is_some());
    match req.tool_choice.unwrap() {
        ToolChoice::Auto { .. } => {}
        other => panic!("expected ToolChoice::Auto, got {:?}", other),
    }
}

#[test]
fn deserialize_tool_choice_specific_tool() {
    let j = json!({"type": "tool", "name": "get_weather"});
    let tc: ToolChoice = serde_json::from_value(j).unwrap();
    match tc {
        ToolChoice::Tool { name } => assert_eq!(name, "get_weather"),
        other => panic!("expected ToolChoice::Tool, got {:?}", other),
    }
}

#[test]
fn content_as_string_vs_blocks() {
    // String form
    let j = json!("just a string");
    let c: Content = serde_json::from_value(j).unwrap();
    match c {
        Content::Text(s) => assert_eq!(s, "just a string"),
        _ => panic!("expected Content::Text"),
    }

    // Blocks form
    let j = json!([{"type": "text", "text": "hello"}]);
    let c: Content = serde_json::from_value(j).unwrap();
    match c {
        Content::Blocks(blocks) => {
            assert_eq!(blocks.len(), 1);
            match &blocks[0] {
                ContentBlock::Text { text } => assert_eq!(text, "hello"),
                _ => panic!("expected ContentBlock::Text"),
            }
        }
        _ => panic!("expected Content::Blocks"),
    }
}

#[test]
fn deserialize_tool_use_block() {
    let j = json!({
        "type": "tool_use",
        "id": "toolu_01A09q90qw90lq917835lqs136",
        "name": "get_weather",
        "input": {"location": "San Francisco, CA"}
    });
    let block: ContentBlock = serde_json::from_value(j).unwrap();
    match block {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "toolu_01A09q90qw90lq917835lqs136");
            assert_eq!(name, "get_weather");
            assert_eq!(input["location"], "San Francisco, CA");
        }
        _ => panic!("expected ContentBlock::ToolUse"),
    }
}

#[test]
fn deserialize_tool_result_block() {
    let j = json!({
        "type": "tool_result",
        "tool_use_id": "toolu_01A09q90qw90lq917835lqs136",
        "content": "72°F, sunny"
    });
    let block: ContentBlock = serde_json::from_value(j).unwrap();
    match block {
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "toolu_01A09q90qw90lq917835lqs136");
            match content.unwrap() {
                ToolResultContent::Text(s) => assert_eq!(s, "72°F, sunny"),
                _ => panic!("expected ToolResultContent::Text"),
            }
            assert!(is_error.is_none());
        }
        _ => panic!("expected ContentBlock::ToolResult"),
    }
}

#[test]
fn deserialize_tool_result_error() {
    let j = json!({
        "type": "tool_result",
        "tool_use_id": "toolu_err",
        "content": "something went wrong",
        "is_error": true
    });
    let block: ContentBlock = serde_json::from_value(j).unwrap();
    match block {
        ContentBlock::ToolResult { is_error, .. } => {
            assert_eq!(is_error, Some(true));
        }
        _ => panic!("expected ContentBlock::ToolResult"),
    }
}

#[test]
fn message_response_round_trip() {
    let resp = MessageResponse {
        id: "msg_01XFDUDYJgAACzvnptvVoYEL".into(),
        response_type: "message".into(),
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "Hello!".into(),
        }],
        model: "claude-3-5-sonnet-20241022".into(),
        stop_reason: Some(StopReason::EndTurn),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            ..Default::default()
        },
        created: None,
    };
    let serialized = serde_json::to_string(&resp).unwrap();
    let deserialized: MessageResponse = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized.id, resp.id);
    assert_eq!(deserialized.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(deserialized.usage.input_tokens, 10);
    assert_eq!(deserialized.usage.output_tokens, 5);
}

#[test]
fn reject_missing_max_tokens() {
    let j = json!({
        "model": "claude-3-5-sonnet-20241022",
        "messages": []
    });
    let result = serde_json::from_value::<MessageCreateRequest>(j);
    assert!(result.is_err(), "should fail without max_tokens");
}

#[test]
fn extra_fields_captured() {
    let j = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 100,
        "messages": [],
        "top_k": 40,
        "unknown_field": "value"
    });
    let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
    assert_eq!(req.top_k, Some(40));
    assert!(req.extra.get("top_k").is_none());
    assert_eq!(req.extra.get("unknown_field").unwrap(), &json!("value"));
}

#[test]
fn stop_reason_variants() {
    assert_eq!(
        serde_json::from_value::<StopReason>(json!("end_turn")).unwrap(),
        StopReason::EndTurn,
    );
    assert_eq!(
        serde_json::from_value::<StopReason>(json!("max_tokens")).unwrap(),
        StopReason::MaxTokens,
    );
    assert_eq!(
        serde_json::from_value::<StopReason>(json!("stop_sequence")).unwrap(),
        StopReason::StopSequence,
    );
    assert_eq!(
        serde_json::from_value::<StopReason>(json!("tool_use")).unwrap(),
        StopReason::ToolUse,
    );
    assert_eq!(
        serde_json::from_value::<StopReason>(json!("pause_turn")).unwrap(),
        StopReason::PauseTurn,
    );
    assert_eq!(
        serde_json::from_value::<StopReason>(json!("refusal")).unwrap(),
        StopReason::Refusal,
    );
}

#[test]
fn modern_content_blocks_deserialize() {
    let server_tool: ContentBlock = serde_json::from_value(json!({
        "type": "server_tool_use",
        "id": "srv_1",
        "name": "web_search",
        "input": {"query": "rust"}
    }))
    .unwrap();
    assert!(matches!(
        server_tool,
        ContentBlock::ServerToolUse { ref name, .. } if name == "web_search"
    ));

    let web_search_result: ContentBlock = serde_json::from_value(json!({
        "type": "web_search_tool_result",
        "tool_use_id": "srv_1",
        "content": [{"type": "web_search_result", "title": "Result"}]
    }))
    .unwrap();
    assert!(matches!(
        web_search_result,
        ContentBlock::WebSearchToolResult { ref tool_use_id, .. } if tool_use_id == "srv_1"
    ));

    let unknown: ContentBlock = serde_json::from_value(json!({
        "type": "future_tool_result",
        "payload": true
    }))
    .unwrap();
    assert!(matches!(unknown, ContentBlock::Unknown));
}

#[test]
fn usage_accepts_nulls_and_new_fields() {
    let usage: Usage = serde_json::from_value(json!({
        "input_tokens": null,
        "output_tokens": null,
        "cache_creation": {"ephemeral_5m_input_tokens": 10},
        "inference_geo": "us",
        "service_tier": "standard",
        "server_tool_use": {"web_search_requests": 2, "web_fetch_requests": 1},
        "speed": "fast"
    }))
    .unwrap();
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert_eq!(usage.inference_geo.as_deref(), Some("us"));
    let server_tool_use = usage.server_tool_use.unwrap();
    assert_eq!(server_tool_use.web_search_requests, Some(2));
    assert_eq!(server_tool_use.web_fetch_requests, Some(1));
}

#[test]
fn usage_optional_cache_fields_omitted() {
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 20,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
        ..Default::default()
    };
    let j = serde_json::to_value(&usage).unwrap();
    assert!(!j
        .as_object()
        .unwrap()
        .contains_key("cache_creation_input_tokens"));
    assert!(!j
        .as_object()
        .unwrap()
        .contains_key("cache_read_input_tokens"));
}

#[test]
fn thinking_config_enabled_roundtrip() {
    let cfg = ThinkingConfig::Enabled {
        budget_tokens: 8192,
    };
    let json = serde_json::to_value(&cfg).unwrap();
    assert_eq!(json, json!({"type": "enabled", "budget_tokens": 8192}));
    let parsed: ThinkingConfig = serde_json::from_value(json).unwrap();
    assert!(matches!(
        parsed,
        ThinkingConfig::Enabled {
            budget_tokens: 8192
        }
    ));
}

#[test]
fn thinking_config_disabled_roundtrip() {
    let cfg = ThinkingConfig::Disabled;
    let json = serde_json::to_value(&cfg).unwrap();
    assert_eq!(json, json!({"type": "disabled"}));
    let parsed: ThinkingConfig = serde_json::from_value(json).unwrap();
    assert!(matches!(parsed, ThinkingConfig::Disabled));
}

#[test]
fn thinking_content_block_roundtrip() {
    let block = ContentBlock::Thinking {
        thinking: "Let me reason about this...".into(),
        signature: Some("sig_abc".into()),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "thinking");
    assert_eq!(json["thinking"], "Let me reason about this...");
    assert_eq!(json["signature"], "sig_abc");
    let parsed: ContentBlock = serde_json::from_value(json).unwrap();
    assert!(matches!(parsed, ContentBlock::Thinking { .. }));
}

#[test]
fn request_with_thinking_deserializes() {
    let j = json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 1024,
        "messages": [],
        "thinking": {"type": "enabled", "budget_tokens": 4096}
    });
    let req: MessageCreateRequest = serde_json::from_value(j).unwrap();
    assert!(matches!(
        req.thinking,
        Some(ThinkingConfig::Enabled {
            budget_tokens: 4096
        })
    ));
}

#[test]
fn deserialize_redacted_thinking_block() {
    let j = json!({
        "type": "redacted_thinking",
        "data": "EqQBCgIYAhIM1gbcDa9GJwZA2b3h"
    });
    let block: ContentBlock = serde_json::from_value(j).unwrap();
    match block {
        ContentBlock::RedactedThinking { data } => {
            assert_eq!(data, "EqQBCgIYAhIM1gbcDa9GJwZA2b3h");
        }
        _ => panic!("expected ContentBlock::RedactedThinking"),
    }
}

#[test]
fn redacted_thinking_round_trip() {
    let block = ContentBlock::RedactedThinking {
        data: "encrypted_data_here".into(),
    };
    let serialized = serde_json::to_string(&block).unwrap();
    assert!(serialized.contains("\"redacted_thinking\""));
    let deserialized: ContentBlock = serde_json::from_str(&serialized).unwrap();
    match deserialized {
        ContentBlock::RedactedThinking { data } => {
            assert_eq!(data, "encrypted_data_here");
        }
        _ => panic!("expected ContentBlock::RedactedThinking"),
    }
}
