use super::*;
use serde_json::json;

// Helper: build a minimal Anthropic request for testing.
fn make_request(messages: Vec<anthropic::InputMessage>) -> anthropic::MessageCreateRequest {
    anthropic::MessageCreateRequest {
        model: "claude-3-5-sonnet-20241022".into(),
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

fn user_text(text: &str) -> anthropic::InputMessage {
    anthropic::InputMessage {
        role: anthropic::Role::User,
        content: anthropic::Content::Text(text.into()),
    }
}

fn user_blocks(blocks: Vec<anthropic::ContentBlock>) -> anthropic::InputMessage {
    anthropic::InputMessage {
        role: anthropic::Role::User,
        content: anthropic::Content::Blocks(blocks),
    }
}

fn assistant_blocks(blocks: Vec<anthropic::ContentBlock>) -> anthropic::InputMessage {
    anthropic::InputMessage {
        role: anthropic::Role::Assistant,
        content: anthropic::Content::Blocks(blocks),
    }
}

// -----------------------------------------------------------------------
// Request mapping tests
// -----------------------------------------------------------------------

#[test]
fn simple_text_message_maps_correctly() {
    let req = make_request(vec![user_text("Hello")]);
    let gem = anthropic_to_gemini_request(&req);
    assert_eq!(gem.contents.len(), 1);
    assert_eq!(gem.contents[0].role.as_deref(), Some("user"));
    assert_eq!(gem.contents[0].parts[0].text.as_deref(), Some("Hello"));
}

#[test]
fn system_prompt_text_extracted_to_system_instruction() {
    let mut req = make_request(vec![user_text("Hi")]);
    req.system = Some(anthropic::System::Text("Be helpful.".into()));
    let gem = anthropic_to_gemini_request(&req);
    let si = gem.system_instruction.unwrap();
    assert!(si.role.is_none(), "systemInstruction should have no role");
    assert_eq!(si.parts[0].text.as_deref(), Some("Be helpful."));
}

#[test]
fn system_blocks_concatenated() {
    let mut req = make_request(vec![user_text("Hi")]);
    req.system = Some(anthropic::System::Blocks(vec![
        anthropic::SystemBlock {
            block_type: "text".into(),
            text: "First.".into(),
            cache_control: None,
        },
        anthropic::SystemBlock {
            block_type: "text".into(),
            text: "Second.".into(),
            cache_control: Some(anthropic::CacheControl {
                cache_type: "ephemeral".into(),
            }),
        },
    ]));
    let gem = anthropic_to_gemini_request(&req);
    let si = gem.system_instruction.unwrap();
    assert_eq!(si.parts[0].text.as_deref(), Some("First.\nSecond."));
}

#[test]
fn assistant_role_maps_to_model() {
    let req = make_request(vec![
        user_text("Hello"),
        assistant_blocks(vec![anthropic::ContentBlock::Text {
            text: "Hi there".into(),
        }]),
    ]);
    let gem = anthropic_to_gemini_request(&req);
    assert_eq!(gem.contents[1].role.as_deref(), Some("model"));
}

#[test]
fn image_content_maps_to_inline_data() {
    let req = make_request(vec![user_blocks(vec![anthropic::ContentBlock::Image {
        source: anthropic::ImageSource {
            source_type: "base64".into(),
            media_type: Some("image/jpeg".into()),
            data: Some("abc123==".into()),
            url: None,
        },
    }])]);
    let gem = anthropic_to_gemini_request(&req);
    let part = &gem.contents[0].parts[0];
    let id = part.inline_data.as_ref().unwrap();
    assert_eq!(id.mime_type, "image/jpeg");
    assert_eq!(id.data, "abc123==");
}

#[test]
fn url_image_dropped() {
    let req = make_request(vec![user_blocks(vec![anthropic::ContentBlock::Image {
        source: anthropic::ImageSource {
            source_type: "url".into(),
            media_type: None,
            data: None,
            url: Some("https://example.com/img.png".into()),
        },
    }])]);
    let gem = anthropic_to_gemini_request(&req);
    // The URL image is dropped; no parts remain, so the content entry is empty
    // and filtered out.
    assert!(gem.contents.is_empty() || gem.contents[0].parts.is_empty());
}

#[test]
fn tool_use_maps_to_function_call_id_stripped() {
    let req = make_request(vec![assistant_blocks(vec![
        anthropic::ContentBlock::ToolUse {
            id: "toolu_abc123".into(),
            name: "get_weather".into(),
            input: json!({"city": "London"}),
        },
    ])]);
    let gem = anthropic_to_gemini_request(&req);
    // A dummy user turn is prepended because Gemini requires user-first; model is at [1].
    let model_content = gem
        .contents
        .iter()
        .find(|c| c.role.as_deref() == Some("model"))
        .expect("should have a model turn");
    let fc = model_content.parts[0].function_call.as_ref().unwrap();
    assert_eq!(fc.name, "get_weather");
    assert_eq!(fc.args, json!({"city": "London"}));
    // No id field on Gemini FunctionCallData.
}

#[test]
fn tool_result_maps_to_function_response_with_name_lookup() {
    let req = make_request(vec![
        assistant_blocks(vec![anthropic::ContentBlock::ToolUse {
            id: "toolu_abc".into(),
            name: "get_weather".into(),
            input: json!({}),
        }]),
        user_blocks(vec![anthropic::ContentBlock::ToolResult {
            tool_use_id: "toolu_abc".into(),
            content: Some(anthropic::ToolResultContent::Text("72F sunny".into())),
            is_error: None,
        }]),
    ]);
    let gem = anthropic_to_gemini_request(&req);
    // Find the user content that contains the function_response (not the dummy
    // empty user turn that was prepended for Gemini's user-first requirement).
    let user_content = gem
        .contents
        .iter()
        .find(|c| {
            c.role.as_deref() == Some("user")
                && c.parts
                    .first()
                    .and_then(|p| p.function_response.as_ref())
                    .is_some()
        })
        .unwrap();
    let fr = user_content.parts[0].function_response.as_ref().unwrap();
    assert_eq!(fr.name, "get_weather");
    assert_eq!(fr.response, json!({"result": "72F sunny"}));
}

#[test]
fn tool_result_error_wraps_in_error_key() {
    let req = make_request(vec![
        assistant_blocks(vec![anthropic::ContentBlock::ToolUse {
            id: "toolu_err".into(),
            name: "broken_tool".into(),
            input: json!({}),
        }]),
        user_blocks(vec![anthropic::ContentBlock::ToolResult {
            tool_use_id: "toolu_err".into(),
            content: Some(anthropic::ToolResultContent::Text("timeout".into())),
            is_error: Some(true),
        }]),
    ]);
    let gem = anthropic_to_gemini_request(&req);
    let user_content = gem
        .contents
        .iter()
        .find(|c| {
            c.role.as_deref() == Some("user")
                && c.parts
                    .first()
                    .and_then(|p| p.function_response.as_ref())
                    .is_some()
        })
        .unwrap();
    let fr = user_content.parts[0].function_response.as_ref().unwrap();
    assert_eq!(fr.response, json!({"error": "timeout"}));
}

#[test]
fn tool_result_unknown_id_is_dropped() {
    // A ToolResult whose tool_use_id is not in the tool_id_map must be dropped
    // rather than emitting "unknown_tool", which would cause a Gemini 400.
    // When the only block in a message is dropped, the whole message is omitted.
    let req = make_request(vec![user_blocks(vec![
        anthropic::ContentBlock::ToolResult {
            tool_use_id: "toolu_missing".into(),
            content: Some(anthropic::ToolResultContent::Text("data".into())),
            is_error: None,
        },
    ])]);
    let gem = anthropic_to_gemini_request(&req);
    // The message has no valid parts, so it is omitted entirely from contents.
    assert!(
        gem.contents.is_empty(),
        "unknown ToolResult should be dropped; resulting empty message should be omitted"
    );
}

#[test]
fn thinking_blocks_dropped() {
    let req = make_request(vec![assistant_blocks(vec![
        anthropic::ContentBlock::Thinking {
            thinking: "Let me think...".into(),
            signature: None,
        },
        anthropic::ContentBlock::Text {
            text: "Answer".into(),
        },
    ])]);
    let gem = anthropic_to_gemini_request(&req);
    // A dummy user turn is prepended; find the model turn by role.
    let model_content = gem
        .contents
        .iter()
        .find(|c| c.role.as_deref() == Some("model"))
        .expect("should have a model turn");
    assert_eq!(model_content.parts.len(), 1);
    assert_eq!(model_content.parts[0].text.as_deref(), Some("Answer"));
}

#[test]
fn redacted_thinking_blocks_dropped() {
    let req = make_request(vec![assistant_blocks(vec![
        anthropic::ContentBlock::RedactedThinking {
            data: "encrypted".into(),
        },
        anthropic::ContentBlock::Text {
            text: "Visible".into(),
        },
    ])]);
    let gem = anthropic_to_gemini_request(&req);
    // A dummy user turn is prepended; find the model turn by role.
    let model_content = gem
        .contents
        .iter()
        .find(|c| c.role.as_deref() == Some("model"))
        .expect("should have a model turn");
    assert_eq!(model_content.parts.len(), 1);
    assert_eq!(model_content.parts[0].text.as_deref(), Some("Visible"));
}

#[test]
fn document_blocks_dropped() {
    let req = make_request(vec![user_blocks(vec![
        anthropic::ContentBlock::Document {
            source: anthropic::DocumentSource {
                source_type: "base64".into(),
                media_type: "application/pdf".into(),
                data: "JVBER...".into(),
            },
            title: Some("doc.pdf".into()),
        },
        anthropic::ContentBlock::Text {
            text: "Summarize this".into(),
        },
    ])]);
    let gem = anthropic_to_gemini_request(&req);
    assert_eq!(gem.contents[0].parts.len(), 1);
    assert_eq!(
        gem.contents[0].parts[0].text.as_deref(),
        Some("Summarize this")
    );
}

#[test]
fn tools_mapped_to_function_declarations() {
    let mut req = make_request(vec![user_text("weather?")]);
    req.tools = Some(vec![anthropic::Tool {
        name: "get_weather".into(),
        description: Some("Get weather info".into()),
        input_schema: json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        }),
    }]);
    let gem = anthropic_to_gemini_request(&req);
    let decls = &gem.tools.unwrap()[0].function_declarations;
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].name, "get_weather");
    assert_eq!(decls[0].description.as_deref(), Some("Get weather info"));
    assert!(decls[0].parameters.is_some());
}

