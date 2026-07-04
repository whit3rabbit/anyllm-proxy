//! Ground-truth store for Anthropic thinking-block repair.
//!
//! Records each assistant response's content blocks (text, thinking,
//! signatures, tool_use ids) as they come off the real Anthropic API, so
//! that a later corrupted replay of the same message can be verified and
//! repaired against the exact bytes the API originally emitted.
//!
//! Backed by `moka::future::Cache`, the same crate the response cache
//! (`crate::cache::memory::MemoryCache`) already uses — bounded capacity,
//! LRU-ish eviction, no new dependency. In-memory only: on proxy restart the
//! store is empty and the repair layer fails open until a fresh response is
//! recorded.

use anyllm_translate::anthropic::ContentBlock;
use std::sync::Arc;

/// Bounded cache capacity per index. Not exposed as config; revisit only if
/// real usage shows it matters.
const CACHE_CAPACITY: u64 = 4096;

/// Ground-truth store, keyed three ways for the lookups `repair` needs:
/// by message id (full recorded content), by thinking-block signature
/// (which message/index it came from), and by tool_use id (which message
/// owns it).
///
/// Every key is scoped by a caller-supplied `namespace` (backend name +
/// virtual-key id, see `server/routes.rs`/`server/passthrough.rs`) so one
/// shared store instance -- built once and cloned into every Anthropic-mode
/// backend's `AppState` -- can never let one tenant's or backend's recorded
/// thinking content resolve or repair a different tenant's/backend's
/// request, even if their raw message ids, signatures, or tool_use ids
/// happen to collide.
pub struct ThinkingRepairStore {
    by_msg: moka::future::Cache<String, Arc<Vec<ContentBlock>>>,
    by_sig: moka::future::Cache<String, (String, usize)>,
    by_tool_use: moka::future::Cache<String, String>,
}

impl ThinkingRepairStore {
    pub fn new() -> Self {
        Self {
            by_msg: moka::future::Cache::new(CACHE_CAPACITY),
            by_sig: moka::future::Cache::new(CACHE_CAPACITY),
            by_tool_use: moka::future::Cache::new(CACHE_CAPACITY),
        }
    }

    /// NUL is not a legal character in any of the raw ids we scope (message
    /// ids, signatures, tool_use ids are all plain API-issued tokens), so it
    /// cannot be used to forge a cross-namespace collision. `pub(crate)` so
    /// other modules' tests can compute the same scoped key a lookup would
    /// return, without exposing it outside this crate.
    pub(crate) fn scoped_key(namespace: &str, id: &str) -> String {
        format!("{namespace}\u{0}{id}")
    }

    /// Record one assistant response's content blocks as ground truth,
    /// indexing thinking-block signatures and tool_use ownership. `namespace`
    /// scopes every key so this message can never be looked up from a
    /// different tenant's or backend's requests.
    pub async fn commit(&self, namespace: &str, msg_id: &str, blocks: Vec<ContentBlock>) {
        let scoped_msg_id = Self::scoped_key(namespace, msg_id);
        for (i, block) in blocks.iter().enumerate() {
            match block {
                ContentBlock::Thinking {
                    signature: Some(sig),
                    ..
                } => {
                    self.by_sig
                        .insert(Self::scoped_key(namespace, sig), (scoped_msg_id.clone(), i))
                        .await;
                }
                ContentBlock::ToolUse { id, .. } => {
                    self.by_tool_use
                        .insert(Self::scoped_key(namespace, id), scoped_msg_id.clone())
                        .await;
                }
                _ => {}
            }
        }
        self.by_msg.insert(scoped_msg_id, Arc::new(blocks)).await;
    }

    /// Which (message id, block index) a thinking-block signature belongs
    /// to, scoped to `namespace`. The returned message id is itself
    /// namespace-scoped -- pass it straight to `message()`.
    pub async fn lookup_sig(&self, namespace: &str, sig: &str) -> Option<(String, usize)> {
        self.by_sig.get(&Self::scoped_key(namespace, sig)).await
    }

