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

        // Once a terminal error or finish has been emitted the stream is over;
        // drop any trailing chunks the backend may still send.
        if self.finished {
            return events;
        }

        // OpenAI-compatible gateways (notably OpenRouter) cannot change the HTTP
        // status once a 200 SSE stream has started, so a mid-generation failure
        // arrives as a chunk carrying a top-level `error` object. Surface it as an
        // Anthropic error event instead of silently mapping to end_turn.
        if let Some(err) = &chunk.error {
            self.finished = true;
            events.push(anthropic::StreamEvent::Error {
                error: crate::mapping::errors_map::openai_stream_error_to_anthropic(err),
            });
            return events;
        }

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
            self.usage.cache_read_input_tokens = crate::mapping::usage_map::extract_cached_tokens(
                usage.prompt_tokens_details.as_ref(),
            );
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
                // A provider may signal a mid-stream failure via finish_reason
                // "error" without a top-level error object (that object case is
                // handled before any choices are processed). Surface it as an
                // Anthropic error event and stop.
                if matches!(finish_reason, openai::FinishReason::Error) {
                    self.finished = true;
                    events.push(anthropic::StreamEvent::Error {
                        error: anthropic::streaming::StreamError {
                            error_type: "api_error".to_string(),
                            message: "upstream returned finish_reason \"error\"".to_string(),
                        },
                    });
                    return events;
                }
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
                        ..Default::default()
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
        // Mid-stream errors are surfaced as a StreamEvent::Error before this
        // mapping is reached on the streaming path (see `process_chunk`). The
        // non-streaming path catches a finish_reason "error" at the HTTP client
        // boundary (`error_in_finished_choices` in the proxy) and returns an
        // error before this mapping runs, so this arm is a defensive fallback
        // only; EndTurn is the safe default.
        openai::FinishReason::Error => anthropic::StopReason::EndTurn,
        // Provider-specific reasons (e.g. DeepSeek "insufficient_system_resource").
        // Log so the unknown value is visible without breaking callers.
        openai::FinishReason::Unknown => {
            tracing::warn!("unknown OpenAI finish_reason received; treating as end_turn");
            anthropic::StopReason::EndTurn
        }
    }
}

#[cfg(test)]
mod tests;
