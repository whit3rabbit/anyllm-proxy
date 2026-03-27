// Integration tests for the fallback chain module.
// Tests the FallbackChain logic, should_fallback predicate, and config parsing.

use anyllm_proxy::fallback::config::{parse_fallback_config, BackendSpec};
use anyllm_proxy::fallback::{FallbackChain, FallbackOutcome, FALLBACK_EXHAUSTED_HEADER};

/// Helper: create a `BackendError::OpenAI(ApiError { .. })` with a given status code.
fn make_api_error(status: u16) -> anyllm_proxy::backend::BackendError {
    use anyllm_translate::openai::errors::{ErrorDetail, ErrorResponse};
    anyllm_proxy::backend::BackendError::OpenAI(
        anyllm_proxy::backend::openai_client::OpenAIClientError::ApiError {
            status,
            error: ErrorResponse {
                error: ErrorDetail {
                    message: format!("mock error {status}"),
                    error_type: "test".to_string(),
                    param: None,
                    code: None,
                },
            },
        },
    )
}

// -- Config parsing tests --

#[test]
fn config_roundtrip() {
    let yaml = r#"
fallback_chains:
  default:
    - name: azure
      env_prefix: AZURE_FB_
    - name: openai
      env_prefix: OPENAI_FB_
"#;
    let config = parse_fallback_config(yaml).unwrap();
    let chain = &config.fallback_chains["default"];
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].name, "azure");
    assert_eq!(chain[1].env_prefix, "OPENAI_FB_");
}

#[test]
fn config_empty_chains() {
    let yaml = "fallback_chains: {}\n";
    let config = parse_fallback_config(yaml).unwrap();
    assert!(config.fallback_chains.is_empty());
}

#[test]
fn config_malformed_yaml_errors() {
    let yaml = "not valid yaml: [[[";
    assert!(parse_fallback_config(yaml).is_err());
}

// -- should_fallback predicate tests --

#[test]
fn should_fallback_server_errors() {
    assert!(FallbackChain::should_fallback(500, false));
    assert!(FallbackChain::should_fallback(502, false));
    assert!(FallbackChain::should_fallback(503, false));
}

#[test]
fn should_fallback_rate_limit() {
    assert!(FallbackChain::should_fallback(429, false));
}

#[test]
fn should_fallback_connection_error() {
    assert!(FallbackChain::should_fallback(0, true));
}

#[test]
fn should_not_fallback_client_errors() {
    assert!(!FallbackChain::should_fallback(400, false));
    assert!(!FallbackChain::should_fallback(401, false));
    assert!(!FallbackChain::should_fallback(403, false));
    assert!(!FallbackChain::should_fallback(404, false));
}

// -- FallbackChain integration tests --

#[tokio::test]
async fn primary_503_falls_back_to_secondary() {
    let chain = FallbackChain::new(vec![
        BackendSpec {
            name: "primary".into(),
            env_prefix: "P_".into(),
        },
        BackendSpec {
            name: "secondary".into(),
            env_prefix: "S_".into(),
        },
    ]);

    let outcome = chain
        .attempt_with_fallback(|spec, _| {
            let name = spec.name.clone();
            async move {
                if name == "primary" {
                    Err(make_api_error(503))
                } else {
                    Ok("ok from secondary")
                }
            }
        })
        .await;

    assert!(outcome.result.is_ok());
    assert_eq!(outcome.backend_name, "secondary");
    assert_eq!(outcome.backend_index, 1);
    assert!(!outcome.exhausted);
}

#[tokio::test]
async fn primary_400_does_not_fallback() {
    let chain = FallbackChain::new(vec![
        BackendSpec {
            name: "primary".into(),
            env_prefix: "P_".into(),
        },
        BackendSpec {
            name: "secondary".into(),
            env_prefix: "S_".into(),
        },
    ]);

    let outcome: FallbackOutcome<&str> = chain
        .attempt_with_fallback(|spec, _| {
            let name = spec.name.clone();
            async move {
                if name == "primary" {
                    Err(make_api_error(400))
                } else {
                    Ok("should not reach")
                }
            }
        })
        .await;

    assert!(outcome.result.is_err());
    assert_eq!(outcome.backend_name, "primary");
    assert_eq!(outcome.backend_index, 0);
    // Not exhausted because we stopped early on a non-retryable error.
    assert!(!outcome.exhausted);
}

#[tokio::test]
async fn all_backends_fail_sets_exhausted() {
    let chain = FallbackChain::new(vec![
        BackendSpec {
            name: "a".into(),
            env_prefix: "A_".into(),
        },
        BackendSpec {
            name: "b".into(),
            env_prefix: "B_".into(),
        },
        BackendSpec {
            name: "c".into(),
            env_prefix: "C_".into(),
        },
    ]);

    let outcome: FallbackOutcome<&str> = chain
        .attempt_with_fallback(|_spec, _| async move { Err(make_api_error(502)) })
        .await;

    assert!(outcome.result.is_err());
    assert!(outcome.exhausted);
    // Last backend tried.
    assert_eq!(outcome.backend_index, 2);
    assert_eq!(outcome.backend_name, "c");
}

#[test]
fn fallback_exhausted_header_value() {
    // Verify the header constant is what callers expect.
    assert_eq!(FALLBACK_EXHAUSTED_HEADER, "x-anyllm-fallback-exhausted");
}
