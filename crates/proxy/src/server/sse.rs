// SSE responder helpers for Anthropic-format streaming
// PLAN.md lines 127-131

use anthropic_openai_translate::anthropic::streaming::StreamEvent;
use axum::response::sse::Event;

/// Format a StreamEvent as an axum SSE Event with the correct Anthropic event type name.
///
/// The event type string matches what Anthropic clients expect:
/// `message_start`, `content_block_start`, `content_block_delta`, etc.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages-streaming>
pub fn stream_event_to_sse(event: &StreamEvent) -> Result<Event, serde_json::Error> {
    let event_type = match event {
        StreamEvent::MessageStart { .. } => "message_start",
        StreamEvent::ContentBlockStart { .. } => "content_block_start",
        StreamEvent::ContentBlockDelta { .. } => "content_block_delta",
        StreamEvent::ContentBlockStop { .. } => "content_block_stop",
        StreamEvent::MessageDelta { .. } => "message_delta",
        StreamEvent::MessageStop { .. } => "message_stop",
        StreamEvent::Ping { .. } => "ping",
        StreamEvent::Error { .. } => "error",
    };
    let data = serde_json::to_string(event)?;
    Ok(Event::default().event(event_type).data(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anthropic_openai_translate::anthropic::messages::{ContentBlock, StopReason, Usage};
    use anthropic_openai_translate::anthropic::streaming::{
        Delta, DeltaUsage, MessageDeltaData, MessageStartData, StreamError,
    };

    /// Verify the function returns Ok and the JSON data round-trips correctly.
    /// We cannot inspect axum Event internals directly, so we test:
    /// 1. The event type mapping logic (via match coverage)
    /// 2. The JSON serialization is valid
    /// 3. The function does not error

    fn assert_sse_ok(event: &StreamEvent) {
        let _ = stream_event_to_sse(event).expect("stream_event_to_sse should not fail");
    }

    #[test]
    fn message_start_produces_sse() {
        let event = StreamEvent::MessageStart {
            message: MessageStartData {
                id: "msg_test".into(),
                msg_type: "message".into(),
                role: "assistant".into(),
                content: vec![],
                model: "gpt-4o".into(),
                stop_reason: None,
                stop_sequence: None,
                usage: Usage::default(),
                created: None,
            },
        };
        assert_sse_ok(&event);
        // Verify the JSON serialization contains expected fields
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("message_start"));
        assert!(json.contains("msg_test"));
    }

    #[test]
    fn content_block_start_produces_sse() {
        let event = StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: String::new(),
            },
        };
        assert_sse_ok(&event);
    }

    #[test]
    fn content_block_delta_text_produces_sse() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta {
                text: "hello".into(),
            },
        };
        assert_sse_ok(&event);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("text_delta"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn content_block_delta_input_json_produces_sse() {
        let event = StreamEvent::ContentBlockDelta {
            index: 1,
            delta: Delta::InputJsonDelta {
                partial_json: "{\"key\":".into(),
            },
        };
        assert_sse_ok(&event);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("input_json_delta"));
    }

    #[test]
    fn content_block_stop_produces_sse() {
        let event = StreamEvent::ContentBlockStop { index: 0 };
        assert_sse_ok(&event);
    }

    #[test]
    fn message_delta_produces_sse() {
        let event = StreamEvent::MessageDelta {
            delta: MessageDeltaData {
                stop_reason: Some(StopReason::EndTurn),
                stop_sequence: None,
            },
            usage: Some(DeltaUsage { output_tokens: 42 }),
        };
        assert_sse_ok(&event);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("end_turn"));
        assert!(json.contains("42"));
    }

    #[test]
    fn message_stop_produces_sse() {
        let event = StreamEvent::MessageStop {};
        assert_sse_ok(&event);
    }

    #[test]
    fn ping_produces_sse() {
        let event = StreamEvent::Ping {};
        assert_sse_ok(&event);
    }

    #[test]
    fn error_produces_sse() {
        let event = StreamEvent::Error {
            error: StreamError {
                error_type: "overloaded_error".into(),
                message: "Overloaded".into(),
            },
        };
        assert_sse_ok(&event);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("overloaded_error"));
    }

    #[test]
    fn event_type_mapping_covers_all_variants() {
        // Verify each variant maps to the correct SSE event type string.
        // We test the mapping logic by calling the function and checking it doesn't panic.
        let events: Vec<StreamEvent> = vec![
            StreamEvent::MessageStart {
                message: MessageStartData {
                    id: "msg_x".into(),
                    msg_type: "message".into(),
                    role: "assistant".into(),
                    content: vec![],
                    model: "m".into(),
                    stop_reason: None,
                    stop_sequence: None,
                    usage: Usage::default(),
                    created: None,
                },
            },
            StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::Text { text: "".into() },
            },
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::TextDelta { text: "t".into() },
            },
            StreamEvent::ContentBlockStop { index: 0 },
            StreamEvent::MessageDelta {
                delta: MessageDeltaData {
                    stop_reason: None,
                    stop_sequence: None,
                },
                usage: None,
            },
            StreamEvent::MessageStop {},
            StreamEvent::Ping {},
            StreamEvent::Error {
                error: StreamError {
                    error_type: "e".into(),
                    message: "m".into(),
                },
            },
        ];
        for event in &events {
            assert_sse_ok(event);
        }
    }

    #[test]
    fn serialized_data_is_valid_json() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta {
                text: "test".into(),
            },
        };
        // The data passed to the SSE event is serde_json::to_string output
        let data = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(parsed["index"], 0);
        assert_eq!(parsed["delta"]["text"], "test");
    }
}
