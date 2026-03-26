// AWS Bedrock client with SigV4 request signing.
// Sends Anthropic Messages API requests directly to Bedrock (no OpenAI translation).
// Bedrock streaming uses AWS Event Stream binary framing, not SSE.

use super::{build_http_client, RateLimitHeaders};
use crate::config::TlsConfig;
use aws_credential_types::Credentials;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
use reqwest::Client;
use tokio::time::sleep;

/// HTTP client for AWS Bedrock with SigV4 request signing.
#[derive(Clone)]
pub struct BedrockClient {
    client: Client,
    region: String,
    credentials: Credentials,
    big_model: String,
    small_model: String,
}

/// Error type for the Bedrock client.
#[derive(Debug)]
pub enum BedrockClientError {
    /// Transport-level error (connection, timeout, DNS).
    Transport(String),
    /// Upstream returned a non-success status. Body is raw bytes for passthrough.
    ApiError { status: u16, body: bytes::Bytes },
    /// SigV4 signing failed.
    Signing(String),
}

impl std::fmt::Display for BedrockClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(msg) => write!(f, "Bedrock transport error: {msg}"),
            Self::ApiError { status, .. } => write!(f, "Bedrock API error (status {status})"),
            Self::Signing(msg) => write!(f, "Bedrock signing error: {msg}"),
        }
    }
}

impl BedrockClient {
    /// Create a new Bedrock client.
    pub fn new(
        region: String,
        credentials: Credentials,
        big_model: String,
        small_model: String,
        tls: &TlsConfig,
    ) -> Self {
        let client = build_http_client(tls);
        Self {
            client,
            region,
            credentials,
            big_model,
            small_model,
        }
    }

    pub fn big_model(&self) -> &str {
        &self.big_model
    }

    pub fn small_model(&self) -> &str {
        &self.small_model
    }

    /// Build the Bedrock InvokeModel URL for a given model.
    fn invoke_url(&self, model_id: &str) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/invoke",
            self.region, model_id
        )
    }

    /// Build the Bedrock InvokeModelWithResponseStream URL.
    fn invoke_stream_url(&self, model_id: &str) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/invoke-with-response-stream",
            self.region, model_id
        )
    }

    /// Sign an HTTP request with SigV4 and return headers to add.
    fn sign_request(
        &self,
        method: &str,
        url: &str,
        body_bytes: &[u8],
        extra_headers: &[(&str, &str)],
    ) -> Result<Vec<(String, String)>, BedrockClientError> {
        let identity: aws_smithy_runtime_api::client::identity::Identity =
            self.credentials.clone().into();
        let settings = SigningSettings::default();
        let params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&self.region)
            .name("bedrock")
            .time(std::time::SystemTime::now())
            .settings(settings)
            .build()
            .map_err(|e| BedrockClientError::Signing(e.to_string()))?;
        let signing_params = params.into();

        let signable = SignableRequest::new(
            method,
            url,
            extra_headers.iter().copied(),
            SignableBody::Bytes(body_bytes),
        )
        .map_err(|e| BedrockClientError::Signing(e.to_string()))?;

        let (instructions, _signature) = sign(signable, &signing_params)
            .map_err(|e| BedrockClientError::Signing(e.to_string()))?
            .into_parts();

        // Collect signing headers
        let headers: Vec<(String, String)> = instructions
            .headers()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Ok(headers)
    }

    /// Forward a non-streaming request. Returns raw response body and rate limit headers.
    pub async fn forward(
        &self,
        body: bytes::Bytes,
        model_id: &str,
    ) -> Result<(bytes::Bytes, RateLimitHeaders), BedrockClientError> {
        let response = self.send_with_retry(body, model_id, false).await?;
        let rate_limits = RateLimitHeaders::default();
        let resp_body = response
            .bytes()
            .await
            .map_err(|e| BedrockClientError::Transport(e.to_string()))?;
        Ok((resp_body, rate_limits))
    }

    /// Forward a streaming request. Returns the raw response for event stream decoding.
    pub async fn forward_stream(
        &self,
        body: bytes::Bytes,
        model_id: &str,
    ) -> Result<(reqwest::Response, RateLimitHeaders), BedrockClientError> {
        let response = self.send_with_retry(body, model_id, true).await?;
        let rate_limits = RateLimitHeaders::default();
        Ok((response, rate_limits))
    }

    /// Send with retry on 429/5xx.
    async fn send_with_retry(
        &self,
        body: bytes::Bytes,
        model_id: &str,
        stream: bool,
    ) -> Result<reqwest::Response, BedrockClientError> {
        let url = if stream {
            self.invoke_stream_url(model_id)
        } else {
            self.invoke_url(model_id)
        };

        let content_type = "application/json";
        let accept = if stream {
            "application/vnd.amazon.eventstream"
        } else {
            "application/json"
        };

        for attempt in 0..=super::MAX_RETRIES {
            let base_headers = [("content-type", content_type), ("accept", accept)];
            let signing_headers = self.sign_request("POST", &url, &body, &base_headers)?;

            let mut rb = self
                .client
                .post(&url)
                .header("content-type", content_type)
                .header("accept", accept)
                .body(body.clone());

            for (k, v) in &signing_headers {
                rb = rb.header(k.as_str(), v.as_str());
            }

            let response = rb
                .send()
                .await
                .map_err(|e| BedrockClientError::Transport(e.to_string()))?;
            let status = response.status().as_u16();

            if (200..300).contains(&status) {
                return Ok(response);
            }

            if attempt < super::MAX_RETRIES && super::is_retryable(status) {
                let retry_after = super::parse_retry_after(response.headers());
                let delay = super::backoff_delay(attempt, retry_after);
                tracing::warn!(
                    status,
                    attempt = attempt + 1,
                    max_retries = super::MAX_RETRIES,
                    delay_ms = delay.as_millis() as u64,
                    "retryable error from Bedrock, backing off"
                );
                drop(response.bytes().await);
                sleep(delay).await;
                continue;
            }

            let resp_body = response.bytes().await.unwrap_or_default();
            return Err(BedrockClientError::ApiError {
                status,
                body: resp_body,
            });
        }
        unreachable!("loop runs MAX_RETRIES+1 times and always returns")
    }
}