#[test]
fn tool_schemas_sanitized() {
    let mut req = make_request(vec![user_text("test")]);
    req.tools = Some(vec![anthropic::Tool {
        name: "t".into(),
        description: None,
        input_schema: json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "additionalProperties": false,
            "properties": {"x": {"type": "string"}}
        }),
    }]);
    let gem = anthropic_to_gemini_request(&req);
    let tools = gem.tools.unwrap();
    let params = tools[0].function_declarations[0]
        .parameters
        .as_ref()
        .unwrap();
    // sanitize_schema_for_gemini strips $schema and additionalProperties
    assert!(params.get("$schema").is_none());
    assert!(params.get("additionalProperties").is_none());
    assert_eq!(params["type"], "object");
}

#[test]
fn tool_choice_auto_maps() {
    let mut req = make_request(vec![user_text("test")]);
    req.tool_choice = Some(anthropic::ToolChoice::Auto {
        disable_parallel_tool_use: None,
    });
    let gem = anthropic_to_gemini_request(&req);
    assert_eq!(
        gem.tool_config.unwrap().function_calling_config.mode,
        "AUTO"
    );
}

#[test]
fn tool_choice_any_maps() {
    let mut req = make_request(vec![user_text("test")]);
    req.tool_choice = Some(anthropic::ToolChoice::Any {
        disable_parallel_tool_use: None,
    });
    let gem = anthropic_to_gemini_request(&req);
    assert_eq!(gem.tool_config.unwrap().function_calling_config.mode, "ANY");
}

