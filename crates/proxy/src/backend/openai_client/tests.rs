use super::*;
use std::time::Duration;

#[test]
fn is_retryable_429() {
    assert!(crate::backend::is_retryable(429));
}

#[test]
fn error_in_success_body_openrouter_numeric_code() {
    // OpenRouter returns errors inside a 200 with a numeric error.code.
    let body = br#"{"error":{"message":"Provider returned error","code":429}}"#;
    let err = error_in_success_body(body).expect("should detect error envelope");
    match err {
        OpenAIClientError::ApiError { status, error } => {
            assert_eq!(status, 429);
            assert_eq!(error.error.message, "Provider returned error");
        }
        other => panic!("expected ApiError, got {other:?}"),
    }
}

#[test]
fn error_in_success_body_openai_string_code_defaults_to_502() {
    // OpenAI-shaped envelope with a string code and no numeric status:
    // default to 502 (upstream failed inside a 2xx) and keep the code.
    let body = br#"{"error":{"message":"flagged","type":"moderation","code":"content_filter"}}"#;
    let err = error_in_success_body(body).expect("should detect error envelope");
    match err {
        OpenAIClientError::ApiError { status, error } => {
            assert_eq!(status, 502);
            assert_eq!(error.error.error_type, "moderation");
            assert_eq!(error.error.code.as_deref(), Some("content_filter"));
        }
        other => panic!("expected ApiError, got {other:?}"),
    }
}

#[test]
fn error_in_success_body_ignores_normal_completion_and_garbage() {
    // A real completion has no top-level error key.
    let ok = br#"{"id":"x","choices":[{"index":0,"message":{"role":"assistant","content":"hi"}}]}"#;
    assert!(error_in_success_body(ok).is_none());
    // Explicit null error is not an error.
    assert!(error_in_success_body(br#"{"error":null,"choices":[]}"#).is_none());
    // Non-JSON / truncated bodies are not error envelopes.
    assert!(error_in_success_body(b"not json").is_none());
}

#[test]
fn parse_chat_completion_response_normalizes_top_level_tool_call_shape() {
    let body = br#"{
        "id": "x",
        "object": "chat.completion",
        "created": 0,
        "model": "local",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "name": "lookup",
                    "arguments": {"q":"x"}
                }]
            },
            "finish_reason": "tool_calls"
        }]
    }"#;

    let parsed = parse_chat_completion_response_bytes(body).expect("normalized response parses");
    let tool_call = &parsed.choices[0].message.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tool_call.call_type, "function");
    assert_eq!(tool_call.function.name, "lookup");
    assert_eq!(tool_call.function.arguments, r#"{"q":"x"}"#);
}

#[test]
fn finished_choice_error_is_surfaced() {
    // A valid 200 body whose choice carries finish_reason "error" (no top-level
    // error envelope) must surface as an ApiError, not a truncated success.
    let body: openai::ChatCompletionResponse = serde_json::from_value(serde_json::json!({
        "id": "x", "object": "chat.completion", "created": 0, "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": ""},
            "finish_reason": "error"
        }]
    }))
    .expect("valid ChatCompletionResponse");
    match error_in_finished_choices(&body) {
        Some(OpenAIClientError::ApiError { status, .. }) => assert_eq!(status, 502),
        other => panic!("expected ApiError, got {other:?}"),
    }
}

#[test]
fn finished_choice_stop_is_ok() {
    // A normal completion (finish_reason "stop") is not an error.
    let body: openai::ChatCompletionResponse = serde_json::from_value(serde_json::json!({
        "id": "x", "object": "chat.completion", "created": 0, "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "hi"},
            "finish_reason": "stop"
        }]
    }))
    .expect("valid ChatCompletionResponse");
    assert!(error_in_finished_choices(&body).is_none());
}

#[test]
fn is_retryable_500() {
    assert!(crate::backend::is_retryable(500));
    assert!(crate::backend::is_retryable(502));
    assert!(crate::backend::is_retryable(503));
    assert!(crate::backend::is_retryable(599));
}

#[test]
fn is_retryable_408() {
    assert!(crate::backend::is_retryable(408));
}

#[test]
fn is_not_retryable_400() {
    assert!(!crate::backend::is_retryable(400));
    assert!(!crate::backend::is_retryable(401));
    assert!(!crate::backend::is_retryable(404));
    assert!(!crate::backend::is_retryable(409));
}

#[test]
fn backoff_respects_retry_after() {
    let delay = crate::backend::backoff_delay(0, Some(Duration::from_secs(5)));
    assert_eq!(delay, Duration::from_secs(5));
}

#[test]
fn backoff_increases_with_attempt() {
    let d0 = crate::backend::backoff_delay(0, None);
    let d1 = crate::backend::backoff_delay(1, None);
    let d2 = crate::backend::backoff_delay(2, None);
    assert!(d1 > d0);
    assert!(d2 > d1);
}

#[test]
fn parse_retry_after_valid() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("retry-after", "3".parse().unwrap());
    let dur = crate::backend::parse_retry_after(&headers);
    assert_eq!(dur, Some(Duration::from_secs(3)));
}

