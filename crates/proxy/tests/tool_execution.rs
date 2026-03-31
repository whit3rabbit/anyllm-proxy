//! Integration tests for the tool execution engine.
//!
//! These tests exercise the public API of `crates/proxy/src/tools/` without
//! a running proxy or live backend.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyllm_proxy::tools::{
    PolicyAction, PolicyRule, Tool, ToolCall, ToolExecutionPolicy, ToolRegistry, ToolResult,
};
use anyllm_proxy::tools::execution::{
    execute_tool_calls, extract_tool_calls, is_duplicate, partition_tool_calls,
    tool_results_to_user_message, LoopConfig,
};
use anyllm_proxy::tools::trace::ToolOutcome;

// ---------------------------------------------------------------------------
// Test tool: uppercases the "text" field of the input
// ---------------------------------------------------------------------------

struct UpperTool;

impl Tool for UpperTool {
    fn name(&self) -> &str {
        "upper"
    }
    fn description(&self) -> &str {
        "Uppercases text"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"]
        })
    }
    fn execute<'a>(
        &'a self,
        input: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send + 'a>> {
        Box::pin(async move {
            let text = input["text"].as_str().unwrap_or("").to_uppercase();
            Ok(serde_json::json!({"result": text}))
        })
    }
}

// ---------------------------------------------------------------------------
// Shared setup
// ---------------------------------------------------------------------------

fn setup() -> (Arc<ToolRegistry>, Arc<ToolExecutionPolicy>) {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(UpperTool));
    let policy = ToolExecutionPolicy {
        default_action: PolicyAction::PassThrough,
        rules: vec![PolicyRule {
            tool_name: "upper".to_string(),
            action: PolicyAction::Allow,
            timeout: None,
            max_concurrency: None,
        }],
    };
    (Arc::new(reg), Arc::new(policy))
}

fn make_call(id: &str, name: &str, input: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        input,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn partition_allows_registered_tools() {
    let (reg, policy) = setup();
    let calls = vec![
        make_call("1", "upper", serde_json::json!({"text": "hi"})),
        make_call("2", "unknown", serde_json::json!({})),
    ];
    let (auto, pass) = partition_tool_calls(&calls, &reg, &policy);
    assert_eq!(auto.len(), 1);
    assert_eq!(auto[0].name, "upper");
    assert_eq!(pass.len(), 1);
    assert_eq!(pass[0].name, "unknown");
}

#[test]
fn partition_passthrough_policy_sends_all_through() {
    // A policy with no Allow rules means even registered tools pass through.
    let (reg, _) = setup();
    let passthrough_policy = ToolExecutionPolicy::default();

    let calls = vec![make_call("1", "upper", serde_json::json!({"text": "hi"}))];
    let (auto, pass) = partition_tool_calls(&calls, &reg, &passthrough_policy);
    assert!(auto.is_empty());
    assert_eq!(pass.len(), 1);
}

#[tokio::test]
async fn execute_registered_tool() {
    let (reg, policy) = setup();
    let config = LoopConfig::default();
    let call = make_call("tc_1", "upper", serde_json::json!({"text": "hello world"}));
    let refs: Vec<&ToolCall> = vec![&call];

    let results = execute_tool_calls(&refs, Arc::clone(&reg), &policy, &config).await;

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tool_use_id, "tc_1");
    match &results[0].outcome {
        ToolOutcome::Success(v) => assert_eq!(v["result"], "HELLO WORLD"),
        other => panic!("expected Success, got {:?}", other),
    }
}

#[tokio::test]
async fn execute_multiple_tools_in_order() {
    // Verify that results come back in submission order despite parallel execution.
    let (reg, policy) = setup();
    let config = LoopConfig::default();
    let c1 = make_call("tc_1", "upper", serde_json::json!({"text": "first"}));
    let c2 = make_call("tc_2", "upper", serde_json::json!({"text": "second"}));
    let refs: Vec<&ToolCall> = vec![&c1, &c2];

    let results = execute_tool_calls(&refs, Arc::clone(&reg), &policy, &config).await;

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].tool_use_id, "tc_1");
    assert_eq!(results[1].tool_use_id, "tc_2");
    match &results[0].outcome {
        ToolOutcome::Success(v) => assert_eq!(v["result"], "FIRST"),
        other => panic!("expected Success for tc_1, got {:?}", other),
    }
    match &results[1].outcome {
        ToolOutcome::Success(v) => assert_eq!(v["result"], "SECOND"),
        other => panic!("expected Success for tc_2, got {:?}", other),
    }
}

