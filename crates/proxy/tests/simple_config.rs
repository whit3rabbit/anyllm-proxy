//! Integration tests: MultiConfig::load() dispatches simple YAML format.

use anyllm_proxy::config::MultiConfig;

#[test]
fn load_dispatches_simple_format_by_models_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routing.yaml");
    std::fs::write(
        &path,
        r#"
models:
  - openai/gpt-4o
  - openai/gpt-4o-mini
routing_strategy: least-busy
"#,
    )
    .unwrap();

    // SAFETY: test-only env var mutation
    unsafe { std::env::set_var("PROXY_CONFIG", path.to_str().unwrap()) };
    unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test") };

    let result = MultiConfig::load();
    assert!(
        result.model_router.is_some(),
        "simple format must produce a model router"
    );

    let router_arc = result.model_router.unwrap();
    let router = router_arc.read().unwrap();
    assert!(router.has_model("gpt-4o"));
    assert!(router.has_model("gpt-4o-mini"));
    assert_eq!(
        router.strategy(),
        anyllm_proxy::config::model_router::RoutingStrategy::LeastBusy
    );

    unsafe {
        std::env::remove_var("PROXY_CONFIG");
        std::env::remove_var("OPENAI_API_KEY");
    };
}

#[test]
fn load_litellm_format_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("litellm.yaml");
    std::fs::write(
        &path,
        r#"
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-test
"#,
    )
    .unwrap();

    unsafe { std::env::set_var("PROXY_CONFIG", path.to_str().unwrap()) };

    let result = MultiConfig::load();
    assert!(result.model_router.is_some());
    let router = result.model_router.unwrap();
    assert!(router.read().unwrap().has_model("gpt-4o"));

    unsafe { std::env::remove_var("PROXY_CONFIG") };
}
