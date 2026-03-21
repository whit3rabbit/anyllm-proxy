// OpenAI Chat Completions request/response types
// PLAN.md lines 78-87, 700-725

use serde::{Deserialize, Serialize};

// --- Request types ---

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<Stop>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatTool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ChatToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// Forward-compatible extension fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct StreamOptions {
    #[serde(default)]
    pub include_usage: bool,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum Stop {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ChatContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

/// Content can be a plain string or an array of typed content parts (multimodal).
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ChatContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageUrl {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// --- Tool call from assistant ---

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String, // always "function"
    pub function: FunctionCall,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String, // JSON string
}

// --- Tool definition ---

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatTool {
    #[serde(rename = "type")]
    pub tool_type: String, // always "function"
    pub function: FunctionDef,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FunctionDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ChatToolChoice {
    Simple(String), // "auto", "none", "required"
    Named(NamedToolChoice),
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct NamedToolChoice {
    #[serde(rename = "type")]
    pub choice_type: String, // "function"
    pub function: NamedFunction,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct NamedFunction {
    pub name: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<serde_json::Value>,
}

// --- Response types ---

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String, // "chat.completion"
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    FunctionCall,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ChatUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_basic_request() {
        let raw = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let req: ChatCompletionRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, ChatRole::User);
        assert!(matches!(&req.messages[0].content, Some(ChatContent::Text(t)) if t == "Hello"));
        assert!(req.max_tokens.is_none());
        assert!(req.tools.is_none());
    }

    #[test]
    fn deserialize_request_with_tools() {
        let raw = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "What is the weather?"}],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather for a location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {"type": "string"}
                            },
                            "required": ["location"]
                        }
                    }
                }
            ],
            "tool_choice": "auto"
        });
        let req: ChatCompletionRequest = serde_json::from_value(raw).unwrap();
        let tools = req.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name, "get_weather");
        assert!(tools[0].function.description.is_some());
        assert!(matches!(&req.tool_choice, Some(ChatToolChoice::Simple(s)) if s == "auto"));
    }

    #[test]
    fn deserialize_content_string_vs_parts() {
        // String content
        let msg_str: ChatMessage = serde_json::from_value(json!({
            "role": "user",
            "content": "plain text"
        }))
        .unwrap();
        assert!(matches!(&msg_str.content, Some(ChatContent::Text(t)) if t == "plain text"));

        // Array content with text + image
        let msg_parts: ChatMessage = serde_json::from_value(json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "Describe this image"},
                {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}
            ]
        }))
        .unwrap();
        match &msg_parts.content {
            Some(ChatContent::Parts(parts)) => {
                assert_eq!(parts.len(), 2);
                assert!(
                    matches!(&parts[0], ChatContentPart::Text { text } if text == "Describe this image")
                );
                assert!(
                    matches!(&parts[1], ChatContentPart::ImageUrl { image_url } if image_url.url == "https://example.com/img.png")
                );
            }
            other => panic!("expected Parts, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_response_with_choices() {
        let raw = json!({
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello! How can I help?"
                    },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            },
            "created": 1700000000
        });
        let resp: ChatCompletionResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.id, "chatcmpl-abc123");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].finish_reason, Some(FinishReason::Stop));
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.total_tokens, 18);
    }

    #[test]
    fn deserialize_response_with_tool_calls() {
        let raw = json!({
            "id": "chatcmpl-xyz",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "tool_calls": [
                            {
                                "id": "call_abc",
                                "type": "function",
                                "function": {
                                    "name": "get_weather",
                                    "arguments": "{\"location\":\"NYC\"}"
                                }
                            }
                        ]
                    },
                    "finish_reason": "tool_calls"
                }
            ]
        });
        let resp: ChatCompletionResponse = serde_json::from_value(raw).unwrap();
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "call_abc");
        assert_eq!(tc[0].function.name, "get_weather");
        assert_eq!(tc[0].function.arguments, "{\"location\":\"NYC\"}");
        assert_eq!(resp.choices[0].finish_reason, Some(FinishReason::ToolCalls));
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: Some(ChatContent::Text("Hi".into())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            max_tokens: Some(100),
            temperature: Some(0.7),
            top_p: None,
            stop: None,
            tools: None,
            tool_choice: None,
            stream: Some(true),
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
            presence_penalty: None,
            frequency_penalty: None,
            response_format: None,
            extra: serde_json::Map::new(),
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let roundtrip: ChatCompletionRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(roundtrip.model, "gpt-4o");
        assert_eq!(roundtrip.max_tokens, Some(100));
        assert_eq!(roundtrip.stream, Some(true));
        assert!(roundtrip.stream_options.unwrap().include_usage);
    }

    #[test]
    fn stop_single_vs_array() {
        let single: Stop = serde_json::from_value(json!("END")).unwrap();
        assert!(matches!(single, Stop::Single(s) if s == "END"));

        let multi: Stop = serde_json::from_value(json!(["END", "STOP"])).unwrap();
        match multi {
            Stop::Multiple(v) => assert_eq!(v, vec!["END", "STOP"]),
            _ => panic!("expected Multiple"),
        }
    }

    #[test]
    fn tool_choice_simple_vs_named() {
        let simple: ChatToolChoice = serde_json::from_value(json!("auto")).unwrap();
        assert!(matches!(simple, ChatToolChoice::Simple(s) if s == "auto"));

        let named: ChatToolChoice = serde_json::from_value(json!({
            "type": "function",
            "function": {"name": "my_tool"}
        }))
        .unwrap();
        match named {
            ChatToolChoice::Named(n) => {
                assert_eq!(n.choice_type, "function");
                assert_eq!(n.function.name, "my_tool");
            }
            _ => panic!("expected Named"),
        }
    }

    #[test]
    fn extra_fields_captured_via_flatten() {
        let raw = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "logprobs": true,
            "seed": 42
        });
        let req: ChatCompletionRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.extra.get("logprobs"), Some(&json!(true)));
        assert_eq!(req.extra.get("seed"), Some(&json!(42)));
    }

    #[test]
    fn reject_malformed_missing_model() {
        let raw = json!({
            "messages": [{"role": "user", "content": "hi"}]
        });
        let result = serde_json::from_value::<ChatCompletionRequest>(raw);
        assert!(result.is_err());
    }
}
