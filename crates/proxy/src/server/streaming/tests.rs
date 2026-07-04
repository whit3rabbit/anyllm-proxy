use super::*;

#[test]
fn anthropic_stream_usage_tracks_complete_sse_frames() {
    let mut usage = AnthropicStreamUsage::default();
    let mut buffer = BytesMut::from(&b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-haiku-4-5\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":11,\"output_tokens\":0}}}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":7}}\n\n"[..]);
    let mut search_from = 0;
    let mut ready = Vec::new();

    observe_anthropic_sse_frames(&mut buffer, &mut search_from, &mut usage, None, &mut ready);

    assert_eq!(usage.tokens(), Some((11, 7)));
    assert!(buffer.is_empty());
    assert!(ready.is_empty(), "no recorder attached -> nothing recorded");
}

#[test]
fn observe_anthropic_sse_frames_records_thinking_blocks_when_recorder_attached() {
    let mut usage = AnthropicStreamUsage::default();
    let mut buffer = BytesMut::from(&b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-haiku-4-5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hmm\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig_1\"}}\n\n\
data: {\"type\":\"message_stop\"}\n\n"[..]);
    let mut search_from = 0;
    let mut recorder = crate::thinking_repair::ThinkingRecorder::new();
    let mut ready = Vec::new();

    observe_anthropic_sse_frames(
        &mut buffer,
        &mut search_from,
        &mut usage,
        Some(&mut recorder),
        &mut ready,
    );

    assert_eq!(ready.len(), 1);
    let (id, blocks) = &ready[0];
    assert_eq!(id, "msg_1");
    assert!(matches!(
        &blocks[0],
        anthropic::ContentBlock::Thinking { thinking, signature }
            if thinking == "hmm" && signature.as_deref() == Some("sig_1")
    ));
}
