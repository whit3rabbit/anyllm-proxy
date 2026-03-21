// OpenAI error types and rate limit headers
// PLAN.md lines 165-171

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ErrorDetail {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_error_response() {
        let raw = json!({
            "error": {
                "message": "Invalid API key",
                "type": "invalid_request_error",
                "param": null,
                "code": "invalid_api_key"
            }
        });
        let err: ErrorResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(err.error.message, "Invalid API key");
        assert_eq!(err.error.error_type, "invalid_request_error");
        assert!(err.error.param.is_none());
        assert_eq!(err.error.code.as_deref(), Some("invalid_api_key"));
    }

    #[test]
    fn deserialize_error_minimal() {
        let raw = json!({
            "error": {
                "message": "Something went wrong",
                "type": "server_error"
            }
        });
        let err: ErrorResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(err.error.error_type, "server_error");
        assert!(err.error.param.is_none());
        assert!(err.error.code.is_none());
    }

    #[test]
    fn serialize_error_skips_none_fields() {
        let err = ErrorResponse {
            error: ErrorDetail {
                message: "bad request".into(),
                error_type: "invalid_request_error".into(),
                param: None,
                code: None,
            },
        };
        let val = serde_json::to_value(&err).unwrap();
        let detail = val.get("error").unwrap();
        assert!(!detail.as_object().unwrap().contains_key("param"));
        assert!(!detail.as_object().unwrap().contains_key("code"));
    }

    #[test]
    fn roundtrip() {
        let err = ErrorResponse {
            error: ErrorDetail {
                message: "Rate limit exceeded".into(),
                error_type: "rate_limit_error".into(),
                param: Some("messages".into()),
                code: Some("rate_limit_exceeded".into()),
            },
        };
        let json_str = serde_json::to_string(&err).unwrap();
        let roundtrip: ErrorResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(roundtrip.error.message, "Rate limit exceeded");
        assert_eq!(roundtrip.error.param.as_deref(), Some("messages"));
    }
}
