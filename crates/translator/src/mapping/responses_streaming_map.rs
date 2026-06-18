// Phase 22: Responses API streaming state machine
//
// Converts OpenAI Responses API SSE events into Anthropic SSE events.
// The Responses API emits typed events (response.created, response.output_text.delta,
// response.completed, etc.) rather than partial JSON chunks like Chat Completions.
// There is no [DONE] sentinel; the stream ends with response.completed.

use crate::anthropic;
use crate::util;
use serde::{Deserialize, Serialize};

/// A single SSE event from the Responses API streaming endpoint.
///
/// The `type` field identifies the event kind. Additional fields vary by event type.
/// We keep the structure flexible with a flattened map for the varying fields.
///
/// OpenAI Responses streaming: <https://platform.openai.com/docs/api-reference/responses-streaming>
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ResponsesStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    /// All other fields, varying by event type.
    #[serde(flatten)]
    pub data: serde_json::Map<String, serde_json::Value>,
}

/// State machine that converts Responses API streaming events into Anthropic SSE events.
///
/// Feed events via `process_event`, then call `finish` after the stream ends.
/// Each call returns zero or more Anthropic SSE events to forward to the client.
pub struct ResponsesStreamingTranslator {
    model: String,
    message_id: String,
    started: bool,
    content_block_index: u32,
    content_block_open: bool,
    /// Number of currently open function call items (used to gate content_part behavior).
    tool_call_depth: u32,
    usage: anthropic::Usage,
    finished: bool,
}

impl ResponsesStreamingTranslator {
    /// Create a new translator for the given model name.
    /// Generates a fresh Anthropic message ID for the translated stream.
    pub fn new(model: String) -> Self {
        Self {
            model,
            message_id: util::ids::generate_message_id(),
            started: false,
            content_block_index: 0,
            content_block_open: false,
            tool_call_depth: 0,
            usage: anthropic::Usage::default(),
            finished: false,
        }
    }

    /// Process one Responses API streaming event.
    /// Returns zero or more Anthropic SSE events.
    pub fn process_event(&mut self, event: &ResponsesStreamEvent) -> Vec<anthropic::StreamEvent> {
        if self.finished {
            return Vec::new();
        }

        match event.event_type.as_str() {
            "response.created" => self.handle_created(),
            "response.output_item.added" => self.handle_output_item_added(event),
            "response.content_part.added" => self.handle_content_part_added(event),
            "response.output_text.delta" => self.handle_text_delta(event),
            "response.output_text.done" => Vec::new(), // We already streamed the text via deltas
            "response.content_part.done" => self.handle_content_part_done(),
            "response.output_item.done" => self.handle_output_item_done(event),
            "response.function_call_arguments.delta" => self.handle_function_call_delta(event),
            "response.function_call_arguments.done" => Vec::new(), // Handled in output_item.done
            "response.completed" => self.handle_completed(event),
            "response.failed" | "response.cancelled" => self.handle_error(event),
            _ => {
                tracing::debug!(
                    event_type = event.event_type,
                    "unhandled Responses API streaming event"
                );
                Vec::new()
            }
        }
    }

    /// Call after all events have been processed (stream ended without response.completed).
    pub fn finish(&mut self) -> Vec<anthropic::StreamEvent> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;

        let mut events = Vec::new();

        // Close any open content block
        if self.content_block_open {
            events.push(anthropic::StreamEvent::ContentBlockStop {
                index: self.content_block_index,
            });
            self.content_block_open = false;
        }

        // Emit message_delta with stop_reason
        events.push(anthropic::StreamEvent::MessageDelta {
            delta: anthropic::streaming::MessageDeltaData {
                stop_reason: Some(anthropic::StopReason::EndTurn),
                stop_sequence: None,
                ..Default::default()
            },
            usage: Some(anthropic::streaming::DeltaUsage {
                output_tokens: self.usage.output_tokens,
            }),
        });

