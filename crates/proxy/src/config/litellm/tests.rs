use super::*;

#[test]
fn parse_provider_model_openai() {
    let (kind, model, _) = parse_provider_model("openai/gpt-4o");
    assert_eq!(kind, BackendKind::OpenAI);
    assert_eq!(model, "gpt-4o");
}

#[test]
fn parse_provider_model_azure() {
    let (kind, model, _) = parse_provider_model("azure/gpt-4o-eu");
    assert_eq!(kind, BackendKind::AzureOpenAI);
    assert_eq!(model, "gpt-4o-eu");
}

#[test]
fn parse_provider_model_no_prefix() {
    let (kind, model, _) = parse_provider_model("gpt-4o");
    assert_eq!(kind, BackendKind::OpenAI);
    assert_eq!(model, "gpt-4o");
}

#[test]
fn parse_provider_model_vertex_ai() {
    let (kind, model, _) = parse_provider_model("vertex_ai/gemini-pro");
    assert_eq!(kind, BackendKind::Vertex);
    assert_eq!(model, "gemini-pro");
}

#[test]
fn parse_provider_model_bedrock() {
    let (kind, model, _) = parse_provider_model("bedrock/anthropic.claude-v2");
    assert_eq!(kind, BackendKind::Bedrock);
    assert_eq!(model, "anthropic.claude-v2");
}

#[test]
fn parse_provider_model_anthropic() {
    let (kind, model, provider) = parse_provider_model("anthropic/claude-sonnet-4-6");
    assert_eq!(kind, BackendKind::Anthropic);
    assert_eq!(model, "claude-sonnet-4-6");
    assert_eq!(provider.unwrap().id, "anthropic");
}

#[test]
fn parse_provider_model_legacy_alias() {
    let (kind, model, provider) = parse_provider_model("gmi_cloud/openai/gpt-5");
    assert_eq!(kind, BackendKind::OpenAI);
    assert_eq!(model, "openai/gpt-5");
    assert_eq!(provider.unwrap().id, "gmi");
}

#[test]
fn parse_provider_model_unknown_treated_as_openai() {
    let (kind, model, _) = parse_provider_model("groq/llama-70b");
    assert_eq!(kind, BackendKind::OpenAI);
    assert_eq!(model, "llama-70b");
}

#[test]
fn minimal_litellm_config() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test-key
"#;

    let (multi, router) = from_litellm_yaml(yaml);
    assert_eq!(multi.backends.len(), 1);
    assert!(router.has_model("gpt-4o"));

    let routed = router.route("gpt-4o").unwrap();
    assert_eq!(routed.actual_model, "gpt-4o");
}

#[test]
#[should_panic(expected = "provider 'azure_ai' requires api_base")]
fn known_provider_without_default_base_requires_api_base() {
    let yaml = r#"
model_list:
  - model_name: phi
    litellm_params:
      model: azure_ai/Phi-4
      api_key: sk-test-key
"#;

    let _ = from_litellm_yaml(yaml);
}

#[test]
fn known_provider_without_default_base_accepts_api_base() {
    let yaml = r#"
model_list:
  - model_name: phi
    litellm_params:
      model: azure_ai/Phi-4
      api_key: sk-test-key
      api_base: https://example.services.ai.azure.com/models
"#;

    let (multi, router) = from_litellm_yaml(yaml);
    assert!(router.has_model("phi"));
    assert_eq!(
        multi.backends["litellm_0"].base_url,
        "https://example.services.ai.azure.com/models"
    );
}

#[test]
fn multiple_deployments_same_model() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-key-1
      rpm: 100
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-key-2
      rpm: 200
"#;

    let (multi, router) = from_litellm_yaml(yaml);
    // Different api_keys = different backends
    assert_eq!(multi.backends.len(), 2);
    assert!(router.has_model("gpt-4o"));

    // Should round-robin between the two
    let r0 = router.route("gpt-4o").unwrap();
    let r1 = router.route("gpt-4o").unwrap();
    assert_ne!(r0.backend_name, r1.backend_name);
}

#[test]
fn backend_deduplication() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-same-key
  - model_name: gpt-4o-mini
    litellm_params:
      model: openai/gpt-4o-mini
      api_key: sk-same-key
"#;

    let (multi, router) = from_litellm_yaml(yaml);
    // Same provider + base_url + api_key = one backend
    assert_eq!(multi.backends.len(), 1);
    assert!(router.has_model("gpt-4o"));
    assert!(router.has_model("gpt-4o-mini"));
}

