/// Hop-by-hop headers that must not be forwarded to clients per RFC 7230.
pub(super) const HOP_BY_HOP: &[&str] = &[
    "transfer-encoding",
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "upgrade",
];

/// Audio transcription and text-to-speech passthrough handlers.
pub mod audio;
/// AWS Bedrock native endpoint handlers (Converse API + InvokeModel, SigV4 signing).
mod bedrock_native;
/// AWS Bedrock passthrough handler (SigV4 signing + event stream decoding).
mod bedrock_passthrough;
/// OpenAI Chat Completions input handler (POST /v1/chat/completions).
mod chat_completions;
/// Gemini native input handler (POST /v1beta/models/{model}:generateContent from gemini-cli).
pub mod gemini_input;
/// Gemini native generateContent handler (POST /v1/messages when GEMINI_API_FORMAT=native).
mod gemini_native;
/// Generic catch-all passthrough for any /v1/* path without an explicit handler.
mod generic_passthrough;
/// Image generation passthrough handler.
pub mod images;
/// Auth validation, request ID injection, size limits, concurrency limits, header logging.
pub mod middleware;
/// OIDC/JWT authentication (optional, enabled via OIDC_ISSUER_URL).
pub mod oidc;
/// Anthropic passthrough handler (no translation, forwards as-is).
mod passthrough;
/// Per-key request policy enforcement (model allowlists).
pub mod policy;
/// Axum router setup and request handlers for all API endpoints.
pub mod routes;
/// Secret redaction for upstream request payloads.
pub(crate) mod secret_redaction;
/// SSE response helpers for Anthropic-format streaming.
pub mod sse;
/// Shared state types for request handlers (AppState, AnthropicJson, ResolvedModel, etc.).
pub mod state;
/// SSE streaming handler with pre-stream error propagation and backpressure.
mod streaming;
/// Approximate token counting via tiktoken.
mod token_counting;

/// Verify that request secret redaction can run when the effective config enables it.
pub fn ensure_secret_redaction_available(enabled: bool) -> Result<(), String> {
    secret_redaction::ensure_available(enabled)
}

#[cfg(test)]
mod secret_redaction_tests {
    use axum::http::header::CONTENT_TYPE;
    use axum::http::HeaderMap;
    use bytes::Bytes;
    use serde_json::json;

    const MEDIA_BASE64: &str =
        "QkNEMTIzNDU2Nzg5MEFCQ0RFRjEyMzQ1Njc4OTBBQkNERUYxMjM0NTY3ODkwQUJDREVGMTIzNDU2Nzg5MA==";

    #[test]
    fn ensure_secret_redaction_available_accepts_disabled() {
        assert!(super::ensure_secret_redaction_available(false).is_ok());
    }

    #[cfg(feature = "secrets-scanner")]
    #[test]
    fn ensure_secret_redaction_available_accepts_enabled_with_scanner_feature() {
        assert!(super::ensure_secret_redaction_available(true).is_ok());
    }

    #[cfg(not(feature = "secrets-scanner"))]
    #[test]
    fn ensure_secret_redaction_available_rejects_enabled_without_scanner_feature() {
        assert!(super::ensure_secret_redaction_available(true).is_err());
    }

