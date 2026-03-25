// Streaming state machine: OpenAI chunks -> Anthropic SSE events

use crate::anthropic;
use crate::openai;
use crate::util;

// Safety cap: prevents unbounded Vec growth if a backend sends a
// malformed chunk with an absurdly large tool_call index.
const MAX_TOOL_CALL_INDEX: usize = 128;

/// State machine that converts OpenAI ChatCompletion chunks into Anthropic SSE events.
///
/// Feed chunks via `process_chunk`, then call `finish` after the OpenAI `[DONE]` sentinel.
/// Each call returns zero or more Anthropic SSE events to forward to the client.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages-streaming>
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/streaming>
pub struct StreamingTranslator {
    model: String,
    message_id: String,
    started: bool,
    content_block_index: u32,
    content_block_open: bool,
    /// Tracks whether a thinking content block is open (for reasoning_content
    /// from DeepSeek/Qwen thinking models).
    thinking_block_open: bool,
    /// Tool calls arrive incrementally across multiple chunks, indexed by
    /// position in the OpenAI tool_calls array. We accumulate them here
    /// so we can emit Anthropic's strict Start -> Delta* -> Stop sequence
    /// per tool when finish_reason arrives.
    active_tool_calls: Vec<ToolCallAccumulator>,
    usage: anthropic::Usage,
    finished: bool,
    created: Option<u64>,
}

struct ToolCallAccumulator {
    block_index: u32,
    closed: bool,
}

impl StreamingTranslator {
    /// Create a new streaming translator for the given model.
    ///
    /// Anthropic: <https://docs.anthropic.com/en/api/messages-streaming>
    /// OpenAI: <https://platform.openai.com/docs/api-reference/chat/streaming>
    pub fn new(model: String) -> Self {
        Self {
            model,
            message_id: util::ids::generate_message_id(),
            started: false,
            content_block_index: 0,
            content_block_open: false,
            thinking_block_open: false,
            active_tool_calls: Vec::new(),
            usage: anthropic::Usage::default(),
            finished: false,
            created: None,
        }
    }

