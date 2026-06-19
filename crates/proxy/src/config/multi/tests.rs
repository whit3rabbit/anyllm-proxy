use super::*;
use crate::config::{resolve_env_value, validate_base_url};

#[test]
fn model_mapping_haiku() {
    let m = ModelMapping {
        big_model: "gpt-4o".into(),
        small_model: "gpt-4o-mini".into(),
    };
    assert_eq!(m.map_model("claude-3-haiku-20240307"), "gpt-4o-mini");
    assert_eq!(m.map_model("claude-haiku-4-5-20251001"), "gpt-4o-mini");
}

#[test]
fn model_mapping_sonnet() {
    let m = ModelMapping {
        big_model: "gpt-4o".into(),
        small_model: "gpt-4o-mini".into(),
    };
    assert_eq!(m.map_model("claude-sonnet-4-6"), "gpt-4o");
    assert_eq!(m.map_model("claude-3-5-sonnet-20241022"), "gpt-4o");
}

#[test]
fn model_mapping_opus() {
    let m = ModelMapping {
        big_model: "gpt-4o".into(),
        small_model: "gpt-4o-mini".into(),
    };
    assert_eq!(m.map_model("claude-opus-4-6"), "gpt-4o");
}

#[test]
fn model_mapping_passthrough() {
    let m = ModelMapping {
        big_model: "gpt-4o".into(),
        small_model: "gpt-4o-mini".into(),
    };
    // Unrecognized models pass through unchanged
    assert_eq!(m.map_model("gpt-4o"), "gpt-4o");
    assert_eq!(m.map_model("custom-model"), "custom-model");
}

#[test]
fn model_mapping_case_insensitive() {
    let m = ModelMapping {
        big_model: "gpt-4o".into(),
        small_model: "gpt-4o-mini".into(),
    };
    assert_eq!(m.map_model("Claude-Sonnet-4-6"), "gpt-4o");
    assert_eq!(m.map_model("CLAUDE-HAIKU-4-5"), "gpt-4o-mini");
}

#[test]
fn model_mapping_custom_values() {
    let m = ModelMapping {
        big_model: "o1-preview".into(),
        small_model: "o1-mini".into(),
    };
    assert_eq!(m.map_model("claude-sonnet-4-6"), "o1-preview");
    assert_eq!(m.map_model("claude-haiku-4-5-20251001"), "o1-mini");
}

// --- Vertex / BackendKind tests ---

#[test]
fn vertex_url_construction() {
    let url = format!(
        "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/endpoints/openapi",
        "us-central1", "my-project", "us-central1"
    );
    assert_eq!(
        url,
        "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/endpoints/openapi"
    );
}

#[test]
fn vertex_base_url_passes_ssrf() {
    let url = "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/endpoints/openapi";
    assert!(validate_base_url(url).is_ok());
}

#[test]
fn vertex_model_defaults() {
    let m = ModelMapping::from_env_with_defaults("gemini-2.5-pro", "gemini-2.5-flash");
    // When BIG_MODEL/SMALL_MODEL env vars are not set, uses Vertex defaults
    // (This test works because env vars are unlikely to be set in test environment)
    assert_eq!(m.map_model("claude-sonnet-4-6"), "gemini-2.5-pro");
    assert_eq!(m.map_model("claude-haiku-4-5"), "gemini-2.5-flash");
}

#[test]
fn backend_auth_debug_redacts() {
    let bearer = BackendAuth::BearerToken("secret-token".into());
    let debug = format!("{:?}", bearer);
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains("secret-token"));

    let api_key = BackendAuth::GoogleApiKey("secret-key".into());
    let debug = format!("{:?}", api_key);
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains("secret-key"));

    let azure_key = BackendAuth::AzureApiKey("azure-secret".into());
    let debug = format!("{:?}", azure_key);
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains("azure-secret"));
}

// --- MultiConfig TOML parsing tests ---

