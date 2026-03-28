// Gemini streaming state machine: full-response diffing -> Anthropic SSE events.
//
// Gemini's streamGenerateContent sends FULL accumulated GenerateContentResponse
// objects per SSE event (not incremental deltas like OpenAI). This state machine
// diffs each response against the previous state to produce Anthropic-format
// delta events.

use crate::anthropic;
use crate::anthropic::streaming::{DeltaUsage, MessageStartData};
use crate::gemini::response::{FinishReason, GenerateContentResponse};
use crate::util;

/// State machine that converts Gemini streaming responses (full accumulated text)
/// into Anthropic SSE delta events by diffing against previous state.
pub struct GeminiStreamingTranslator {
    model: String,
    message_id: String,
    started: bool,
    content_block_index: u32,
    text_block_open: bool,
    /// Length of text already emitted as deltas. Used to diff full-text responses.
    prev_text_len: usize,
    /// Number of tool calls already processed.
    prev_tool_count: usize,
    usage: anthropic::Usage,
    finished: bool,
}

impl GeminiStreamingTranslator {
    pub fn new(model: String) -> Self {
        Self {
            model,
            message_id: util::ids::generate_message_id(),
            started: false,
            content_block_index: 0,
            text_block_open: false,
            prev_text_len: 0,
            prev_tool_count: 0,
            usage: anthropic::Usage::default(),
            finished: false,
        }
    }

    /// Process one streaming GenerateContentResponse and emit Anthropic events.
    ///
    /// Each Gemini streaming event contains the FULL accumulated response so far,
    /// so we diff against `prev_text_len` to produce incremental deltas.
    pub fn process_response(
        &mut self,
        resp: &GenerateContentResponse,
    ) -> Vec<anthropic::StreamEvent> {
        let mut events = Vec::new();

        // Emit message_start on first call
        if !self.started {
            self.started = true;
            events.push(self.make_message_start());
        }

        let candidate = match resp.candidates.first() {
            Some(c) => c,
            None => return events,
        };

        // Collect full accumulated text from all text parts
        let mut current_text = String::new();
        for part in &candidate.content.parts {
            if let Some(ref t) = part.text {
                current_text.push_str(t);
            }
        }

        // Text delta: diff against what we already emitted
        if current_text.len() > self.prev_text_len {
            if !self.text_block_open {
                events.push(anthropic::StreamEvent::ContentBlockStart {
                    index: self.content_block_index,
                    content_block: anthropic::ContentBlock::Text {
                        text: String::new(),
                    },
                });
                self.text_block_open = true;
            }
            let delta_text = &current_text[self.prev_text_len..];
            events.push(anthropic::StreamEvent::ContentBlockDelta {
                index: self.content_block_index,
                delta: anthropic::streaming::Delta::TextDelta {
                    text: delta_text.to_string(),
                },
            });
            self.prev_text_len = current_text.len();
        }

        // Tool calls: count function_call parts
        let tool_calls: Vec<_> = candidate
            .content
            .parts
            .iter()
            .filter(|p| p.function_call.is_some())
            .collect();
        let tool_count = tool_calls.len();

        if tool_count > self.prev_tool_count {
            // Close open text block before emitting tool calls
            if self.text_block_open {
                events.push(anthropic::StreamEvent::ContentBlockStop {
                    index: self.content_block_index,
                });
                self.text_block_open = false;
                self.content_block_index += 1;
            }

            // Emit events for each new tool call
            for tc_part in &tool_calls[self.prev_tool_count..] {
                let fc = tc_part.function_call.as_ref().unwrap();
                let tool_id = util::ids::generate_tool_use_id();

                events.push(anthropic::StreamEvent::ContentBlockStart {
                    index: self.content_block_index,
                    content_block: anthropic::ContentBlock::ToolUse {
                        id: tool_id,
                        name: fc.name.clone(),
                        input: serde_json::Value::Object(serde_json::Map::new()),
                    },
                });

                let args_json = serde_json::to_string(&fc.args).unwrap_or_default();
                events.push(anthropic::StreamEvent::ContentBlockDelta {
                    index: self.content_block_index,
                    delta: anthropic::streaming::Delta::InputJsonDelta {
                        partial_json: args_json,
                    },
                });

                events.push(anthropic::StreamEvent::ContentBlockStop {
                    index: self.content_block_index,
                });

                self.content_block_index += 1;
            }
            self.prev_tool_count = tool_count;
        }

        // Extract usage metadata
        if let Some(ref um) = resp.usage_metadata {
            self.usage.input_tokens = um.prompt_token_count;
            self.usage.output_tokens = um.candidates_token_count;
        }

        // Finish detection
        if let Some(ref reason) = candidate.finish_reason {
            self.emit_finish(reason, tool_count > 0, &mut events);
        }

        events
    }

