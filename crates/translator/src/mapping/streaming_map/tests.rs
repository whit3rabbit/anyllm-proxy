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
        error: None,
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
        error: None,
    }
}

/// Helper: build a chunk with finish_reason.
fn finish_chunk(id: &str, model: &str, reason: crate::openai::FinishReason) -> ChatCompletionChunk {
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
        error: None,
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
        error: None,
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
        error: None,
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
        error: None,
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
            _ => "unknown",
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
        error: None,
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
        error: None,
    }
}

#[test]
fn reasoning_content_emits_thinking_block() {
    let mut translator = StreamingTranslator::new("deepseek-reasoner".into());

    // First reasoning chunk should open a thinking block
    let events = translator.process_chunk(&reasoning_chunk("c1", "deepseek-reasoner", "Let me"));
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
    let events = translator.process_chunk(&reasoning_chunk("c1", "deepseek-reasoner", " think..."));
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
            assert!(matches!(delta, anthropic::Delta::TextDelta { text } if text == "Answer: 4"));
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

#[test]
fn usage_chunk_with_cached_tokens_maps_cache_read() {
    let mut translator = StreamingTranslator::new("gpt-4o".into());
    let chunk = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        model: "gpt-4o".into(),
        choices: vec![],
        usage: Some(crate::openai::ChatUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            completion_tokens_details: None,
            prompt_tokens_details: Some(serde_json::json!({"cached_tokens": 42})),
        }),
        created: None,
        system_fingerprint: None,
        error: None,
    };
    translator.process_chunk(&chunk);
    let usage = translator.usage().expect("usage should be present");
    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.output_tokens, 50);
    assert_eq!(usage.cache_read_input_tokens, Some(42));
}

#[test]
fn usage_chunk_without_cached_tokens_leaves_cache_read_none() {
    let mut translator = StreamingTranslator::new("gpt-4o".into());
    translator.process_chunk(&usage_chunk("c1", "gpt-4o", 10, 5));
    let usage = translator.usage().expect("usage should be present");
    assert!(usage.cache_read_input_tokens.is_none());
}

// Boundary: idx == MAX_TOOL_CALL_INDEX (128) uses `>` not `>=`, so 128 is accepted.
#[test]
fn tool_call_at_max_index_is_accepted() {
    let mut translator = StreamingTranslator::new("gpt-4o".into());
    translator.process_chunk(&role_chunk("c1", "gpt-4o"));

    // idx == 128: 128 > 128 is false, so the chunk is processed.
    let events = translator.process_chunk(&tool_call_chunk(
        "c1",
        "gpt-4o",
        MAX_TOOL_CALL_INDEX as u32,
        Some("call_at_max"),
        Some("my_tool"),
        Some("{}"),
    ));

    // Must emit at least a ContentBlockStart for the tool call.
    let has_tool_start = events.iter().any(|e| {
        matches!(
            e,
            anthropic::StreamEvent::ContentBlockStart {
                content_block: anthropic::ContentBlock::ToolUse { .. },
                ..
            }
        )
    });
    assert!(
        has_tool_start,
        "expected ContentBlockStart for tool_use at index {MAX_TOOL_CALL_INDEX}, got {events:?}"
    );
}

// Boundary: idx == MAX_TOOL_CALL_INDEX + 1 (129) satisfies `>`, so it is dropped.
#[test]
fn tool_call_above_max_index_is_dropped() {
    let mut translator = StreamingTranslator::new("gpt-4o".into());
    translator.process_chunk(&role_chunk("c1", "gpt-4o"));

    // idx == 129: 129 > 128 is true, so the chunk is silently skipped.
    let events = translator.process_chunk(&tool_call_chunk(
        "c1",
        "gpt-4o",
        (MAX_TOOL_CALL_INDEX + 1) as u32,
        Some("call_over_max"),
        Some("bad_tool"),
        Some("{}"),
    ));

    // No tool-related events should be emitted for this chunk.
    let has_tool_event = events.iter().any(|e| {
        matches!(
            e,
            anthropic::StreamEvent::ContentBlockStart {
                content_block: anthropic::ContentBlock::ToolUse { .. },
                ..
            } | anthropic::StreamEvent::ContentBlockDelta {
                delta: anthropic::Delta::InputJsonDelta { .. },
                ..
            }
        )
    });
    assert!(
        !has_tool_event,
        "expected no tool events for index > {MAX_TOOL_CALL_INDEX}, got {events:?}"
    );
}

// --- Mid-stream error handling ---

#[test]
fn midstream_error_object_emits_error_event() {
    let mut translator = StreamingTranslator::new("gpt-4o".into());
    translator.process_chunk(&text_chunk("c1", "gpt-4o", "partial"));

    // OpenRouter-style mid-stream error chunk: top-level error + finish_reason "error".
    let chunk = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta::default(),
            finish_reason: Some(crate::openai::FinishReason::Error),
            logprobs: None,
        }],
        usage: None,
        created: None,
        system_fingerprint: None,
        error: Some(crate::openai::streaming::ChunkError {
            code: Some(serde_json::Value::Number(429.into())),
            message: Some("rate limited".into()),
            metadata: None,
        }),
    };
    let events = translator.process_chunk(&chunk);
    assert_eq!(
        events.len(),
        1,
        "expected exactly one error event: {events:?}"
    );
    match &events[0] {
        anthropic::StreamEvent::Error { error } => {
            // Numeric code 429 maps to the rate_limit_error wire string.
            assert_eq!(error.error_type, "rate_limit_error");
            assert_eq!(error.message, "rate limited");
        }
        other => panic!("expected StreamEvent::Error, got {other:?}"),
    }

    // finish() must not emit a trailing message_stop after a terminal error.
    assert!(translator.finish().is_empty());

    // Any further chunks are dropped once finished.
    let after = translator.process_chunk(&text_chunk("c1", "gpt-4o", "more"));
    assert!(
        after.is_empty(),
        "post-error chunk should be dropped: {after:?}"
    );
}

#[test]
fn finish_reason_error_without_object_emits_error_event() {
    let mut translator = StreamingTranslator::new("gpt-4o".into());
    translator.process_chunk(&text_chunk("c1", "gpt-4o", "partial"));

    // Provider sends finish_reason "error" with no top-level error object.
    let events = translator.process_chunk(&finish_chunk(
        "c1",
        "gpt-4o",
        crate::openai::FinishReason::Error,
    ));
    assert_eq!(events.len(), 1, "expected one error event: {events:?}");
    match &events[0] {
        anthropic::StreamEvent::Error { error } => {
            assert_eq!(error.error_type, "api_error");
            assert!(error.message.contains("finish_reason"));
        }
        other => panic!("expected StreamEvent::Error, got {other:?}"),
    }
    assert!(translator.finish().is_empty());
}
