use super::eventstream;
use bytes::BytesMut;

/// Build a minimal AWS Event Stream frame with the given payload.
/// Emits real CRC32 checksums so the decoder's validation passes.
fn build_frame(headers: &[u8], payload: &[u8]) -> Vec<u8> {
    let total_len = (12 + headers.len() + payload.len() + 4) as u32;
    let headers_len = headers.len() as u32;
    let mut frame: Vec<u8> = Vec::with_capacity(total_len as usize);
    frame.extend_from_slice(&total_len.to_be_bytes());
    frame.extend_from_slice(&headers_len.to_be_bytes());
    // Prelude CRC: CRC32 of bytes 0-7.
    let prelude_crc = crc32fast::hash(&frame[..8]);
    frame.extend_from_slice(&prelude_crc.to_be_bytes());
    frame.extend_from_slice(headers);
    frame.extend_from_slice(payload);
    // Message CRC: CRC32 of everything so far.
    let msg_crc = crc32fast::hash(&frame);
    frame.extend_from_slice(&msg_crc.to_be_bytes());
    frame
}

#[test]
fn decode_frame_empty_payload() {
    let frame = build_frame(&[], &[]);
    let mut buf = BytesMut::from(frame.as_slice());
    let payload = eventstream::decode_frame(&mut buf).unwrap().unwrap();
    assert!(payload.is_empty());
    assert!(buf.is_empty());
}

#[test]
fn decode_frame_with_payload() {
    let payload_data = b"hello world";
    let frame = build_frame(&[], payload_data);
    let mut buf = BytesMut::from(frame.as_slice());
    let payload = eventstream::decode_frame(&mut buf).unwrap().unwrap();
    assert_eq!(payload, b"hello world");
    assert!(buf.is_empty());
}

#[test]
fn decode_frame_incomplete() {
    let frame = build_frame(&[], b"hello");
    let mut buf = BytesMut::from(&frame[..frame.len() - 2]); // truncate
    assert!(eventstream::decode_frame(&mut buf).unwrap().is_none());
}

#[test]
fn decode_multiple_frames() {
    let frame1 = build_frame(&[], b"first");
    let frame2 = build_frame(&[], b"second");
    let mut buf = BytesMut::new();
    buf.extend_from_slice(&frame1);
    buf.extend_from_slice(&frame2);

    let p1 = eventstream::decode_frame(&mut buf).unwrap().unwrap();
    assert_eq!(p1, b"first");
    let p2 = eventstream::decode_frame(&mut buf).unwrap().unwrap();
    assert_eq!(p2, b"second");
    assert!(buf.is_empty());
}

#[test]
fn decode_frame_with_headers() {
    let headers = b"\x00\x04test";
    let payload_data = b"data";
    let frame = build_frame(headers, payload_data);
    let mut buf = BytesMut::from(frame.as_slice());
    let payload = eventstream::decode_frame(&mut buf).unwrap().unwrap();
    assert_eq!(payload, b"data");
}

#[test]
fn decode_frame_rejects_bad_prelude_crc() {
    let payload = b"{}";
    let mut frame = build_frame(b"", payload);
    frame[8] ^= 0xFF; // corrupt prelude CRC
    let mut buf = BytesMut::from(frame.as_slice());
    let result = eventstream::decode_frame(&mut buf);
    assert!(result.is_err(), "bad prelude CRC must be rejected");
}

#[test]
fn decode_frame_prelude_crc_failure_does_not_advance_buffer() {
    // total_len comes from the first 4 bytes of the frame, which are covered
    // by the prelude CRC. If the prelude CRC fails, total_len is untrustworthy
    // and must NOT be used to advance the buffer. The caller closes the connection.
    let payload = b"{}";
    let mut frame = build_frame(b"", payload);
    let original_len = frame.len();
    frame[8] ^= 0xFF; // corrupt prelude CRC byte
    let mut buf = BytesMut::from(frame.as_slice());
    let result = eventstream::decode_frame(&mut buf);
    assert!(result.is_err());
    assert_eq!(
        buf.len(),
        original_len,
        "buffer must not be consumed when prelude CRC fails (total_len is untrustworthy)"
    );
}

#[test]
fn decode_frame_rejects_bad_message_crc() {
    let payload = b"{}";
    let mut frame = build_frame(b"", payload);
    let last = frame.len() - 1;
    frame[last] ^= 0xFF; // corrupt message CRC
    let mut buf = BytesMut::from(frame.as_slice());
    let result = eventstream::decode_frame(&mut buf);
    assert!(result.is_err(), "bad message CRC must be rejected");
}

#[test]
fn decode_frame_accepts_valid_crc() {
    let payload = b"{}";
    let frame = build_frame(b"", payload);
    let mut buf = BytesMut::from(frame.as_slice());
    let result = eventstream::decode_frame(&mut buf);
    assert!(result.is_ok(), "valid CRC must be accepted");
    assert!(result.unwrap().is_some());
}

#[test]
fn extract_event_from_valid_payload() {
    use base64::Engine;
    let event_json = r#"{"type":"content_block_delta","index":0}"#;
    let b64 = base64::engine::general_purpose::STANDARD.encode(event_json);
    let wrapper = format!(r#"{{"bytes":"{b64}"}}"#);
    let result = eventstream::extract_event_from_payload(wrapper.as_bytes());
    assert_eq!(result.unwrap(), event_json);
}

#[test]
fn extract_event_empty_payload() {
    assert!(eventstream::extract_event_from_payload(&[]).is_none());
}

#[test]
fn extract_event_invalid_json() {
    assert!(eventstream::extract_event_from_payload(b"not json").is_none());
}

#[test]
fn extract_event_missing_bytes_field() {
    let payload = r#"{"other":"field"}"#;
    assert!(eventstream::extract_event_from_payload(payload.as_bytes()).is_none());
}
