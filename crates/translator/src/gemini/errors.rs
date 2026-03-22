use serde::{Deserialize, Serialize};

/// Gemini API error response wrapper.
///
/// See <https://ai.google.dev/api/generate-content#v1beta.GenerateContentResponse>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

/// Error details with code, message, and gRPC status string.
///
/// Status is kept as String for forward compatibility with the full set of
/// gRPC status codes (INVALID_ARGUMENT, NOT_FOUND, PERMISSION_DENIED, etc.).
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ErrorDetail {
    pub code: u16,
    pub message: String,
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_error() {
        let raw = json!({
            "error": {
                "code": 400,
                "message": "Invalid value at 'contents'",
                "status": "INVALID_ARGUMENT"
            }
        });
        let err: ErrorResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(err.error.code, 400);
        assert_eq!(err.error.message, "Invalid value at 'contents'");
        assert_eq!(err.error.status, "INVALID_ARGUMENT");
    }

    #[test]
    fn roundtrip() {
        let err = ErrorResponse {
            error: ErrorDetail {
                code: 403,
                message: "Permission denied".into(),
                status: "PERMISSION_DENIED".into(),
            },
        };
        let json_str = serde_json::to_string(&err).unwrap();
        let roundtrip: ErrorResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(roundtrip.error.code, 403);
        assert_eq!(roundtrip.error.message, "Permission denied");
        assert_eq!(roundtrip.error.status, "PERMISSION_DENIED");
    }
}
