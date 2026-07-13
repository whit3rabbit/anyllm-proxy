//! Opt-in command-aware tool-output compression (RTK integration).
//!
//! Rewrites the inner text of tool-output blocks (`tool_result` / `role:tool`)
//! using the declarative RTK filter catalog, shrinking test/build/git/log noise
//! before it reaches the backend. The IO-free engine lives in `anyllm_rtk`; this
//! module is the transform-only glue held on `AppState`. Gating (enable flag +
//! model scope) lives on `AppState` so it reads the live `RuntimeConfig`.
//!
//! **Opt-in cascade** mirrors pxpipe: `RTK_COMPRESS=true` env → seeds
//! `RuntimeConfig.rtk_compress` (live admin toggle). **Scope** is
//! `RuntimeConfig.rtk_models`, seeded from `RTK_MODELS` (default empty = ALL
//! models — RTK, unlike pxpipe, has no vision requirement, so it fails OPEN to
//! every model when unscoped).
//!
//! Determinism keeps the Anthropic prompt cache warm; `cache_control`-marked
//! blocks are preserved byte-for-byte by the engine.

use bytes::Bytes;
use serde_json::Value;

/// Transform-only RTK engine. Held on `AppState`; gating is external.
#[derive(Default)]
pub struct RtkEngine;

impl RtkEngine {
    pub fn new() -> Self {
        RtkEngine
    }

    /// Compress an Anthropic Messages body. The caller has already confirmed
    /// this model is enabled and in scope. Fails open (returns the body
    /// unchanged) on any parse/serialize error or when nothing compressed.
    pub fn compress_anthropic(
        &self,
        body: Bytes,
        model: &str,
        metrics: &crate::metrics::Metrics,
    ) -> Bytes {
        let mut root: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => return body,
        };
        let info = anyllm_rtk::transform_anthropic(&mut root);
        if !info.compressed {
            return body;
        }
        match serde_json::to_vec(&root) {
            Ok(bytes) => {
                // Verify the output is actually smaller; serde_json re-serialization
                // may inflate body size even when text content shrank (whitespace
                // normalization, number reformatting, key ordering). Return the
                // original if the output isn't strictly smaller.
                if bytes.len() >= body.len() {
                    return body;
                }
                let saved = info.chars_before.saturating_sub(info.chars_after) as u64;
                metrics.record_rtk_compression(info.blocks_compressed as u64, saved);
                tracing::info!(
                    model,
                    blocks = info.blocks_compressed,
                    chars_before = info.chars_before,
                    chars_after = info.chars_after,
                    bytes_before = body.len(),
                    bytes_after = bytes.len(),
                    "rtk: compressed request"
                );
                Bytes::from(bytes)
            }
            Err(_) => body,
        }
    }

    /// Compress an OpenAI Chat Completions request (as a `serde_json::Value`) in
    /// place, used on the translate path. Returns `Some((blocks, saved_chars))`
    /// when it compressed, else `None`. Does NOT record metrics: the caller must
    /// re-deserialize the mutated Value into a typed request first and count the
    /// compression only once that commit succeeds.
    pub fn compress_openai_chat(&self, req: &mut Value) -> Option<(u64, u64)> {
        let info = anyllm_rtk::transform_openai_chat(req);
        if info.compressed {
            let saved = info.chars_before.saturating_sub(info.chars_after) as u64;
            Some((info.blocks_compressed as u64, saved))
        } else {
            None
        }
    }
}

/// Resolve the default enable flag from `RTK_COMPRESS`. Seeds `RuntimeConfig.rtk_compress`.
pub fn resolve_default_enabled() -> bool {
    std::env::var("RTK_COMPRESS")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Resolve the default model-scope CSV from `RTK_MODELS` (empty = all models).
/// Seeds `RuntimeConfig.rtk_models`; the admin UI edits the runtime value.
pub fn resolve_default_models_csv() -> String {
    match std::env::var("RTK_MODELS") {
        Ok(csv) if !csv.trim().is_empty() => crate::config::helpers::normalize_csv(&csv),
        _ => String::new(),
    }
}

/// True when `model` is in scope. **Empty CSV = ALL models in scope** (RTK is
/// not vision-gated, so unscoped means "compress every model's tool output").
/// Non-empty CSV is a substring allowlist over the lowercased id, matching
/// pxpipe's `model_in_scope`.
pub fn model_in_scope(model: &str, models_csv: &str) -> bool {
    let csv = models_csv.trim();
    if csv.is_empty() {
        return true;
    }
    let m = model.to_ascii_lowercase();
    csv.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .any(|base| m.contains(&base))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_scope_matches_all() {
        assert!(model_in_scope("claude-opus-4-8", ""));
        assert!(model_in_scope("gpt-5", "   "));
    }

    #[test]
    fn nonempty_scope_is_allowlist() {
        let csv = "claude, gpt-5";
        assert!(model_in_scope("claude-sonnet-5", csv));
        assert!(model_in_scope("openai/gpt-5", csv));
        assert!(!model_in_scope("gemini-2.5-pro", csv));
    }

    #[test]
    fn compress_anthropic_shrinks_and_counts() {
        let mut noise = String::from("On branch main\nChanges not staged for commit:\n");
        for i in 0..200 {
            noise.push_str(&format!("  (use \"git add ...\" file {i})\n"));
        }
        let body = serde_json::json!({
            "model": "claude-sonnet-5",
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "bash", "input": {"command": "git status"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": noise}
                ]}
            ]
        });
        let raw = Bytes::from(serde_json::to_vec(&body).unwrap());
        let metrics = crate::metrics::Metrics::new();
        let out = RtkEngine::new().compress_anthropic(raw.clone(), "claude-sonnet-5", &metrics);
        assert!(out.len() < raw.len(), "compressed body is smaller");
        assert_eq!(metrics.snapshot().rtk_compressed_total, 1);
    }

    #[test]
    fn compress_anthropic_fails_open_on_garbage() {
        let raw = Bytes::from_static(b"not json");
        let metrics = crate::metrics::Metrics::new();
        let out = RtkEngine::new().compress_anthropic(raw.clone(), "m", &metrics);
        assert_eq!(out, raw, "garbage body returned unchanged");
        assert_eq!(metrics.snapshot().rtk_compressed_total, 0);
    }
}
