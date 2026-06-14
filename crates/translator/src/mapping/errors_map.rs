//! Error type and HTTP status mapping between Anthropic and OpenAI error shapes.
//!
//! All functions are stateless — they convert between error representations without
//! any network calls or side effects.

use crate::anthropic;
use crate::openai;

/// Map an HTTP status code from OpenAI to the corresponding Anthropic error type.
///
/// Anthropic: <https://docs.anthropic.com/en/api/errors>
/// OpenAI: <https://platform.openai.com/docs/guides/error-codes>
pub fn openai_status_to_anthropic_error_type(status: u16) -> anthropic::ErrorType {
    match status {
        400 => anthropic::ErrorType::InvalidRequestError,
        401 => anthropic::ErrorType::AuthenticationError,
        402 => anthropic::ErrorType::BillingError,
        403 => anthropic::ErrorType::PermissionError,
        404 => anthropic::ErrorType::NotFoundError,
        // Anthropic documents 504 as timeout_error. 408 (request timeout) has
        // no Anthropic code but is the same class, so it maps here too. Both are
        // transient and signal the client to retry with backoff.
        // <https://platform.claude.com/docs/en/api/errors>
        408 | 504 => anthropic::ErrorType::TimeoutError,
        413 => anthropic::ErrorType::RequestTooLarge,
        429 => anthropic::ErrorType::RateLimitError,
        500..=502 => anthropic::ErrorType::ApiError,
        // 529 (Anthropic overloaded) and 503 (generic/Cloudflare transient
        // capacity) both map to OverloadedError to trigger client-side backoff.
        529 | 503 => anthropic::ErrorType::OverloadedError,
        _ => anthropic::ErrorType::ApiError,
    }
}

/// Map an Anthropic error type to an HTTP status code.
///
/// Anthropic: <https://docs.anthropic.com/en/api/errors>
pub fn anthropic_error_type_to_status(error_type: &anthropic::ErrorType) -> u16 {
    match error_type {
        anthropic::ErrorType::InvalidRequestError => 400,
        anthropic::ErrorType::AuthenticationError => 401,
        anthropic::ErrorType::BillingError => 402,
        anthropic::ErrorType::PermissionError => 403,
        anthropic::ErrorType::NotFoundError => 404,
        anthropic::ErrorType::RequestTooLarge => 413,
        anthropic::ErrorType::RateLimitError => 429,
        anthropic::ErrorType::ApiError => 500,
        anthropic::ErrorType::TimeoutError => 504,
        anthropic::ErrorType::OverloadedError => 529,
    }
}

/// Convert an HTTP status code and error message to an Anthropic error response.
/// Works for any backend (OpenAI, Gemini, etc.) since it only needs standard HTTP semantics.
pub fn status_to_anthropic_error(
    status: u16,
    message: &str,
    request_id: Option<String>,
) -> anthropic::errors::ErrorResponse {
    anthropic::errors::ErrorResponse {
        response_type: "error".to_string(),
        error: anthropic::errors::ErrorDetail {
            error_type: openai_status_to_anthropic_error_type(status),
            message: message.to_string(),
        },
        request_id,
    }
}

/// Classify a streaming chunk's `error.code` into an Anthropic error type.
///
/// `code` can arrive in three shapes:
/// - a numeric HTTP-like status (OpenRouter's pre-stream encoding) -> mapped via
///   [`openai_status_to_anthropic_error_type`];
/// - a string that is itself an Anthropic error-type wire string (this happens when
///   our own reverse translator round-trips an Anthropic error through the OpenAI
///   chunk shape, see `reverse_streaming_map`) -> recovered to the original type so
///   the classification is not lost on a re-translation;
/// - any other string (e.g. OpenRouter's `"server_error"`) -> `api_error`.
pub fn classify_chunk_error_code(code: Option<&serde_json::Value>) -> anthropic::ErrorType {
    let Some(code) = code else {
        return anthropic::ErrorType::ApiError;
    };
    if let Some(n) = code.as_u64() {
        if (400..=599).contains(&n) {
            return openai_status_to_anthropic_error_type(n as u16);
        }
    }
    if let Some(s) = code.as_str() {
        // Recover an Anthropic error type encoded as its wire string.
        if let Ok(et) =
            serde_json::from_value::<anthropic::ErrorType>(serde_json::Value::String(s.to_string()))
        {
            return et;
        }
    }
    anthropic::ErrorType::ApiError
}

