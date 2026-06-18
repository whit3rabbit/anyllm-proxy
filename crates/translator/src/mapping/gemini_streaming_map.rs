//! Gemini streaming state machine: full-response diffing -> Anthropic SSE events.
//!
//! Gemini's `streamGenerateContent` sends FULL accumulated `GenerateContentResponse`
//! objects per SSE event (not incremental deltas like OpenAI). This state machine
//! diffs each response against the previous state to produce Anthropic-format
//! delta events.
//!
//! The key challenge: Gemini may retroactively shorten text between chunks when safety
//! filters activate mid-stream. `guard_shrinkage` detects and resets the diff baseline
//! so subsequent deltas remain valid (at the cost of that chunk's content being dropped).

use crate::anthropic;
use crate::anthropic::streaming::{DeltaUsage, MessageStartData};
use crate::gemini::response::{FinishReason, GenerateContentResponse};
use crate::util;

/// Reset `prev_len` to 0 if `current_len` has shrunk, and log a warning.
/// Gemini safety filtering can retroactively truncate accumulated text between
/// chunks; without this reset the subsequent diff would produce no delta,
/// silently losing the remaining content.
fn guard_shrinkage(current_len: usize, prev_len: &mut usize, warn_msg: &str) {
    if current_len < *prev_len {
        tracing::warn!(prev = *prev_len, current = current_len, "{warn_msg}");
        *prev_len = 0;
    }
}

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
    /// Whether a thinking content block is currently open.
    thought_block_open: bool,
    /// Length of thought text already emitted as deltas.
    prev_thought_len: usize,
    usage: anthropic::Usage,
    finished: bool,
}

impl GeminiStreamingTranslator {
    /// Create a new translator for the given model name.
    pub fn new(model: String) -> Self {
        Self {
            model,
            message_id: util::ids::generate_message_id(),
            started: false,
            content_block_index: 0,
            text_block_open: false,
            prev_text_len: 0,
            prev_tool_count: 0,
            thought_block_open: false,
            prev_thought_len: 0,
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

        // Separate thought parts (thinking models) from answer parts.
        let mut current_thought = String::new();
        let mut current_text = String::new();
        for part in &candidate.content.parts {
            if part.thought == Some(true) {
                if let Some(ref t) = part.text {
                    current_thought.push_str(t);
                }
            } else if let Some(ref t) = part.text {
                current_text.push_str(t);
            }
        }

        // Thought delta: diff against what we already emitted.
        // Guard against shrinkage (e.g., Gemini safety filtering retroactively
        // truncates text between chunks). Reset baseline so future diffs stay valid.
        guard_shrinkage(
            current_thought.len(),
            &mut self.prev_thought_len,
            "Gemini thought text shrank — resetting diff baseline",
        );
        if current_thought.len() > self.prev_thought_len {
            if !self.thought_block_open {
                events.push(anthropic::StreamEvent::ContentBlockStart {
                    index: self.content_block_index,
                    content_block: anthropic::ContentBlock::Thinking {
                        thinking: String::new(),
                        signature: None,
                    },
                });
                self.thought_block_open = true;
            }
            let delta_thought = &current_thought[self.prev_thought_len..];
            events.push(anthropic::StreamEvent::ContentBlockDelta {
                index: self.content_block_index,
                delta: anthropic::streaming::Delta::ThinkingDelta {
                    thinking: delta_thought.to_string(),
                },
            });
            self.prev_thought_len = current_thought.len();
        }

        // Text delta: diff against what we already emitted.
        // Guard against shrinkage (safety filtering may truncate cumulative text).
        guard_shrinkage(
            current_text.len(),
            &mut self.prev_text_len,
            "Gemini text shrank — possible safety truncation; resetting diff baseline",
        );
        if current_text.len() > self.prev_text_len {
            // Close the thought block before opening the text block.
            if self.thought_block_open {
                events.push(anthropic::StreamEvent::ContentBlockStop {
                    index: self.content_block_index,
                });
                self.thought_block_open = false;
                // Increment so the following text block gets the next index.
                // Thought and text blocks occupy distinct slots per Anthropic SSE spec.
                self.content_block_index += 1;
            }
            if !self.text_block_open {
                events.push(anthropic::StreamEvent::ContentBlockStart {
                    index: self.content_block_index,
                    content_block: anthropic::ContentBlock::Text {
                        text: String::new(),
                    },
                });
                self.text_block_open = true;
            }
            // floor_char_boundary clamps to a valid char boundary in case
            // Gemini adjusts text between chunks; a naive byte slice would
            // panic if prev_text_len lands mid-codepoint.
            let safe_start =
                current_text.floor_char_boundary(self.prev_text_len.min(current_text.len()));
            let delta_text = &current_text[safe_start..];
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

        // Close any open thought block
        if self.thought_block_open {
            events.push(anthropic::StreamEvent::ContentBlockStop {
                index: self.content_block_index,
            });
            self.thought_block_open = false;
            self.content_block_index += 1;
        }

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
                ..Default::default()
            },
            usage: Some(DeltaUsage {
                output_tokens: self.usage.output_tokens,
            }),
        });
        events.push(anthropic::StreamEvent::MessageStop {});
        self.finished = true;
        events
    }

    /// Returns true after `finish_reason` has been seen. Caller should stop feeding responses.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Return accumulated usage if Gemini sent token metadata, None otherwise.
    pub fn usage(&self) -> Option<&anthropic::Usage> {
        if self.usage.input_tokens > 0 || self.usage.output_tokens > 0 {
            Some(&self.usage)
        } else {
            None
        }
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

        // Close any open thought block. Can happen if Gemini sends a thought-only
        // response with no answer text and no tool calls (finishReason arrives while
        // thought_block_open is still true).
        if self.thought_block_open {
            events.push(anthropic::StreamEvent::ContentBlockStop {
                index: self.content_block_index,
            });
            self.thought_block_open = false;
            self.content_block_index += 1;
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
                ..Default::default()
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
mod tests;
