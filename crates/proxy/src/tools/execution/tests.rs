use super::*;
use crate::tools::policy::{PolicyAction, PolicyRule, ToolExecutionPolicy};
use crate::tools::registry::ToolRegistry;
use serde_json::json;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

// --- Test tool implementations ---

struct EchoTool;

impl crate::tools::registry::Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "Echoes input text in uppercase."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"text": {"type": "string"}}})
    }
    fn execute<'a>(
        &'a self,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>> {
        Box::pin(async move {
            let text = input
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_uppercase();
            Ok(json!({"result": text}))
        })
    }
}

/// Registered under the name a guardrail heuristic (`is_grep_tool`) matches,
/// so tests can prove nudges only apply to calls this proxy actually owns.
struct GrepStubTool;

impl crate::tools::registry::Tool for GrepStubTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        "Stub grep tool for guardrail tests."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"pattern": {"type": "string"}}})
    }
    fn execute<'a>(
        &'a self,
        _input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>> {
        Box::pin(async move { Ok(json!({"matches": []})) })
    }
}

struct FailTool;

impl crate::tools::registry::Tool for FailTool {
    fn name(&self) -> &str {
        "fail"
    }
    fn description(&self) -> &str {
        "Always returns an error."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn execute<'a>(
        &'a self,
        _input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>> {
        Box::pin(async move { Err("always fails".to_string()) })
    }
}

fn allow_policy(tool_name: &str) -> ToolExecutionPolicy {
    ToolExecutionPolicy {
        default_action: PolicyAction::PassThrough,
        rules: vec![PolicyRule {
            tool_name: tool_name.to_string(),
            action: PolicyAction::Allow,
            timeout: None,
            max_concurrency: None,
        }],
    }
}

fn passthrough_policy() -> ToolExecutionPolicy {
    ToolExecutionPolicy::default()
}

fn make_call(id: &str, name: &str, input: Value) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        input,
    }
}

fn advertised(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

// 1. passthrough policy -> all tools pass through
#[test]
fn partition_no_auto_execute() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(EchoTool));
    let policy = passthrough_policy();

    let calls = vec![make_call("id1", "echo", json!({"text": "hi"}))];
    let advertised_tools = advertised(&["echo"]);
    let (auto, pass, denied) = partition_tool_calls(&calls, &registry, &policy, &advertised_tools);

    assert!(auto.is_empty());
    assert_eq!(pass.len(), 1);
    assert!(denied.is_empty());
}

// 2. allow policy + registered tool -> auto-execute; unregistered -> pass through
#[test]
fn partition_with_allow_policy() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(EchoTool));
    let policy = allow_policy("echo");

    let calls = vec![
        make_call("id1", "echo", json!({"text": "hi"})),
        make_call("id2", "unknown_tool", json!({})),
    ];
    let advertised_tools = advertised(&["echo", "unknown_tool"]);
    let (auto, pass, denied) = partition_tool_calls(&calls, &registry, &policy, &advertised_tools);

    assert_eq!(auto.len(), 1);
    assert_eq!(auto[0].name, "echo");
    assert_eq!(pass.len(), 1);
    assert_eq!(pass[0].name, "unknown_tool");
    assert!(denied.is_empty());
}

#[test]
fn partition_allow_policy_requires_server_advertised_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(EchoTool));
    let policy = allow_policy("echo");

    let calls = vec![make_call("id1", "echo", json!({"text": "hi"}))];
    let advertised_tools = advertised(&[]);
    let (auto, pass, denied) = partition_tool_calls(&calls, &registry, &policy, &advertised_tools);

    assert!(auto.is_empty());
    assert_eq!(pass.len(), 1);
    assert_eq!(pass[0].name, "echo");
    assert!(denied.is_empty());
}

// 2b. deny policy -> tool goes to denied bucket, not pass_through
#[test]
fn partition_deny_policy() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(EchoTool));
    let policy = ToolExecutionPolicy {
        default_action: PolicyAction::PassThrough,
        rules: vec![PolicyRule {
            tool_name: "echo".to_string(),
            action: PolicyAction::Deny,
            timeout: None,
            max_concurrency: None,
        }],
    };

    let calls = vec![make_call("id1", "echo", json!({"text": "hi"}))];
    let advertised_tools = advertised(&["echo"]);
    let (auto, pass, denied) = partition_tool_calls(&calls, &registry, &policy, &advertised_tools);

    assert!(auto.is_empty());
    assert!(pass.is_empty());
    assert_eq!(denied.len(), 1);
    assert_eq!(denied[0].name, "echo");
}

// 2c. denied_tool_results generates error ToolResult with correct message
#[test]
fn denied_tool_results_generates_error_results() {
    let calls = [make_call("id1", "rm_rf", json!({}))];
    let refs: Vec<&ToolCall> = calls.iter().collect();
    let results = denied_tool_results(&refs);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tool_use_id, "id1");
    assert_eq!(results[0].tool_name, "rm_rf");
    match &results[0].outcome {
        ToolOutcome::Error { message, retryable } => {
            assert!(message.contains("rm_rf"));
            assert!(message.contains("denied"));
            assert!(!retryable);
        }
        other => panic!("expected Error outcome, got {:?}", other),
    }
}

