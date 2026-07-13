use super::*;
use crate::tools::execution::ToolCall;
use anyllm_translate::anthropic::Tool;
use serde_json::{json, Value};

fn tool_spec(name: &str) -> Tool {
    Tool {
        name: name.to_string(),
        description: None,
        input_schema: json!({"type": "object"}),
    }
}

fn call(name: &str, input: Value) -> ToolCall {
    ToolCall {
        id: "toolu_1".to_string(),
        name: name.to_string(),
        input,
    }
}

#[test]
fn lsp_nudge_requires_replacement_tool() {
    let mut state = ToolGuardrailRequestState::new();
    let config = ToolGuardrailConfig {
        lsp_first: true,
        ..ToolGuardrailConfig::disabled()
    };
    let calls = vec![call("grep", json!({"pattern": "UserService"}))];

    assert!(evaluate_tool_guardrails(&calls, &[tool_spec("grep")], &config, &mut state).is_empty());

    let nudges = evaluate_tool_guardrails(
        &calls,
        &[tool_spec("grep"), tool_spec("find_definition")],
        &config,
        &mut state,
    );
    assert_eq!(nudges.len(), 1);
    let nudge = &nudges[0];
    assert_eq!(nudge.kind, "lsp_first");
    assert_eq!(nudge.call_id, "toolu_1");
    assert!(nudge.content.contains("find_definition"));
    assert!(nudge.content.contains("UserService"));
}

#[test]
fn quiet_command_nudges_once() {
    let mut state = ToolGuardrailRequestState::new();
    let config = ToolGuardrailConfig {
        quiet_commands: true,
        ..ToolGuardrailConfig::disabled()
    };
    let calls = vec![call("bash", json!({"command": "cargo test"}))];
    let first = evaluate_tool_guardrails(&calls, &[], &config, &mut state);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].kind, "quiet_command");
    assert!(first[0].content.contains("cargo test --quiet"));
    assert!(evaluate_tool_guardrails(&calls, &[], &config, &mut state).is_empty());
}

#[test]
fn only_offending_call_is_nudged() {
    let mut state = ToolGuardrailRequestState::new();
    let config = ToolGuardrailConfig {
        lsp_first: true,
        ..ToolGuardrailConfig::disabled()
    };
    let calls = vec![
        ToolCall {
            id: "toolu_grep".into(),
            name: "grep".into(),
            input: json!({"pattern": "UserService"}),
        },
        ToolCall {
            id: "toolu_write".into(),
            name: "write_file".into(),
            input: json!({"content": "ok"}),
        },
    ];
    let nudges = evaluate_tool_guardrails(
        &calls,
        &[tool_spec("grep"), tool_spec("find_definition")],
        &config,
        &mut state,
    );
    assert_eq!(nudges.len(), 1);
    assert_eq!(nudges[0].call_id, "toolu_grep");
}

#[test]
fn write_payload_cap_detects_oversized_payload() {
    let mut state = ToolGuardrailRequestState::new();
    let config = ToolGuardrailConfig {
        write_payload_caps: true,
        max_write_payload_bytes: 4,
        ..ToolGuardrailConfig::disabled()
    };
    let calls = vec![call("write_file", json!({"content": "12345"}))];
    let nudges = evaluate_tool_guardrails(&calls, &[], &config, &mut state);
    assert_eq!(nudges.len(), 1);
    assert_eq!(nudges[0].kind, "write_payload_cap");
    assert!(nudges[0].content.contains("5 bytes > 4 bytes"));
}

#[test]
fn write_payload_cap_zero_disables_nudge() {
    let mut state = ToolGuardrailRequestState::new();
    let config = ToolGuardrailConfig {
        write_payload_caps: true,
        max_write_payload_bytes: 0,
        ..ToolGuardrailConfig::disabled()
    };
    let calls = vec![call("write_file", json!({"content": "12345"}))];
    assert!(evaluate_tool_guardrails(&calls, &[], &config, &mut state).is_empty());
}

#[test]
fn write_payload_cap_does_not_collide_on_equal_length_different_content() {
    let mut state = ToolGuardrailRequestState::new();
    let config = ToolGuardrailConfig {
        write_payload_caps: true,
        max_write_payload_bytes: 4,
        ..ToolGuardrailConfig::disabled()
    };
    // Same byte length (5), different content/target -- both must nudge.
    let first = vec![call("write_file", json!({"content": "aaaaa"}))];
    let second = vec![call("write_file", json!({"content": "bbbbb"}))];
    assert_eq!(
        evaluate_tool_guardrails(&first, &[], &config, &mut state).len(),
        1
    );
    assert_eq!(
        evaluate_tool_guardrails(&second, &[], &config, &mut state).len(),
        1,
        "an unrelated oversized write of equal byte length must still be nudged"
    );
}

#[test]
fn write_payload_bytes_falls_back_to_unrecognized_field_names() {
    let args = json!({"body": "12345"});
    assert_eq!(write_cap::write_payload_bytes(args.as_object().unwrap()), 5);
}

#[test]
fn resolve_runtime_guardrails_prefers_runtime_override() {
    let static_config = ToolGuardrailConfig::disabled();
    let resolved = resolve_runtime_guardrails(&static_config, "standard");
    assert_eq!(resolved, ToolGuardrailConfig::standard());
}

#[test]
fn resolve_runtime_guardrails_keeps_static_when_modes_match() {
    let static_config = ToolGuardrailConfig {
        max_write_payload_bytes: 123,
        ..ToolGuardrailConfig::standard()
    };
    let resolved = resolve_runtime_guardrails(&static_config, "standard");
    // Same mode as the static preset -- the static config (with its
    // custom max_write_payload_bytes) must be preserved, not rebuilt
    // from the bare preset.
    assert_eq!(resolved, static_config);
}

#[test]
fn resolve_runtime_guardrails_falls_back_on_unparseable_value() {
    let static_config = ToolGuardrailConfig::standard();
    let resolved = resolve_runtime_guardrails(&static_config, "not-a-real-mode");
    assert_eq!(resolved, static_config);
}
