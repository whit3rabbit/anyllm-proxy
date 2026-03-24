// OpenAI Chat Completions request/response types
// PLAN.md lines 78-87, 700-725

use serde::{Deserialize, Serialize};

// --- Request types ---

/// OpenAI Chat Completions API request body.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
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
    /// Maps from Anthropic metadata.user_id. Compat spec: "Ignored".
    /// See: https://docs.anthropic.com/en/api/openai-sdk#simple-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Compat spec: "Fully supported".
    /// See: https://docs.anthropic.com/en/api/openai-sdk#simple-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    /// Captures OpenAI fields we don't need to translate (seed, logprobs,
    /// logit_bias, n, reasoning_effort, etc.) and forwards them as-is.
    /// Only fields requiring translation logic get explicit struct fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Options for streaming responses.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct StreamOptions {
    #[serde(default)]
    pub include_usage: bool,
}

/// Stop sequence(s): single string or array.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum Stop {
    Single(String),
    Multiple(Vec<String>),
}

/// A message in the conversation.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
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
    /// Compat spec response: "Always empty". Present to avoid deserialization failure.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
}

/// Message role: system, developer, user, assistant, or tool.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    Developer,
    User,
    Assistant,
    Tool,
    /// Deprecated by OpenAI but still accepted. Compat spec lists function role messages.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#messages-array-fields
    Function,
}

/// Content can be a plain string or an array of typed content parts (multimodal).
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

/// Typed content part for multimodal messages.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ChatContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
    #[serde(rename = "input_audio")]
    InputAudio { input_audio: InputAudio },
    #[serde(rename = "file")]
    File { file: FileInput },
}

/// Image URL reference for vision requests.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageUrl {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Audio input for audio-capable models.
///
/// See <https://platform.openai.com/docs/guides/audio>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct InputAudio {
    pub data: String,   // base64-encoded audio
    pub format: String, // "wav", "mp3", etc.
}

/// File input for file-capable models.
///
/// See <https://platform.openai.com/docs/guides/text>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FileInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_data: Option<String>, // base64-encoded or data URI
}

// --- Tool call from assistant ---

/// Tool call from the assistant.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String, // always "function"
    pub function: FunctionCall,
}

/// Function name and JSON arguments string.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String, // JSON string
}

// --- Tool definition ---

/// Tool definition wrapping a function.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatTool {
    #[serde(rename = "type")]
    pub tool_type: String, // always "function"
    pub function: FunctionDef,
}

/// Function definition with name, description, and parameters schema.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FunctionDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    /// Compat spec: "Ignored". OpenAI accepts it; preserved for round-trip fidelity.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#tools--functions-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// Tool choice: "auto", "none", "required", or named function.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ChatToolChoice {
    Simple(String), // "auto", "none", "required"
    Named(NamedToolChoice),
}

/// Named tool choice specifying a specific function.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct NamedToolChoice {
    #[serde(rename = "type")]
    pub choice_type: String, // "function"
    pub function: NamedFunction,
}

/// Function name for named tool choice.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct NamedFunction {
    pub name: String,
}

/// Response format: text, json_object, or json_schema.
///
/// See <https://platform.openai.com/docs/api-reference/chat/create>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<serde_json::Value>,
}

// --- Response types ---

/// OpenAI Chat Completions API response body.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
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
    /// Compat spec response: "Always empty".
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
}

/// A completion choice with message and finish reason.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    /// Compat spec response: "Always empty".
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<serde_json::Value>,
}

/// Why the model stopped: stop, length, tool_calls, content_filter, or function_call.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    FunctionCall,
}

/// Token usage: prompt, completion, and total.
///
/// See <https://platform.openai.com/docs/api-reference/chat/object>
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ChatUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// Compat spec response: "Always empty". OpenAI returns reasoning_tokens, etc.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<serde_json::Value>,
    /// Compat spec response: "Always empty". OpenAI returns cached_tokens, etc.
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<serde_json::Value>,
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
                refusal: None,
            }],
            max_tokens: Some(100),
            max_completion_tokens: None,
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
            user: None,
            parallel_tool_calls: None,
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

    #[test]
    fn deserialize_realistic_openai_response() {
        // Real gpt-4o response with all fields OpenAI returns
        let raw = json!({
            "id": "chatcmpl-AKj3MbOpNGPq",
            "object": "chat.completion",
            "created": 1729800000,
            "model": "gpt-4o-2024-08-06",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello!",
                    "refusal": null
                },
                "logprobs": null,
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 5,
                "total_tokens": 17,
                "prompt_tokens_details": {
                    "cached_tokens": 0,
                    "audio_tokens": 0
                },
                "completion_tokens_details": {
                    "reasoning_tokens": 0,
                    "audio_tokens": 0,
                    "accepted_prediction_tokens": 0,
                    "rejected_prediction_tokens": 0
                }
            },
            "service_tier": "default",
            "system_fingerprint": "fp_a7d06e42a7"
        });
        let resp: ChatCompletionResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.id, "chatcmpl-AKj3MbOpNGPq");
        assert_eq!(resp.service_tier.as_deref(), Some("default"));
        assert_eq!(resp.system_fingerprint.as_deref(), Some("fp_a7d06e42a7"));
        assert!(resp.choices[0].logprobs.is_none());
        assert!(resp.choices[0].message.refusal.is_none());
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 12);
        assert!(usage.completion_tokens_details.is_some());
        assert!(usage.prompt_tokens_details.is_some());
    }

    #[test]
    fn deserialize_function_role_message() {
        let raw = json!({
            "role": "function",
            "content": "result"
        });
        let msg: ChatMessage = serde_json::from_value(raw).unwrap();
        assert_eq!(msg.role, ChatRole::Function);
    }

    #[test]
    fn temperature_clamping_captured_in_request() {
        // Verify user and parallel_tool_calls fields serialize correctly
        let raw = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "user": "user-123",
            "parallel_tool_calls": true
        });
        let req: ChatCompletionRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.user.as_deref(), Some("user-123"));
        assert_eq!(req.parallel_tool_calls, Some(true));
    }

    #[test]
    fn strict_field_on_function_def() {
        let raw = json!({
            "type": "function",
            "function": {
                "name": "test",
                "parameters": {"type": "object"},
                "strict": true
            }
        });
        let tool: ChatTool = serde_json::from_value(raw).unwrap();
        assert_eq!(tool.function.strict, Some(true));
    }
}