#[test]
fn os_environ_syntax_in_litellm_yaml() {
    let _env = crate::config::ENV_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Set env var for test
    unsafe { std::env::set_var("TEST_LITELLM_KEY", "sk-from-env") };

    let yaml = r#"
model_list:
  - model_name: test-model
    litellm_params:
      model: openai/gpt-4o
      api_key: "os.environ/TEST_LITELLM_KEY"
"#;

    let (multi, _) = from_litellm_yaml(yaml);
    let bc = multi.backends.values().next().unwrap();
    assert_eq!(bc.api_key, "sk-from-env");

    unsafe { std::env::remove_var("TEST_LITELLM_KEY") };
}

#[test]
fn unknown_settings_are_accepted() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test
      some_unknown_param: true

litellm_settings:
  drop_params: true
  some_future_setting: 42

general_settings:
  some_unknown_general: "value"
"#;

    // Should not panic; unknown fields are captured by serde(flatten).
    let (multi, router) = from_litellm_yaml(yaml);
    assert_eq!(multi.backends.len(), 1);
    assert!(router.has_model("gpt-4o"));
}

#[test]
fn routing_strategy_parsed() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test

router_settings:
  routing_strategy: least-busy
"#;
    let (_, router) = from_litellm_yaml(yaml);
    assert_eq!(router.strategy(), RoutingStrategy::LeastBusy);
}

#[test]
fn routing_strategy_latency() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test

router_settings:
  routing_strategy: latency-based-routing
"#;
    let (_, router) = from_litellm_yaml(yaml);
    assert_eq!(router.strategy(), RoutingStrategy::LatencyBased);
}

#[test]
fn routing_strategy_cost_based() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test

router_settings:
  routing_strategy: cost-based
"#;
    let (_, router) = from_litellm_yaml(yaml);
    assert_eq!(router.strategy(), RoutingStrategy::CostBased);
}

#[test]
fn routing_strategy_defaults_to_round_robin() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test
"#;
    let (_, router) = from_litellm_yaml(yaml);
    assert_eq!(router.strategy(), RoutingStrategy::RoundRobin);
}

#[test]
fn weight_field_parsed() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-key-1
      weight: 3
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-key-2
      weight: 1

router_settings:
  routing_strategy: weighted
"#;
    let (_, router) = from_litellm_yaml(yaml);
    assert_eq!(router.strategy(), RoutingStrategy::Weighted);
    assert!(router.has_model("gpt-4o"));
}

#[test]
fn langfuse_callback_sets_flag() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test
litellm_settings:
  callbacks:
    - langfuse
"#;
    let parsed = parse_litellm_yaml(yaml);
    assert!(parsed.langfuse_requested);
    assert!(parsed.callback_urls.is_empty()); // "langfuse" filtered out
}

#[test]
fn webhook_url_not_flagged_as_langfuse() {
    let yaml = r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test
litellm_settings:
  callbacks:
    - https://my-webhook.example.com/hook
"#;
    let parsed = parse_litellm_yaml(yaml);
    assert!(!parsed.langfuse_requested);
    assert_eq!(parsed.callback_urls.len(), 1);
}

#[test]
#[should_panic(expected = "at least one entry")]
fn empty_model_list_panics() {
    let yaml = r#"
model_list: []
"#;
    from_litellm_yaml(yaml);
}

#[test]
fn gemini_provider() {
    let yaml = r#"
model_list:
  - model_name: gemini-pro
    litellm_params:
      model: gemini/gemini-pro
      api_key: AIzaSy-test
"#;

    let (multi, router) = from_litellm_yaml(yaml);
    assert_eq!(multi.backends.len(), 1);
    let bc = multi.backends.values().next().unwrap();
    assert_eq!(bc.kind, BackendKind::Gemini);
    assert!(router.has_model("gemini-pro"));
}

#[test]
fn anthropic_provider_uses_env_fallback_and_base_url() {
    let _env = crate::config::ENV_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "sk-anthropic-env");
        std::env::set_var("ANTHROPIC_BASE_URL", "https://anthropic-proxy.example");
    }

    let yaml = r#"
model_list:
  - model_name: claude
    litellm_params:
      model: anthropic/claude-sonnet-4-6
"#;

    let (multi, router) = from_litellm_yaml(yaml);
    assert_eq!(multi.backends.len(), 1);
    let bc = multi.backends.values().next().unwrap();
    assert_eq!(bc.kind, BackendKind::Anthropic);
    assert_eq!(bc.api_key, "sk-anthropic-env");
    assert_eq!(bc.base_url, "https://anthropic-proxy.example");

    let routed = router.route("claude").unwrap();
    assert_eq!(routed.actual_model, "claude-sonnet-4-6");

    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ANTHROPIC_BASE_URL");
    }
}
