// OpenAI SSE streaming types (ChatCompletions chunks + Responses events)

use serde::{Deserialize, Serialize};

use super::chat_completions::{ChatRole, ChatUsage, FinishReason};

/// A single chunk in a streamed Chat Completions response.
///
/// See <https://platform.openai.com/docs/api-reference/chat/streaming>
///
/// `id`, `object`, `model`, and `choices` default when absent. Some backends
/// emit usage-only final chunks with no `choices` array; defaulting to an
/// empty vec lets those pass without error.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatCompletionChunk {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub object: String, // "chat.completion.chunk"
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub choices: Vec<ChunkChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
    /// Compat spec response: "Always empty".
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
    /// Mid-stream error envelope. OpenAI-compatible gateways (notably OpenRouter)
    /// cannot change the HTTP status once a 200 SSE stream has started, so a failure
    /// during generation arrives as a chunk carrying a top-level `error` object
    /// alongside a choice with `finish_reason: "error"`.
    /// See: <https://openrouter.ai/docs/api/reference/errors-and-debugging>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ChunkError>,
}

/// Error payload embedded in a streaming chunk by gateways that signal mid-stream
/// failures over an already-200 SSE response.
///
/// `code` is untyped because OpenRouter sends a string code (`"server_error"`)
/// mid-stream but a numeric HTTP status (`400`) for pre-stream errors.
///
/// See <https://openrouter.ai/docs/api/reference/errors-and-debugging>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChunkError {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A choice within a streaming chunk.
///
/// See <https://platform.openai.com/docs/api-reference/chat/streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChunkDelta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    /// Compat spec response: "Always empty".
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<serde_json::Value>,
}

/// Incremental content delta in a streaming chunk.
///
/// See <https://platform.openai.com/docs/api-reference/chat/streaming>
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ChunkDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<ChatRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChunkToolCall>>,
    /// DeepSeek/Qwen thinking model output. Maps to Anthropic thinking block deltas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

/// Streaming tool calls arrive incrementally, with partial function arguments.
///
/// See <https://platform.openai.com/docs/api-reference/chat/streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChunkToolCall {
    pub index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<ChunkFunctionCall>,
}

