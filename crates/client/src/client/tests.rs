use super::*;

#[test]
fn client_config_builder_defaults() {
    let config = ClientConfig::builder()
        .backend_url("https://api.openai.com/v1/chat/completions")
        .auth(Auth::Bearer("sk-test".into()))
        .build();

    assert_eq!(
        config.chat_completions_url,
        "https://api.openai.com/v1/chat/completions"
    );
    assert!(matches!(config.auth, Auth::Bearer(ref s) if s == "sk-test"));
}

#[test]
fn client_config_builder_with_translation() {
    let translation = TranslationConfig::builder()
        .model_map("haiku", "gpt-4o-mini")
        .model_map("sonnet", "gpt-4o")
        .build();

    let config = ClientConfig::builder()
        .backend_url("https://api.openai.com/v1/chat/completions")
        .auth(Auth::Bearer("sk-test".into()))
        .translation(translation)
        .build();

    assert!(config.translation.map_model("claude-3-haiku").is_ok());
}

#[test]
fn client_creates_without_panic() {
    let config = ClientConfig::builder()
        .backend_url("https://api.openai.com/v1/chat/completions")
        .auth(Auth::Bearer("sk-test".into()))
        .http(HttpClientConfig {
            ssrf_protection: false,
            ..Default::default()
        })
        .build();

    let _client = Client::new(config);
}

#[test]
fn client_builder_success() {
    let client = ClientBuilder::new()
        .base_url("https://api.openai.com/v1/chat/completions")
        .api_key("sk-test")
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(120))
        .read_timeout(std::time::Duration::from_secs(30))
        .max_retries(2)
        .build();
    assert!(client.is_ok());
}

#[test]
fn client_builder_empty_api_key_rejected() {
    let result = ClientBuilder::new()
        .base_url("https://api.openai.com/v1/chat/completions")
        .api_key("")
        .build();
    assert!(result.is_err());
}

#[test]
fn client_builder_max_retries_stored() {
    let client = ClientBuilder::new()
        .base_url("https://api.openai.com/v1/chat/completions")
        .max_retries(7)
        .build()
        .unwrap();
    assert_eq!(client.retry_policy().max_retries, 7);
}

#[test]
fn client_builder_missing_url() {
    let result = ClientBuilder::new().api_key("sk-test").build();
    assert!(result.is_err());
}

#[test]
fn client_builder_default_api_key() {
    // No api_key set: should still build (empty bearer token).
    let client = ClientBuilder::new().base_url("https://example.com").build();
    assert!(client.is_ok());
}

#[test]
fn client_builder_via_client() {
    let client = Client::builder().base_url("https://example.com").build();
    assert!(client.is_ok());
}

#[test]
fn client_builder_default_trait() {
    let builder = ClientBuilder::default();
    assert!(builder.base_url.is_none());
}

#[test]
fn with_http_client_max_retries_override() {
    let config = ClientConfig::builder()
        .backend_url("https://example.com")
        .auth(Auth::Bearer("sk-test".into()))
        .http(HttpClientConfig {
            ssrf_protection: false,
            ..Default::default()
        })
        .build();
    let http = build_http_client(&config.http);
    let client = Client::with_http_client(http, config).with_max_retries(7);
    assert_eq!(client.retry_policy().max_retries, 7);
}

#[test]
fn with_transport_retries_chaining() {
    let client = Client::builder()
        .base_url("https://example.com")
        .build()
        .unwrap()
        .with_transport_retries(true);
    assert!(client.retry_policy().retry_transport_errors);
}

#[test]
fn client_builder_transport_retries_flag() {
    let client = ClientBuilder::new()
        .base_url("https://example.com")
        .retry_transport_errors(true)
        .build()
        .unwrap();
    assert!(client.retry_policy().retry_transport_errors);
}

#[test]
fn client_builder_extra_header() {
    // Just verify it builds; header arrival on wire is tested in integration tests.
    let client = ClientBuilder::new()
        .base_url("https://example.com")
        .extra_header("HTTP-Referer", "https://myapp.com")
        .extra_header("X-Title", "My App")
        .build()
        .unwrap();
    assert_eq!(client.config.http.extra_headers.len(), 2);
}