#[test]
fn multi_config_parses_openai_backend() {
    let toml = r#"
        listen_port = 4000
        default_backend = "openai"

        [backends.openai]
        kind = "openai"
        api_key = "sk-test"
        big_model = "gpt-4o"
        small_model = "gpt-4o-mini"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert_eq!(mc.listen_port, 4000);
    assert_eq!(mc.default_backend, "openai");
    assert_eq!(mc.backends.len(), 1);
    let bc = &mc.backends["openai"];
    assert_eq!(bc.kind, BackendKind::OpenAI);
    assert_eq!(bc.api_key, "sk-test");
    assert_eq!(bc.model_mapping.big_model, "gpt-4o");
    assert_eq!(bc.model_mapping.small_model, "gpt-4o-mini");
}

#[test]
fn multi_config_parses_multiple_backends() {
    let toml = r#"
        default_backend = "openai"

        [backends.openai]
        kind = "openai"
        api_key = "sk-test"

        [backends.gemini]
        kind = "gemini"
        api_key = "AIzaSy"

        [backends.claude]
        kind = "anthropic"
        api_key = "sk-ant-test"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert_eq!(mc.backends.len(), 3);
    assert_eq!(mc.backends["openai"].kind, BackendKind::OpenAI);
    assert_eq!(mc.backends["gemini"].kind, BackendKind::Gemini);
    assert_eq!(mc.backends["claude"].kind, BackendKind::Anthropic);
}

#[test]
fn multi_config_defaults_first_backend_as_default() {
    let toml = r#"
        [backends.gemini]
        kind = "gemini"
        api_key = "AIzaSy"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert_eq!(mc.default_backend, "gemini");
}

#[test]
fn multi_config_defaults_listen_port() {
    let toml = r#"
        [backends.openai]
        kind = "openai"
        api_key = "sk-test"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert_eq!(mc.listen_port, 3000);
}

#[test]
fn multi_config_openai_defaults_base_url() {
    let toml = r#"
        [backends.openai]
        kind = "openai"
        api_key = "sk-test"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert_eq!(mc.backends["openai"].base_url, "https://api.openai.com");
}

#[test]
fn multi_config_anthropic_defaults_base_url() {
    let toml = r#"
        [backends.claude]
        kind = "anthropic"
        api_key = "sk-ant-test"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert_eq!(mc.backends["claude"].base_url, "https://api.anthropic.com");
}

#[test]
fn multi_config_custom_base_url() {
    let toml = r#"
        [backends.openai]
        kind = "openai"
        api_key = "sk-test"
        base_url = "https://custom.openai.example.com"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert_eq!(
        mc.backends["openai"].base_url,
        "https://custom.openai.example.com"
    );
}

#[test]
fn multi_config_api_format_responses() {
    let toml = r#"
        [backends.openai]
        kind = "openai"
        api_key = "sk-test"
        api_format = "responses"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert_eq!(mc.backends["openai"].api_format, OpenAIApiFormat::Responses);
}

#[test]
#[should_panic(expected = "must define at least one backend")]
fn multi_config_panics_no_backends() {
    let toml = r#"
        listen_port = 3000
    "#;
    MultiConfig::from_toml_str(toml);
}

#[test]
#[should_panic(expected = "not found in configured backends")]
fn multi_config_panics_invalid_default() {
    let toml = r#"
        default_backend = "nonexistent"

        [backends.openai]
        kind = "openai"
        api_key = "sk-test"
    "#;
    MultiConfig::from_toml_str(toml);
}

#[test]
#[should_panic(expected = "unknown backend kind")]
fn multi_config_panics_unknown_kind() {
    let toml = r#"
        [backends.foo]
        kind = "unknown_provider"
        api_key = "test"
    "#;
    MultiConfig::from_toml_str(toml);
}

#[test]
#[should_panic(expected = "api_key is required for gemini")]
fn multi_config_panics_gemini_no_key() {
    let toml = r#"
        [backends.gemini]
        kind = "gemini"
    "#;
    MultiConfig::from_toml_str(toml);
}

#[test]
#[should_panic(expected = "api_key is required for anthropic")]
fn multi_config_panics_anthropic_no_key() {
    let toml = r#"
        [backends.claude]
        kind = "anthropic"
    "#;
    MultiConfig::from_toml_str(toml);
}

#[test]
fn resolve_env_value_inline() {
    assert_eq!(resolve_env_value("my-key").unwrap(), "my-key");
}

