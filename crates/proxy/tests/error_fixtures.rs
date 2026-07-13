use anyllm_translate::anthropic::ErrorType;
use anyllm_translate::mapping::errors_map::{
    anthropic_error_type_to_status, classify_chunk_error_code, create_anthropic_error,
    openai_status_to_anthropic_error_type, openai_to_anthropic_error, status_to_anthropic_error,
};
use anyllm_translate::openai;

#[test]
fn malformed_openai_response_fails_deserialization() {
    let json = include_str!("../../../fixtures/openai/chat_completion_malformed.json");
    let result = serde_json::from_str::<anyllm_translate::openai::ChatCompletionResponse>(json);
    assert!(
        result.is_err(),
        "malformed response should fail deserialization"
    );
}

// --- Fixture-based error translation tests ---

#[test]
fn fixture_openai_401_translates_to_anthropic_auth() {
    let json = include_str!("../../../fixtures/openai/error_401.json");
    let openai_err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    let anthropic_err = openai_to_anthropic_error(&openai_err, 401, Some("req_test".into()));
    assert_eq!(
        anthropic_err.error.error_type,
        ErrorType::AuthenticationError
    );
    assert!(anthropic_err.error.message.contains("Incorrect API key"));
    assert_eq!(anthropic_err.request_id.unwrap(), "req_test");
}

#[test]
fn fixture_openai_429_translates_to_anthropic_rate_limit() {
    let json = include_str!("../../../fixtures/openai/error_429.json");
    let openai_err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    let anthropic_err = openai_to_anthropic_error(&openai_err, 429, None);
    assert_eq!(anthropic_err.error.error_type, ErrorType::RateLimitError);
    assert!(anthropic_err.error.message.contains("Rate limit"));
    assert!(anthropic_err.request_id.is_none());
}

#[test]
fn fixture_openai_500_translates_to_anthropic_api_error() {
    let json = include_str!("../../../fixtures/openai/error_500.json");
    let openai_err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
    let anthropic_err = openai_to_anthropic_error(&openai_err, 500, None);
    assert_eq!(anthropic_err.error.error_type, ErrorType::ApiError);
}

#[test]
fn fixture_anthropic_invalid_request_deserializes() {
    let json = include_str!("../../../fixtures/anthropic/error_invalid_request.json");
    let err: anyllm_translate::anthropic::errors::ErrorResponse =
        serde_json::from_str(json).unwrap();
    assert_eq!(err.error.error_type, ErrorType::InvalidRequestError);
    assert_eq!(err.response_type, "error");
}

#[test]
fn fixture_anthropic_rate_limit_deserializes() {
    let json = include_str!("../../../fixtures/anthropic/error_rate_limit.json");
    let err: anyllm_translate::anthropic::errors::ErrorResponse =
        serde_json::from_str(json).unwrap();
    assert_eq!(err.error.error_type, ErrorType::RateLimitError);
    assert_eq!(err.request_id.unwrap(), "req_01XYZ");
}

// --- Status-to-error mapping tests ---

#[test]
fn status_mapping_coverage_all_anthropic_types() {
    let types = [
        ErrorType::InvalidRequestError,
        ErrorType::AuthenticationError,
        ErrorType::BillingError,
        ErrorType::PermissionError,
        ErrorType::NotFoundError,
        ErrorType::RequestTooLarge,
        ErrorType::RateLimitError,
        ErrorType::ApiError,
        ErrorType::TimeoutError,
        ErrorType::OverloadedError,
    ];
    for t in &types {
        let status = anthropic_error_type_to_status(t);
        let back = openai_status_to_anthropic_error_type(status);
        assert_eq!(&back, t, "round-trip failed for {t:?}");
    }
}

#[test]
fn status_to_anthropic_error_with_and_without_request_id() {
    let with_id = status_to_anthropic_error(429, "Too fast", Some("req_abc".into()));
    assert_eq!(with_id.error.error_type, ErrorType::RateLimitError);
    assert_eq!(with_id.request_id.unwrap(), "req_abc");

    let without_id = status_to_anthropic_error(500, "Server error", None);
    assert!(without_id.request_id.is_none());
}

#[test]
fn create_anthropic_error_preserves_all_fields() {
    let err = create_anthropic_error(
        ErrorType::NotFoundError,
        "Model not found".into(),
        Some("req_xyz".into()),
    );
    assert_eq!(err.response_type, "error");
    assert_eq!(err.error.error_type, ErrorType::NotFoundError);
    assert_eq!(err.error.message, "Model not found");
    assert_eq!(err.request_id.unwrap(), "req_xyz");
}

// --- Chunk error code classification tests ---

#[test]
fn classify_chunk_numeric_code_400_599_is_mapped() {
    assert_eq!(
        classify_chunk_error_code(Some(&serde_json::json!(401))),
        ErrorType::AuthenticationError
    );
}

#[test]
fn classify_chunk_numeric_code_out_of_range_is_api_error() {
    assert_eq!(
        classify_chunk_error_code(Some(&serde_json::json!(600))),
        ErrorType::ApiError
    );
}

#[test]
fn classify_chunk_string_anthropic_wire_string_is_recovered() {
    assert_eq!(
        classify_chunk_error_code(Some(&serde_json::json!("overloaded_error"))),
        ErrorType::OverloadedError
    );
}

#[test]
fn classify_chunk_string_unknown_is_api_error() {
    assert_eq!(
        classify_chunk_error_code(Some(&serde_json::json!("some_error"))),
        ErrorType::ApiError
    );
}

#[test]
fn classify_chunk_none_is_api_error() {
    assert_eq!(classify_chunk_error_code(None), ErrorType::ApiError);
}

// --- Status boundary tests ---

#[test]
fn status_408_and_504_both_map_to_timeout() {
    assert_eq!(
        openai_status_to_anthropic_error_type(408),
        ErrorType::TimeoutError
    );
    assert_eq!(
        openai_status_to_anthropic_error_type(504),
        ErrorType::TimeoutError
    );
}

#[test]
fn status_503_and_529_both_map_to_overloaded() {
    assert_eq!(
        openai_status_to_anthropic_error_type(503),
        ErrorType::OverloadedError
    );
    assert_eq!(
        openai_status_to_anthropic_error_type(529),
        ErrorType::OverloadedError
    );
}

// --- Provider-specific error format tests ---

#[test]
fn azure_auth_error_maps_to_anthropic_auth() {
    // Azure OpenAI returns 401 with a specific message format
    let err = openai::errors::ErrorResponse {
        error: openai::errors::ErrorDetail {
            message: "Access denied due to invalid subscription key or wrong API endpoint.".into(),
            error_type: "access_denied".into(),
            param: None,
            code: Some("401".into()),
        },
    };
    let result = openai_to_anthropic_error(&err, 401, None);
    assert_eq!(result.error.error_type, ErrorType::AuthenticationError);
}

#[test]
fn openai_context_window_error_maps_to_invalid_request() {
    // OpenAI returns 400 when context length is exceeded
    let err = openai::errors::ErrorResponse {
        error: openai::errors::ErrorDetail {
            message:
                "This model's maximum context length is 128000 tokens. You requested 130000 tokens."
                    .into(),
            error_type: "invalid_request_error".into(),
            param: None,
            code: Some("context_length_exceeded".into()),
        },
    };
    let result = openai_to_anthropic_error(&err, 400, None);
    assert_eq!(result.error.error_type, ErrorType::InvalidRequestError);
}
