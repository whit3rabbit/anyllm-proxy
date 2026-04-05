// Reverse streaming: Anthropic SSE events -> OpenAI ChatCompletionChunk SSE
//
// Consumes Anthropic StreamEvent items and emits OpenAI ChatCompletionChunk
// objects. This is the inverse of StreamingTranslator in streaming_map.rs.

use crate::anthropic;
use crate::mapping::reverse_message_map::anthropic_stop_reason_to_openai;
use crate::openai;
use crate::openai::streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall,
};

/// Sentinel value returned by `process_event` to signal the stream is done.
/// The caller should emit `data: [DONE]\n\n` when it sees this.
pub const DONE_SENTINEL: &str = "[DONE]";

/// State machine that converts Anthropic SSE events into OpenAI ChatCompletionChunk objects.
///
/// Feed events via `process_event`, which returns zero or more chunks to send.
/// When `message_stop` is received, `is_done()` returns true and the caller
/// should emit `data: [DONE]\n\n`.
pub struct ReverseStreamingTranslator {
    message_id: String,
    model: String,
    tool_call_index: i32,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
    created: u64,
    done: bool,
}

impl ReverseStreamingTranslator {
    pub fn new(id: String, model: String) -> Self {
        Self {
            message_id: id,
            model,
            tool_call_index: -1,
            input_tokens: None,
            output_tokens: None,
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            done: false,
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Process a single Anthropic StreamEvent and return zero or more OpenAI chunks.
    pub fn process_event(&mut self, event: &anthropic::StreamEvent) -> Vec<ChatCompletionChunk> {
        match event {
            anthropic::StreamEvent::MessageStart { message } => {
                self.input_tokens = Some(message.usage.input_tokens);
                if let Some(created) = message.created {
                    self.created = created;
                }
                // Emit first chunk with role
                vec![self.make_chunk(
                    ChunkDelta {
                        role: Some(openai::ChatRole::Assistant),
                        ..Default::default()
                    },
                    None,
                )]
            }
            anthropic::StreamEvent::ContentBlockStart { content_block, .. } => {
                match content_block {
                    anthropic::ContentBlock::ToolUse { id, name, .. } => {
                        self.tool_call_index += 1;
                        let tc = ChunkToolCall {
                            index: self.tool_call_index as u32,
                            id: Some(id.clone()),
                            call_type: Some("function".to_string()),
                            function: Some(ChunkFunctionCall {
                                name: Some(name.clone()),
                                arguments: Some(String::new()),
                            }),
                        };
                        vec![self.make_chunk(
                            ChunkDelta {
                                tool_calls: Some(vec![tc]),
                                ..Default::default()
                            },
                            None,
                        )]
                    }
                    // Text and Thinking blocks emit their content via deltas
                    _ => vec![],
                }
            }
            anthropic::StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                anthropic::streaming::Delta::TextDelta { text } => {
                    vec![self.make_chunk(
                        ChunkDelta {
                            content: Some(text.clone()),
                            ..Default::default()
                        },
                        None,
                    )]
                }
                anthropic::streaming::Delta::InputJsonDelta { partial_json } => {
                    if self.tool_call_index < 0 {
                        return vec![];
                    }
                    let tc = ChunkToolCall {
                        index: self.tool_call_index as u32,
                        id: None,
                        call_type: None,
                        function: Some(ChunkFunctionCall {
                            name: None,
                            arguments: Some(partial_json.clone()),
                        }),
                    };
                    vec![self.make_chunk(
                        ChunkDelta {
                            tool_calls: Some(vec![tc]),
                            ..Default::default()
                        },
                        None,
                    )]
                }
                anthropic::streaming::Delta::ThinkingDelta { thinking } => {
                    vec![self.make_chunk(
                        ChunkDelta {
                            reasoning_content: Some(thinking.clone()),
                            ..Default::default()
                        },
                        None,
                    )]
                }
                anthropic::streaming::Delta::SignatureDelta { .. } => vec![],
            },
            anthropic::StreamEvent::ContentBlockStop { .. } => vec![],
            anthropic::StreamEvent::MessageDelta { delta, usage } => {
                if let Some(u) = usage {
                    self.output_tokens = Some(u.output_tokens);
                }
                let finish_reason = delta
                    .stop_reason
                    .as_ref()
                    .map(anthropic_stop_reason_to_openai);
                let mut chunks = vec![self.make_chunk(ChunkDelta::default(), finish_reason)];
                // Emit usage chunk if we have token counts
                if let (Some(input), Some(output)) = (self.input_tokens, self.output_tokens) {
                    chunks.push(ChatCompletionChunk {
                        id: self.message_id.clone(),
                        object: "chat.completion.chunk".to_string(),
                        model: self.model.clone(),
                        // OpenAI streaming spec: the usage chunk intentionally has
                        // choices: []. Some community parsers assume choices[0] always
                        // exists; those parsers are non-compliant with the spec.
                        choices: vec![],
                        usage: Some(openai::ChatUsage {
                            prompt_tokens: input,
                            completion_tokens: output,
                            total_tokens: input + output,
                            completion_tokens_details: None,
                            prompt_tokens_details: None,
                        }),
                        created: Some(self.created),
                        system_fingerprint: None,
                    });
                }
                chunks
            }
            anthropic::StreamEvent::MessageStop {} => {
                self.done = true;
                vec![]
            }
            anthropic::StreamEvent::Ping {} => vec![],
            anthropic::StreamEvent::Error { .. } => {
                self.done = true;
                vec![]
            }
        }
    }

    fn make_chunk(
        &self,
        delta: ChunkDelta,
        finish_reason: Option<openai::FinishReason>,
    ) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: self.message_id.clone(),
            object: "chat.completion.chunk".to_string(),
            model: self.model.clone(),
            choices: vec![ChunkChoice {
                index: 0,
                delta,
                finish_reason,
                logprobs: None,
            }],
            usage: None,
            created: Some(self.created),
            system_fingerprint: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anthropic::messages::{ContentBlock, StopReason, Usage};
    use crate::anthropic::streaming::*;

    fn make_translator() -> ReverseStreamingTranslator {
        ReverseStreamingTranslator::new("chatcmpl-test".to_string(), "gpt-4o".to_string())
    }

    #[test]
    fn message_start_emits_role_chunk() {
        let mut t = make_translator();
        let event = StreamEvent::MessageStart {
            message: MessageStartData {
                id: "msg_123".to_string(),
                msg_type: "message".to_string(),
                role: "assistant".to_string(),
                content: vec![],
                model: "claude-sonnet".to_string(),
                stop_reason: None,
                stop_sequence: None,
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
                created: Some(1700000000),
            },
        };
        let chunks = t.process_event(&event);
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0].choices[0].delta.role,
            Some(openai::ChatRole::Assistant)
        );
        assert!(chunks[0].choices[0].finish_reason.is_none());
    }

    #[test]
    fn text_delta_emits_content_chunk() {
        let mut t = make_translator();
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta {
                text: "Hello".to_string(),
            },
        };
        let chunks = t.process_event(&event);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn tool_use_streaming() {
        let mut t = make_translator();
        // Start tool use block
        let start = StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::ToolUse {
                id: "call_123".to_string(),
                name: "get_weather".to_string(),
                input: serde_json::Value::Object(serde_json::Map::new()),
            },
        };
        let chunks = t.process_event(&start);
        assert_eq!(chunks.len(), 1);
        let tc = &chunks[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id.as_deref(), Some("call_123"));
        assert_eq!(
            tc.function.as_ref().unwrap().name.as_deref(),
            Some("get_weather")
        );

        // Delta with args
        let delta = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::InputJsonDelta {
                partial_json: "{\"loc".to_string(),
            },
        };
        let chunks = t.process_event(&delta);
        assert_eq!(chunks.len(), 1);
        let tc = &chunks[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.index, 0);
        assert!(tc.id.is_none()); // Only first chunk has id
        assert_eq!(
            tc.function.as_ref().unwrap().arguments.as_deref(),
            Some("{\"loc")
        );
    }

    #[test]
    fn thinking_delta_emits_reasoning_content() {
        let mut t = make_translator();
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::ThinkingDelta {
                thinking: "Let me think...".to_string(),
            },
        };
        let chunks = t.process_event(&event);
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0].choices[0].delta.reasoning_content.as_deref(),
            Some("Let me think...")
        );
    }

    #[test]
    fn message_delta_emits_finish_reason_and_usage() {
        let mut t = make_translator();
        // Set input tokens via message_start
        let start = StreamEvent::MessageStart {
            message: MessageStartData {
                id: "msg_1".to_string(),
                msg_type: "message".to_string(),
                role: "assistant".to_string(),
                content: vec![],
                model: "claude".to_string(),
                stop_reason: None,
                stop_sequence: None,
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
                created: None,
            },
        };
        t.process_event(&start);

        let event = StreamEvent::MessageDelta {
            delta: MessageDeltaData {
                stop_reason: Some(StopReason::EndTurn),
                stop_sequence: None,
            },
            usage: Some(DeltaUsage { output_tokens: 5 }),
        };
        let chunks = t.process_event(&event);
        assert_eq!(chunks.len(), 2); // finish chunk + usage chunk
        assert_eq!(
            chunks[0].choices[0].finish_reason,
            Some(openai::FinishReason::Stop)
        );
        let usage = chunks[1].usage.as_ref().unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn message_stop_sets_done() {
        let mut t = make_translator();
        assert!(!t.is_done());
        t.process_event(&StreamEvent::MessageStop {});
        assert!(t.is_done());
    }

    #[test]
    fn ping_produces_no_chunks() {
        let mut t = make_translator();
        let chunks = t.process_event(&StreamEvent::Ping {});
        assert!(chunks.is_empty());
    }

    #[test]
    fn multiple_tool_calls_track_index() {
        let mut t = make_translator();
        // First tool
        let start1 = StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "fn_a".to_string(),
                input: serde_json::Value::Object(serde_json::Map::new()),
            },
        };
        let chunks = t.process_event(&start1);
        assert_eq!(
            chunks[0].choices[0].delta.tool_calls.as_ref().unwrap()[0].index,
            0
        );

        // Second tool
        let start2 = StreamEvent::ContentBlockStart {
            index: 1,
            content_block: ContentBlock::ToolUse {
                id: "call_2".to_string(),
                name: "fn_b".to_string(),
                input: serde_json::Value::Object(serde_json::Map::new()),
            },
        };
        let chunks = t.process_event(&start2);
        assert_eq!(
            chunks[0].choices[0].delta.tool_calls.as_ref().unwrap()[0].index,
            1
        );
    }
}