#[test]
fn resolve_env_value_from_env() {
    std::env::set_var("TEST_RESOLVE_KEY_12345", "resolved-value");
    assert_eq!(
        resolve_env_value("env:TEST_RESOLVE_KEY_12345").unwrap(),
        "resolved-value"
    );
    std::env::remove_var("TEST_RESOLVE_KEY_12345");
}

#[test]
fn resolve_env_value_missing_env() {
    let err = resolve_env_value("env:NONEXISTENT_VAR_99999").unwrap_err();
    assert!(err.contains("not set"));
}

#[test]
fn multi_config_env_prefix_resolves() {
    std::env::set_var("TEST_OPENAI_KEY_TOML", "sk-from-env");
    let toml = r#"
        [backends.openai]
        kind = "openai"
        api_key = "env:TEST_OPENAI_KEY_TOML"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert_eq!(mc.backends["openai"].api_key, "sk-from-env");
    std::env::remove_var("TEST_OPENAI_KEY_TOML");
}

#[test]
fn multi_config_log_bodies() {
    let toml = r#"
        log_bodies = true

        [backends.openai]
        kind = "openai"
        api_key = "sk-test"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert!(mc.log_bodies);
    assert!(mc.backends["openai"].log_bodies);
}

#[test]
fn multi_config_redact_secrets() {
    let toml = r#"
        redact_secrets = true

        [backends.openai]
        kind = "openai"
        api_key = "sk-test"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    assert!(mc.redact_secrets);
}

#[test]
fn multi_config_gemini_defaults() {
    let toml = r#"
        [backends.gemini]
        kind = "gemini"
        api_key = "AIzaSy"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    let bc = &mc.backends["gemini"];
    assert_eq!(bc.model_mapping.big_model, "gemini-2.5-pro");
    assert_eq!(bc.model_mapping.small_model, "gemini-2.5-flash");
    // /openai is appended to route through Gemini's OpenAI-compatible endpoint
    assert_eq!(
        bc.base_url,
        "https://generativelanguage.googleapis.com/v1beta/openai"
    );
}

// --- Azure OpenAI tests ---

#[test]
fn multi_config_parses_azure_backend() {
    let toml = r#"
        [backends.azure]
        kind = "azure"
        api_key = "az-test-key"
        endpoint = "https://my-resource.openai.azure.com"
        deployment = "gpt-4o-deploy"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    let bc = &mc.backends["azure"];
    assert_eq!(bc.kind, BackendKind::AzureOpenAI);
    assert_eq!(
        bc.base_url,
        "https://my-resource.openai.azure.com/openai/deployments/gpt-4o-deploy/chat/completions?api-version=2024-10-21"
    );
    assert!(matches!(bc.backend_auth, BackendAuth::AzureApiKey(_)));
}

#[test]
fn multi_config_azure_custom_api_version() {
    let toml = r#"
        [backends.azure]
        kind = "azure"
        api_key = "az-test-key"
        endpoint = "https://my-resource.openai.azure.com"
        deployment = "gpt-4o-deploy"
        api_version = "2025-01-01"
    "#;
    let mc = MultiConfig::from_toml_str(toml);
    let bc = &mc.backends["azure"];
    assert!(bc.base_url.contains("api-version=2025-01-01"));
}

#[test]
#[should_panic(expected = "api_key is required for azure")]
fn multi_config_panics_azure_no_key() {
    let toml = r#"
        [backends.azure]
        kind = "azure"
        endpoint = "https://my-resource.openai.azure.com"
        deployment = "gpt-4o-deploy"
    "#;
    MultiConfig::from_toml_str(toml);
}

#[test]
#[should_panic(expected = "endpoint' is required for azure")]
fn multi_config_panics_azure_no_endpoint() {
    let toml = r#"
        [backends.azure]
        kind = "azure"
        api_key = "az-test-key"
        deployment = "gpt-4o-deploy"
    "#;
    MultiConfig::from_toml_str(toml);
}

#[test]
#[should_panic(expected = "deployment' is required for azure")]
fn multi_config_panics_azure_no_deployment() {
    let toml = r#"
        [backends.azure]
        kind = "azure"
        api_key = "az-test-key"
        endpoint = "https://my-resource.openai.azure.com"
    "#;
    MultiConfig::from_toml_str(toml);
}