#[test]
fn parse_retry_after_missing() {
    let headers = reqwest::header::HeaderMap::new();
    assert_eq!(crate::backend::parse_retry_after(&headers), None);
}

#[test]
fn parse_retry_after_http_date_future() {
    // Use a date far in the future so it's always ahead of now
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "retry-after",
        "Wed, 21 Oct 2037 07:28:00 GMT".parse().unwrap(),
    );
    let dur = crate::backend::parse_retry_after(&headers);
    assert!(dur.is_some(), "future HTTP date should parse to Some");
    assert!(dur.unwrap().as_secs() > 0);
}

#[test]
fn parse_retry_after_http_date_past() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "retry-after",
        "Mon, 01 Jan 2024 00:00:00 GMT".parse().unwrap(),
    );
    // Past date: no wait needed
    assert_eq!(crate::backend::parse_retry_after(&headers), None);
}

#[test]
fn parse_retry_after_garbage() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("retry-after", "not-a-date-or-number".parse().unwrap());
    assert_eq!(crate::backend::parse_retry_after(&headers), None);
}

#[test]
fn client_builds_without_tls() {
    use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
    let config = Config {
        backend: BackendKind::OpenAI,
        openai_api_key: "test".into(),
        openai_base_url: "https://api.openai.com".into(),
        listen_port: 3000,
        model_mapping: ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("test".into()),
        log_bodies: false,
        redact_secrets: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    };
    // Should not panic
    let _client = OpenAIClient::new(&config);
}

#[test]
fn client_builds_vertex_config() {
    use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
    let config = Config {
        backend: BackendKind::Vertex,
        openai_api_key: String::new(),
        openai_base_url: "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/endpoints/openapi".into(),
        listen_port: 3000,
        model_mapping: ModelMapping {
            big_model: "gemini-2.5-pro".into(),
            small_model: "gemini-2.5-flash".into(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::GoogleApiKey("test-key".into()),
        log_bodies: false,
        redact_secrets: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    };
    let client = OpenAIClient::new(&config);
    // Verify URL construction for Vertex (no /v1 prefix)
    assert!(client
        .chat_completions_url
        .ends_with("/openapi/chat/completions"));
}

#[test]
fn embeddings_url_openai() {
    use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
    let config = Config {
        backend: BackendKind::OpenAI,
        openai_api_key: "test".into(),
        openai_base_url: "https://api.openai.com".into(),
        listen_port: 3000,
        model_mapping: ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("test".into()),
        log_bodies: false,
        redact_secrets: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    };
    let client = OpenAIClient::new(&config);
    assert_eq!(
        client.embeddings_url,
        "https://api.openai.com/v1/embeddings"
    );
}

#[test]
fn embeddings_url_vertex() {
    use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
    let config = Config {
        backend: BackendKind::Vertex,
        openai_api_key: String::new(),
        openai_base_url: "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/endpoints/openapi".into(),
        listen_port: 3000,
        model_mapping: ModelMapping {
            big_model: "gemini-2.5-pro".into(),
            small_model: "gemini-2.5-flash".into(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::GoogleApiKey("test-key".into()),
        log_bodies: false,
        redact_secrets: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    };
    let client = OpenAIClient::new(&config);
    assert!(
        client.embeddings_url.ends_with("/openapi/embeddings"),
        "got: {}",
        client.embeddings_url
    );
}

#[test]
fn embeddings_url_gemini() {
    use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
    let config = Config {
        backend: BackendKind::Gemini,
        openai_api_key: String::new(),
        // Config appends /openai to the base, so this is what arrives here
        openai_base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
        listen_port: 3000,
        model_mapping: ModelMapping {
            big_model: "gemini-2.5-pro".into(),
            small_model: "gemini-2.5-flash".into(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::GoogleApiKey("test-gemini-key".into()),
        log_bodies: false,
        redact_secrets: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    };
    let client = OpenAIClient::new(&config);
    assert_eq!(
        client.embeddings_url,
        "https://generativelanguage.googleapis.com/v1beta/openai/embeddings"
    );
}

#[test]
fn azure_url_passthrough() {
    use crate::config::{BackendKind, ModelMapping, OpenAIApiFormat, TlsConfig};
    let config = Config {
        backend: BackendKind::AzureOpenAI,
        openai_api_key: String::new(),
        openai_base_url: "https://myresource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-10-21".into(),
        listen_port: 3000,
        model_mapping: ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::AzureApiKey("test-azure-key".into()),
        log_bodies: false,
        redact_secrets: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    };
    let client = OpenAIClient::new(&config);
    // Chat completions URL is the pre-built URL, unchanged
    assert_eq!(
        client.chat_completions_url,
        "https://myresource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-10-21"
    );
    // Embeddings URL is derived from the endpoint and deployment
    assert_eq!(
        client.embeddings_url,
        "https://myresource.openai.azure.com/openai/deployments/gpt-4o/embeddings?api-version=2024-10-21"
    );
}