#[test]
fn tool_choice_none_maps() {
    let mut req = make_request(vec![user_text("test")]);
    req.tool_choice = Some(anthropic::ToolChoice::None);
    let gem = anthropic_to_gemini_request(&req);
    assert_eq!(
        gem.tool_config.unwrap().function_calling_config.mode,
        "NONE"
    );
}

#[test]
fn tool_choice_specific_tool_maps_to_any_with_allowed_names() {
    let mut req = make_request(vec![user_text("test")]);
    req.tool_choice = Some(anthropic::ToolChoice::Tool {
        name: "get_weather".into(),
    });
    let gem = anthropic_to_gemini_request(&req);
    let fc = gem.tool_config.unwrap().function_calling_config;
    assert_eq!(fc.mode, "ANY");
    assert_eq!(
        fc.allowed_function_names,
        Some(vec!["get_weather".to_string()])
    );
}

#[test]
fn generation_config_fields_mapped() {
    let mut req = make_request(vec![user_text("test")]);
    req.max_tokens = 2048;
    req.temperature = Some(0.7);
    req.top_p = Some(0.9);
    req.top_k = Some(40);
    req.stop_sequences = Some(vec!["STOP".into()]);
    let gem = anthropic_to_gemini_request(&req);
    let gc = gem.generation_config.unwrap();
    assert_eq!(gc.max_output_tokens, Some(2048));
    let temp = gc.temperature.unwrap();
    assert!((temp - 0.7).abs() < 0.001);
    let top_p = gc.top_p.unwrap();
    assert!((top_p - 0.9).abs() < 0.001);
    assert_eq!(gc.top_k, Some(40));
    assert_eq!(gc.stop_sequences, Some(vec!["STOP".into()]));
}