    /// Which recorded message owns a given tool_use id, scoped to
    /// `namespace`. The returned message id is namespace-scoped.
    pub async fn owner_of_tool_use(&self, namespace: &str, id: &str) -> Option<String> {
        self.by_tool_use.get(&Self::scoped_key(namespace, id)).await
    }

    /// The full recorded content blocks for a namespace-scoped message id,
    /// as returned by `lookup_sig`/`owner_of_tool_use`.
    pub async fn message(&self, scoped_msg_id: &str) -> Option<Arc<Vec<ContentBlock>>> {
        self.by_msg.get(scoped_msg_id).await
    }
}

impl Default for ThinkingRepairStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thinking(text: &str, sig: &str) -> ContentBlock {
        ContentBlock::Thinking {
            thinking: text.to_string(),
            signature: Some(sig.to_string()),
        }
    }

    fn tool_use(id: &str) -> ContentBlock {
        ContentBlock::ToolUse {
            id: id.to_string(),
            name: "get_weather".to_string(),
            input: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn commit_indexes_signature_and_tool_use() {
        let store = ThinkingRepairStore::new();
        store
            .commit(
                "ns1",
                "msg_1",
                vec![thinking("let me think", "sig_abc"), tool_use("toolu_1")],
            )
            .await;

        let scoped_msg_1 = ThinkingRepairStore::scoped_key("ns1", "msg_1");
        assert_eq!(
            store.lookup_sig("ns1", "sig_abc").await,
            Some((scoped_msg_1.clone(), 0))
        );
        assert_eq!(
            store.owner_of_tool_use("ns1", "toolu_1").await,
            Some(scoped_msg_1.clone())
        );
        let recorded = store.message(&scoped_msg_1).await.unwrap();
        assert_eq!(recorded.len(), 2);
    }

    #[tokio::test]
    async fn unknown_signature_and_tool_use_miss() {
        let store = ThinkingRepairStore::new();
        assert_eq!(store.lookup_sig("ns1", "nope").await, None);
        assert_eq!(store.owner_of_tool_use("ns1", "nope").await, None);
        assert!(store.message("nope").await.is_none());
    }

    #[tokio::test]
    async fn thinking_block_without_signature_is_not_indexed() {
        let store = ThinkingRepairStore::new();
        store
            .commit(
                "ns1",
                "msg_1",
                vec![ContentBlock::Thinking {
                    thinking: "no sig yet".to_string(),
                    signature: None,
                }],
            )
            .await;
        // No signature to index, but the message itself is still recorded.
        assert!(store
            .message(&ThinkingRepairStore::scoped_key("ns1", "msg_1"))
            .await
            .is_some());
    }

    #[tokio::test]
    async fn same_msg_id_in_different_namespaces_does_not_collide() {
        let store = ThinkingRepairStore::new();
        store
            .commit(
                "tenant_a",
                "msg_1",
                vec![thinking("tenant a's thought", "sig_shared")],
            )
            .await;
        store
            .commit(
                "tenant_b",
                "msg_1",
                vec![thinking("tenant b's thought", "sig_shared")],
            )
            .await;

        let (owner_a, _) = store.lookup_sig("tenant_a", "sig_shared").await.unwrap();
        let (owner_b, _) = store.lookup_sig("tenant_b", "sig_shared").await.unwrap();
        assert_ne!(owner_a, owner_b);
        assert!(matches!(
            &store.message(&owner_a).await.unwrap()[0],
            ContentBlock::Thinking { thinking, .. } if thinking == "tenant a's thought"
        ));
        assert!(matches!(
            &store.message(&owner_b).await.unwrap()[0],
            ContentBlock::Thinking { thinking, .. } if thinking == "tenant b's thought"
        ));
    }
}
