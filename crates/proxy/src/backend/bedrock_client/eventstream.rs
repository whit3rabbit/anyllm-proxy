use bytes::BytesMut;

/// Minimum frame size: 4 (total_len) + 4 (headers_len) + 4 (prelude CRC)
///                     + 0 (headers) + 0 (payload) + 4 (message CRC) = 16
const MIN_FRAME_SIZE: usize = 16;

/// Try to extract one complete event stream frame from the buffer.
/// Returns `Some(payload_bytes)` and advances the buffer past the frame,
/// or `None` if the buffer does not contain a complete frame yet.
/// Returns `Err` if CRC validation fails (corrupted frame).
pub fn decode_frame(buf: &mut BytesMut) -> Result<Option<Vec<u8>>, String> {
    if buf.len() < MIN_FRAME_SIZE {
        return Ok(None);
    }

    let total_len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if buf.len() < total_len {
        return Ok(None); // incomplete frame
    }

    let headers_len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;

    // Validate prelude CRC (bytes 8-11 = CRC32 of bytes 0-7).
    let prelude_crc_stored = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
    let prelude_crc_computed = crc32fast::hash(&buf[..8]);
    if prelude_crc_stored != prelude_crc_computed {
        // Do NOT advance the buffer: total_len came from the same bytes that
        // failed the CRC check and is therefore untrustworthy. Splitting by
        // a corrupted length would permanently misalign the frame decoder.
        // Return the error directly so the caller closes the connection.
        return Err(format!(
            "event stream prelude CRC mismatch: stored={prelude_crc_stored:#010x} computed={prelude_crc_computed:#010x}"
        ));
    }

    // Validate message CRC (last 4 bytes = CRC32 of frame minus final 4 bytes).
    let msg_crc_offset = total_len - 4;
    let msg_crc_stored = u32::from_be_bytes([
        buf[msg_crc_offset],
        buf[msg_crc_offset + 1],
        buf[msg_crc_offset + 2],
        buf[msg_crc_offset + 3],
    ]);
    let msg_crc_computed = crc32fast::hash(&buf[..msg_crc_offset]);
    if msg_crc_stored != msg_crc_computed {
        let _ = buf.split_to(total_len);
        return Err(format!(
            "event stream message CRC mismatch: stored={msg_crc_stored:#010x} computed={msg_crc_computed:#010x}"
        ));
    }

    // Prelude is 8 bytes (total_len + headers_len), then 4-byte prelude CRC
    let headers_start = 12; // 4 + 4 + 4 (prelude CRC)
    let payload_start = headers_start + headers_len;
    // Message CRC is the last 4 bytes
    let payload_end = total_len.saturating_sub(4);

    if payload_start > payload_end || payload_end > buf.len() {
        // Malformed frame: skip it
        let _ = buf.split_to(total_len);
        return Ok(Some(Vec::new()));
    }

    let payload = buf[payload_start..payload_end].to_vec();

    // Advance buffer past this frame
    let _ = buf.split_to(total_len);

    Ok(Some(payload))
}

/// Extract the Anthropic event JSON string from a Bedrock event stream payload.
/// Bedrock wraps the Anthropic event in `{"bytes":"<base64>"}`.
/// Returns None if the payload is not a chunk event or is malformed.
pub fn extract_event_from_payload(payload: &[u8]) -> Option<String> {
    if payload.is_empty() {
        return None;
    }

    // Parse as JSON to extract the base64-encoded bytes field
    let parsed: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let b64 = parsed.get("bytes")?.as_str()?;

    // Base64 decode
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    String::from_utf8(decoded).ok()
}