// 3. EchoTool executes successfully
#[tokio::test]
async fn execute_tools_parallel_success() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(EchoTool));
    let registry = Arc::new(registry);
    let policy = allow_policy("echo");
    let config = LoopConfig::default();

    let call = make_call("id1", "echo", json!({"text": "hello"}));
    let refs: Vec<&ToolCall> = vec![&call];

    let results = execute_tool_calls(&refs, registry, &policy, &config).await;

    assert_eq!(results.len(), 1);
    match &results[0].outcome {
        ToolOutcome::Success(v) => assert_eq!(v["result"], "HELLO"),
        other => panic!("expected Success, got {:?}", other),
    }
}

// 4. FailTool -> Error with "always fails"
#[tokio::test]
async fn execute_tools_parallel_failure() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(FailTool));
    let registry = Arc::new(registry);
    let policy = allow_policy("fail");
    let config = LoopConfig::default();

    let call = make_call("id2", "fail", json!({}));
    let refs: Vec<&ToolCall> = vec![&call];

    let results = execute_tool_calls(&refs, registry, &policy, &config).await;

    assert_eq!(results.len(), 1);
    match &results[0].outcome {
        ToolOutcome::Error { message, .. } => assert_eq!(message, "always fails"),
        other => panic!("expected Error, got {:?}", other),
    }
}

// 5. Same name+input, different IDs -> duplicate
#[test]
fn duplicate_detection_identifies_same_calls() {
    let a = vec![make_call("id1", "echo", json!({"text": "hi"}))];
    let b = vec![make_call("id2", "echo", json!({"text": "hi"}))];
    assert!(is_duplicate(&a, &b));
}

// 6. Same name, different input -> not duplicate
#[test]
fn duplicate_detection_different_args() {
    let a = vec![make_call("id1", "echo", json!({"text": "hello"}))];
    let b = vec![make_call("id2", "echo", json!({"text": "world"}))];
    assert!(!is_duplicate(&a, &b));
}

// 7. extract_tool_calls picks up ToolUse blocks
#[test]
fn extract_tool_calls_finds_tool_use_blocks() {
    use anyllm_translate::anthropic::{ContentBlock, MessageResponse, Role, StopReason, Usage};

    let resp = MessageResponse {
        id: "msg_1".into(),
        response_type: "message".into(),
        role: Role::Assistant,
        content: vec![
            ContentBlock::Text {
                text: "Let me call a tool.".into(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "echo".into(),
                input: json!({"text": "hello"}),
            },
            ContentBlock::ToolUse {
                id: "tu_2".into(),
                name: "search".into(),
                input: json!({"query": "rust"}),
            },
        ],
        model: "test".into(),
        stop_reason: Some(StopReason::ToolUse),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            ..Default::default()
        },
        created: None,
    };

    let calls = extract_tool_calls(&resp);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].name, "echo");
    assert_eq!(calls[0].id, "tu_1");
    assert_eq!(calls[1].name, "search");
    assert_eq!(calls[1].id, "tu_2");
}

// 8. extract_tool_calls returns empty vec when no ToolUse blocks
#[test]
fn extract_tool_calls_empty_when_no_tool_use() {
    use anyllm_translate::anthropic::{ContentBlock, MessageResponse, Role, StopReason, Usage};

    let resp = MessageResponse {
        id: "msg_2".into(),
        response_type: "message".into(),
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "Just text, no tools.".into(),
        }],
        model: "test".into(),
        stop_reason: Some(StopReason::EndTurn),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 5,
            output_tokens: 10,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            ..Default::default()
        },
        created: None,
    };

    let calls = extract_tool_calls(&resp);
    assert!(calls.is_empty());
}

fn message_response_with_tool(
    name: &str,
    input: Value,
) -> anyllm_translate::anthropic::MessageResponse {
    use anyllm_translate::anthropic::{ContentBlock, MessageResponse, Role, StopReason, Usage};

    MessageResponse {
        id: format!("msg_{name}"),
        response_type: "message".into(),
        role: Role::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: format!("toolu_{name}"),
            name: name.to_string(),
            input,
        }],
        model: "test".into(),
        stop_reason: Some(StopReason::ToolUse),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            ..Default::default()
        },
        created: None,
    }
}

fn request_with_tools() -> anyllm_translate::anthropic::MessageCreateRequest {
    use anyllm_translate::anthropic::{Content, InputMessage, MessageCreateRequest, Role, Tool};

    MessageCreateRequest {
        model: "test".into(),
        max_tokens: 128,
        messages: vec![InputMessage {
            role: Role::User,
            content: Content::Text("find UserService".into()),
        }],
        system: None,
        temperature: None,
        top_p: None,
        top_k: None,
        stop_sequences: None,
        tools: Some(vec![
            Tool {
                name: "grep".into(),
                description: None,
                input_schema: json!({"type": "object"}),
            },
            Tool {
                name: "find_definition".into(),
                description: None,
                input_schema: json!({"type": "object"}),
            },
        ]),
        tool_choice: None,
        metadata: None,
        thinking: None,
        stream: None,
        extra: Default::default(),
    }
}