#[test]
fn consecutive_user_messages_merged() {
    let req = make_request(vec![user_text("first"), user_text("second")]);
    let gem = anthropic_to_gemini_request(&req);
    assert_eq!(gem.contents.len(), 1, "should merge into one content");
    assert_eq!(gem.contents[0].parts.len(), 2);
    assert_eq!(gem.contents[0].parts[0].text.as_deref(), Some("first"));
    assert_eq!(gem.contents[0].parts[1].text.as_deref(), Some("second"));
}

#[test]
fn user_user_model_becomes_user_model() {
    let req = make_request(vec![
        user_text("a"),
        user_text("b"),
        assistant_blocks(vec![anthropic::ContentBlock::Text {
            text: "reply".into(),
        }]),
    ]);
    let gem = anthropic_to_gemini_request(&req);
    assert_eq!(gem.contents.len(), 2);
    assert_eq!(gem.contents[0].role.as_deref(), Some("user"));
    assert_eq!(gem.contents[0].parts.len(), 2);
    assert_eq!(gem.contents[1].role.as_deref(), Some("model"));
}

#[test]
fn empty_messages_list() {
    let req = make_request(vec![]);
    let gem = anthropic_to_gemini_request(&req);
    assert!(gem.contents.is_empty());
}

#[test]
fn content_text_shorthand_works() {
    // Content::Text(string) is a shorthand accepted by Anthropic API
    let req = make_request(vec![anthropic::InputMessage {
        role: anthropic::Role::User,
        content: anthropic::Content::Text("shorthand".into()),
    }]);
    let gem = anthropic_to_gemini_request(&req);
    assert_eq!(gem.contents[0].parts[0].text.as_deref(), Some("shorthand"));
}

// -----------------------------------------------------------------------
// build_tool_id_map tests
// -----------------------------------------------------------------------

