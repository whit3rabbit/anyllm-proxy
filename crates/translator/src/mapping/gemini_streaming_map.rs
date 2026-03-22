// Phase 20f: Gemini streaming state machine (TASKS.md Phase 20f)
//
// Converts Gemini GenerateContentResponse chunks (each a full response with
// accumulated content) into Anthropic SSE events. Gemini streams full response
// objects, not deltas, so this state machine tracks what has already been
// emitted and produces incremental events.

use crate::anthropic;
use crate::gemini::generate_content::{FinishReason, GenerateContentResponse, Part};
use crate::mapping::gemini_message_map;
use crate::util;

/// State machine that converts Gemini streaming chunks into Anthropic SSE events.
///
/// Feed chunks via `process_chunk`, then call `finish` after the stream ends.
/// Each call returns zero or more Anthropic SSE events to forward to the client.
pub struct GeminiStreamingTranslator {
    model: String,
    message_id: String,
    started: bool,
    /// Track how many text chars we've already emitted, keyed by part index.
    emitted_text_len: Vec<usize>,
    /// Track which parts have had their content_block_start emitted.
    opened_blocks: Vec<bool>,
    usage: anthropic::Usage,
    finished: bool,
}

impl GeminiStreamingTranslator {
    pub fn new(model: String) -> Self {
        Self {
            model,
            message_id: util::ids::generate_message_id(),
            started: false,
            emitted_text_len: Vec::new(),
            opened_blocks: Vec::new(),
            usage: anthropic::Usage::default(),
            finished: false,
        }
    }

    /// Process one Gemini GenerateContentResponse chunk.
    /// Returns zero or more Anthropic SSE events.
    pub fn process_chunk(
        &mut self,
        chunk: &GenerateContentResponse,
    ) -> Vec<anthropic::StreamEvent> {
        let mut events = Vec::new();

        // Emit message_start on first chunk
        if !self.started {
            self.started = true;
            events.push(self.make_message_start());
        }

        // Update usage from chunk
        if let Some(ref usage) = chunk.usage_metadata {
            self.usage.input_tokens = usage.prompt_token_count.unwrap_or(0);
            self.usage.output_tokens = usage.candidates_token_count.unwrap_or(0);
        }

        // Get parts from first candidate
        let candidate = chunk.candidates.as_ref().and_then(|c| c.first());

        if let Some(candidate) = candidate {
            if let Some(ref content) = candidate.content {
                self.process_parts(&content.parts, &mut events);
            }

            // Handle finish reason
            if let Some(ref reason) = candidate.finish_reason {
                self.handle_finish(reason, &mut events);
            }
        }

        events
    }

    /// Call after all chunks have been processed (stream ended).
    pub fn finish(&mut self) -> Vec<anthropic::StreamEvent> {
        let mut events = Vec::new();
        if !self.finished {
            self.finished = true;
            // Close any open blocks
            self.close_all_blocks(&mut events);
            events.push(anthropic::StreamEvent::MessageStop {});
        }
        events
    }