        events.push(anthropic::StreamEvent::MessageStop {});
        events
    }

    fn ensure_started(&mut self) -> Vec<anthropic::StreamEvent> {
        if self.started {
            return Vec::new();
        }
        self.started = true;
        vec![self.make_message_start()]
    }

    fn handle_created(&mut self) -> Vec<anthropic::StreamEvent> {
        self.ensure_started()
    }

    fn handle_output_item_added(
        &mut self,
        event: &ResponsesStreamEvent,
    ) -> Vec<anthropic::StreamEvent> {
        let mut events = self.ensure_started();

        // Check if this is a function_call item
        if let Some(item) = event.data.get("item") {
            if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                // Close any open text block first
                if self.content_block_open {
                    events.push(anthropic::StreamEvent::ContentBlockStop {
                        index: self.content_block_index,
                    });
                    self.content_block_open = false;
                    self.content_block_index += 1;
                }

                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");

                let id = if call_id.is_empty() {
                    util::ids::generate_tool_use_id()
                } else {
                    call_id.to_string()
                };

                events.push(anthropic::StreamEvent::ContentBlockStart {
                    index: self.content_block_index,
                    content_block: anthropic::ContentBlock::ToolUse {
                        id,
                        name: name.to_string(),
                        input: serde_json::json!({}),
                    },
                });
                self.content_block_open = true;
                self.tool_call_depth += 1;
            }
        }

        events
    }

    fn handle_content_part_added(
        &mut self,
        _event: &ResponsesStreamEvent,
    ) -> Vec<anthropic::StreamEvent> {
        let mut events = self.ensure_started();

        // Close previous block if open (text block ending, new one starting)
        if self.content_block_open && self.tool_call_depth == 0 {
            events.push(anthropic::StreamEvent::ContentBlockStop {
                index: self.content_block_index,
            });
            self.content_block_open = false;
            self.content_block_index += 1;
        } else if !self.content_block_open {
            // First content part
        }

        events.push(anthropic::StreamEvent::ContentBlockStart {
            index: self.content_block_index,
            content_block: anthropic::ContentBlock::Text {
                text: String::new(),
            },
        });
        self.content_block_open = true;

        events
    }

    fn handle_text_delta(&mut self, event: &ResponsesStreamEvent) -> Vec<anthropic::StreamEvent> {
        let delta = event
            .data
            .get("delta")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if delta.is_empty() {
            return Vec::new();
        }

        vec![anthropic::StreamEvent::ContentBlockDelta {
            index: self.content_block_index,
            delta: anthropic::Delta::TextDelta {
                text: delta.to_string(),
            },
        }]
    }

    fn handle_content_part_done(&mut self) -> Vec<anthropic::StreamEvent> {
        if !self.content_block_open || self.tool_call_depth > 0 {
            return Vec::new();
        }

        self.content_block_open = false;
        let events = vec![anthropic::StreamEvent::ContentBlockStop {
            index: self.content_block_index,
        }];
        self.content_block_index += 1;
        events
    }

    fn handle_output_item_done(
        &mut self,
        event: &ResponsesStreamEvent,
    ) -> Vec<anthropic::StreamEvent> {
        let mut events = Vec::new();

        // If this was a function_call item, close its content block
        if let Some(item) = event.data.get("item") {
            if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                if self.content_block_open {
                    events.push(anthropic::StreamEvent::ContentBlockStop {
                        index: self.content_block_index,
                    });
                    self.content_block_open = false;
                    self.content_block_index += 1;
                }
                self.tool_call_depth = self.tool_call_depth.saturating_sub(1);
            }
        }

        events
    }

    fn handle_function_call_delta(
        &mut self,
        event: &ResponsesStreamEvent,
    ) -> Vec<anthropic::StreamEvent> {
        let delta = event
            .data
            .get("delta")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if delta.is_empty() {
            return Vec::new();
        }

        vec![anthropic::StreamEvent::ContentBlockDelta {
            index: self.content_block_index,
            delta: anthropic::Delta::InputJsonDelta {
                partial_json: delta.to_string(),
            },
        }]
    }

    fn handle_completed(&mut self, event: &ResponsesStreamEvent) -> Vec<anthropic::StreamEvent> {
        self.finished = true;

        let mut events = self.ensure_started();

        // Close any open content block
        if self.content_block_open {
            events.push(anthropic::StreamEvent::ContentBlockStop {
                index: self.content_block_index,
            });
            self.content_block_open = false;
        }

        // Extract usage from the completed response
        if let Some(response) = event.data.get("response") {
            if let Some(usage) = response.get("usage") {
                self.usage.input_tokens = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                self.usage.output_tokens = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
            }
        }

        // Determine stop reason
        let stop_reason = if let Some(response) = event.data.get("response") {
            match response.get("status").and_then(|v| v.as_str()) {
                Some("incomplete") => anthropic::StopReason::MaxTokens,
                _ => anthropic::StopReason::EndTurn,
            }
        } else {
            anthropic::StopReason::EndTurn
        };

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

        events.push(anthropic::StreamEvent::MessageStop {});
        events
    }

    fn handle_error(&mut self, event: &ResponsesStreamEvent) -> Vec<anthropic::StreamEvent> {
        self.finished = true; // Error is terminal; prevent finish() from emitting closure events
        let message = event
            .data
            .get("response")
            .and_then(|r| r.get("status_details"))
            .and_then(|d| d.get("error"))
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");

        vec![anthropic::StreamEvent::Error {
            error: anthropic::streaming::StreamError {
                error_type: "api_error".to_string(),
                message: message.to_string(),
            },
        }]
    }

    /// Return accumulated usage if any tokens were counted, None otherwise.
    /// Only populated after a `response.completed` event has been processed.
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
                content: Vec::new(),
                model: self.model.clone(),
                stop_reason: None,
                stop_sequence: None,
                usage: self.usage.clone(),
                created: None,
            },
        }
    }
}

#[cfg(test)]
mod tests;