#[test]
fn build_tool_id_map_finds_tool_uses() {
    let messages = vec![
        assistant_blocks(vec![
            anthropic::ContentBlock::ToolUse {
                id: "toolu_1".into(),
                name: "calc".into(),
                input: json!({}),
            },
            anthropic::ContentBlock::ToolUse {
                id: "toolu_2".into(),
                name: "search".into(),
                input: json!({}),
            },
        ]),
        user_blocks(vec![anthropic::ContentBlock::ToolResult {
            tool_use_id: "toolu_1".into(),
            content: None,
            is_error: None,
        }]),
    ];
    let map = build_tool_id_map(&messages);
    assert_eq!(map.get("toolu_1").unwrap(), "calc");
    assert_eq!(map.get("toolu_2").unwrap(), "search");
}

#[test]
fn build_tool_id_map_empty_on_no_tool_use() {
    let messages = vec![user_text("no tools here")];
    let map = build_tool_id_map(&messages);
    assert!(map.is_empty());
}

// -----------------------------------------------------------------------
// merge_consecutive_roles tests
// -----------------------------------------------------------------------

#[test]
fn merge_consecutive_roles_no_op_for_alternating() {
    let contents = vec![
        gemini::Content {
            role: Some("user".into()),
            parts: vec![gemini::Part::text("a")],
        },
        gemini::Content {
            role: Some("model".into()),
            parts: vec![gemini::Part::text("b")],
        },
    ];
    let merged = merge_consecutive_roles(contents);
    assert_eq!(merged.len(), 2);
}

#[test]
fn merge_consecutive_roles_merges_same_role() {
    let contents = vec![
        gemini::Content {
            role: Some("user".into()),
            parts: vec![gemini::Part::text("a")],
        },
        gemini::Content {
            role: Some("user".into()),
            parts: vec![gemini::Part::text("b")],
        },
        gemini::Content {
            role: Some("model".into()),
            parts: vec![gemini::Part::text("c")],
        },
    ];
    let merged = merge_consecutive_roles(contents);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].parts.len(), 2);
}

// -----------------------------------------------------------------------
// Response mapping tests
// -----------------------------------------------------------------------

fn make_gemini_response(
    parts: Vec<gemini::Part>,
    finish_reason: Option<gemini_resp::FinishReason>,
) -> gemini_resp::GenerateContentResponse {
    gemini_resp::GenerateContentResponse {
        candidates: vec![gemini_resp::Candidate {
            content: gemini::Content {
                role: Some("model".into()),
                parts,
            },
            finish_reason,
            safety_ratings: None,
        }],
        usage_metadata: Some(gemini_resp::UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
            cached_content_token_count: 0,
        }),
        model_version: None,
    }
}

#[test]
fn simple_text_response() {
    let resp = make_gemini_response(
        vec![gemini::Part::text("Hello!")],
        Some(gemini_resp::FinishReason::STOP),
    );
    let msg = gemini_to_anthropic_response(&resp, "gemini-2.5-flash");
    assert_eq!(msg.content.len(), 1);
    match &msg.content[0] {
        anthropic::ContentBlock::Text { text } => assert_eq!(text, "Hello!"),
        _ => panic!("expected Text block"),
    }
    assert_eq!(msg.model, "gemini-2.5-flash");
    assert_eq!(msg.stop_reason, Some(anthropic::StopReason::EndTurn));
}

#[test]
fn function_call_maps_to_tool_use_with_synthesized_id() {
    let resp = make_gemini_response(
        vec![gemini::Part::function_call(
            "get_weather",
            json!({"city": "NYC"}),
        )],
        Some(gemini_resp::FinishReason::STOP),
    );
    let msg = gemini_to_anthropic_response(&resp, "gemini-2.5-flash");
    assert_eq!(msg.content.len(), 1);
    match &msg.content[0] {
        anthropic::ContentBlock::ToolUse { id, name, input } => {
            assert!(
                id.starts_with("toolu_"),
                "synthesized ID should have toolu_ prefix"
            );
            assert_eq!(name, "get_weather");
            assert_eq!(input, &json!({"city": "NYC"}));
        }
        _ => panic!("expected ToolUse block"),
    }
}

