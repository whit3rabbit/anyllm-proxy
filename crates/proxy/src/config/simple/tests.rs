use super::*;

#[test]
fn simple_config_roundtrip_string_shorthand() {
    let yaml = r#"
models:
  - gpt-4o
  - openai/gpt-4o-mini
  - anthropic/claude-3-5-sonnet-20241022
"#;
    let cfg: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(cfg.models.len(), 3);
    match &cfg.models[0] {
        SimpleModelEntry::Shorthand(s) => assert_eq!(s, "gpt-4o"),
        SimpleModelEntry::Full(_) => panic!("expected shorthand"),
    }
}

#[test]
fn simple_config_roundtrip_full_entry() {
    let yaml = r#"
routing_strategy: weighted
models:
  - name: smart
    model: gpt-4o
    provider: openai
    weight: 3
    rpm: 1000
"#;
    let cfg: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(cfg.routing_strategy.as_deref(), Some("weighted"));
    assert_eq!(cfg.models.len(), 1);
    match &cfg.models[0] {
        SimpleModelEntry::Full(f) => {
            assert_eq!(f.name.as_deref(), Some("smart"));
            assert_eq!(f.model, "gpt-4o");
            assert_eq!(f.provider.as_deref(), Some("openai"));
            assert_eq!(f.weight, Some(3));
            assert_eq!(f.rpm, Some(1000));
        }
        SimpleModelEntry::Shorthand(_) => panic!("expected full entry"),
    }
}

#[test]
fn simple_config_mixed_entries() {
    let yaml = r#"
models:
  - gpt-4o
  - name: my-model
    model: claude-3-5-sonnet-20241022
    provider: anthropic
"#;
    let cfg: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(cfg.models.len(), 2);
}

#[test]
fn parse_single_openai_model() {
    unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };
    let yaml = r#"
models:
  - gpt-4o
"#;
    let parsed = parse_simple_yaml(yaml);
    assert_eq!(parsed.multi_config.backends.len(), 1);
    assert!(parsed.router.has_model("gpt-4o"));
    let routed = parsed.router.route("gpt-4o").unwrap();
    assert_eq!(routed.actual_model, "gpt-4o");
    unsafe { std::env::remove_var("OPENAI_API_KEY") };
}

#[test]
fn parse_simple_yaml_redact_secrets() {
    unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };
    let yaml = r#"
redact_secrets: true
models:
  - gpt-4o
"#;
    let parsed = parse_simple_yaml(yaml);
    assert!(parsed.multi_config.redact_secrets);
    unsafe { std::env::remove_var("OPENAI_API_KEY") };
}

#[test]
fn parse_provider_slash_model_shorthand() {
    unsafe {
        std::env::set_var("OPENAI_API_KEY", "sk-openai");
        std::env::set_var("ANTHROPIC_API_KEY", "sk-anthropic");
    };
    let yaml = r#"
models:
  - openai/gpt-4o
  - anthropic/claude-3-5-sonnet-20241022
"#;
    let parsed = parse_simple_yaml(yaml);
    assert_eq!(parsed.multi_config.backends.len(), 2);
    assert!(parsed.router.has_model("gpt-4o"));
    assert!(parsed.router.has_model("claude-3-5-sonnet-20241022"));
    unsafe {
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");
    };
}

#[test]
fn parse_full_entry_with_virtual_name() {
    unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };
    let yaml = r#"
models:
  - name: smart
    model: gpt-4o
    provider: openai
    weight: 3
"#;
    let parsed = parse_simple_yaml(yaml);
    assert!(parsed.router.has_model("smart"));
    assert!(!parsed.router.has_model("gpt-4o"));
    let routed = parsed.router.route("smart").unwrap();
    assert_eq!(routed.actual_model, "gpt-4o");
    unsafe { std::env::remove_var("OPENAI_API_KEY") };
}

#[test]
fn parse_routing_strategy_latency() {
    unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };
    let yaml = r#"
routing_strategy: latency-based
models:
  - gpt-4o
"#;
    let parsed = parse_simple_yaml(yaml);
    assert_eq!(
        parsed.router.strategy(),
        crate::config::model_router::RoutingStrategy::LatencyBased
    );
    unsafe { std::env::remove_var("OPENAI_API_KEY") };
}

#[test]
fn parse_weighted_two_deployments_same_virtual_name() {
    unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };
    let yaml = r#"
routing_strategy: weighted
models:
  - name: smart
    model: gpt-4o
    provider: openai
    weight: 3
  - name: smart
    model: gpt-4o-mini
    provider: openai
    weight: 1
"#;
    let parsed = parse_simple_yaml(yaml);
    assert!(parsed.router.has_model("smart"));
    let list = parsed.router.list_models();
    let (_, count) = list.iter().find(|(n, _)| *n == "smart").unwrap();
    assert_eq!(*count, 2);
    unsafe { std::env::remove_var("OPENAI_API_KEY") };
}

