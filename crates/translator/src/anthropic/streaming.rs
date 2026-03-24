// Anthropic SSE streaming event types
// PLAN.md lines 125-136

use serde::{Deserialize, Serialize};

use super::messages::{ContentBlock, StopReason, Usage};

/// Top-level SSE event, internally tagged on `"type"`.
///
/// See <https://docs.anthropic.com/en/api/messages-streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageStartData },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u32,
        content_block: ContentBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u32, delta: Delta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDeltaData,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<DeltaUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop {},
    #[serde(rename = "ping")]
    Ping {},
    #[serde(rename = "error")]
    Error { error: StreamError },
}

/// Data payload for the message_start event.
///
/// See <https://docs.anthropic.com/en/api/messages-streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct MessageStartData {
    pub id: String,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
}

/// Content block delta: text or tool input JSON.
///
/// See <https://docs.anthropic.com/en/api/messages-streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    /// Signature delta: sent just before content_block_stop for thinking blocks.
    /// Used to verify integrity of the thinking block.
    #[serde(rename = "signature_delta")]
    SignatureDelta { signature: String },
}

/// Top-level message changes (stop_reason, stop_sequence).
///
/// See <https://docs.anthropic.com/en/api/messages-streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct MessageDeltaData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

/// Cumulative output token count in message_delta events.
///
/// See <https://docs.anthropic.com/en/api/messages-streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct DeltaUsage {
    pub output_tokens: u32,
}

/// Error event in the SSE stream.
///
/// See <https://docs.anthropic.com/en/api/messages-streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct StreamError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_message_start() {
        let j = json!({
            "type": "message_start",
            "message": {
                "id": "msg_1nZdL29xx5MUA1yADyHTEsnR8uuvGzszyY",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-3-5-sonnet-20241022",
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": 25, "output_tokens": 1}
            }
        });
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::MessageStart { message } => {
                assert_eq!(message.id, "msg_1nZdL29xx5MUA1yADyHTEsnR8uuvGzszyY");
                assert_eq!(message.role, "assistant");
                assert_eq!(message.usage.input_tokens, 25);
            }
            other => panic!("expected MessageStart, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_content_block_start_text() {
        let j = json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        });
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                assert_eq!(index, 0);
                match content_block {
                    ContentBlock::Text { text } => assert_eq!(text, ""),
                    _ => panic!("expected ContentBlock::Text"),
                }
            }
            other => panic!("expected ContentBlockStart, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_content_block_start_tool_use() {
        let j = json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "tool_use",
                "id": "toolu_xyz",
                "name": "get_weather",
                "input": {}
            }
        });
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                assert_eq!(index, 1);
                match content_block {
                    ContentBlock::ToolUse { id, name, .. } => {
                        assert_eq!(id, "toolu_xyz");
                        assert_eq!(name, "get_weather");
                    }
                    _ => panic!("expected ContentBlock::ToolUse"),
                }
            }
            other => panic!("expected ContentBlockStart, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_text_delta() {
        let j = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"}
        });
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 0);
                match delta {
                    Delta::TextDelta { text } => assert_eq!(text, "Hello"),
                    _ => panic!("expected Delta::TextDelta"),
                }
            }
            other => panic!("expected ContentBlockDelta, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_input_json_delta() {
        let j = json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "{\"location\":"}
        });
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                Delta::InputJsonDelta { partial_json } => {
                    assert_eq!(partial_json, "{\"location\":");
                }
                _ => panic!("expected Delta::InputJsonDelta"),
            },
            other => panic!("expected ContentBlockDelta, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_content_block_stop() {
        let j = json!({"type": "content_block_stop", "index": 0});
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::ContentBlockStop { index } => assert_eq!(index, 0),
            other => panic!("expected ContentBlockStop, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_message_delta() {
        let j = json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn", "stop_sequence": null},
            "usage": {"output_tokens": 15}
        });
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::MessageDelta { delta, usage } => {
                assert_eq!(delta.stop_reason, Some(StopReason::EndTurn));
                assert!(delta.stop_sequence.is_none());
                assert_eq!(usage.unwrap().output_tokens, 15);
            }
            other => panic!("expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_message_stop() {
        let j = json!({"type": "message_stop"});
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::MessageStop {} => {}
            other => panic!("expected MessageStop, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_ping() {
        let j = json!({"type": "ping"});
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::Ping {} => {}
            other => panic!("expected Ping, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_error_event() {
        let j = json!({
            "type": "error",
            "error": {"type": "overloaded_error", "message": "Overloaded"}
        });
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::Error { error } => {
                assert_eq!(error.error_type, "overloaded_error");
                assert_eq!(error.message, "Overloaded");
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn round_trip_text_delta_event() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta {
                text: "world".into(),
            },
        };
        let serialized = serde_json::to_string(&event).unwrap();
        let deserialized: StreamEvent = serde_json::from_str(&serialized).unwrap();
        match deserialized {
            StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 0);
                match delta {
                    Delta::TextDelta { text } => assert_eq!(text, "world"),
                    _ => panic!("expected Delta::TextDelta"),
                }
            }
            other => panic!("expected ContentBlockDelta, got {:?}", other),
        }
    }

    #[test]
    fn thinking_delta_roundtrip() {
        let j = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "thinking_delta", "thinking": "Let me think..."}
        });
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                Delta::ThinkingDelta { thinking } => {
                    assert_eq!(thinking, "Let me think...");
                }
                _ => panic!("expected Delta::ThinkingDelta"),
            },
            other => panic!("expected ContentBlockDelta, got {:?}", other),
        }
    }

    #[test]
    fn deserialize_signature_delta() {
        let j = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "signature_delta", "signature": "EqQBCgIYAhIM1gbcDa9GJwZA2b3h"}
        });
        let event: StreamEvent = serde_json::from_value(j).unwrap();
        match event {
            StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                Delta::SignatureDelta { signature } => {
                    assert_eq!(signature, "EqQBCgIYAhIM1gbcDa9GJwZA2b3h");
                }
                _ => panic!("expected Delta::SignatureDelta"),
            },
            other => panic!("expected ContentBlockDelta, got {:?}", other),
        }
    }
}
