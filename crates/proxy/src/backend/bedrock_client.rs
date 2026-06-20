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
use zeroize::Zeroizing;

/// HTTP client for AWS Bedrock with SigV4 request signing.
/// Secret fields (secret_access_key, session_token) are wrapped in `Zeroizing`
/// so they are zeroed from memory when the client is dropped.
#[derive(Clone)]
pub struct BedrockClient {
    client: Client,
    region: String,
    access_key_id: String,
    secret_access_key: Zeroizing<String>,
    session_token: Option<Zeroizing<String>>,
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
    /// Create a new Bedrock client. Decomposes `Credentials` so that secret
    /// fields are stored in `Zeroizing<String>` and wiped on drop.
    pub fn new(
        region: String,
        credentials: Credentials,
        big_model: String,
        small_model: String,
        tls: &TlsConfig,
    ) -> Self {
        let client = build_http_client(tls);
        let access_key_id = credentials.access_key_id().to_string();
        let secret_access_key = Zeroizing::new(credentials.secret_access_key().to_string());
        let session_token = credentials
            .session_token()
            .map(|t| Zeroizing::new(t.to_string()));
        Self {
            client,
            region,
            access_key_id,
            secret_access_key,
            session_token,
            big_model,
            small_model,
        }
    }

    /// The model ID used for sonnet/opus-class requests.
    pub fn big_model(&self) -> &str {
        &self.big_model
    }

    /// The model ID used for haiku-class requests.
    pub fn small_model(&self) -> &str {
        &self.small_model
    }

    /// Build a Bedrock runtime URL for any native endpoint suffix.
    /// e.g. `suffix = "converse"` → `.../model/{modelId}/converse`
    pub fn native_endpoint_url(&self, model_id: &str, suffix: &str) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/{suffix}",
            self.region, model_id
        )
    }

    /// Build the Bedrock InvokeModel URL for a given model.
    fn invoke_url(&self, model_id: &str) -> String {
        self.native_endpoint_url(model_id, "invoke")
    }

    /// Build the Bedrock InvokeModelWithResponseStream URL.
    fn invoke_stream_url(&self, model_id: &str) -> String {
        self.native_endpoint_url(model_id, "invoke-with-response-stream")
    }

    /// Forward a native Bedrock request (Converse or Invoke format) to the given URL.
    /// Signs with SigV4. Returns the raw response so the caller can stream or buffer it.
    /// No format translation — caller is responsible for using the correct Bedrock schema.
    pub async fn forward_native(
        &self,
        url: &str,
        body: bytes::Bytes,
        streaming: bool,
    ) -> Result<reqwest::Response, BedrockClientError> {
        let content_type = "application/json";
        let accept = if streaming {
            "application/vnd.amazon.eventstream"
        } else {
            "application/json"
        };

        let base_headers = [("content-type", content_type), ("accept", accept)];
        let signing_headers = self.sign_request("POST", url, &body, &base_headers)?;

        let mut builder = self
            .client
            .post(url)
            .header("content-type", content_type)
            .header("accept", accept)
            .body(body);

        for (k, v) in &signing_headers {
            builder = builder.header(k.as_str(), v.as_str());
        }

        let response = builder
            .send()
            .await
            .map_err(|e| BedrockClientError::Transport(e.to_string()))?;
        let status = response.status().as_u16();

        if !(200..300).contains(&status) {
            let resp_body = response
                .bytes()
                .await
                .map_err(|e| BedrockClientError::Transport(e.to_string()))?;
            return Err(BedrockClientError::ApiError {
                status,
                body: resp_body,
            });
        }

        Ok(response)
    }

    /// Sign an HTTP request with SigV4 and return headers to add.
    fn sign_request(
        &self,
        method: &str,
        url: &str,
        body_bytes: &[u8],
        extra_headers: &[(&str, &str)],
    ) -> Result<Vec<(String, String)>, BedrockClientError> {
        // Reconstruct Credentials on each call; the struct fields hold the
        // canonical copies wrapped in Zeroizing for safe drop.
        let creds = Credentials::new(
            self.access_key_id.clone(),
            self.secret_access_key.as_str(),
            self.session_token.as_deref().map(|s| s.to_string()),
            None,     // expiration
            "anyllm", // provider name
        );
        let identity: aws_smithy_runtime_api::client::identity::Identity = creds.into();
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
                // A 429 carrying hard quota/credit exhaustion never clears by
                // waiting; surface it immediately, consistent with the shared
                // client retry loop. Reading the body also returns the connection
                // to the pool.
                if status == 429 {
                    let resp_body = response.bytes().await.unwrap_or_default();
                    if anyllm_client::retry::is_quota_exhausted(&String::from_utf8_lossy(
                        &resp_body,
                    )) {
                        tracing::warn!(
                            status,
                            "Bedrock returned quota/credit exhaustion; not retrying"
                        );
                        return Err(BedrockClientError::ApiError {
                            status,
                            body: resp_body,
                        });
                    }
                } else {
                    drop(response.bytes().await);
                }
                tracing::warn!(
                    status,
                    attempt = attempt + 1,
                    max_retries = super::MAX_RETRIES,
                    delay_ms = delay.as_millis() as u64,
                    "retryable error from Bedrock, backing off"
                );
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
pub mod eventstream;

#[cfg(test)]
mod tests;
