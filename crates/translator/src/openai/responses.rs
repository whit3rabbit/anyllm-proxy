// OpenAI Responses API request/response types

use serde::{Deserialize, Serialize};

/// OpenAI Responses API request body.
///
/// See <https://platform.openai.com/docs/api-reference/responses/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: ResponsesInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Responses API input: text string or array of items.
///
/// See <https://platform.openai.com/docs/api-reference/responses/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ResponsesInput {
    Text(String),
    Items(Vec<serde_json::Value>),
}

/// OpenAI Responses API response body.
///
/// See <https://platform.openai.com/docs/api-reference/responses/object>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ResponsesResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub model: String,
    pub output: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ResponsesUsage>,
    pub status: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Token usage for Responses API.
///
/// See <https://platform.openai.com/docs/api-reference/responses/object>
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ResponsesUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
    /// OpenAI returns cached_tokens here; mapped to Anthropic cache_read_input_tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_token_details: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_text_input_request() {
        let raw = json!({
            "model": "gpt-4o",
            "input": "Tell me a joke"
        });
        let req: ResponsesRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.model, "gpt-4o");
        assert!(matches!(req.input, ResponsesInput::Text(ref t) if t == "Tell me a joke"));
    }

    #[test]
    fn deserialize_items_input_request() {
        let raw = json!({
            "model": "gpt-4o",
            "input": [
                {"type": "message", "role": "user", "content": "Hi"}
            ],
            "instructions": "Be helpful"
        });
        let req: ResponsesRequest = serde_json::from_value(raw).unwrap();
        assert!(matches!(req.input, ResponsesInput::Items(ref items) if items.len() == 1));
        assert_eq!(req.instructions.as_deref(), Some("Be helpful"));
    }

    #[test]
    fn deserialize_response() {
        let raw = json!({
            "id": "resp_abc",
            "type": "response",
            "model": "gpt-4o",
            "output": [
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hi!"}]}
            ],
            "usage": {
                "input_tokens": 5,
                "output_tokens": 3,
                "total_tokens": 8
            },
            "status": "completed"
        });
        let resp: ResponsesResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.id, "resp_abc");
        assert_eq!(resp.response_type, "response");
        assert_eq!(resp.status, "completed");
        let usage = resp.usage.unwrap();
        assert_eq!(usage.input_tokens, 5);
        assert_eq!(usage.total_tokens, 8);
    }

    #[test]
    fn extra_fields_preserved() {
        let raw = json!({
            "model": "gpt-4o",
            "input": "hi",
            "metadata": {"user_id": "u123"}
        });
        let req: ResponsesRequest = serde_json::from_value(raw).unwrap();
        assert!(req.extra.contains_key("metadata"));
    }

    #[test]
    fn roundtrip_request() {
        let req = ResponsesRequest {
            model: "gpt-4o".into(),
            input: ResponsesInput::Text("hello".into()),
            instructions: Some("Be concise".into()),
            max_output_tokens: Some(200),
            temperature: None,
            tools: None,
            stream: None,
            extra: serde_json::Map::new(),
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let roundtrip: ResponsesRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(roundtrip.model, "gpt-4o");
        assert_eq!(roundtrip.max_output_tokens, Some(200));
    }
}
