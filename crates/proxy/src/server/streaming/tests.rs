use super::*;

#[test]
fn anthropic_stream_usage_tracks_complete_sse_frames() {
    let mut usage = AnthropicStreamUsage::default();
    let mut buffer = BytesMut::from(&b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-haiku-4-5\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":11,\"output_tokens\":0}}}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":7}}\n\n"[..]);
    let mut search_from = 0;

    observe_anthropic_sse_frames(&mut buffer, &mut search_from, &mut usage);

    assert_eq!(usage.tokens(), Some((11, 7)));
    assert!(buffer.is_empty());
}