// Guardrail nudges must only ever apply to a tool call this proxy actually
// owns (registered + server-advertised + policy Allow) -- i.e. one that
// would otherwise land in `auto_exec`. "grep" here is registered, advertised,
// and Allow-policied, so it IS proxy-owned, and the nudge legitimately
// intercepts it before execution.
#[tokio::test]
async fn maybe_execute_tools_retries_after_guardrail_nudge() {
    use anyllm_translate::anthropic::{Content, ContentBlock, ToolResultContent};
    use std::sync::Mutex;

    let mut registry = ToolRegistry::new();
    registry.register(Box::new(GrepStubTool));
    let engine = ToolEngineState {
        registry: Arc::new(registry),
        policy: Arc::new(allow_policy("grep")),
        loop_config: LoopConfig {
            max_iterations: 2,
            ..LoopConfig::default()
        },
        guardrails: crate::tools::ToolGuardrailConfig {
            lsp_first: true,
            ..crate::tools::ToolGuardrailConfig::disabled()
        },
        mcp_manager: None,
    };
    let original_req = request_with_tools();
    let initial_response = message_response_with_tool("grep", json!({"pattern": "UserService"}));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_for_call = seen.clone();

    let (final_response, trace) = maybe_execute_tools(
        &engine,
        &original_req,
        &advertised(&["grep"]),
        initial_response,
        &engine.guardrails,
        move |follow_up_req| {
            let seen = seen_for_call.clone();
            async move {
                seen.lock().unwrap().push(follow_up_req.clone());
                Ok(message_response_with_tool(
                    "find_definition",
                    json!({"symbol": "UserService"}),
                ))
            }
        },
    )
    .await;

    assert_eq!(trace.iterations.len(), 1);
    assert_eq!(seen.lock().unwrap().len(), 1);
    let follow_up = seen.lock().unwrap()[0].clone();
    assert_eq!(follow_up.messages.len(), 3);
    match &follow_up.messages[1].content {
        Content::Blocks(blocks) => {
            assert!(matches!(
                blocks.first(),
                Some(ContentBlock::ToolUse { name, .. }) if name == "grep"
            ));
        }
        other => panic!("expected assistant tool_use blocks, got {:?}", other),
    }
    match &follow_up.messages[2].content {
        Content::Blocks(blocks) => match blocks.first() {
            Some(ContentBlock::ToolResult {
                content: Some(ToolResultContent::Text(text)),
                is_error,
                ..
            }) => {
                assert_eq!(*is_error, Some(true));
                assert!(text.contains("[ToolCallPolicyNudge]"));
                assert!(text.contains("find_definition"));
            }
            other => panic!("expected nudge tool_result, got {:?}", other),
        },
        other => panic!("expected user tool_result blocks, got {:?}", other),
    }

    let calls = extract_tool_calls(&final_response);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "find_definition");
    assert_eq!(calls[0].input["symbol"], "UserService");
}

// The proxy must never nudge a pass-through tool call: an unregistered
// (client-owned) "grep" call that matches the same guardrail heuristic must
// come back untouched, with the model's real tool_use response returned
// immediately -- never swallowed into a synthetic nudge + follow-up call.
#[tokio::test]
async fn maybe_execute_tools_never_nudges_a_pass_through_call() {
    use std::sync::Mutex;

    let engine = ToolEngineState {
        registry: Arc::new(ToolRegistry::new()), // "grep" not registered
        policy: Arc::new(ToolExecutionPolicy::default()),
        loop_config: LoopConfig::default(),
        guardrails: crate::tools::ToolGuardrailConfig {
            lsp_first: true,
            ..crate::tools::ToolGuardrailConfig::disabled()
        },
        mcp_manager: None,
    };
    let original_req = request_with_tools();
    let initial_response = message_response_with_tool("grep", json!({"pattern": "UserService"}));
    let called = Arc::new(Mutex::new(false));
    let called_for_call = called.clone();

    // "grep" is not registered and not server-advertised (empty set below),
    // so it is pass-through: the proxy does not own it and must not answer
    // on its behalf, regardless of the lsp_first guardrail matching its name.
    let (final_response, trace) = maybe_execute_tools(
        &engine,
        &original_req,
        &HashSet::new(),
        initial_response,
        &engine.guardrails,
        move |_follow_up_req| {
            let called = called_for_call.clone();
            async move {
                *called.lock().unwrap() = true;
                panic!("backend_call must not be invoked for a pass-through-only turn");
            }
        },
    )
    .await;

    assert!(
        !*called.lock().unwrap(),
        "must not make a follow-up backend call for a pass-through tool call"
    );
    assert_eq!(trace.iterations.len(), 0);
    let calls = extract_tool_calls(&final_response);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "grep");
    assert_eq!(calls[0].input["pattern"], "UserService");
}