    /// Process one OpenAI chunk and return zero or more Anthropic SSE events.
    ///
    /// Anthropic: <https://docs.anthropic.com/en/api/messages-streaming>
    /// OpenAI: <https://platform.openai.com/docs/api-reference/chat/streaming>
    pub fn process_chunk(
        &mut self,
        chunk: &openai::ChatCompletionChunk,
    ) -> Vec<anthropic::StreamEvent> {
        let mut events = Vec::new();

        // Emit message_start on first chunk
        if !self.started {
            self.started = true;
            self.created = chunk.created;
            events.push(self.make_message_start());
        }

        // Capture usage from the final chunk (OpenAI sends it with stream_options.include_usage)
        if let Some(ref usage) = chunk.usage {
            self.usage.input_tokens = usage.prompt_tokens;
            self.usage.output_tokens = usage.completion_tokens;
        }

        for choice in &chunk.choices {
            // Handle reasoning_content (DeepSeek/Qwen thinking models).
            // Emitted as a separate Anthropic thinking content block before text.
            if let Some(ref reasoning) = choice.delta.reasoning_content {
                if !self.thinking_block_open {
                    events.push(anthropic::StreamEvent::ContentBlockStart {
                        index: self.content_block_index,
                        content_block: anthropic::ContentBlock::Thinking {
                            thinking: String::new(),
                            signature: None,
                        },
                    });
                    self.thinking_block_open = true;
                }
                events.push(anthropic::StreamEvent::ContentBlockDelta {
                    index: self.content_block_index,
                    delta: anthropic::Delta::ThinkingDelta {
                        thinking: reasoning.clone(),
                    },
                });
            }

            // Handle text content deltas
            if let Some(ref text) = choice.delta.content {
                // Close thinking block if transitioning from reasoning to content
                if self.thinking_block_open {
                    events.push(anthropic::StreamEvent::ContentBlockStop {
                        index: self.content_block_index,
                    });
                    self.thinking_block_open = false;
                    self.content_block_index += 1;
                }
                if !self.content_block_open {
                    events.push(anthropic::StreamEvent::ContentBlockStart {
                        index: self.content_block_index,
                        content_block: anthropic::ContentBlock::Text {
                            text: String::new(),
                        },
                    });
                    self.content_block_open = true;
                }
                events.push(anthropic::StreamEvent::ContentBlockDelta {
                    index: self.content_block_index,
                    delta: anthropic::Delta::TextDelta { text: text.clone() },
                });
            }

            // Handle refusals (safety filter triggered during streaming).
            // Anthropic has no refusal type; surface as text so the client sees it.
            if let Some(ref refusal) = choice.delta.refusal {
                if !self.content_block_open {
                    events.push(anthropic::StreamEvent::ContentBlockStart {
                        index: self.content_block_index,
                        content_block: anthropic::ContentBlock::Text {
                            text: String::new(),
                        },
                    });
                    self.content_block_open = true;
                }
                events.push(anthropic::StreamEvent::ContentBlockDelta {
                    index: self.content_block_index,
                    delta: anthropic::Delta::TextDelta {
                        text: super::format_refusal(refusal),
                    },
                });
            }

            // Handle tool call deltas
            if let Some(ref tool_calls) = choice.delta.tool_calls {
                for tc in tool_calls {
                    self.handle_tool_call_delta(tc, &mut events);
                }
            }

            // Handle finish_reason
            if let Some(ref finish_reason) = choice.finish_reason {
                // Close any open thinking block
                if self.thinking_block_open {
                    events.push(anthropic::StreamEvent::ContentBlockStop {
                        index: self.content_block_index,
                    });
                    self.thinking_block_open = false;
                    self.content_block_index += 1;
                }
                // Close any open text content block
                if self.content_block_open {
                    events.push(anthropic::StreamEvent::ContentBlockStop {
                        index: self.content_block_index,
                    });
                    self.content_block_open = false;
                    self.content_block_index += 1;
                }

                // Flush any accumulated tool calls
                self.flush_tool_calls(&mut events);

                // Map OpenAI finish_reason to Anthropic stop_reason
                let stop_reason = map_finish_reason(finish_reason);

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
        }

        events
    }

    /// Call after all chunks have been processed (when OpenAI sends `[DONE]`).
    ///
    /// Anthropic: <https://docs.anthropic.com/en/api/messages-streaming>
    /// OpenAI: <https://platform.openai.com/docs/api-reference/chat/streaming>
    pub fn finish(&mut self) -> Vec<anthropic::StreamEvent> {
        let mut events = Vec::new();
        if !self.finished {
            self.finished = true;
            events.push(anthropic::StreamEvent::MessageStop {});
        }
        events
    }

    /// Return accumulated usage if any tokens were counted, None otherwise.
    pub fn usage(&self) -> Option<&anthropic::Usage> {
        if self.usage.input_tokens > 0 || self.usage.output_tokens > 0 {
            Some(&self.usage)
        } else {
            None
        }
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
                created: self.created,
            },
        }
    }

    fn handle_tool_call_delta(
        &mut self,
        tc: &openai::ChunkToolCall,
        events: &mut Vec<anthropic::StreamEvent>,
    ) {
        let idx = tc.index as usize;
        if idx > MAX_TOOL_CALL_INDEX {
            tracing::warn!(
                index = idx,
                "tool call index exceeds maximum ({MAX_TOOL_CALL_INDEX}); skipping"
            );
            return;
        }

        // Determine if this chunk starts a new tool call. OpenAI-compliant backends
        // send `id` on the first chunk; local LLMs may omit `id` but include `name`.
        let has_id = tc.id.is_some();
        let has_name = tc.function.as_ref().and_then(|f| f.name.as_ref()).is_some();
        let is_new_tool = has_id || has_name;

        // Bug 4 guard: if the accumulator at this index is already open (not closed),
        // this is a continuation chunk (e.g., local LLM sending id:"" on every chunk),
        // not a genuinely new tool call.
        let already_active = self.active_tool_calls.get(idx).is_some_and(|tc| !tc.closed);

        if is_new_tool && !already_active {
            // Close any open text content block first
            if self.content_block_open {
                events.push(anthropic::StreamEvent::ContentBlockStop {
                    index: self.content_block_index,
                });
                self.content_block_open = false;
                self.content_block_index += 1;
            }

            // Close the previous tool call block before starting a new one.
            // Anthropic streaming protocol requires sequential: Start -> Delta -> Stop per block.
            if let Some(last_tc) = self.active_tool_calls.last_mut() {
                if !last_tc.closed {
                    events.push(anthropic::StreamEvent::ContentBlockStop {
                        index: last_tc.block_index,
                    });
                    last_tc.closed = true;
                }
            }

            let name = tc
                .function
                .as_ref()
                .and_then(|f| f.name.clone())
                .unwrap_or_default();
            // Skip tool calls with empty name (matches non-streaming behavior).
            if name.is_empty() {
                let id_str = tc.id.as_deref().unwrap_or("<none>");
                tracing::warn!(id = %id_str, "streaming tool call has empty function name; skipping");
                return;
            }

            // Local LLMs may send empty or missing tool call ID
            let tool_id = match tc.id.as_deref() {
                Some(id) if !id.is_empty() => id.to_string(),
                _ => {
                    let synthetic = crate::util::ids::generate_tool_use_id();
                    tracing::warn!(
                        synthetic_id = synthetic,
                        "streaming tool call had empty/missing ID; generated synthetic toolu_ ID"
                    );
                    synthetic
                }
            };

            // OpenAI indexes tool calls within a single chunk (0, 1, 2...);
            // Anthropic uses sequential content block indices across the
            // entire message. Merge the two index spaces by offsetting.
            let block_index = self.content_block_index + idx as u32;

            events.push(anthropic::StreamEvent::ContentBlockStart {
                index: block_index,
                content_block: anthropic::ContentBlock::ToolUse {
                    id: tool_id,
                    name: name.clone(),
                    input: serde_json::Value::Object(serde_json::Map::new()),
                },
            });

            // Grow the accumulator vec to fit this index. OpenAI chunks may
            // report tool calls out of order, so we pre-fill with defaults
            // to avoid index-out-of-bounds, then overwrite at [idx].
            while self.active_tool_calls.len() <= idx {
                self.active_tool_calls.push(ToolCallAccumulator {
                    block_index: 0,
                    closed: true, // Padding: never opened, so must not emit ContentBlockStop
                });
            }
            self.active_tool_calls[idx] = ToolCallAccumulator {
                block_index,
                closed: false,
            };
        }

        // Emit argument fragments as input_json_delta events
        if let Some(ref func) = tc.function {
            if let Some(ref args) = func.arguments {
                if idx < self.active_tool_calls.len() {
                    let block_index = self.active_tool_calls[idx].block_index;
                    events.push(anthropic::StreamEvent::ContentBlockDelta {
                        index: block_index,
                        delta: anthropic::Delta::InputJsonDelta {
                            partial_json: args.clone(),
                        },
                    });
                }
            }
        }
    }

    fn flush_tool_calls(&mut self, events: &mut Vec<anthropic::StreamEvent>) {
        for tc in self.active_tool_calls.drain(..) {
            if !tc.closed {
                events.push(anthropic::StreamEvent::ContentBlockStop {
                    index: tc.block_index,
                });
            }
        }
    }
}