    #[tokio::test]
    async fn redact_json_text_body_replaces_secret() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        let secret = "MYCO_aB3dE5gH7jK9mN2pQ4sT6vW8xY1zC0bD";
        let body = Bytes::from(format!(r#"{{"prompt":"send {secret}"}}"#));

        let redacted = super::secret_redaction::redact_body(true, &headers, body)
            .await
            .expect("redaction should succeed");
        let redacted = String::from_utf8(redacted.to_vec()).unwrap();

        assert!(redacted.contains("[REDACTED_SECRET]"));
        assert!(!redacted.contains(secret));
    }

    #[tokio::test]
    async fn redact_effective_json_content_type_without_headers() {
        let secret = "MYCO_aB3dE5gH7jK9mN2pQ4sT6vW8xY1zC0bD";
        let body = Bytes::from(format!(r#"{{"prompt":"send {secret}"}}"#));

        let redacted = super::secret_redaction::redact_body_with_content_type(
            true,
            Some("application/json"),
            body,
        )
        .await
        .expect("redaction should succeed");
        let redacted = String::from_utf8(redacted.to_vec()).unwrap();

        assert!(redacted.contains("[REDACTED_SECRET]"));
        assert!(!redacted.contains(secret));
    }

    #[tokio::test]
    async fn redact_body_scans_when_content_type_missing() {
        // Fail-closed: a missing Content-Type must NOT bypass redaction.
        let headers = HeaderMap::new();
        let secret = "MYCO_aB3dE5gH7jK9mN2pQ4sT6vW8xY1zC0bD";
        let body = Bytes::from(format!(r#"{{"prompt":"send {secret}"}}"#));

        let redacted = super::secret_redaction::redact_body(true, &headers, body)
            .await
            .expect("redaction should succeed");
        let redacted = String::from_utf8(redacted.to_vec()).unwrap();

        assert!(redacted.contains("[REDACTED_SECRET]"));
        assert!(!redacted.contains(secret));
    }

    #[tokio::test]
    async fn redact_body_skips_multipart() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            "multipart/form-data; boundary=abc".parse().unwrap(),
        );
        let body = Bytes::from_static(b"MYCO_aB3dE5gH7jK9mN2pQ4sT6vW8xY1zC0bD");

        let redacted = super::secret_redaction::redact_body(true, &headers, body.clone())
            .await
            .expect("multipart should be skipped");

        assert_eq!(redacted, body);
    }

    #[tokio::test]
    async fn redact_body_skips_binary_content() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/octet-stream".parse().unwrap());
        let body = Bytes::from_static(b"\x00MYCO_aB3dE5gH7jK9mN2pQ4sT6vW8xY1zC0bD\xff");

        let redacted = super::secret_redaction::redact_body(true, &headers, body.clone())
            .await
            .expect("binary content should be skipped");

        assert_eq!(redacted, body);
    }

    #[tokio::test]
    async fn redact_json_value_preserves_anthropic_image_base64_source_data() {
        let secret = "MYCO_aB3dE5gH7jK9mN2pQ4sT6vW8xY1zC0bD";
        let body = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 16,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": format!("keep {secret} private")},
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": MEDIA_BASE64
                        }
                    }
                ]
            }]
        });

        let redacted = super::secret_redaction::redact_json_value(true, body)
            .await
            .expect("redaction should succeed");
        let redacted_text = serde_json::to_string(&redacted).unwrap();

        assert_eq!(
            redacted["messages"][0]["content"][1]["source"]["data"],
            MEDIA_BASE64
        );
        assert!(redacted_text.contains("[REDACTED_SECRET]"));
        assert!(!redacted_text.contains(secret));
    }

    #[tokio::test]
    async fn redact_json_value_preserves_anthropic_document_base64_source_data() {
        let secret = "MYCO_aB3dE5gH7jK9mN2pQ4sT6vW8xY1zC0bD";
        let body = json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 16,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": format!("keep {secret} private")},
                    {
                        "type": "document",
                        "source": {
                            "type": "base64",
                            "media_type": "application/pdf",
                            "data": MEDIA_BASE64
                        }
                    }
                ]
            }]
        });

        let redacted = super::secret_redaction::redact_json_value(true, body)
            .await
            .expect("redaction should succeed");
        let redacted_text = serde_json::to_string(&redacted).unwrap();

        assert_eq!(
            redacted["messages"][0]["content"][1]["source"]["data"],
            MEDIA_BASE64
        );
        assert!(redacted_text.contains("[REDACTED_SECRET]"));
        assert!(!redacted_text.contains(secret));
    }

    #[tokio::test]
    async fn redact_json_value_redacts_keyword_anchored_bare_base64() {
        let body = json!({
            "api_key": MEDIA_BASE64,
            "prompt": "hello"
        });

        let redacted = super::secret_redaction::redact_json_value(true, body)
            .await
            .expect("redaction should succeed");
        let redacted_text = serde_json::to_string(&redacted).unwrap();

        assert!(redacted_text.contains("[REDACTED_SECRET]"));
        assert!(!redacted_text.contains(MEDIA_BASE64));
    }
}
