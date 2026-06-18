use super::*;
use serde_json::json;

fn make_event(event_type: &str, data: serde_json::Value) -> ResponsesStreamEvent {
    let data_map = match data {
        serde_json::Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    ResponsesStreamEvent {
        event_type: event_type.to_string(),
        data: data_map,
    }
}

#[test]
fn created_emits_message_start() {
    let mut t = ResponsesStreamingTranslator::new("gpt-4o".into());
    let events = t.process_event(&make_event("response.created", json!({})));
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        anthropic::StreamEvent::MessageStart { .. }
    ));
}

#[test]
fn text_delta_emits_content_block_delta() {
    let mut t = ResponsesStreamingTranslator::new("gpt-4o".into());
    t.process_event(&make_event("response.created", json!({})));
    t.process_event(&make_event(
        "response.output_item.added",
        json!({"item": {"type": "message"}}),
    ));
    t.process_event(&make_event(
        "response.content_part.added",
        json!({"part": {"type": "output_text"}}),
    ));

    let events = t.process_event(&make_event(
        "response.output_text.delta",
        json!({"delta": "Hello"}),
    ));
    assert_eq!(events.len(), 1);
    match &events[0] {
        anthropic::StreamEvent::ContentBlockDelta { delta, .. } => {
            assert!(matches!(delta, anthropic::Delta::TextDelta { text } if text == "Hello"));
        }
        _ => panic!("expected ContentBlockDelta"),
    }
}

#[test]
fn completed_emits_final_events() {
    let mut t = ResponsesStreamingTranslator::new("gpt-4o".into());
    t.process_event(&make_event("response.created", json!({})));
    t.process_event(&make_event("response.content_part.added", json!({})));
    t.process_event(&make_event(
        "response.output_text.delta",
        json!({"delta": "Hi"}),
    ));
    t.process_event(&make_event("response.content_part.done", json!({})));

    let events = t.process_event(&make_event(
        "response.completed",
        json!({
            "response": {
                "status": "completed",
                "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}
            }
        }),
    ));

    // Should have MessageDelta + MessageStop (content block already closed)
    assert!(events
        .iter()
        .any(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, anthropic::StreamEvent::MessageStop {})));
}

#[test]
fn incomplete_status_maps_to_max_tokens() {
    let mut t = ResponsesStreamingTranslator::new("gpt-4o".into());
    t.process_event(&make_event("response.created", json!({})));
    t.process_event(&make_event("response.content_part.added", json!({})));

    let events = t.process_event(&make_event("response.completed", json!({
        "response": {"status": "incomplete", "usage": {"input_tokens": 10, "output_tokens": 100}}
    })));

    let delta = events
        .iter()
        .find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
    match delta {
        Some(anthropic::StreamEvent::MessageDelta { delta, .. }) => {
            assert_eq!(delta.stop_reason, Some(anthropic::StopReason::MaxTokens));
        }
        _ => panic!("expected MessageDelta"),
    }
}

#[test]
fn function_call_streaming() {
    let mut t = ResponsesStreamingTranslator::new("gpt-4o".into());
    t.process_event(&make_event("response.created", json!({})));

    // Function call item added
    let events = t.process_event(&make_event(
        "response.output_item.added",
        json!({
            "item": {"type": "function_call", "name": "get_weather", "call_id": "call_1"}
        }),
    ));
    assert!(events.iter().any(|e| matches!(e, anthropic::StreamEvent::ContentBlockStart { content_block: anthropic::ContentBlock::ToolUse { name, .. }, .. } if name == "get_weather")));

    // Function call arguments delta
    let events = t.process_event(&make_event(
        "response.function_call_arguments.delta",
        json!({"delta": "{\"city\":"}),
    ));
    assert!(events.iter().any(|e| matches!(
        e,
        anthropic::StreamEvent::ContentBlockDelta {
            delta: anthropic::Delta::InputJsonDelta { .. },
            ..
        }
    )));

    // Output item done
    let events = t.process_event(&make_event(
        "response.output_item.done",
        json!({
            "item": {"type": "function_call"}
        }),
    ));
    assert!(events
        .iter()
        .any(|e| matches!(e, anthropic::StreamEvent::ContentBlockStop { .. })));
}

#[test]
fn finish_without_completed_event() {
    let mut t = ResponsesStreamingTranslator::new("gpt-4o".into());
    t.process_event(&make_event("response.created", json!({})));
    t.process_event(&make_event("response.content_part.added", json!({})));
    t.process_event(&make_event(
        "response.output_text.delta",
        json!({"delta": "Hi"}),
    ));

    // Stream ends without response.completed (connection dropped, etc.)
    let events = t.finish();
    assert!(events
        .iter()
        .any(|e| matches!(e, anthropic::StreamEvent::ContentBlockStop { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, anthropic::StreamEvent::MessageStop {})));
}

#[test]
fn empty_delta_ignored() {
    let mut t = ResponsesStreamingTranslator::new("gpt-4o".into());
    t.process_event(&make_event("response.created", json!({})));
    let events = t.process_event(&make_event(
        "response.output_text.delta",
        json!({"delta": ""}),
    ));
    assert!(events.is_empty());
}

#[test]
fn error_event_produces_stream_error() {
    let mut t = ResponsesStreamingTranslator::new("gpt-4o".into());
    t.process_event(&make_event("response.created", json!({})));

    let events = t.process_event(&make_event(
        "response.failed",
        json!({
            "response": {"status_details": {"error": {"message": "Rate limit exceeded"}}}
        }),
    ));
    assert!(
        matches!(&events[0], anthropic::StreamEvent::Error { error } if error.message == "Rate limit exceeded")
    );
}

#[test]
fn double_finish_is_noop() {
    let mut t = ResponsesStreamingTranslator::new("gpt-4o".into());
    t.process_event(&make_event("response.created", json!({})));
    t.process_event(&make_event(
        "response.completed",
        json!({
            "response": {"status": "completed", "usage": {"input_tokens": 1, "output_tokens": 1}}
        }),
    ));

    let events = t.finish();
    assert!(events.is_empty());
}

#[test]
fn translator_usage_returns_none_before_any_events() {
    let t = ResponsesStreamingTranslator::new("gpt-4o".into());
    assert!(t.usage().is_none());
}

#[test]
fn translator_usage_returns_tokens_after_completed_event() {
    let mut t = ResponsesStreamingTranslator::new("gpt-4o".into());
    let completed = make_event(
        "response.completed",
        json!({
            "response": {
                "status": "completed",
                "usage": {"input_tokens": 42, "output_tokens": 17, "total_tokens": 59}
            }
        }),
    );
    t.process_event(&completed);
    let usage = t
        .usage()
        .expect("usage should be Some after completed event");
    assert_eq!(usage.input_tokens, 42);
    assert_eq!(usage.output_tokens, 17);
}