/// Map OpenAI finish_reason to Anthropic stop_reason.
///
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/object>
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub fn map_finish_reason(reason: &openai::FinishReason) -> anthropic::StopReason {
    match reason {
        openai::FinishReason::Stop => anthropic::StopReason::EndTurn,
        openai::FinishReason::Length => anthropic::StopReason::MaxTokens,
        openai::FinishReason::ToolCalls => anthropic::StopReason::ToolUse,
        // Anthropic has no content_filter stop reason; EndTurn is the
        // closest approximation. Refusal text is already surfaced via
        // the refusal handling path above.
        openai::FinishReason::ContentFilter => anthropic::StopReason::EndTurn,
        openai::FinishReason::FunctionCall => anthropic::StopReason::ToolUse,
        // Provider-specific reasons (e.g. DeepSeek "insufficient_system_resource")
        openai::FinishReason::Unknown => anthropic::StopReason::EndTurn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::streaming::*;

    /// Helper: build a ChatCompletionChunk with text content.
    fn text_chunk(id: &str, model: &str, text: &str) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: id.into(),
            object: "chat.completion.chunk".into(),
            model: model.into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: Some(text.into()),
                    refusal: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
            created: None,
            system_fingerprint: None,
        }
    }

    /// Helper: build a chunk with only a role delta (first chunk from OpenAI).
    fn role_chunk(id: &str, model: &str) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: id.into(),
            object: "chat.completion.chunk".into(),
            model: model.into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: Some(crate::openai::ChatRole::Assistant),
                    content: None,
                    refusal: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
            created: None,
            system_fingerprint: None,
        }
    }

    /// Helper: build a chunk with finish_reason.
    fn finish_chunk(
        id: &str,
        model: &str,
        reason: crate::openai::FinishReason,
    ) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: id.into(),
            object: "chat.completion.chunk".into(),
            model: model.into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta::default(),
                finish_reason: Some(reason),
                logprobs: None,
            }],
            usage: None,
            created: None,
            system_fingerprint: None,
        }
    }

    /// Helper: build a chunk with usage info (no choices).
    fn usage_chunk(id: &str, model: &str, prompt: u32, completion: u32) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: id.into(),
            object: "chat.completion.chunk".into(),
            model: model.into(),
            choices: vec![],
            usage: Some(crate::openai::ChatUsage {
                prompt_tokens: prompt,
                completion_tokens: completion,
                total_tokens: prompt + completion,
                completion_tokens_details: None,
                prompt_tokens_details: None,
            }),
            created: None,
            system_fingerprint: None,
        }
    }

    /// Helper: build a chunk with a tool call delta.
    fn tool_call_chunk(
        id_str: &str,
        model: &str,
        tc_index: u32,
        tc_id: Option<&str>,
        name: Option<&str>,
        args: Option<&str>,
    ) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: id_str.into(),
            object: "chat.completion.chunk".into(),
            model: model.into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: None,
                    refusal: None,
                    tool_calls: Some(vec![ChunkToolCall {
                        index: tc_index,
                        id: tc_id.map(Into::into),
                        call_type: tc_id.map(|_| "function".into()),
                        function: Some(ChunkFunctionCall {
                            name: name.map(Into::into),
                            arguments: args.map(Into::into),
                        }),
                    }]),
                    reasoning_content: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
            created: None,
            system_fingerprint: None,
        }
    }

    #[test]
    fn first_chunk_emits_message_start() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());
        let chunk = role_chunk("chatcmpl-1", "gpt-4o");
        let events = translator.process_chunk(&chunk);

        assert_eq!(events.len(), 1);
        match &events[0] {
            anthropic::StreamEvent::MessageStart { message } => {
                assert!(message.id.starts_with("msg_"));
                assert_eq!(message.model, "gpt-4o");
                assert_eq!(message.role, "assistant");
                assert!(message.content.is_empty());
                assert!(message.stop_reason.is_none());
            }
            other => panic!("expected MessageStart, got {:?}", other),
        }
    }

    #[test]
    fn text_chunks_emit_block_start_and_deltas() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());

        // First text chunk: should emit message_start + content_block_start + delta
        let events = translator.process_chunk(&text_chunk("c1", "gpt-4o", "Hello"));
        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[0],
            anthropic::StreamEvent::MessageStart { .. }
        ));
        assert!(matches!(
            &events[1],
            anthropic::StreamEvent::ContentBlockStart { index: 0, .. }
        ));
        match &events[2] {
            anthropic::StreamEvent::ContentBlockDelta {
                index: 0,
                delta: anthropic::Delta::TextDelta { text },
            } => assert_eq!(text, "Hello"),
            other => panic!("expected TextDelta, got {:?}", other),
        }

        // Second text chunk: only delta (no message_start, no block_start)
        let events = translator.process_chunk(&text_chunk("c1", "gpt-4o", " world"));
        assert_eq!(events.len(), 1);
        match &events[0] {
            anthropic::StreamEvent::ContentBlockDelta {
                index: 0,
                delta: anthropic::Delta::TextDelta { text },
            } => assert_eq!(text, " world"),
            other => panic!("expected TextDelta, got {:?}", other),
        }
    }

    #[test]
    fn finish_reason_stop_emits_block_stop_and_message_delta() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());
        translator.process_chunk(&text_chunk("c1", "gpt-4o", "Hi"));

        let events =
            translator.process_chunk(&finish_chunk("c1", "gpt-4o", openai::FinishReason::Stop));

        // Should emit: ContentBlockStop, MessageDelta
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            anthropic::StreamEvent::ContentBlockStop { index: 0 }
        ));
        match &events[1] {
            anthropic::StreamEvent::MessageDelta { delta, usage } => {
                assert_eq!(delta.stop_reason, Some(anthropic::StopReason::EndTurn));
                assert!(usage.is_some());
            }
            other => panic!("expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn finish_emits_message_stop() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());
        translator.process_chunk(&text_chunk("c1", "gpt-4o", "Hi"));

        let events = translator.finish();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], anthropic::StreamEvent::MessageStop {}));

        // Calling finish again should produce nothing
        let events = translator.finish();
        assert!(events.is_empty());
    }

    #[test]
    fn usage_chunk_updates_token_counts() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());
        translator.process_chunk(&text_chunk("c1", "gpt-4o", "Hi"));
        translator.process_chunk(&usage_chunk("c1", "gpt-4o", 10, 5));

        let events =
            translator.process_chunk(&finish_chunk("c1", "gpt-4o", openai::FinishReason::Stop));

        // The MessageDelta should carry the usage from the usage chunk
        let msg_delta = events
            .iter()
            .find(|e| matches!(e, anthropic::StreamEvent::MessageDelta { .. }));
        match msg_delta {
            Some(anthropic::StreamEvent::MessageDelta { usage, .. }) => {
                let u = usage.as_ref().unwrap();
                assert_eq!(u.output_tokens, 5);
            }
            other => panic!("expected MessageDelta with usage, got {:?}", other),
        }
    }

    #[test]
    fn tool_call_chunks_emit_tool_use_events() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());
        translator.process_chunk(&role_chunk("c1", "gpt-4o"));

        // First tool call chunk: has id + name + partial args
        let events = translator.process_chunk(&tool_call_chunk(
            "c1",
            "gpt-4o",
            0,
            Some("call_abc"),
            Some("get_weather"),
            Some("{\"loc"),
        ));

        // Should emit ContentBlockStart (tool_use) + ContentBlockDelta (input_json_delta)
        assert_eq!(events.len(), 2);
        match &events[0] {
            anthropic::StreamEvent::ContentBlockStart {
                index: 0,
                content_block,
            } => match content_block {
                anthropic::ContentBlock::ToolUse { id, name, .. } => {
                    assert_eq!(id, "call_abc");
                    assert_eq!(name, "get_weather");
                }
                other => panic!("expected ToolUse content block, got {:?}", other),
            },
            other => panic!("expected ContentBlockStart, got {:?}", other),
        }
        match &events[1] {
            anthropic::StreamEvent::ContentBlockDelta {
                index: 0,
                delta: anthropic::Delta::InputJsonDelta { partial_json },
            } => assert_eq!(partial_json, "{\"loc"),
            other => panic!("expected InputJsonDelta, got {:?}", other),
        }

        // Continuation chunk: more args
        let events = translator.process_chunk(&tool_call_chunk(
            "c1",
            "gpt-4o",
            0,
            None,
            None,
            Some("ation\": \"NYC\"}"),
        ));
        assert_eq!(events.len(), 1);
        match &events[0] {
            anthropic::StreamEvent::ContentBlockDelta {
                index: 0,
                delta: anthropic::Delta::InputJsonDelta { partial_json },
            } => assert_eq!(partial_json, "ation\": \"NYC\"}"),
            other => panic!("expected InputJsonDelta, got {:?}", other),
        }
    }

    #[test]
    fn tool_call_finish_flushes_and_emits_stop() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());
        translator.process_chunk(&role_chunk("c1", "gpt-4o"));
        translator.process_chunk(&tool_call_chunk(
            "c1",
            "gpt-4o",
            0,
            Some("call_abc"),
            Some("get_weather"),
            Some("{\"location\": \"NYC\"}"),
        ));

        let events = translator.process_chunk(&finish_chunk(
            "c1",
            "gpt-4o",
            openai::FinishReason::ToolCalls,
        ));

        // Should emit: ContentBlockStop (for tool call), MessageDelta
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            anthropic::StreamEvent::ContentBlockStop { index: 0 }
        ));
        match &events[1] {
            anthropic::StreamEvent::MessageDelta { delta, .. } => {
                assert_eq!(delta.stop_reason, Some(anthropic::StopReason::ToolUse));
            }
            other => panic!("expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn text_then_tool_call_closes_text_block() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());

        // Text content first
        translator.process_chunk(&text_chunk("c1", "gpt-4o", "Let me check"));

        // Then a tool call arrives: should close text block first
        let events = translator.process_chunk(&tool_call_chunk(
            "c1",
            "gpt-4o",
            0,
            Some("call_xyz"),
            Some("search"),
            Some("{}"),
        ));

        // ContentBlockStop (text, index 0), ContentBlockStart (tool, index 1), ContentBlockDelta
        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[0],
            anthropic::StreamEvent::ContentBlockStop { index: 0 }
        ));
        match &events[1] {
            anthropic::StreamEvent::ContentBlockStart {
                index: 1,
                content_block: anthropic::ContentBlock::ToolUse { id, .. },
            } => assert_eq!(id, "call_xyz"),
            other => panic!(
                "expected ContentBlockStart for tool_use at index 1, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn empty_choices_chunk_only_emits_message_start() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());
        let chunk = ChatCompletionChunk {
            id: "c1".into(),
            object: "chat.completion.chunk".into(),
            model: "gpt-4o".into(),
            choices: vec![],
            usage: None,
            created: None,
            system_fingerprint: None,
        };
        let events = translator.process_chunk(&chunk);
        // Only message_start on first call
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            anthropic::StreamEvent::MessageStart { .. }
        ));

        // Subsequent empty chunk: no events
        let events = translator.process_chunk(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn map_finish_reason_length() {
        assert_eq!(
            map_finish_reason(&openai::FinishReason::Length),
            anthropic::StopReason::MaxTokens
        );
    }

    #[test]
    fn map_finish_reason_content_filter() {
        // Content filter maps to EndTurn (best approximation)
        assert_eq!(
            map_finish_reason(&openai::FinishReason::ContentFilter),
            anthropic::StopReason::EndTurn
        );
    }

    #[test]
    fn full_text_stream_sequence() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());

        // Simulate a complete text streaming sequence
        let mut all_events = Vec::new();
        all_events.extend(translator.process_chunk(&role_chunk("c1", "gpt-4o")));
        all_events.extend(translator.process_chunk(&text_chunk("c1", "gpt-4o", "Hello")));
        all_events.extend(translator.process_chunk(&text_chunk("c1", "gpt-4o", " world")));
        all_events.extend(translator.process_chunk(&usage_chunk("c1", "gpt-4o", 10, 5)));
        all_events.extend(translator.process_chunk(&finish_chunk(
            "c1",
            "gpt-4o",
            openai::FinishReason::Stop,
        )));
        all_events.extend(translator.finish());

        // Verify event sequence: MessageStart, ContentBlockStart, TextDelta, TextDelta,
        //   ContentBlockStop, MessageDelta, MessageStop
        let types: Vec<&str> = all_events
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
            .collect();

        assert_eq!(
            types,
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

    // --- Local LLM robustness ---

    #[test]
    fn streaming_tool_call_empty_id_gets_synthetic() {
        let mut translator = StreamingTranslator::new("llama".into());
        translator.process_chunk(&role_chunk("c1", "llama"));

        // First tool call chunk with empty string ID
        let events = translator.process_chunk(&tool_call_chunk(
            "c1",
            "llama",
            0,
            Some(""), // empty ID from local LLM
            Some("Read"),
            Some("{\"file"),
        ));

        assert_eq!(events.len(), 2);
        match &events[0] {
            anthropic::StreamEvent::ContentBlockStart {
                content_block: anthropic::ContentBlock::ToolUse { id, name, .. },
                ..
            } => {
                assert!(
                    id.starts_with("toolu_"),
                    "expected synthetic toolu_ ID, got: {}",
                    id
                );
                assert_eq!(name, "Read");
            }
            other => panic!("expected ContentBlockStart with ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn streaming_tool_call_empty_name_skipped() {
        let mut translator = StreamingTranslator::new("llama".into());
        translator.process_chunk(&role_chunk("c1", "llama"));

        // Tool call chunk with no name should be skipped (consistent with non-streaming).
        let events = translator.process_chunk(&tool_call_chunk(
            "c1",
            "llama",
            0,
            Some("call_1"),
            None, // no name from local LLM
            Some("{}"),
        ));

        // Empty name causes early return -- no ContentBlockStart emitted.
        assert!(
            events.iter().all(|e| !matches!(
                e,
                anthropic::StreamEvent::ContentBlockStart {
                    content_block: anthropic::ContentBlock::ToolUse { .. },
                    ..
                }
            )),
            "tool call with empty name should be skipped, got: {:?}",
            events
        );
    }

    #[test]
    fn streaming_tool_call_none_id_with_name_gets_synthetic() {
        // Bug 3: local LLMs may omit id entirely but provide name
        let mut translator = StreamingTranslator::new("llama".into());
        translator.process_chunk(&role_chunk("c1", "llama"));

        // First chunk: id is None, but name is present
        let events = translator.process_chunk(&tool_call_chunk(
            "c1",
            "llama",
            0,
            None, // no ID at all
            Some("get_weather"),
            Some("{\"loc"),
        ));

        assert_eq!(
            events.len(),
            2,
            "expected ContentBlockStart + ContentBlockDelta"
        );
        match &events[0] {
            anthropic::StreamEvent::ContentBlockStart {
                content_block: anthropic::ContentBlock::ToolUse { id, name, .. },
                ..
            } => {
                assert!(
                    id.starts_with("toolu_"),
                    "expected synthetic toolu_ ID, got: {}",
                    id
                );
                assert_eq!(name, "get_weather");
            }
            other => panic!("expected ContentBlockStart with ToolUse, got {:?}", other),
        }

        // Second chunk: continuation with more arguments (no id, no name)
        let events2 = translator.process_chunk(&tool_call_chunk(
            "c1",
            "llama",
            0,
            None,
            None,
            Some("ation\"}"),
        ));

        assert_eq!(
            events2.len(),
            1,
            "expected only ContentBlockDelta for continuation"
        );
        assert!(matches!(
            &events2[0],
            anthropic::StreamEvent::ContentBlockDelta {
                delta: anthropic::Delta::InputJsonDelta { partial_json },
                ..
            } if partial_json == "ation\"}"
        ));
    }

    #[test]
    fn streaming_tool_call_repeated_empty_id_not_corrupted() {
        // Bug 4: backend sends id:"" on every chunk of the same tool call;
        // only the first chunk should open a new block.
        let mut translator = StreamingTranslator::new("llama".into());
        translator.process_chunk(&role_chunk("c1", "llama"));

        // First chunk with empty id + name: opens a new tool block
        let events1 = translator.process_chunk(&tool_call_chunk(
            "c1",
            "llama",
            0,
            Some(""),
            Some("Read"),
            Some("{\"f"),
        ));
        assert_eq!(
            events1.len(),
            2,
            "expected ContentBlockStart + ContentBlockDelta"
        );
        let first_id = match &events1[0] {
            anthropic::StreamEvent::ContentBlockStart {
                content_block: anthropic::ContentBlock::ToolUse { id, .. },
                ..
            } => id.clone(),
            other => panic!("expected ContentBlockStart, got {:?}", other),
        };

        // Second chunk with empty id again: should NOT open a new block
        let events2 = translator.process_chunk(&tool_call_chunk(
            "c1",
            "llama",
            0,
            Some(""),
            None,
            Some("ile\"}"),
        ));

        // Should only have the argument delta, no new ContentBlockStart
        assert_eq!(
            events2.len(),
            1,
            "repeated empty id should not re-open block"
        );
        assert!(
            matches!(
                &events2[0],
                anthropic::StreamEvent::ContentBlockDelta { .. }
            ),
            "expected ContentBlockDelta, got {:?}",
            events2[0]
        );

        // Verify no second synthetic ID was generated (only one ContentBlockStart total)
        let all_starts: Vec<_> = events1
            .iter()
            .chain(events2.iter())
            .filter(|e| matches!(e, anthropic::StreamEvent::ContentBlockStart { .. }))
            .collect();
        assert_eq!(
            all_starts.len(),
            1,
            "should have exactly one ContentBlockStart, got {}",
            all_starts.len()
        );
        // The synthetic ID from the first chunk should be used
        assert!(first_id.starts_with("toolu_"));
    }

    #[test]
    fn streaming_refusal_emits_text_delta() {
        let mut translator = StreamingTranslator::new("gpt-4o".into());
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-1".into(),
            object: "chat.completion.chunk".into(),
            model: "gpt-4o".into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: None,
                    refusal: Some("content policy violation".into()),
                    tool_calls: None,
                    reasoning_content: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
            created: None,
            system_fingerprint: None,
        };
        let events = translator.process_chunk(&chunk);
        // message_start + content_block_start + content_block_delta
        assert!(
            events.len() >= 3,
            "expected at least 3 events, got {}",
            events.len()
        );
        match &events[events.len() - 1] {
            anthropic::StreamEvent::ContentBlockDelta {
                delta: anthropic::Delta::TextDelta { text },
                ..
            } => {
                assert!(
                    text.contains("Refusal"),
                    "expected refusal text, got: {}",
                    text
                );
                assert!(text.contains("content policy violation"));
            }
            other => panic!("expected TextDelta with refusal, got {:?}", other),
        }
    }

    /// Helper: build a chunk with reasoning_content (DeepSeek/Qwen thinking).
    fn reasoning_chunk(id: &str, model: &str, reasoning: &str) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: id.into(),
            object: "chat.completion.chunk".into(),
            model: model.into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: None,
                    refusal: None,
                    tool_calls: None,
                    reasoning_content: Some(reasoning.into()),
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
            created: None,
            system_fingerprint: None,
        }
    }

    #[test]
    fn reasoning_content_emits_thinking_block() {
        let mut translator = StreamingTranslator::new("deepseek-reasoner".into());

        // First reasoning chunk should open a thinking block
        let events =
            translator.process_chunk(&reasoning_chunk("c1", "deepseek-reasoner", "Let me"));
        assert_eq!(events.len(), 3); // message_start + content_block_start + thinking_delta

        match &events[1] {
            anthropic::StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                assert_eq!(*index, 0);
                assert!(matches!(
                    content_block,
                    anthropic::ContentBlock::Thinking { .. }
                ));
            }
            other => panic!("expected ContentBlockStart, got {:?}", other),
        }
        match &events[2] {
            anthropic::StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(*index, 0);
                match delta {
                    anthropic::Delta::ThinkingDelta { thinking } => {
                        assert_eq!(thinking, "Let me");
                    }
                    other => panic!("expected ThinkingDelta, got {:?}", other),
                }
            }
            other => panic!("expected ContentBlockDelta, got {:?}", other),
        }

        // Second reasoning chunk continues the thinking block
        let events =
            translator.process_chunk(&reasoning_chunk("c1", "deepseek-reasoner", " think..."));
        assert_eq!(events.len(), 1); // just a thinking delta
        match &events[0] {
            anthropic::StreamEvent::ContentBlockDelta { delta, .. } => {
                assert!(
                    matches!(delta, anthropic::Delta::ThinkingDelta { thinking } if thinking == " think...")
                );
            }
            other => panic!("expected ThinkingDelta, got {:?}", other),
        }

        // Text chunk should close thinking block and open text block
        let events = translator.process_chunk(&text_chunk("c1", "deepseek-reasoner", "Answer: 4"));
        assert_eq!(events.len(), 3); // content_block_stop (thinking) + content_block_start (text) + text_delta

        assert!(
            matches!(&events[0], anthropic::StreamEvent::ContentBlockStop { index } if *index == 0)
        );
        assert!(matches!(
            &events[1],
            anthropic::StreamEvent::ContentBlockStart { index: 1, .. }
        ));
        match &events[2] {
            anthropic::StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(*index, 1);
                assert!(
                    matches!(delta, anthropic::Delta::TextDelta { text } if text == "Answer: 4")
                );
            }
            other => panic!("expected TextDelta, got {:?}", other),
        }

        // Finish
        let events = translator.process_chunk(&finish_chunk(
            "c1",
            "deepseek-reasoner",
            openai::FinishReason::Stop,
        ));
        // content_block_stop (text) + message_delta
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn reasoning_only_without_text_content() {
        // Some thinking models may return only reasoning_content with no text content
        let mut translator = StreamingTranslator::new("deepseek-reasoner".into());
        translator.process_chunk(&reasoning_chunk("c1", "deepseek-reasoner", "Thinking..."));

        let events = translator.process_chunk(&finish_chunk(
            "c1",
            "deepseek-reasoner",
            openai::FinishReason::Stop,
        ));
        // Should close thinking block + message_delta
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], anthropic::StreamEvent::ContentBlockStop { index } if *index == 0)
        );
    }
}