/// Incremental function call data in a streaming chunk.
///
/// See <https://platform.openai.com/docs/api-reference/chat/streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChunkFunctionCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_chunk_with_text_delta() {
        let raw = json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion.chunk",
            "model": "gpt-4o",
            "choices": [
                {
                    "index": 0,
                    "delta": {
                        "content": "Hello"
                    },
                    "finish_reason": null
                }
            ],
            "created": 1700000000
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(raw).unwrap();
        assert_eq!(chunk.id, "chatcmpl-abc");
        assert_eq!(chunk.object, "chat.completion.chunk");
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
        assert!(chunk.choices[0].finish_reason.is_none());
    }

    #[test]
    fn deserialize_chunk_with_role_delta() {
        let raw = json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion.chunk",
            "model": "gpt-4o",
            "choices": [
                {
                    "index": 0,
                    "delta": {
                        "role": "assistant"
                    }
                }
            ]
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(raw).unwrap();
        assert_eq!(chunk.choices[0].delta.role, Some(ChatRole::Assistant));
        assert!(chunk.choices[0].delta.content.is_none());
    }

    #[test]
    fn deserialize_chunk_with_tool_call_delta() {
        let raw = json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion.chunk",
            "model": "gpt-4o",
            "choices": [
                {
                    "index": 0,
                    "delta": {
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "call_xyz",
                                "type": "function",
                                "function": {
                                    "name": "get_weather",
                                    "arguments": "{\"loc"
                                }
                            }
                        ]
                    }
                }
            ]
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(raw).unwrap();
        let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.index, 0);
        assert_eq!(tc.id.as_deref(), Some("call_xyz"));
        assert_eq!(
            tc.function.as_ref().unwrap().name.as_deref(),
            Some("get_weather")
        );
        assert_eq!(
            tc.function.as_ref().unwrap().arguments.as_deref(),
            Some("{\"loc")
        );
    }

    #[test]
    fn deserialize_chunk_with_finish_reason() {
        let raw = json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion.chunk",
            "model": "gpt-4o",
            "choices": [
                {
                    "index": 0,
                    "delta": {},
                    "finish_reason": "stop"
                }
            ]
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(raw).unwrap();
        assert_eq!(chunk.choices[0].finish_reason, Some(FinishReason::Stop));
        assert!(chunk.choices[0].delta.content.is_none());
    }

    #[test]
    fn deserialize_chunk_with_usage() {
        let raw = json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion.chunk",
            "model": "gpt-4o",
            "choices": [],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(raw).unwrap();
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn roundtrip_chunk() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-test".into(),
            object: "chat.completion.chunk".into(),
            model: "gpt-4o".into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: Some("world".into()),
                    refusal: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
            created: Some(1700000000),
            system_fingerprint: None,
            error: None,
        };
        let json_str = serde_json::to_string(&chunk).unwrap();
        let roundtrip: ChatCompletionChunk = serde_json::from_str(&json_str).unwrap();
        assert_eq!(roundtrip.choices[0].delta.content.as_deref(), Some("world"));
        assert_eq!(roundtrip.created, Some(1700000000));
    }

    #[test]
    fn chunk_missing_id_object_model_deserializes() {
        // Usage-only final chunks emitted by some gateways omit id/object/model/choices.
        let raw = json!({
            "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(raw).unwrap();
        assert_eq!(chunk.id, "");
        assert_eq!(chunk.choices.len(), 0);
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 5);
    }

    #[test]
    fn deserialize_openrouter_midstream_error_chunk() {
        // OpenRouter signals a mid-generation failure (after a 200 SSE response has
        // begun) as a chunk with a top-level `error` object and finish_reason "error".
        let raw = json!({
            "id": "cmpl-abc123",
            "object": "chat.completion.chunk",
            "created": 1234567890,
            "model": "openai/gpt-4o",
            "provider": "openai",
            "error": {"code": "server_error", "message": "Provider disconnected"},
            "choices": [{"index": 0, "delta": {"content": ""}, "finish_reason": "error"}]
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(raw).unwrap();
        let err = chunk.error.expect("error envelope should deserialize");
        assert_eq!(err.code.as_ref().unwrap().as_str(), Some("server_error"));
        assert_eq!(err.message.as_deref(), Some("Provider disconnected"));
        assert_eq!(
            chunk.choices[0].finish_reason,
            Some(FinishReason::Error),
            "finish_reason \"error\" should bind to FinishReason::Error, not Unknown"
        );
    }

    #[test]
    fn normal_chunk_omits_error_field_on_serialize() {
        // The new `error` field must not appear in serialized output when absent.
        let chunk = ChatCompletionChunk {
            id: "c1".into(),
            object: "chat.completion.chunk".into(),
            model: "gpt-4o".into(),
            choices: vec![],
            usage: None,
            created: None,
            system_fingerprint: None,
            error: None,
        };
        let s = serde_json::to_string(&chunk).unwrap();
        assert!(!s.contains("\"error\""), "unexpected error field: {s}");
    }

    #[test]
    fn deserialize_realistic_streaming_chunk() {
        // Real gpt-4o streaming chunk with all fields
        let raw = json!({
            "id": "chatcmpl-AKj3",
            "object": "chat.completion.chunk",
            "created": 1729800000,
            "model": "gpt-4o-2024-08-06",
            "system_fingerprint": "fp_a7d06e42a7",
            "choices": [{
                "index": 0,
                "delta": {"content": "Hi"},
                "logprobs": null,
                "finish_reason": null
            }]
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(raw).unwrap();
        assert_eq!(chunk.system_fingerprint.as_deref(), Some("fp_a7d06e42a7"));
        assert!(chunk.choices[0].logprobs.is_none());
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hi"));
    }
}