#[test]
fn tool_results_convert_to_user_message() {
    let results = vec![ToolResult {
        tool_use_id: "tc_1".into(),
        tool_name: "upper".into(),
        outcome: ToolOutcome::Success(serde_json::json!({"result": "HI"})),
    }];
    let msg = tool_results_to_user_message(&results);

    assert_eq!(msg.role, anyllm_translate::anthropic::Role::User);
    match &msg.content {
        anyllm_translate::anthropic::Content::Blocks(blocks) => {
            assert_eq!(blocks.len(), 1);
            match &blocks[0] {
                anyllm_translate::anthropic::ContentBlock::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } => {
                    assert_eq!(tool_use_id, "tc_1");
                    assert_eq!(*is_error, Some(false));
                }
                other => panic!("expected ToolResult block, got {:?}", other),
            }
        }
        other => panic!("expected Blocks content, got {:?}", other),
    }
}

#[test]
fn tool_results_error_outcome_sets_is_error_true() {
    let results = vec![ToolResult {
        tool_use_id: "tc_err".into(),
        tool_name: "upper".into(),
        outcome: ToolOutcome::Error {
            message: "something broke".into(),
            retryable: false,
        },
    }];
    let msg = tool_results_to_user_message(&results);
    match &msg.content {
        anyllm_translate::anthropic::Content::Blocks(blocks) => {
            match &blocks[0] {
                anyllm_translate::anthropic::ContentBlock::ToolResult { is_error, .. } => {
                    assert_eq!(*is_error, Some(true));
                }
                other => panic!("expected ToolResult block, got {:?}", other),
            }
        }
        other => panic!("expected Blocks content, got {:?}", other),
    }
}

#[test]
fn duplicate_detection_works() {
    let a = vec![make_call("1", "upper", serde_json::json!({"text": "same"}))];
    let b = vec![make_call("2", "upper", serde_json::json!({"text": "same"}))];
    assert!(is_duplicate(&a, &b), "same name+input with different IDs should be duplicate");

    let c = vec![make_call("3", "upper", serde_json::json!({"text": "different"}))];
    assert!(!is_duplicate(&a, &c), "different input should not be duplicate");
}

#[test]
fn duplicate_detection_different_lengths() {
    let a = vec![
        make_call("1", "upper", serde_json::json!({"text": "x"})),
        make_call("2", "upper", serde_json::json!({"text": "y"})),
    ];
    let b = vec![make_call("3", "upper", serde_json::json!({"text": "x"}))];
    assert!(!is_duplicate(&a, &b));
}

#[test]
fn extract_tool_calls_finds_tool_use_blocks() {
    use anyllm_translate::anthropic::{ContentBlock, MessageResponse, Role, StopReason, Usage};

    let resp = MessageResponse {
        id: "msg_1".into(),
        response_type: "message".into(),
        role: Role::Assistant,
        content: vec![
            ContentBlock::Text {
                text: "I will call the upper tool.".into(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "upper".into(),
                input: serde_json::json!({"text": "hello"}),
            },
        ],
        model: "test".into(),
        stop_reason: Some(StopReason::ToolUse),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        created: None,
    };

    let calls = extract_tool_calls(&resp);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "tu_1");
    assert_eq!(calls[0].name, "upper");
    assert_eq!(calls[0].input["text"], "hello");
}

#[test]
fn extract_tool_calls_empty_when_no_tool_use() {
    use anyllm_translate::anthropic::{ContentBlock, MessageResponse, Role, StopReason, Usage};

    let resp = MessageResponse {
        id: "msg_2".into(),
        response_type: "message".into(),
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "No tools here.".into(),
        }],
        model: "test".into(),
        stop_reason: Some(StopReason::EndTurn),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 5,
            output_tokens: 3,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        created: None,
    };

    let calls = extract_tool_calls(&resp);
    assert!(calls.is_empty());
}
