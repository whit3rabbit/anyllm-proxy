// Anthropic error types and status codes
// PLAN.md lines 156-163

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ErrorResponse {
    #[serde(rename = "type")]
    pub response_type: String, // always "error"
    pub error: ErrorDetail,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ErrorDetail {
    #[serde(rename = "type")]
    pub error_type: ErrorType,
    pub message: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    InvalidRequestError,
    AuthenticationError,
    PermissionError,
    NotFoundError,
    RequestTooLarge,
    RateLimitError,
    ApiError,
    OverloadedError,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn deserialize_error_response() {
        let j = json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "max_tokens is required"
            }
        });
        let err: ErrorResponse = serde_json::from_value(j).unwrap();
        assert_eq!(err.response_type, "error");
        assert_eq!(err.error.error_type, ErrorType::InvalidRequestError);
        assert_eq!(err.error.message, "max_tokens is required");
        assert!(err.request_id.is_none());
    }

    #[test]
    fn deserialize_error_with_request_id() {
        let j = json!({
            "type": "error",
            "error": {
                "type": "authentication_error",
                "message": "invalid x-api-key"
            },
            "request_id": "req_01234"
        });
        let err: ErrorResponse = serde_json::from_value(j).unwrap();
        assert_eq!(err.request_id.as_deref(), Some("req_01234"));
        assert_eq!(err.error.error_type, ErrorType::AuthenticationError);
    }

    #[test]
    fn round_trip() {
        let err = ErrorResponse {
            response_type: "error".into(),
            error: ErrorDetail {
                error_type: ErrorType::RateLimitError,
                message: "Too many requests".into(),
            },
            request_id: Some("req_abc".into()),
        };
        let serialized = serde_json::to_string(&err).unwrap();
        let deserialized: ErrorResponse = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.error.error_type, ErrorType::RateLimitError);
        assert_eq!(deserialized.error.message, "Too many requests");
        assert_eq!(deserialized.request_id.as_deref(), Some("req_abc"));
    }

    #[test]
    fn all_error_type_variants() {
        let cases = [
            ("invalid_request_error", ErrorType::InvalidRequestError),
            ("authentication_error", ErrorType::AuthenticationError),
            ("permission_error", ErrorType::PermissionError),
            ("not_found_error", ErrorType::NotFoundError),
            ("request_too_large", ErrorType::RequestTooLarge),
            ("rate_limit_error", ErrorType::RateLimitError),
            ("api_error", ErrorType::ApiError),
            ("overloaded_error", ErrorType::OverloadedError),
        ];
        for (s, expected) in cases {
            let val = json!(s);
            let parsed: ErrorType = serde_json::from_value(val).unwrap();
            assert_eq!(parsed, expected, "failed for {}", s);

            // Round-trip: serialize back and compare
            let re_serialized = serde_json::to_value(&parsed).unwrap();
            assert_eq!(re_serialized.as_str().unwrap(), s);
        }
    }

    #[test]
    fn optional_request_id_omitted_when_none() {
        let err = ErrorResponse {
            response_type: "error".into(),
            error: ErrorDetail {
                error_type: ErrorType::ApiError,
                message: "internal".into(),
            },
            request_id: None,
        };
        let j = serde_json::to_value(&err).unwrap();
        assert!(!j.as_object().unwrap().contains_key("request_id"));
    }
}
