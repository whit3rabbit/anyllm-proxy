// OpenAI SSE streaming types (ChatCompletions chunks + Responses events)

use serde::{Deserialize, Serialize};

use super::chat_completions::{ChatRole, ChatUsage, FinishReason};

/// A single chunk in a streamed Chat Completions response.
///
/// See <https://platform.openai.com/docs/api-reference/chat/streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String, // "chat.completion.chunk"
    pub model: String,
    pub choices: Vec<ChunkChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
    /// Compat spec response: "Always empty".
    /// See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
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
        };
        let json_str = serde_json::to_string(&chunk).unwrap();
        let roundtrip: ChatCompletionChunk = serde_json::from_str(&json_str).unwrap();
        assert_eq!(roundtrip.choices[0].delta.content.as_deref(), Some("world"));
        assert_eq!(roundtrip.created, Some(1700000000));
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