    fn make_message_start(&self) -> anthropic::StreamEvent {
        anthropic::StreamEvent::MessageStart {
            message: anthropic::streaming::MessageStartData {
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

    fn process_parts(&mut self, parts: &[Part], events: &mut Vec<anthropic::StreamEvent>) {
        for (i, part) in parts.iter().enumerate() {
            // Grow tracking vecs as needed
            while self.emitted_text_len.len() <= i {
                self.emitted_text_len.push(0);
            }
            while self.opened_blocks.len() <= i {
                self.opened_blocks.push(false);
            }

            match part {
                Part::Text { text } => {
                    let prev_len = self.emitted_text_len[i];
                    if text.len() > prev_len {
                        // Open block if not yet opened
                        if !self.opened_blocks[i] {
                            self.opened_blocks[i] = true;
                            events.push(anthropic::StreamEvent::ContentBlockStart {
                                index: i as u32,
                                content_block: anthropic::ContentBlock::Text {
                                    text: String::new(),
                                },
                            });
                        }
                        // Emit the delta (new text since last chunk)
                        let delta_text = &text[prev_len..];
                        events.push(anthropic::StreamEvent::ContentBlockDelta {
                            index: i as u32,
                            delta: anthropic::Delta::TextDelta {
                                text: delta_text.to_string(),
                            },
                        });
                        self.emitted_text_len[i] = text.len();
                    }
                }
                Part::FunctionCall { function_call } => {
                    if !self.opened_blocks[i] {
                        self.opened_blocks[i] = true;
                        // Emit tool_use block start with full data
                        // Gemini sends function calls as complete objects, not incrementally
                        events.push(anthropic::StreamEvent::ContentBlockStart {
                            index: i as u32,
                            content_block: anthropic::ContentBlock::ToolUse {
                                id: util::ids::generate_tool_use_id(),
                                name: function_call.name.clone(),
                                input: serde_json::Value::Object(serde_json::Map::new()),
                            },
                        });
                        // Emit the full arguments as a single input_json_delta
                        let args_str =
                            serde_json::to_string(&function_call.args).unwrap_or_default();
                        events.push(anthropic::StreamEvent::ContentBlockDelta {
                            index: i as u32,
                            delta: anthropic::Delta::InputJsonDelta {
                                partial_json: args_str,
                            },
                        });
                    }
                }
                // Other part types (InlineData, FileData, FunctionResponse) are
                // not expected in streaming model output
                _ => {}
            }
        }
    }

    fn handle_finish(&mut self, reason: &FinishReason, events: &mut Vec<anthropic::StreamEvent>) {
        // Close all open blocks
        self.close_all_blocks(events);

        let stop_reason = gemini_message_map::map_finish_reason(Some(reason));

        events.push(anthropic::StreamEvent::MessageDelta {
            delta: anthropic::streaming::MessageDeltaData {
                stop_reason: Some(stop_reason),
                stop_sequence: None,
            },
            usage: Some(anthropic::streaming::DeltaUsage {
                output_tokens: self.usage.output_tokens,
            }),
        });
    }

    fn close_all_blocks(&mut self, events: &mut Vec<anthropic::StreamEvent>) {
        for (i, opened) in self.opened_blocks.iter_mut().enumerate() {
            if *opened {
                events.push(anthropic::StreamEvent::ContentBlockStop { index: i as u32 });
                *opened = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gemini::generate_content::{
        Candidate, Content, FunctionCallData, GeminiRole, UsageMetadata,
    };
    use serde_json::json;

    fn text_chunk(text: &str, finish: Option<FinishReason>) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some(GeminiRole::Model),
                    parts: vec![Part::Text {
                        text: text.to_string(),
                    }],
                }),
                finish_reason: finish,
                safety_ratings: None,
                citation_metadata: None,
                index: Some(0),
            }]),
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: Some(10),
                candidates_token_count: Some(5),
                total_token_count: Some(15),
            }),
            prompt_feedback: None,
        }
    }

    fn tool_call_chunk(
        name: &str,
        args: serde_json::Value,
        finish: Option<FinishReason>,
    ) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    role: Some(GeminiRole::Model),
                    parts: vec![Part::FunctionCall {
                        function_call: FunctionCallData {
                            name: name.to_string(),
                            args,
                        },
                    }],
                }),
                finish_reason: finish,
                safety_ratings: None,
                citation_metadata: None,
                index: Some(0),
            }]),
            usage_metadata: None,
            prompt_feedback: None,
        }
    }

    fn event_types(events: &[anthropic::StreamEvent]) -> Vec<&str> {
        events
            .iter()
            .map(|e| match e {
                anthropic::StreamEvent::MessageStart { .. } => "message_start",
                anthropic::StreamEvent::ContentBlockStart { .. } => "content_block_start",
                anthropic::StreamEvent::ContentBlockDelta { .. } => "content_block_delta",
                anthropic::StreamEvent::ContentBlockStop { .. } => "content_block_stop",
                anthropic::StreamEvent::MessageDelta { .. } => "message_delta",
                anthropic::StreamEvent::MessageStop {} => "message_stop",
                anthropic::StreamEvent::Ping {} => "ping",
                anthropic::StreamEvent::Error { .. } => "error",
            })
            .collect()
    }

    #[test]
    fn first_chunk_emits_message_start() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.process_chunk(&text_chunk("Hi", None));

        assert!(events.len() >= 1);
        match &events[0] {
            anthropic::StreamEvent::MessageStart { message } => {
                assert!(message.id.starts_with("msg_"));
                assert_eq!(message.model, "gemini-2.5-pro");
                assert_eq!(message.role, "assistant");
            }
            other => panic!("expected MessageStart, got {:?}", other),
        }
    }

    #[test]
    fn incremental_text_streaming() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());

        // First chunk: "Hello"
        let events = t.process_chunk(&text_chunk("Hello", None));
        assert_eq!(
            event_types(&events),
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta"
            ]
        );
        match &events[2] {
            anthropic::StreamEvent::ContentBlockDelta {
                delta: anthropic::Delta::TextDelta { text },
                ..
            } => assert_eq!(text, "Hello"),
            other => panic!("expected TextDelta, got {:?}", other),
        }

        // Second chunk: "Hello world" (accumulated)
        let events = t.process_chunk(&text_chunk("Hello world", None));
        assert_eq!(event_types(&events), vec!["content_block_delta"]);
        match &events[0] {
            anthropic::StreamEvent::ContentBlockDelta {
                delta: anthropic::Delta::TextDelta { text },
                ..
            } => assert_eq!(text, " world"),
            other => panic!("expected TextDelta with ' world', got {:?}", other),
        }
    }

    #[test]
    fn no_delta_for_unchanged_text() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        t.process_chunk(&text_chunk("Hello", None));

        // Same text again: no new events
        let events = t.process_chunk(&text_chunk("Hello", None));
        assert!(events.is_empty());
    }

    #[test]
    fn finish_reason_emits_block_stop_and_message_delta() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        t.process_chunk(&text_chunk("Hello", None));

        let events = t.process_chunk(&text_chunk("Hello world", Some(FinishReason::Stop)));
        // Should include: delta, block_stop, message_delta
        let types = event_types(&events);
        assert!(types.contains(&"content_block_stop"));
        assert!(types.contains(&"message_delta"));

        match events
            .iter()
            .find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }))
        {
            Some(anthropic::StreamEvent::MessageDelta { delta, usage }) => {
                assert_eq!(delta.stop_reason, Some(anthropic::StopReason::EndTurn));
                assert!(usage.is_some());
            }
            other => panic!("expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn finish_emits_message_stop() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        t.process_chunk(&text_chunk("Hi", Some(FinishReason::Stop)));

        let events = t.finish();
        assert_eq!(event_types(&events), vec!["message_stop"]);

        // Calling finish again: no events
        assert!(t.finish().is_empty());
    }

    #[test]
    fn tool_call_streaming() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let chunk = tool_call_chunk(
            "get_weather",
            json!({"location": "NYC"}),
            Some(FinishReason::Stop),
        );
        let events = t.process_chunk(&chunk);

        let types = event_types(&events);
        assert!(types.contains(&"message_start"));
        assert!(types.contains(&"content_block_start"));
        assert!(types.contains(&"content_block_delta"));

        // Verify tool use block
        let start = events
            .iter()
            .find(|e| matches!(e, anthropic::StreamEvent::ContentBlockStart { .. }));
        match start {
            Some(anthropic::StreamEvent::ContentBlockStart { content_block, .. }) => {
                match content_block {
                    anthropic::ContentBlock::ToolUse { id, name, .. } => {
                        assert!(id.starts_with("toolu_"));
                        assert_eq!(name, "get_weather");
                    }
                    other => panic!("expected ToolUse, got {:?}", other),
                }
            }
            other => panic!("expected ContentBlockStart, got {:?}", other),
        }

        // Verify input_json_delta
        let delta = events.iter().find(|e| {
            matches!(
                e,
                anthropic::StreamEvent::ContentBlockDelta {
                    delta: anthropic::Delta::InputJsonDelta { .. },
                    ..
                }
            )
        });
        match delta {
            Some(anthropic::StreamEvent::ContentBlockDelta {
                delta: anthropic::Delta::InputJsonDelta { partial_json },
                ..
            }) => {
                let parsed: serde_json::Value = serde_json::from_str(partial_json).unwrap();
                assert_eq!(parsed["location"], "NYC");
            }
            other => panic!("expected InputJsonDelta, got {:?}", other),
        }
    }

    #[test]
    fn max_tokens_finish_reason() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.process_chunk(&text_chunk("Partial", Some(FinishReason::MaxTokens)));

        let msg_delta = events
            .iter()
            .find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        match msg_delta {
            Some(anthropic::StreamEvent::MessageDelta { delta, .. }) => {
                assert_eq!(delta.stop_reason, Some(anthropic::StopReason::MaxTokens));
            }
            other => panic!("expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn safety_finish_reason() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let events = t.process_chunk(&text_chunk("", Some(FinishReason::Safety)));

        let msg_delta = events
            .iter()
            .find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        match msg_delta {
            Some(anthropic::StreamEvent::MessageDelta { delta, .. }) => {
                assert_eq!(delta.stop_reason, Some(anthropic::StopReason::EndTurn));
            }
            other => panic!("expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn usage_tracking() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        t.process_chunk(&text_chunk("Hi", None));

        // Chunk with updated usage
        let mut chunk = text_chunk("Hi there", Some(FinishReason::Stop));
        chunk.usage_metadata = Some(UsageMetadata {
            prompt_token_count: Some(25),
            candidates_token_count: Some(12),
            total_token_count: Some(37),
        });
        let events = t.process_chunk(&chunk);

        let msg_delta = events
            .iter()
            .find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        match msg_delta {
            Some(anthropic::StreamEvent::MessageDelta { usage, .. }) => {
                assert_eq!(usage.as_ref().unwrap().output_tokens, 12);
            }
            other => panic!("expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn full_text_stream_sequence() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let mut all_events = Vec::new();

        all_events.extend(t.process_chunk(&text_chunk("Hello", None)));
        all_events.extend(t.process_chunk(&text_chunk("Hello world", Some(FinishReason::Stop))));
        all_events.extend(t.finish());

        assert_eq!(
            event_types(&all_events),
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
    }

    #[test]
    fn empty_candidates_chunk() {
        let mut t = GeminiStreamingTranslator::new("gemini-2.5-pro".into());
        let chunk = GenerateContentResponse {
            candidates: None,
            usage_metadata: None,
            prompt_feedback: None,
        };
        let events = t.process_chunk(&chunk);
        // Only message_start
        assert_eq!(event_types(&events), vec!["message_start"]);

        // Second empty chunk: no events
        let events = t.process_chunk(&chunk);
        assert!(events.is_empty());
    }
}
