// Error and stop_reason mapping
// PLAN.md lines 916-920

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
        408 => anthropic::ErrorType::OverloadedError,
        413 => anthropic::ErrorType::RequestTooLarge,
        429 => anthropic::ErrorType::RateLimitError,
        500..=502 => anthropic::ErrorType::ApiError,
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
            (408, anthropic::ErrorType::OverloadedError),
            (413, anthropic::ErrorType::RequestTooLarge),
            (429, anthropic::ErrorType::RateLimitError),
            (500, anthropic::ErrorType::ApiError),
            (501, anthropic::ErrorType::ApiError),
            (502, anthropic::ErrorType::ApiError),
            (503, anthropic::ErrorType::OverloadedError),
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
        for status in [0, 204, 418, 504] {
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
