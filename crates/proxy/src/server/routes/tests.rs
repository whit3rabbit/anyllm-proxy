use super::handlers::anthropic_catalog_model_rows;
use super::helpers::cache_auth_identity;
use anyllm_providers::ProviderCatalog;
use std::sync::Arc;

#[test]
fn catalog_model_list_excludes_deprecated_sonnet() {
    let catalog = ProviderCatalog::bundled();
    let rows = anthropic_catalog_model_rows(&catalog);
    let ids: Vec<&str> = rows.iter().filter_map(|m| m["id"].as_str()).collect();
    assert!(
        !ids.contains(&"claude-3-sonnet-20240229"),
        "claude-3-sonnet-20240229 is deprecated and must not appear in the model list"
    );
}

#[test]
fn catalog_model_list_reads_provided_catalog() {
    let catalog = ProviderCatalog::from_litellm_json(
        r#"{
            "anthropic/claude-fresh-20260607": {
                "litellm_provider": "anthropic",
                "mode": "chat",
                "max_input_tokens": 200000,
                "max_output_tokens": 8192
            }
        }"#,
    )
    .expect("runtime catalog");

    let rows = anthropic_catalog_model_rows(&catalog);
    let ids: Vec<&str> = rows.iter().filter_map(|m| m["id"].as_str()).collect();
    assert_eq!(ids, vec!["claude-fresh-20260607"]);
}

#[test]
fn cache_auth_identity_uses_x_api_key() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-api-key", "key-a".parse().unwrap());
    let first = cache_auth_identity(&headers, &None);

    headers.insert("x-api-key", "key-b".parse().unwrap());
    let second = cache_auth_identity(&headers, &None);

    assert_ne!(first, second);
    assert!(first.starts_with("credential:"));
    assert!(!first.contains("key-a"));
}

#[test]
fn cache_auth_identity_prefers_virtual_key_context() {
    let ctx = super::super::middleware::VirtualKeyContext {
        key_id: 42,
        rate_state: Arc::new(crate::admin::keys::RateLimitState::new()),
        allowed_models: None,
        allowed_routes: None,
        period_reset: None,
    };
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-api-key", "key-a".parse().unwrap());

    assert_eq!(cache_auth_identity(&headers, &Some(ctx)), "virtual-key:42");
}
