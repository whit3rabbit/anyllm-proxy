//! Semantic cache backed by Qdrant vector store.
//!
//! Requires `--features qdrant` and `QDRANT_URL` env var.
//! When `QDRANT_URL` is not set, `SemanticCache::new()` returns `None`
//! and the proxy falls back to exact-match caching only.
//!
//! The caller is responsible for generating embeddings (via the backend's
//! embedding endpoint). This module handles only vector store operations.

use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, PointStruct, SearchPointsBuilder, UpsertPointsBuilder,
    VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use std::sync::atomic::{AtomicBool, Ordering};

/// Semantic cache that stores and searches response embeddings in Qdrant.
pub struct SemanticCache {
    client: Qdrant,
    collection: String,
    threshold: f32,
    /// Whether the collection has been verified/created.
    collection_ready: AtomicBool,
}

impl SemanticCache {
    /// Create a new semantic cache connected to Qdrant.
    ///
    /// Returns `None` if `QDRANT_URL` is not set, enabling graceful
    /// degradation: the proxy starts without semantic caching.
    pub fn new() -> Option<Self> {
        let url = std::env::var("QDRANT_URL").ok()?;
        // QDRANT_URL and REDIS_URL are operator-controlled infra addresses.
        // Private IPs (localhost, 10.x, etc.) are expected for these services.
        // No SSRF validation needed since these are not user-controlled inputs.
        let collection =
            std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "anyllm_cache".to_string());
        let threshold: f32 = std::env::var("SEMANTIC_CACHE_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.95);

        let client = Qdrant::from_url(&url).build().ok()?;

        Some(Self {
            client,
            collection,
            threshold,
            collection_ready: AtomicBool::new(false),
        })
    }

    /// Ensure the Qdrant collection exists with the right vector dimensions.
    /// Called lazily on first use to avoid blocking startup.
    pub async fn ensure_collection(&self, vector_size: u64) -> Result<(), String> {
        if self.collection_ready.load(Ordering::Acquire) {
            return Ok(());
        }

        // Check if collection exists
        let exists = self
            .client
            .collection_exists(&self.collection)
            .await
            .map_err(|e| format!("Qdrant collection_exists check failed: {e}"))?;

        if !exists {
            // Concurrent callers may both attempt create_collection.
            // Treat "already exists" as success to avoid TOCTOU race.
            match self
                .client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection)
                        .vectors_config(VectorParamsBuilder::new(vector_size, Distance::Cosine)),
                )
                .await
            {
                Ok(_) => {
                    tracing::info!(
                        collection = %self.collection,
                        vector_size = vector_size,
                        "created Qdrant collection for semantic cache"
                    );
                }
                Err(e) => {
                    // If another caller already created it, that's fine.
                    let msg = e.to_string();
                    if !msg.contains("already exists") {
                        return Err(format!("Qdrant create_collection failed: {e}"));
                    }
                }
            }
        }

        self.collection_ready.store(true, Ordering::Release);
        Ok(())
    }

    /// Search for a semantically similar cached response.
    ///
    /// Returns the cached response body and model if the top result's similarity
    /// score meets or exceeds the configured threshold.
    pub async fn search(&self, embedding: &[f32]) -> Option<super::CacheEntry> {
        use qdrant_client::qdrant::with_payload_selector::SelectorOptions;

        let results = self
            .client
            .search_points(
                SearchPointsBuilder::new(&self.collection, embedding.to_vec(), 1)
                    .with_payload(SelectorOptions::Enable(true)),
            )
            .await
            .ok()?;

        let point = results.result.first()?;
        if point.score < self.threshold {
            tracing::debug!(
                score = point.score,
                threshold = self.threshold,
                "semantic cache miss (below threshold)"
            );
            return None;
        }

        // Extract string values from Qdrant payload (protobuf Value type).
        let payload = &point.payload;
        let response_body = extract_string_value(payload.get("response_body")?)?;
        let model = extract_string_value(payload.get("model")?)?;

        tracing::debug!(
            score = point.score,
            model = %model,
            "semantic cache hit"
        );

        Some(super::CacheEntry {
            response_body: bytes::Bytes::from(response_body),
            model,
            created_at: std::time::Instant::now(),
            ttl_secs: None, // Semantic cache does not use per-entry TTL
        })
    }

    /// Store a response with its embedding vector in Qdrant.
    pub async fn store(&self, embedding: &[f32], entry: &super::CacheEntry, cache_key: &str) {
        let payload = Payload::try_from(serde_json::json!({
            "response_body": String::from_utf8_lossy(&entry.response_body).to_string(),
            "model": entry.model.clone(),
            "cache_key": cache_key,
        }));
        let payload = match payload {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "failed to build Qdrant payload");
                return;
            }
        };

        let point = PointStruct::new(
            uuid::Uuid::new_v4().to_string(),
            embedding.to_vec(),
            payload,
        );

        if let Err(e) = self
            .client
            .upsert_points(UpsertPointsBuilder::new(&self.collection, vec![point]))
            .await
        {
            tracing::warn!(error = %e, "failed to store in semantic cache");
        }
    }
}