/// Convert an OpenRouter-style mid-stream chunk error into an Anthropic stream error.
///
/// Mid-stream failures arrive as a chunk carrying a top-level `error` object once a
/// 200 SSE response has already started. The `code` is classified by
/// [`classify_chunk_error_code`].
///
/// OpenRouter: <https://openrouter.ai/docs/api/reference/errors-and-debugging>
/// Anthropic streaming errors: <https://docs.anthropic.com/en/api/messages-streaming>
pub fn openai_stream_error_to_anthropic(
    err: &openai::streaming::ChunkError,
) -> anthropic::streaming::StreamError {
    let error_type = classify_chunk_error_code(err.code.as_ref());
    let message = err
        .message
        .clone()
        .unwrap_or_else(|| "upstream returned an error mid-stream".to_string());
    anthropic::streaming::StreamError {
        error_type: error_type.as_wire_str().to_string(),
        message,
    }
}

/// Convert an OpenAI error response to an Anthropic error response.
pub fn openai_to_anthropic_error(
    openai_err: &openai::errors::ErrorResponse,
    status: u16,
    request_id: Option<String>,
) -> anthropic::errors::ErrorResponse {
    status_to_anthropic_error(status, &openai_err.error.message, request_id)
}

/// Create an Anthropic error response from scratch.
///
/// Anthropic: <https://docs.anthropic.com/en/api/errors>
pub fn create_anthropic_error(
    error_type: anthropic::ErrorType,
    message: String,
    request_id: Option<String>,
) -> anthropic::errors::ErrorResponse {
    anthropic::errors::ErrorResponse {
        response_type: "error".to_string(),
        error: anthropic::errors::ErrorDetail {
            error_type,
            message,
        },
        request_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn status_to_error_type_known_codes() {
        let cases: &[(u16, anthropic::ErrorType)] = &[
            (400, anthropic::ErrorType::InvalidRequestError),
            (401, anthropic::ErrorType::AuthenticationError),
            (402, anthropic::ErrorType::BillingError),
            (403, anthropic::ErrorType::PermissionError),
            (404, anthropic::ErrorType::NotFoundError),
            (408, anthropic::ErrorType::TimeoutError),
            (413, anthropic::ErrorType::RequestTooLarge),
            (429, anthropic::ErrorType::RateLimitError),
            (500, anthropic::ErrorType::ApiError),
            (501, anthropic::ErrorType::ApiError),
            (502, anthropic::ErrorType::ApiError),
            (503, anthropic::ErrorType::OverloadedError),
            (504, anthropic::ErrorType::TimeoutError),
            (529, anthropic::ErrorType::OverloadedError),
        ];
        for (status, expected) in cases {
            assert_eq!(
                openai_status_to_anthropic_error_type(*status),
                *expected,
                "status {}",
                status
            );
        }
    }

    #[test]
    fn unknown_status_maps_to_api_error() {
        for status in [0, 204, 418, 405] {
            assert_eq!(
                openai_status_to_anthropic_error_type(status),
                anthropic::ErrorType::ApiError,
                "status {} should map to ApiError",
                status
            );
        }
    }

    #[test]
    fn error_type_to_status_all_variants() {
        let cases: &[(anthropic::ErrorType, u16)] = &[
            (anthropic::ErrorType::InvalidRequestError, 400),
            (anthropic::ErrorType::AuthenticationError, 401),
            (anthropic::ErrorType::BillingError, 402),
            (anthropic::ErrorType::PermissionError, 403),
            (anthropic::ErrorType::NotFoundError, 404),
            (anthropic::ErrorType::RequestTooLarge, 413),
            (anthropic::ErrorType::RateLimitError, 429),
            (anthropic::ErrorType::ApiError, 500),
            (anthropic::ErrorType::TimeoutError, 504),
            (anthropic::ErrorType::OverloadedError, 529),
        ];
        for (error_type, expected_status) in cases {
            assert_eq!(anthropic_error_type_to_status(error_type), *expected_status,);
        }
    }

    #[test]
    fn round_trip_error_type_through_status() {
        // Every error type should survive a round-trip through status code
        // (except OverloadedError: 529 is not in the 500..=502 range, but
        // 529 maps back to OverloadedError via the explicit match arm).
        let all_types = [
            anthropic::ErrorType::InvalidRequestError,
            anthropic::ErrorType::AuthenticationError,
            anthropic::ErrorType::BillingError,
            anthropic::ErrorType::PermissionError,
            anthropic::ErrorType::NotFoundError,
            anthropic::ErrorType::RequestTooLarge,
            anthropic::ErrorType::RateLimitError,
            anthropic::ErrorType::ApiError,
            anthropic::ErrorType::TimeoutError,
            anthropic::ErrorType::OverloadedError,
        ];
        for error_type in &all_types {
            let status = anthropic_error_type_to_status(error_type);
            let back = openai_status_to_anthropic_error_type(status);
            assert_eq!(&back, error_type, "round-trip failed for {:?}", error_type);
        }
    }

    #[test]
    fn openai_error_to_anthropic_error() {
        let openai_err = openai::errors::ErrorResponse {
            error: openai::errors::ErrorDetail {
                message: "Invalid API key".into(),
                error_type: "invalid_request_error".into(),
                param: None,
                code: Some("invalid_api_key".into()),
            },
        };

        let result = openai_to_anthropic_error(&openai_err, 401, Some("req_123".into()));

        assert_eq!(result.response_type, "error");
        assert_eq!(
            result.error.error_type,
            anthropic::ErrorType::AuthenticationError
        );
        assert_eq!(result.error.message, "Invalid API key");
        assert_eq!(result.request_id.as_deref(), Some("req_123"));
    }

    #[test]
    fn openai_error_to_anthropic_no_request_id() {
        let openai_err = openai::errors::ErrorResponse {
            error: openai::errors::ErrorDetail {
                message: "Rate limit exceeded".into(),
                error_type: "rate_limit_error".into(),
                param: None,
                code: None,
            },
        };

        let result = openai_to_anthropic_error(&openai_err, 429, None);

        assert_eq!(
            result.error.error_type,
            anthropic::ErrorType::RateLimitError
        );
        assert!(result.request_id.is_none());
    }

    #[test]
    fn create_anthropic_error_helper() {
        let err = create_anthropic_error(
            anthropic::ErrorType::NotFoundError,
            "Model not found".into(),
            Some("req_abc".into()),
        );

        assert_eq!(err.response_type, "error");
        assert_eq!(err.error.error_type, anthropic::ErrorType::NotFoundError);
        assert_eq!(err.error.message, "Model not found");
        assert_eq!(err.request_id.as_deref(), Some("req_abc"));
    }

    #[test]
    fn create_anthropic_error_no_request_id() {
        let err = create_anthropic_error(
            anthropic::ErrorType::ApiError,
            "Internal error".into(),
            None,
        );

        assert_eq!(err.response_type, "error");
        assert_eq!(err.error.error_type, anthropic::ErrorType::ApiError);
        assert!(err.request_id.is_none());
    }

    // --- Fixture deserialization tests ---

    #[test]
    fn fixture_openai_error_401_deserializes() {
        let json = include_str!("../../../../fixtures/openai/error_401.json");
        let err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.code.as_deref(), Some("invalid_api_key"));
    }

    #[test]
    fn fixture_openai_error_429_deserializes() {
        let json = include_str!("../../../../fixtures/openai/error_429.json");
        let err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
        assert!(err.error.message.contains("Rate limit"));
    }

    #[test]
    fn fixture_openai_error_500_deserializes() {
        let json = include_str!("../../../../fixtures/openai/error_500.json");
        let err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.error_type, "server_error");
    }

    #[test]
    fn fixture_anthropic_error_invalid_request_deserializes() {
        let json = include_str!("../../../../fixtures/anthropic/error_invalid_request.json");
        let err: anthropic::errors::ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            err.error.error_type,
            anthropic::ErrorType::InvalidRequestError
        );
    }

    #[test]
    fn fixture_anthropic_error_rate_limit_deserializes() {
        let json = include_str!("../../../../fixtures/anthropic/error_rate_limit.json");
        let err: anthropic::errors::ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.error_type, anthropic::ErrorType::RateLimitError);
        assert_eq!(err.request_id.as_deref(), Some("req_01XYZ"));
    }

    // --- Fixture translation tests ---

    #[test]
    fn fixture_openai_401_translates_to_anthropic_auth_error() {
        let json = include_str!("../../../../fixtures/openai/error_401.json");
        let openai_err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
        let anthropic_err = openai_to_anthropic_error(&openai_err, 401, Some("req_test".into()));
        assert_eq!(
            anthropic_err.error.error_type,
            anthropic::ErrorType::AuthenticationError
        );
    }

    #[test]
    fn fixture_openai_429_translates_to_anthropic_rate_limit() {
        let json = include_str!("../../../../fixtures/openai/error_429.json");
        let openai_err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
        let anthropic_err = openai_to_anthropic_error(&openai_err, 429, None);
        assert_eq!(
            anthropic_err.error.error_type,
            anthropic::ErrorType::RateLimitError
        );
    }

    #[test]
    fn stream_error_numeric_code_maps_to_typed_error() {
        let err = openai::streaming::ChunkError {
            code: Some(serde_json::Value::Number(429.into())),
            message: Some("slow down".into()),
            metadata: None,
        };
        let result = openai_stream_error_to_anthropic(&err);
        assert_eq!(result.error_type, "rate_limit_error");
        assert_eq!(result.message, "slow down");
    }

    #[test]
    fn stream_error_recovers_anthropic_wire_string_code() {
        // The reverse translator encodes an Anthropic error_type as a string `code`;
        // re-translating it forward must recover the original typed classification
        // rather than degrading to api_error.
        let err = openai::streaming::ChunkError {
            code: Some(serde_json::Value::String("overloaded_error".into())),
            message: Some("Overloaded".into()),
            metadata: None,
        };
        let result = openai_stream_error_to_anthropic(&err);
        assert_eq!(result.error_type, "overloaded_error");
        assert_eq!(result.message, "Overloaded");
    }

    #[test]
    fn stream_error_string_code_falls_back_to_api_error() {
        let err = openai::streaming::ChunkError {
            code: Some(serde_json::Value::String("server_error".into())),
            message: Some("Provider disconnected".into()),
            metadata: None,
        };
        let result = openai_stream_error_to_anthropic(&err);
        assert_eq!(result.error_type, "api_error");
        assert_eq!(result.message, "Provider disconnected");
    }

    #[test]
    fn stream_error_missing_message_uses_fallback() {
        let err = openai::streaming::ChunkError {
            code: None,
            message: None,
            metadata: None,
        };
        let result = openai_stream_error_to_anthropic(&err);
        assert_eq!(result.error_type, "api_error");
        assert!(!result.message.is_empty());
    }

    #[test]
    fn fixture_openai_500_translates_to_anthropic_api_error() {
        let json = include_str!("../../../../fixtures/openai/error_500.json");
        let openai_err: openai::errors::ErrorResponse = serde_json::from_str(json).unwrap();
        let anthropic_err = openai_to_anthropic_error(&openai_err, 500, None);
        assert_eq!(
            anthropic_err.error.error_type,
            anthropic::ErrorType::ApiError
        );
    }
}
