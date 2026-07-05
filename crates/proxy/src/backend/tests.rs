use super::*;

#[test]
fn api_error_details_anthropic_returns_message() {
    let err = BackendError::Anthropic(AnthropicClientError::ApiError {
        status: 429,
        body: bytes::Bytes::from_static(b"rate limit exceeded"),
    });
    let details = err.api_error_details();
    assert!(details.is_some(), "Anthropic ApiError must return details");
    let (msg, status) = details.unwrap();
    assert_eq!(status, 429);
    assert!(
        msg.contains("rate limit"),
        "message should contain upstream body"
    );
}

#[test]
fn api_error_details_bedrock_returns_message() {
    let err = BackendError::Bedrock(BedrockClientError::ApiError {
        status: 403,
        body: bytes::Bytes::from_static(b"access denied"),
    });
    let details = err.api_error_details();
    assert!(details.is_some());
    let (msg, status) = details.unwrap();
    assert_eq!(status, 403);
    assert!(msg.contains("access denied"));
}

#[test]
fn api_error_details_gemini_returns_message() {
    let err = BackendError::Gemini(GeminiClientError::ApiError {
        status: 400,
        body: "bad request from gemini".to_string(),
    });
    let details = err.api_error_details();
    assert!(details.is_some());
    let (msg, status) = details.unwrap();
    assert_eq!(status, 400);
    assert!(msg.contains("gemini"));
}

#[test]
fn api_error_details_transport_returns_none() {
    let err = BackendError::Anthropic(AnthropicClientError::Transport("timeout".into()));
    assert!(err.api_error_details().is_none());
}

#[test]
fn single_backend_anthropic_uses_backend_auth_token() {
    let config = Config {
        backend: BackendKind::Anthropic,
        openai_api_key: String::new(),
        openai_base_url: "https://api.anthropic.com".to_string(),
        listen_port: 0,
        model_mapping: crate::config::ModelMapping {
            big_model: String::new(),
            small_model: String::new(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::AnthropicApiKey("sk-ant-configured".to_string()),
        log_bodies: false,
        redact_secrets: true,
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    };

    let BackendClient::Anthropic(client) = BackendClient::new(&config) else {
        panic!("expected Anthropic backend");
    };

    let (name, value) = client.auth_header();
    assert_eq!(name, "x-api-key");
    assert_eq!(value, "sk-ant-configured");
}

#[test]
fn api_error_details_openai_non_api_error_returns_none() {
    // Non-ApiError variants on any backend return None.
    // Use Bedrock::Signing since OpenAIClientError::Request requires a live reqwest::Error.
    let err = BackendError::Bedrock(BedrockClientError::Signing("bad key".into()));
    assert!(err.api_error_details().is_none());
}

#[test]
fn backend_error_kind_classifies_common_cases() {
    let rate_limited = BackendError::Gemini(GeminiClientError::ApiError {
        status: 429,
        body: "quota hit".to_string(),
    });
    assert_eq!(rate_limited.error_kind(), "rate_limit");

    let timeout = BackendError::Anthropic(AnthropicClientError::Transport(
        "request timeout".to_string(),
    ));
    assert_eq!(timeout.error_kind(), "timeout");

    let signing = BackendError::Bedrock(BedrockClientError::Signing("bad sig".into()));
    assert_eq!(signing.error_kind(), "signing");
}

#[test]
fn backend_error_kind_covers_auth_forbidden_and_gateway_timeout() {
    // The "big" status codes must classify consistently regardless of which
    // backend produced them: 401/403/404 are client errors, 429 rate_limit,
    // 504 (gateway timeout) a timeout, 5xx a backend_error.
    let cases: &[(u16, &str)] = &[
        (401, "client_error"),
        (403, "client_error"),
        (404, "client_error"),
        (429, "rate_limit"),
        (500, "backend_error"),
        (503, "backend_error"),
        (504, "timeout"),
    ];
    for (status, kind) in cases {
        // OpenAI variant
        let openai = BackendError::OpenAI(OpenAIClientError::ApiError {
            status: *status,
            error: anyllm_translate::openai::errors::ErrorResponse {
                error: anyllm_translate::openai::errors::ErrorDetail {
                    message: "err".to_string(),
                    error_type: "test".to_string(),
                    param: None,
                    code: None,
                },
            },
        });
        assert_eq!(openai.error_kind(), *kind, "openai status {status}");
        assert_eq!(openai.status_code(), *status, "openai status {status}");

        // Anthropic passthrough variant (raw bytes body)
        let anthropic = BackendError::Anthropic(AnthropicClientError::ApiError {
            status: *status,
            body: bytes::Bytes::from_static(b"err"),
        });
        assert_eq!(anthropic.error_kind(), *kind, "anthropic status {status}");
        assert_eq!(
            anthropic.status_code(),
            *status,
            "anthropic status {status}"
        );
    }
}