    /// Finalize the stream when no more events are expected (e.g., connection drop
    /// without a finishReason from Gemini).
    pub fn finish(&mut self) -> Vec<anthropic::StreamEvent> {
        if self.finished {
            return Vec::new();
        }
        let mut events = Vec::new();

        // Close any open text block
        if self.text_block_open {
            events.push(anthropic::StreamEvent::ContentBlockStop {
                index: self.content_block_index,
            });
            self.text_block_open = false;
        }

        events.push(anthropic::StreamEvent::MessageDelta {
            delta: anthropic::streaming::MessageDeltaData {
                stop_reason: Some(anthropic::StopReason::EndTurn),
                stop_sequence: None,
            },
            usage: Some(DeltaUsage {
                output_tokens: self.usage.output_tokens,
            }),
        });
        events.push(anthropic::StreamEvent::MessageStop {});
        self.finished = true;
        events
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn make_message_start(&self) -> anthropic::StreamEvent {
        anthropic::StreamEvent::MessageStart {
            message: MessageStartData {
                id: self.message_id.clone(),
                msg_type: "message".to_string(),
                role: "assistant".to_string(),
                content: vec![],
                model: self.model.clone(),
                stop_reason: None,
                stop_sequence: None,
                usage: self.usage.clone(),
                created: None,
            },
        }
    }

    fn emit_finish(
        &mut self,
        reason: &FinishReason,
        has_tool_calls: bool,
        events: &mut Vec<anthropic::StreamEvent>,
    ) {
        if self.finished {
            return;
        }

        // Close any open text block
        if self.text_block_open {
            events.push(anthropic::StreamEvent::ContentBlockStop {
                index: self.content_block_index,
            });
            self.text_block_open = false;
        }

        let stop_reason = match reason {
            FinishReason::STOP if has_tool_calls => anthropic::StopReason::ToolUse,
            FinishReason::STOP => anthropic::StopReason::EndTurn,
            FinishReason::MAX_TOKENS => anthropic::StopReason::MaxTokens,
            // SAFETY, RECITATION, LANGUAGE, OTHER, Unknown all map to EndTurn
            _ => anthropic::StopReason::EndTurn,
        };

        events.push(anthropic::StreamEvent::MessageDelta {
            delta: anthropic::streaming::MessageDeltaData {
                stop_reason: Some(stop_reason),
                stop_sequence: None,
            },
            usage: Some(DeltaUsage {
                output_tokens: self.usage.output_tokens,
            }),
        });
        events.push(anthropic::StreamEvent::MessageStop {});
        self.finished = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gemini::request::{Content, Part};
    use crate::gemini::response::{Candidate, FinishReason, GenerateContentResponse, UsageMetadata};
    use serde_json::json;

    // --- Test helpers ---

    fn make_text_response(
        text: &str,
        finish_reason: Option<FinishReason>,
    ) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::text(text)],
                },
                finish_reason,
                safety_ratings: None,
            }],
            usage_metadata: None,
            model_version: None,
        }
    }

    fn make_text_response_with_usage(
        text: &str,
        finish_reason: Option<FinishReason>,
        usage: UsageMetadata,
    ) -> GenerateContentResponse {
        let mut resp = make_text_response(text, finish_reason);
        resp.usage_metadata = Some(usage);
        resp
    }

    fn make_tool_call_response(
        name: &str,
        args: serde_json::Value,
        finish_reason: Option<FinishReason>,
    ) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::function_call(name, args)],
                },
                finish_reason,
                safety_ratings: None,
            }],
            usage_metadata: None,
            model_version: None,
        }
    }

    fn make_mixed_response(
        text: &str,
        calls: &[(&str, serde_json::Value)],
        finish_reason: Option<FinishReason>,
    ) -> GenerateContentResponse {
        let mut parts = vec![Part::text(text)];
        for (name, args) in calls {
            parts.push(Part::function_call(*name, args.clone()));
        }
        GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts,
                },
                finish_reason,
                safety_ratings: None,
            }],
            usage_metadata: None,
            model_version: None,
        }
    }

    fn make_empty_response() -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: vec![],
            usage_metadata: None,
            model_version: None,
        }
    }

    fn count_event_type(events: &[anthropic::StreamEvent], type_name: &str) -> usize {
        events
            .iter()
            .filter(|e| match (e, type_name) {
                (anthropic::StreamEvent::MessageStart { .. }, "message_start") => true,
                (anthropic::StreamEvent::ContentBlockStart { .. }, "content_block_start") => true,
                (anthropic::StreamEvent::ContentBlockDelta { .. }, "content_block_delta") => true,
                (anthropic::StreamEvent::ContentBlockStop { .. }, "content_block_stop") => true,
                (anthropic::StreamEvent::MessageDelta { .. }, "message_delta") => true,
                (anthropic::StreamEvent::MessageStop { .. }, "message_stop") => true,
                _ => false,
            })
            .count()
    }

    fn extract_text_deltas(events: &[anthropic::StreamEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                anthropic::StreamEvent::ContentBlockDelta {
                    delta: anthropic::streaming::Delta::TextDelta { text },
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    // --- Tests ---

    #[test]
    fn text_only_stream_multi_event() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());

        // Event 1: "Hello"
        let events1 = t.process_response(&make_text_response("Hello", None));
        assert_eq!(count_event_type(&events1, "message_start"), 1);
        assert_eq!(count_event_type(&events1, "content_block_start"), 1);
        assert_eq!(extract_text_deltas(&events1), vec!["Hello"]);

        // Event 2: "Hello world" (accumulated)
        let events2 = t.process_response(&make_text_response("Hello world", None));
        assert_eq!(count_event_type(&events2, "message_start"), 0);
        assert_eq!(count_event_type(&events2, "content_block_start"), 0);
        assert_eq!(extract_text_deltas(&events2), vec![" world"]);

        // Event 3: "Hello world!" with STOP
        let events3 = t.process_response(&make_text_response("Hello world!", Some(FinishReason::STOP)));
        assert_eq!(extract_text_deltas(&events3), vec!["!"]);
        assert_eq!(count_event_type(&events3, "content_block_stop"), 1);
        assert_eq!(count_event_type(&events3, "message_delta"), 1);
        assert_eq!(count_event_type(&events3, "message_stop"), 1);
        assert!(t.is_finished());
    }

    #[test]
    fn single_event_with_finish() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-flash".into());
        let events = t.process_response(&make_text_response("Done.", Some(FinishReason::STOP)));

        assert_eq!(count_event_type(&events, "message_start"), 1);
        assert_eq!(count_event_type(&events, "content_block_start"), 1);
        assert_eq!(extract_text_deltas(&events), vec!["Done."]);
        assert_eq!(count_event_type(&events, "content_block_stop"), 1);
        assert_eq!(count_event_type(&events, "message_delta"), 1);
        assert_eq!(count_event_type(&events, "message_stop"), 1);
        assert!(t.is_finished());
    }

    #[test]
    fn tool_call_stream() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());

        // Event 1: text appears
        let events1 = t.process_response(&make_text_response("Let me check", None));
        assert_eq!(extract_text_deltas(&events1), vec!["Let me check"]);

        // Event 2: text + tool call with STOP
        let resp2 = make_mixed_response(
            "Let me check",
            &[("get_weather", json!({"city": "London"}))],
            Some(FinishReason::STOP),
        );
        let events2 = t.process_response(&resp2);
        // No text delta (same text length)
        assert!(extract_text_deltas(&events2).is_empty());
        // Text block closed, tool block started/deltad/stopped
        assert_eq!(count_event_type(&events2, "content_block_stop"), 2); // text close + tool close
        assert_eq!(count_event_type(&events2, "content_block_start"), 1); // tool start

        // Should finish with tool_use stop reason
        let delta_event = events2.iter().find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        if let Some(anthropic::StreamEvent::MessageDelta { delta, .. }) = delta_event {
            assert_eq!(delta.stop_reason, Some(anthropic::StopReason::ToolUse));
        } else {
            panic!("expected MessageDelta");
        }
        assert!(t.is_finished());
    }

    #[test]
    fn tool_call_only_no_text() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let resp = make_tool_call_response("search", json!({"q": "rust"}), Some(FinishReason::STOP));
        let events = t.process_response(&resp);

        assert_eq!(count_event_type(&events, "message_start"), 1);
        // No text block should be opened
        assert!(extract_text_deltas(&events).is_empty());
        // Tool call events
        let tool_starts: Vec<_> = events.iter().filter(|e| matches!(e, anthropic::StreamEvent::ContentBlockStart { content_block: anthropic::ContentBlock::ToolUse { .. }, .. })).collect();
        assert_eq!(tool_starts.len(), 1);

        // Verify tool name
        if let anthropic::StreamEvent::ContentBlockStart { content_block: anthropic::ContentBlock::ToolUse { name, .. }, .. } = &tool_starts[0] {
            assert_eq!(name, "search");
        }
        assert!(t.is_finished());
    }

    #[test]
    fn multiple_tool_calls() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let resp = make_mixed_response(
            "",
            &[
                ("get_weather", json!({"city": "London"})),
                ("get_time", json!({"tz": "UTC"})),
            ],
            Some(FinishReason::STOP),
        );
        let events = t.process_response(&resp);

        let tool_starts: Vec<_> = events.iter().filter(|e| matches!(e, anthropic::StreamEvent::ContentBlockStart { content_block: anthropic::ContentBlock::ToolUse { .. }, .. })).collect();
        assert_eq!(tool_starts.len(), 2);
        assert!(t.is_finished());
    }

    #[test]
    fn empty_response_no_candidates() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.process_response(&make_empty_response());
        // Only message_start
        assert_eq!(count_event_type(&events, "message_start"), 1);
        assert_eq!(events.len(), 1);
        assert!(!t.is_finished());
    }

    #[test]
    fn safety_stop() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.process_response(&make_text_response("I can't", Some(FinishReason::SAFETY)));

        let delta_event = events.iter().find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        if let Some(anthropic::StreamEvent::MessageDelta { delta, .. }) = delta_event {
            assert_eq!(delta.stop_reason, Some(anthropic::StopReason::EndTurn));
        } else {
            panic!("expected MessageDelta");
        }
        assert!(t.is_finished());
    }

    #[test]
    fn max_tokens_stop() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.process_response(&make_text_response("truncated", Some(FinishReason::MAX_TOKENS)));

        let delta_event = events.iter().find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        if let Some(anthropic::StreamEvent::MessageDelta { delta, .. }) = delta_event {
            assert_eq!(delta.stop_reason, Some(anthropic::StopReason::MaxTokens));
        } else {
            panic!("expected MessageDelta");
        }
    }

    #[test]
    fn no_new_content_between_events() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());

        let events1 = t.process_response(&make_text_response("Hello", None));
        assert_eq!(extract_text_deltas(&events1), vec!["Hello"]);

        // Same text, no new content
        let events2 = t.process_response(&make_text_response("Hello", None));
        // Only message-level events, no deltas or block starts
        assert!(extract_text_deltas(&events2).is_empty());
        assert_eq!(count_event_type(&events2, "content_block_start"), 0);
    }

    #[test]
    fn usage_metadata_extracted() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let usage = UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 25,
            total_token_count: 35,
            cached_content_token_count: 0,
        };
        let resp = make_text_response_with_usage("done", Some(FinishReason::STOP), usage);
        let events = t.process_response(&resp);

        let delta_event = events.iter().find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        if let Some(anthropic::StreamEvent::MessageDelta { usage: Some(u), .. }) = delta_event {
            assert_eq!(u.output_tokens, 25);
        } else {
            panic!("expected MessageDelta with usage");
        }
    }

    #[test]
    fn finish_called_without_finish_reason() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());

        let _ = t.process_response(&make_text_response("partial", None));
        assert!(!t.is_finished());

        let events = t.finish();
        assert_eq!(count_event_type(&events, "content_block_stop"), 1);
        assert_eq!(count_event_type(&events, "message_delta"), 1);
        assert_eq!(count_event_type(&events, "message_stop"), 1);
        assert!(t.is_finished());

        // Double finish should be no-op
        let events2 = t.finish();
        assert!(events2.is_empty());
    }

    #[test]
    fn first_event_emits_message_start() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.process_response(&make_text_response("hi", None));

        match &events[0] {
            anthropic::StreamEvent::MessageStart { message } => {
                assert!(message.id.starts_with("msg_"));
                assert_eq!(message.model, "gemini-2.5-pro");
                assert_eq!(message.role, "assistant");
                assert!(message.content.is_empty());
                assert!(message.stop_reason.is_none());
            }
            other => panic!("expected MessageStart, got {:?}", other),
        }
    }

    #[test]
    fn text_block_opened_lazily() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());

        // Empty text part should not open a text block
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::text("")],
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
            model_version: None,
        };
        let events = t.process_response(&resp);
        // Only message_start, no content block start
        assert_eq!(count_event_type(&events, "content_block_start"), 0);
        assert!(!t.text_block_open);
    }

    #[test]
    fn tool_id_has_toolu_prefix() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let resp = make_tool_call_response("test_fn", json!({}), Some(FinishReason::STOP));
        let events = t.process_response(&resp);

        let tool_start = events.iter().find(|e| matches!(e, anthropic::StreamEvent::ContentBlockStart { content_block: anthropic::ContentBlock::ToolUse { .. }, .. }));
        if let Some(anthropic::StreamEvent::ContentBlockStart { content_block: anthropic::ContentBlock::ToolUse { id, .. }, .. }) = tool_start {
            assert!(id.starts_with("toolu_"), "tool ID should start with toolu_, got: {id}");
        } else {
            panic!("expected tool use content block start");
        }
    }

    #[test]
    fn mixed_text_and_tool_call_sequence() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());

        // Event 1: text starts
        let events1 = t.process_response(&make_text_response("I'll help.", None));
        assert_eq!(count_event_type(&events1, "message_start"), 1);
        assert_eq!(count_event_type(&events1, "content_block_start"), 1);
        assert_eq!(extract_text_deltas(&events1), vec!["I'll help."]);

        // Event 2: same text + tool call + finish
        let resp2 = make_mixed_response(
            "I'll help.",
            &[("lookup", json!({"id": 42}))],
            Some(FinishReason::STOP),
        );
        let events2 = t.process_response(&resp2);

        // Text block closed, tool block opened/deltad/stopped, then finish
        assert_eq!(count_event_type(&events2, "content_block_stop"), 2); // text + tool
        assert_eq!(count_event_type(&events2, "content_block_start"), 1); // tool

        // Verify InputJsonDelta was emitted for the tool
        let json_deltas: Vec<_> = events2.iter().filter_map(|e| match e {
            anthropic::StreamEvent::ContentBlockDelta { delta: anthropic::streaming::Delta::InputJsonDelta { partial_json }, .. } => Some(partial_json.clone()),
            _ => None,
        }).collect();
        assert_eq!(json_deltas.len(), 1);
        assert!(json_deltas[0].contains("42"));

        assert!(t.is_finished());
    }

    #[test]
    fn content_block_indices_increment_correctly() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());

        let resp = make_mixed_response(
            "text",
            &[("fn_a", json!({})), ("fn_b", json!({}))],
            Some(FinishReason::STOP),
        );
        let events = t.process_response(&resp);

        // Collect all indices from ContentBlockStart events
        let start_indices: Vec<u32> = events.iter().filter_map(|e| match e {
            anthropic::StreamEvent::ContentBlockStart { index, .. } => Some(*index),
            _ => None,
        }).collect();
        // text block at 0, fn_a at 1, fn_b at 2
        assert_eq!(start_indices, vec![0, 1, 2]);
    }

    #[test]
    fn recitation_stop_maps_to_end_turn() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.process_response(&make_text_response("x", Some(FinishReason::RECITATION)));

        let delta_event = events.iter().find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        if let Some(anthropic::StreamEvent::MessageDelta { delta, .. }) = delta_event {
            assert_eq!(delta.stop_reason, Some(anthropic::StopReason::EndTurn));
        }
    }

    #[test]
    fn unknown_finish_reason_maps_to_end_turn() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.process_response(&make_text_response("x", Some(FinishReason::Unknown)));

        let delta_event = events.iter().find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        if let Some(anthropic::StreamEvent::MessageDelta { delta, .. }) = delta_event {
            assert_eq!(delta.stop_reason, Some(anthropic::StopReason::EndTurn));
        }
    }

    #[test]
    fn finish_on_empty_stream() {
        // finish() on a translator that never received any events
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.finish();
        // No text block was open, so just message_delta + message_stop
        assert_eq!(count_event_type(&events, "content_block_stop"), 0);
        assert_eq!(count_event_type(&events, "message_delta"), 1);
        assert_eq!(count_event_type(&events, "message_stop"), 1);
        assert!(t.is_finished());
    }

    #[test]
    fn stop_with_tool_calls_gives_tool_use_reason() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let resp = make_tool_call_response("run", json!({}), Some(FinishReason::STOP));
        let events = t.process_response(&resp);

        let delta_event = events.iter().find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        if let Some(anthropic::StreamEvent::MessageDelta { delta, .. }) = delta_event {
            assert_eq!(delta.stop_reason, Some(anthropic::StopReason::ToolUse));
        } else {
            panic!("expected MessageDelta");
        }
    }

    #[test]
    fn tool_call_args_serialized_as_json() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let args = json!({"city": "Paris", "units": "celsius"});
        let resp = make_tool_call_response("weather", args.clone(), Some(FinishReason::STOP));
        let events = t.process_response(&resp);

        let json_deltas: Vec<_> = events.iter().filter_map(|e| match e {
            anthropic::StreamEvent::ContentBlockDelta { delta: anthropic::streaming::Delta::InputJsonDelta { partial_json }, .. } => Some(partial_json.clone()),
            _ => None,
        }).collect();
        assert_eq!(json_deltas.len(), 1);
        // Parse back and verify
        let parsed: serde_json::Value = serde_json::from_str(&json_deltas[0]).unwrap();
        assert_eq!(parsed, args);
    }

    #[test]
    fn process_after_finished_is_noop() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let _ = t.process_response(&make_text_response("done", Some(FinishReason::STOP)));
        assert!(t.is_finished());

        // Further process calls should still work but emit no finish events again
        let events = t.process_response(&make_text_response("extra", None));
        // finished flag prevents double-finish
        assert_eq!(count_event_type(&events, "message_delta"), 0);
        assert_eq!(count_event_type(&events, "message_stop"), 0);
    }

    #[test]
    fn message_id_is_unique_per_translator() {
        let t1 = GeminiStreamingTranslator::new("m".into());
        let t2 = GeminiStreamingTranslator::new("m".into());
        assert_ne!(t1.message_id, t2.message_id);
    }

    #[test]
    fn text_delta_with_multibyte_characters() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());

        let events1 = t.process_response(&make_text_response("Hello", None));
        assert_eq!(extract_text_deltas(&events1), vec!["Hello"]);

        // Append unicode characters
        let events2 = t.process_response(&make_text_response("Hello, world!", None));
        assert_eq!(extract_text_deltas(&events2), vec![", world!"]);
    }

    #[test]
    fn incremental_tool_calls_across_events() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());

        // Event 1: first tool call
        let resp1 = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::function_call("fn_a", json!({"a": 1}))],
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
            model_version: None,
        };
        let events1 = t.process_response(&resp1);
        let tool_starts_1: usize = events1.iter().filter(|e| matches!(e, anthropic::StreamEvent::ContentBlockStart { content_block: anthropic::ContentBlock::ToolUse { .. }, .. })).count();
        assert_eq!(tool_starts_1, 1);

        // Event 2: two tool calls (first is same, second is new)
        let resp2 = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![
                        Part::function_call("fn_a", json!({"a": 1})),
                        Part::function_call("fn_b", json!({"b": 2})),
                    ],
                },
                finish_reason: Some(FinishReason::STOP),
                safety_ratings: None,
            }],
            usage_metadata: None,
            model_version: None,
        };
        let events2 = t.process_response(&resp2);
        let tool_starts_2: usize = events2.iter().filter(|e| matches!(e, anthropic::StreamEvent::ContentBlockStart { content_block: anthropic::ContentBlock::ToolUse { .. }, .. })).count();
        // Only the new tool call should produce a start event
        assert_eq!(tool_starts_2, 1);
        assert!(t.is_finished());
    }

    #[test]
    fn all_events_serialize_to_valid_json() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.process_response(&make_text_response("test", Some(FinishReason::STOP)));

        for event in &events {
            let json = serde_json::to_string(event);
            assert!(json.is_ok(), "event should serialize: {:?}", event);
        }
    }
}
