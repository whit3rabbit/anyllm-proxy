// OpenAI SSE streaming types (ChatCompletions chunks + Responses events)
// PLAN.md lines 138-146

use serde::{Deserialize, Serialize};

use super::chat_completions::{ChatRole, ChatUsage, FinishReason};

/// A single chunk in a streamed Chat Completions response.
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
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChunkDelta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ChunkDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<ChatRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChunkToolCall>>,
}

/// Streaming tool calls arrive incrementally, with partial function arguments.
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
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            created: Some(1700000000),
        };
        let json_str = serde_json::to_string(&chunk).unwrap();
        let roundtrip: ChatCompletionChunk = serde_json::from_str(&json_str).unwrap();
        assert_eq!(roundtrip.choices[0].delta.content.as_deref(), Some("world"));
        assert_eq!(roundtrip.created, Some(1700000000));
    }
}