#[test]
fn mixed_text_and_function_call() {
    let resp = make_gemini_response(
        vec![
            gemini::Part::text("Let me check the weather."),
            gemini::Part::function_call("get_weather", json!({"city": "London"})),
        ],
        Some(gemini_resp::FinishReason::STOP),
    );
    let msg = gemini_to_anthropic_response(&resp, "gemini-2.5-flash");
    assert_eq!(msg.content.len(), 2);
    assert!(matches!(
        &msg.content[0],
        anthropic::ContentBlock::Text { .. }
    ));
    assert!(matches!(
        &msg.content[1],
        anthropic::ContentBlock::ToolUse { .. }
    ));
    // Has function call + STOP -> ToolUse stop reason.
    assert_eq!(msg.stop_reason, Some(anthropic::StopReason::ToolUse));
}

#[test]
fn finish_reason_stop_without_tools_maps_to_end_turn() {
    let resp = make_gemini_response(
        vec![gemini::Part::text("done")],
        Some(gemini_resp::FinishReason::STOP),
    );
    let msg = gemini_to_anthropic_response(&resp, "test");
    assert_eq!(msg.stop_reason, Some(anthropic::StopReason::EndTurn));
}

#[test]
fn finish_reason_stop_with_function_call_maps_to_tool_use() {
    let resp = make_gemini_response(
        vec![gemini::Part::function_call("f", json!({}))],
        Some(gemini_resp::FinishReason::STOP),
    );
    let msg = gemini_to_anthropic_response(&resp, "test");
    assert_eq!(msg.stop_reason, Some(anthropic::StopReason::ToolUse));
}

#[test]
fn finish_reason_max_tokens_maps_to_max_tokens() {
    let resp = make_gemini_response(
        vec![gemini::Part::text("trunc")],
        Some(gemini_resp::FinishReason::MAX_TOKENS),
    );
    let msg = gemini_to_anthropic_response(&resp, "test");
    assert_eq!(msg.stop_reason, Some(anthropic::StopReason::MaxTokens));
}

#[test]
fn finish_reason_safety_maps_to_end_turn() {
    let resp = make_gemini_response(vec![], Some(gemini_resp::FinishReason::SAFETY));
    let msg = gemini_to_anthropic_response(&resp, "test");
    assert_eq!(msg.stop_reason, Some(anthropic::StopReason::EndTurn));
}

#[test]
fn usage_metadata_mapped() {
    let resp = make_gemini_response(
        vec![gemini::Part::text("ok")],
        Some(gemini_resp::FinishReason::STOP),
    );
    let msg = gemini_to_anthropic_response(&resp, "test");
    assert_eq!(msg.usage.input_tokens, 10);
    assert_eq!(msg.usage.output_tokens, 20);
}

#[test]
fn empty_candidates_gives_empty_content() {
    let resp = gemini_resp::GenerateContentResponse {
        candidates: vec![],
        usage_metadata: None,
        model_version: None,
    };
    let msg = gemini_to_anthropic_response(&resp, "test");
    assert!(msg.content.is_empty());
    assert!(msg.stop_reason.is_none());
    assert_eq!(msg.usage, anthropic::Usage::default());
}

#[test]
fn no_finish_reason_defaults_to_end_turn() {
    let resp = make_gemini_response(vec![gemini::Part::text("partial")], None);
    let msg = gemini_to_anthropic_response(&resp, "test");
    assert_eq!(msg.stop_reason, Some(anthropic::StopReason::EndTurn));
}

#[test]
fn multiple_text_parts_become_separate_blocks() {
    let resp = make_gemini_response(
        vec![gemini::Part::text("one"), gemini::Part::text("two")],
        Some(gemini_resp::FinishReason::STOP),
    );
    let msg = gemini_to_anthropic_response(&resp, "test");
    assert_eq!(msg.content.len(), 2);
    match (&msg.content[0], &msg.content[1]) {
        (anthropic::ContentBlock::Text { text: a }, anthropic::ContentBlock::Text { text: b }) => {
            assert_eq!(a, "one");
            assert_eq!(b, "two");
        }
        _ => panic!("expected two Text blocks"),
    }
}

