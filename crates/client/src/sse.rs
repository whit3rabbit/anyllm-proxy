//! Framework-agnostic SSE frame parser.
//!
//! Reads raw bytes from a `reqwest::Response` stream, splits on SSE frame
//! boundaries (`\n\n` or `\r\n\r\n`), and delivers each `data:` line to a
//! caller-supplied callback. No dependency on axum or any web framework.

use bytes::BytesMut;

/// Maximum SSE buffer size (10 MB). Protects against unbounded memory growth
/// if the backend sends data without frame delimiters.
pub const MAX_SSE_BUFFER_SIZE: usize = 10 * 1024 * 1024;

/// Errors from SSE stream parsing.
#[derive(Debug, thiserror::Error)]
pub enum SseError {
    #[error("stream read error: {0}")]
    ReadError(#[from] reqwest::Error),
    #[error("SSE buffer exceeded maximum size ({MAX_SSE_BUFFER_SIZE} bytes)")]
    BufferOverflow,
}

/// Find the first SSE frame boundary (`\n\n` or `\r\n\r\n`) in a byte slice,
/// starting the search at `start`. Returns `(position, delimiter_length)` so
/// the caller can skip the full delimiter.
pub fn find_double_newline(buf: &[u8], start: usize) -> Option<(usize, usize)> {
    let len = buf.len();
    let mut i = start;
    while i < len.saturating_sub(1) {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some((i, 2));
        }
        if buf[i] == b'\r'
            && i + 3 < len
            && buf[i + 1] == b'\n'
            && buf[i + 2] == b'\r'
            && buf[i + 3] == b'\n'
        {
            return Some((i, 4));
        }
        i += 1;
    }
    None
}

/// Read SSE frames from a response stream, calling `on_data` for each `data:` line.
///
/// Returns `Ok(())` on normal stream completion, or an `SseError` on failure.
/// The `on_data` callback receives the JSON string after `data: ` and returns
/// an optional list of translated events. The `on_events` callback is called
/// with each batch of events from a complete SSE frame.
///
/// This is the framework-agnostic core of SSE parsing. It does not depend on
/// axum, tokio channels, or any specific event type.
pub async fn read_sse_stream<T, F, G>(
    response: reqwest::Response,
    mut on_data: F,
    mut on_events: G,
) -> Result<(), SseError>
where
    F: FnMut(&str) -> Option<Vec<T>>,
    G: FnMut(&[T]) -> bool, // returns false if consumer disconnected
{
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    // BytesMut (not String) because TCP chunks may split mid-UTF-8 character.
    let mut buffer = BytesMut::new();
    let mut frame_events: Vec<T> = Vec::new();
    let mut search_from: usize = 0;

    while let Some(chunk_result) = stream.next().await {
        let bytes = chunk_result?;
        buffer.extend_from_slice(&bytes);

        if buffer.len() > MAX_SSE_BUFFER_SIZE {
            return Err(SseError::BufferOverflow);
        }

        while let Some((pos, delim_len)) = find_double_newline(&buffer, search_from) {
            frame_events.clear();
            match std::str::from_utf8(&buffer[..pos]) {
                Ok(frame_str) => {
                    for line in frame_str.lines() {
                        let line = line.trim();
                        if let Some(json_str) = line.strip_prefix("data: ") {
                            if let Some(mut events) = on_data(json_str) {
                                frame_events.append(&mut events);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("skipping non-UTF-8 SSE frame: {e}");
                }
            }
            let _ = buffer.split_to(pos + delim_len);
            search_from = 0;

            if !on_events(&frame_events) {
                return Ok(()); // consumer disconnected
            }
        }
        // Next chunk: resume scanning 3 bytes back from the end. The 4-byte
        // delimiter \r\n\r\n could straddle the chunk boundary.
        search_from = buffer.len().saturating_sub(3);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_double_newline_lf() {
        let buf = b"data: hello\n\ndata: world\n\n";
        let (pos, len) = find_double_newline(buf, 0).unwrap();
        assert_eq!(pos, 11);
        assert_eq!(len, 2);
    }

    #[test]
    fn find_double_newline_crlf() {
        let buf = b"data: hello\r\n\r\ndata: world\r\n\r\n";
        let (pos, len) = find_double_newline(buf, 0).unwrap();
        assert_eq!(pos, 11);
        assert_eq!(len, 4);
    }

    #[test]
    fn find_double_newline_from_offset() {
        let buf = b"data: hello\n\ndata: world\n\n";
        let (pos, len) = find_double_newline(buf, 13).unwrap();
        assert_eq!(pos, 24);
        assert_eq!(len, 2);
    }

    #[test]
    fn find_double_newline_none() {
        let buf = b"data: hello\n";
        assert!(find_double_newline(buf, 0).is_none());
    }

    #[test]
    fn find_double_newline_empty() {
        assert!(find_double_newline(b"", 0).is_none());
    }

    #[test]
    fn find_double_newline_single_newline() {
        assert!(find_double_newline(b"\n", 0).is_none());
    }

    #[test]
    fn find_double_newline_just_delimiter() {
        let (pos, len) = find_double_newline(b"\n\n", 0).unwrap();
        assert_eq!(pos, 0);
        assert_eq!(len, 2);
    }
}