/// Extract a string from a Qdrant protobuf Value.
fn extract_string_value(value: &qdrant_client::qdrant::Value) -> Option<String> {
    use qdrant_client::qdrant::value::Kind;
    match &value.kind {
        Some(Kind::StringValue(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Generate an embedding for the given text using the backend's embeddings endpoint.
///
/// Calls `embeddings_passthrough` on the backend client with an OpenAI-format
/// embedding request. Returns `None` if the backend doesn't support embeddings
/// or if the request fails.
pub async fn embed_text(
    backend: &crate::backend::BackendClient,
    text: &str,
    model: &str,
) -> Option<Vec<f32>> {
    let body = serde_json::json!({
        "input": text,
        "model": model,
    });
    let bytes = serde_json::to_vec(&body).ok()?;
    let (status, _, resp_body) = backend
        .embeddings_passthrough(bytes::Bytes::from(bytes), "application/json")
        .await
        .ok()?;

    if !status.is_success() {
        tracing::debug!(
            status = %status,
            "embedding request failed for semantic cache"
        );
        return None;
    }

    // OpenAI embeddings response: { "data": [{ "embedding": [...] }] }
    // Deserialize into a minimal struct to avoid cloning the full JSON value.
    #[derive(serde::Deserialize)]
    struct EmbeddingData {
        embedding: Vec<f32>,
    }
    #[derive(serde::Deserialize)]
    struct EmbeddingResponse {
        data: Vec<EmbeddingData>,
    }
    let resp: EmbeddingResponse = serde_json::from_slice(&resp_body).ok()?;
    resp.data.into_iter().next().map(|d| d.embedding)
}

/// Extract the last user message text from an Anthropic MessageCreateRequest
/// for use as the semantic cache key.
pub fn extract_last_user_text(
    request: &anyllm_translate::anthropic::MessageCreateRequest,
) -> Option<String> {
    use anyllm_translate::anthropic::{Content, ContentBlock, Role};

    for msg in request.messages.iter().rev() {
        if msg.role == Role::User {
            match &msg.content {
                Content::Text(text) => {
                    if !text.is_empty() {
                        return Some(text.clone());
                    }
                }
                Content::Blocks(blocks) => {
                    let mut text_parts = Vec::new();
                    for block in blocks {
                        if let ContentBlock::Text { text } = block {
                            text_parts.push(text.as_str());
                        }
                    }
                    if !text_parts.is_empty() {
                        return Some(text_parts.join(" "));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_none_without_env() {
        // Ensure QDRANT_URL is not set for this test.
        if std::env::var("QDRANT_URL").is_ok() {
            return;
        }
        assert!(
            SemanticCache::new().is_none(),
            "SemanticCache::new() should return None when QDRANT_URL is unset"
        );
    }

    #[test]
    fn extract_last_user_text_finds_text() {
        let j = serde_json::json!({
            "model": "test",
            "max_tokens": 100,
            "messages": [
                {"role": "user", "content": "first message"},
                {"role": "assistant", "content": "reply"},
                {"role": "user", "content": "second message"}
            ]
        });
        let request: anyllm_translate::anthropic::MessageCreateRequest =
            serde_json::from_value(j).unwrap();
        assert_eq!(
            extract_last_user_text(&request),
            Some("second message".to_string())
        );
    }

    #[test]
    fn extract_last_user_text_empty_messages() {
        let j = serde_json::json!({
            "model": "test",
            "max_tokens": 100,
            "messages": []
        });
        let request: anyllm_translate::anthropic::MessageCreateRequest =
            serde_json::from_value(j).unwrap();
        assert_eq!(extract_last_user_text(&request), None);
    }

    #[test]
    fn parse_embedding_response() {
        let resp = serde_json::json!({
            "data": [{
                "embedding": [0.1, 0.2, 0.3],
                "index": 0,
                "object": "embedding"
            }],
            "model": "text-embedding-3-small",
            "usage": {"prompt_tokens": 5, "total_tokens": 5}
        });
        let embedding: Vec<f32> = serde_json::from_value(
            resp.get("data")
                .unwrap()
                .get(0)
                .unwrap()
                .get("embedding")
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert_eq!(embedding, vec![0.1, 0.2, 0.3]);
    }
}