// ---------------------------------------------------------------------------
// AWS Event Stream binary frame decoder
// ---------------------------------------------------------------------------

/// Decode AWS Event Stream frames from a byte buffer.
/// Each frame: 4-byte total_len | 4-byte headers_len | 4-byte prelude CRC |
///             headers | payload | 4-byte message CRC
///
/// The payload contains `{"bytes":"<base64>"}` where base64 decodes to an
/// Anthropic SSE JSON event string.
pub mod eventstream {
    use bytes::BytesMut;

    /// Minimum frame size: 4 (total_len) + 4 (headers_len) + 4 (prelude CRC)
    ///                     + 0 (headers) + 0 (payload) + 4 (message CRC) = 16
    const MIN_FRAME_SIZE: usize = 16;

    /// Try to extract one complete event stream frame from the buffer.
    /// Returns `Some(payload_bytes)` and advances the buffer past the frame,
    /// or `None` if the buffer does not contain a complete frame yet.
    pub fn decode_frame(buf: &mut BytesMut) -> Option<Vec<u8>> {
        if buf.len() < MIN_FRAME_SIZE {
            return None;
        }

        let total_len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if buf.len() < total_len {
            return None; // incomplete frame
        }

        let headers_len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;

        // Prelude is 8 bytes (total_len + headers_len), then 4-byte prelude CRC
        let headers_start = 12; // 4 + 4 + 4 (prelude CRC)
        let payload_start = headers_start + headers_len;
        // Message CRC is the last 4 bytes
        let payload_end = total_len.saturating_sub(4);

        if payload_start > payload_end || payload_end > total_len {
            // Malformed frame: skip it
            let _ = buf.split_to(total_len);
            return Some(Vec::new());
        }

        let payload = buf[payload_start..payload_end].to_vec();

        // Advance buffer past this frame
        let _ = buf.split_to(total_len);

        Some(payload)
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
}

#[cfg(test)]
mod tests {
    use super::eventstream;
    use bytes::BytesMut;

    /// Build a minimal AWS Event Stream frame with the given payload.
    /// Uses zero CRCs (we don't validate CRCs in the decoder).
    fn build_frame(headers: &[u8], payload: &[u8]) -> Vec<u8> {
        let total_len = 12 + headers.len() + payload.len() + 4; // prelude(12) + headers + payload + msg CRC(4)
        let headers_len = headers.len();
        let mut frame = Vec::with_capacity(total_len);
        frame.extend_from_slice(&(total_len as u32).to_be_bytes());
        frame.extend_from_slice(&(headers_len as u32).to_be_bytes());
        frame.extend_from_slice(&[0u8; 4]); // prelude CRC (not validated)
        frame.extend_from_slice(headers);
        frame.extend_from_slice(payload);
        frame.extend_from_slice(&[0u8; 4]); // message CRC (not validated)
        frame
    }

    #[test]
    fn decode_frame_empty_payload() {
        let frame = build_frame(&[], &[]);
        let mut buf = BytesMut::from(frame.as_slice());
        let payload = eventstream::decode_frame(&mut buf).unwrap();
        assert!(payload.is_empty());
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_frame_with_payload() {
        let payload_data = b"hello world";
        let frame = build_frame(&[], payload_data);
        let mut buf = BytesMut::from(frame.as_slice());
        let payload = eventstream::decode_frame(&mut buf).unwrap();
        assert_eq!(payload, b"hello world");
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_frame_incomplete() {
        let frame = build_frame(&[], b"hello");
        let mut buf = BytesMut::from(&frame[..frame.len() - 2]); // truncate
        assert!(eventstream::decode_frame(&mut buf).is_none());
    }

    #[test]
    fn decode_multiple_frames() {
        let frame1 = build_frame(&[], b"first");
        let frame2 = build_frame(&[], b"second");
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&frame1);
        buf.extend_from_slice(&frame2);

        let p1 = eventstream::decode_frame(&mut buf).unwrap();
        assert_eq!(p1, b"first");
        let p2 = eventstream::decode_frame(&mut buf).unwrap();
        assert_eq!(p2, b"second");
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_frame_with_headers() {
        let headers = b"\x00\x04test";
        let payload_data = b"data";
        let frame = build_frame(headers, payload_data);
        let mut buf = BytesMut::from(frame.as_slice());
        let payload = eventstream::decode_frame(&mut buf).unwrap();
        assert_eq!(payload, b"data");
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
}