#[test]
fn model_name_passed_through() {
    let resp = make_gemini_response(
        vec![gemini::Part::text("x")],
        Some(gemini_resp::FinishReason::STOP),
    );
    let msg = gemini_to_anthropic_response(&resp, "gemini-2.5-pro");
    assert_eq!(msg.model, "gemini-2.5-pro");
}

#[test]
fn message_id_has_correct_format() {
    let resp = make_gemini_response(
        vec![gemini::Part::text("x")],
        Some(gemini_resp::FinishReason::STOP),
    );
    let msg = gemini_to_anthropic_response(&resp, "test");
    assert!(msg.id.starts_with("msg_"));
    assert_eq!(msg.response_type, "message");
    assert_eq!(msg.role, anthropic::Role::Assistant);
}

#[test]
fn response_without_usage_metadata_gives_zero_usage() {
    let mut resp = make_gemini_response(
        vec![gemini::Part::text("x")],
        Some(gemini_resp::FinishReason::STOP),
    );
    resp.usage_metadata = None;
    let msg = gemini_to_anthropic_response(&resp, "test");
    assert_eq!(msg.usage.input_tokens, 0);
    assert_eq!(msg.usage.output_tokens, 0);
}

// -----------------------------------------------------------------------
// Thinking config tests
// -----------------------------------------------------------------------

#[test]
fn thinking_config_enabled_sets_gemini_thinking_config() {
    let mut req = make_request(vec![user_text("think hard")]);
    req.thinking = Some(anthropic::ThinkingConfig::Enabled {
        budget_tokens: 8192,
    });
    let gem = anthropic_to_gemini_request(&req);
    let gc = gem.generation_config.unwrap();
    let tc = gc.thinking_config.expect("thinkingConfig should be set");
    assert_eq!(tc.thinking_budget, 8192);
    assert_eq!(tc.include_thoughts, Some(true));
}

#[test]
fn thinking_config_disabled_no_thinking_config() {
    let mut req = make_request(vec![user_text("hi")]);
    req.thinking = Some(anthropic::ThinkingConfig::Disabled);
    let gem = anthropic_to_gemini_request(&req);
    let gc = gem.generation_config.unwrap();
    assert!(
        gc.thinking_config.is_none(),
        "disabled should not set thinkingConfig"
    );
}

#[test]
fn thinking_config_absent_no_thinking_config() {
    let req = make_request(vec![user_text("hi")]);
    let gem = anthropic_to_gemini_request(&req);
    let gc = gem.generation_config.unwrap();
    assert!(gc.thinking_config.is_none());
}

#[test]
fn gemini_thought_parts_become_thinking_blocks() {
    let thought_part = gemini::Part {
        thought: Some(true),
        text: Some("Let me reason...".into()),
        ..Default::default()
    };
    let resp = make_gemini_response(
        vec![thought_part, gemini::Part::text("Answer")],
        Some(gemini_resp::FinishReason::STOP),
    );
    let msg = gemini_to_anthropic_response(&resp, "gemini-2.5-pro");
    assert_eq!(msg.content.len(), 2);
    match &msg.content[0] {
        anthropic::ContentBlock::Thinking { thinking, .. } => {
            assert_eq!(thinking, "Let me reason...")
        }
        _ => panic!("expected Thinking block first"),
    }
    match &msg.content[1] {
        anthropic::ContentBlock::Text { text } => assert_eq!(text, "Answer"),
        _ => panic!("expected Text block second"),
    }
}

#[test]
fn gemini_thought_only_no_text() {
    let thought_part = gemini::Part {
        thought: Some(true),
        text: Some("Only thinking".into()),
        ..Default::default()
    };
    let resp = make_gemini_response(vec![thought_part], Some(gemini_resp::FinishReason::STOP));
    let msg = gemini_to_anthropic_response(&resp, "gemini-2.5-pro");
    assert_eq!(msg.content.len(), 1);
    assert!(matches!(
        &msg.content[0],
        anthropic::ContentBlock::Thinking { .. }
    ));
}