#[test]
fn parse_api_key_inline_overrides_env() {
    unsafe { std::env::set_var("OPENAI_API_KEY", "sk-from-env") };
    let yaml = r#"
models:
  - name: my-model
    model: gpt-4o
    provider: openai
    api_key: sk-inline-key
"#;
    let parsed = parse_simple_yaml(yaml);
    let bc = parsed.multi_config.backends.values().next().unwrap();
    assert_eq!(bc.api_key, "sk-inline-key");
    unsafe { std::env::remove_var("OPENAI_API_KEY") };
}

#[test]
#[should_panic(expected = "must define at least one model")]
fn parse_empty_models_panics() {
    let yaml = "models: []\n";
    parse_simple_yaml(yaml);
}

#[test]
fn parse_tool_execution_config() {
    let yaml = r#"
models:
  - gpt-4o

tool_execution:
  max_iterations: 3
  tool_timeout_secs: 60
  total_timeout_secs: 600
  guardrails: standard
  max_write_payload_bytes: 4096

builtin_tools:
  execute_bash:
    enabled: false
  read_file:
    enabled: true
    policy: allow

mcp_servers:
  - name: github
    url: https://mcp.github.com/sse
    policy: allow
"#;
    let config: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    let te = config.tool_execution.unwrap();
    assert_eq!(te.max_iterations, Some(3));
    assert_eq!(te.tool_timeout_secs, Some(60));
    assert_eq!(te.total_timeout_secs, Some(600));
    assert_eq!(te.guardrails.as_deref(), Some("standard"));
    assert_eq!(te.max_write_payload_bytes, Some(4096));

    let builtins = config.builtin_tools.unwrap();
    let bash = builtins.get("execute_bash").unwrap();
    assert!(!bash.enabled);
    let rf = builtins.get("read_file").unwrap();
    assert!(rf.enabled);
    assert_eq!(rf.policy.as_deref(), Some("allow"));

    let mcp = config.mcp_servers.unwrap();
    assert_eq!(mcp.len(), 1);
    assert_eq!(mcp[0].name, "github");
    assert_eq!(mcp[0].policy.as_deref(), Some("allow"));
}

#[test]
fn parse_config_without_tool_sections() {
    let yaml = r#"
models:
  - gpt-4o
"#;
    let config: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    assert!(config.tool_execution.is_none());
    assert!(config.builtin_tools.is_none());
    assert!(config.mcp_servers.is_none());
}

#[test]
fn build_tool_policy_from_config() {
    let yaml = r#"
models:
  - gpt-4o

builtin_tools:
  execute_bash:
    enabled: true
    policy: deny
  read_file:
    enabled: true
    policy: allow
    timeout_secs: 10

mcp_servers:
  - name: github
    url: https://mcp.github.com/sse
    policy: allow
"#;
    let config: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    let (policy, loop_config) = config.build_tool_config();

    use crate::tools::policy::PolicyAction;
    assert_eq!(policy.resolve("execute_bash"), PolicyAction::Deny);
    assert_eq!(policy.resolve("read_file"), PolicyAction::Allow);
    assert_eq!(policy.resolve("unknown"), PolicyAction::PassThrough);

    let rule = policy.find_rule("read_file").unwrap();
    assert_eq!(rule.timeout, Some(std::time::Duration::from_secs(10)));

    // MCP glob rule
    assert_eq!(
        policy.resolve("mcp_github_search_repos"),
        PolicyAction::Allow
    );

    // Default loop config (no tool_execution section)
    assert_eq!(loop_config.max_iterations, 1);
}

#[test]
fn build_tool_policy_with_loop_config() {
    let yaml = r#"
models:
  - gpt-4o

tool_execution:
  max_iterations: 5
  tool_timeout_secs: 45
"#;
    let config: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    let (_policy, loop_config) = config.build_tool_config();
    assert_eq!(loop_config.max_iterations, 5);
    assert_eq!(loop_config.tool_timeout, std::time::Duration::from_secs(45));
    assert_eq!(
        loop_config.total_timeout,
        std::time::Duration::from_secs(300)
    );
}

#[test]
fn build_tool_guardrail_config_from_tool_execution() {
    let yaml = r#"
models:
  - gpt-4o

tool_execution:
  guardrails: standard
  max_write_payload_bytes: 12
"#;
    let config: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    let guardrails = config.build_tool_guardrail_config();

    assert_eq!(guardrails.mode, crate::tools::ToolGuardrailMode::Standard);
    assert!(guardrails.lsp_first);
    assert!(guardrails.quiet_commands);
    assert!(guardrails.write_payload_caps);
    assert_eq!(guardrails.max_write_payload_bytes, 12);
}

#[test]
fn tool_guardrails_default_to_disabled() {
    let yaml = r#"
models:
  - gpt-4o
"#;
    let config: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    let guardrails = config.build_tool_guardrail_config();

    assert_eq!(guardrails.mode, crate::tools::ToolGuardrailMode::Disabled);
    assert!(!guardrails.enabled());
}
