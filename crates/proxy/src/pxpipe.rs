//! Opt-in text-to-image context compression (pxpipe integration).
//!
//! Renders the stable system+tools slab of an Anthropic Messages request into a
//! PNG image block and swaps it in, saving input tokens on vision models. The
//! heavy lifting (deterministic renderer + Value-based transform) lives in the
//! IO-free `anyllm_pxpipe` crate; this module holds the transform-only engine.
//! All gating (enable flag, model scope, vision check) lives on `AppState` so it
//! can read the live `RuntimeConfig`.
//!
//! **Opt-in cascade.** The *enable* switch mirrors `anthropic_thinking_repair`:
//! YAML `pxpipe_compress: true` / `PXPIPE_COMPRESS=true` env → `MultiConfig` →
//! `RuntimeConfig.pxpipe_compress` (live admin toggle). **Scope** is a live
//! `RuntimeConfig.pxpipe_models` CSV of model bases, seeded from `PXPIPE_MODELS`
//! env (default `DEFAULT_MODELS`) and editable per-model from the admin UI —
//! only vision-capable models are offered. Both are gated per-request via
//! `AppState::pxpipe_engine_for(model)` in `server/passthrough.rs`
//! (`BACKEND=anthropic`), which also enforces the catalog vision check.

use anyllm_pxpipe::{AnthropicOpts, GptOpts};
use bytes::Bytes;
use serde_json::Value;

/// Transform-only compression engine. Held on `AppState`; gating is external.
pub struct PxpipeEngine {
    opts: AnthropicOpts,
}

/// Default model scope when `PXPIPE_MODELS` is unset. Conservative: pxpipe's
/// FINDINGS show weaker readers (Opus, GPT 5.5) degrade on imaged content, so we
/// do NOT silently image them. Widen from the admin UI or `PXPIPE_MODELS`.
const DEFAULT_MODELS: &[&str] = &["claude-fable-5"];

/// Resolve the default model-scope CSV: `PXPIPE_MODELS` env, else `DEFAULT_MODELS`.
/// Seeds `RuntimeConfig.pxpipe_models`; the admin UI edits the runtime value.
pub fn resolve_default_models_csv() -> String {
    match std::env::var("PXPIPE_MODELS") {
        Ok(csv) if !csv.trim().is_empty() => crate::config::helpers::normalize_csv(&csv),
        _ => DEFAULT_MODELS.join(","),
    }
}

/// Models the admin UI offers as per-model scope toggles, sorted + deduped:
/// vision-capable `claude*` ids from the LIVE catalog (same source as the
/// request-time vision gate, so the offered set and the enforced set can't
/// drift), plus any entry already in the current scope CSV. The union means
/// operator-added translate-path models (e.g. a `gpt-*` base set via
/// `PXPIPE_MODELS`) stay visible and toggleable even though the default UI list
/// is Anthropic-focused.
pub fn available_vision_models(
    catalog: &anyllm_providers::ProviderCatalog,
    current_scope_csv: &str,
) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for p in catalog.all_providers() {
        for m in catalog.list_models(&p.id) {
            if m.capabilities.vision && m.id.starts_with("claude") {
                set.insert(m.id.clone());
            }
        }
    }
    for base in current_scope_csv
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        set.insert(base.to_string());
    }
    set.into_iter().collect()
}

/// True when `model` falls in the scope CSV. Substring match on a lowercased id
/// so a base like `claude-fable-5` matches `claude-fable-5-20260101` and any
/// `anthropic/`-prefixed alias. Empty CSV = nothing in scope (fail-closed).
pub fn model_in_scope(model: &str, models_csv: &str) -> bool {
    let m = model.to_ascii_lowercase();
    models_csv
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .any(|base| m.contains(&base))
}

impl PxpipeEngine {
    pub fn new() -> Self {
        // Conversation-history collapse is the highest cache-stability risk in the
        // port, so it ships default-off behind its own env sub-flag until validated
        // live. `PXPIPE_COMPRESS` alone images only the static slab + live regions.
        let compress_history = std::env::var("PXPIPE_HISTORY")
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);
        Self {
            opts: AnthropicOpts {
                compress_history,
                ..AnthropicOpts::default()
            },
        }
    }

    /// Compress an Anthropic Messages body. The caller (`AppState`) has already
    /// confirmed this model is enabled, in scope, and vision-capable. Fails open
    /// (returns the body unchanged) on any parse/serialize error or when the
    /// transform declines (below threshold / not profitable).
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
        let info = anyllm_pxpipe::transform_anthropic(&mut root, &self.opts);
        if !info.compressed {
            tracing::debug!(reason = info.reason, model, "pxpipe: not compressed");
            return body;
        }
        match serde_json::to_vec(&root) {
            Ok(bytes) => {
                metrics.record_pxpipe_compression(
                    info.image_count as u64,
                    info.compressed_chars as u64,
                );
                tracing::info!(
                    model,
                    images = info.image_count,
                    reminder_imgs = info.reminder_imgs,
                    tool_result_imgs = info.tool_result_imgs,
                    truncated_tool_results = info.truncated_tool_results,
                    omitted_chars = info.omitted_chars,
                    image_bytes = info.image_bytes,
                    imaged_chars = info.compressed_chars,
                    dropped = info.dropped_chars,
                    anchor_relocated = info.relocated_cache_anchor,
                    "pxpipe: compressed request"
                );
                Bytes::from(bytes)
            }
            Err(_) => body,
        }
    }

    /// Compress an OpenAI Chat Completions request (as a `serde_json::Value`) in
    /// place, used on the translate path after `anthropic_to_openai_request`.
    /// Returns `Some((images, imaged_chars))` when it compressed, else `None`.
    /// Deliberately does NOT record metrics: the caller must re-deserialize the
    /// mutated Value back into a typed request first, and only count the
    /// compression once that commit succeeds (a failed round-trip forwards the
    /// original, uncompressed request). The caller has already confirmed the
    /// target model is enabled, in scope, and vision-capable.
    pub fn compress_openai_chat(&self, req: &mut Value, model: &str) -> Option<(u64, u64)> {
        let info = anyllm_pxpipe::transform_openai_chat(req, &GptOpts::default());
        if info.compressed {
            tracing::info!(
                model,
                images = info.image_count,
                image_bytes = info.image_bytes,
                imaged_chars = info.compressed_chars,
                dropped = info.dropped_chars,
                "pxpipe: compressed OpenAI request"
            );
            Some((info.image_count as u64, info.compressed_chars as u64))
        } else {
            tracing::debug!(reason = info.reason, model, "pxpipe: OpenAI not compressed");
            None
        }
    }
}

impl Default for PxpipeEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_matches_variants() {
        let csv = "claude-fable-5, claude-sonnet-5";
        assert!(model_in_scope("claude-fable-5", csv));
        assert!(model_in_scope("claude-fable-5-20260101", csv));
        assert!(model_in_scope("anthropic/claude-fable-5", csv));
        assert!(model_in_scope("claude-sonnet-5", csv));
        assert!(!model_in_scope("claude-opus-4-8", csv));
    }

    #[test]
    fn empty_scope_is_closed() {
        assert!(!model_in_scope("claude-fable-5", ""));
        assert!(!model_in_scope("claude-fable-5", "  ,  "));
    }

    #[test]
    fn default_csv_from_default_models() {
        // With PXPIPE_MODELS unset the default scope contains the safe reader.
        // (Env is process-global; we only assert the non-env branch shape here.)
        assert_eq!(resolve_default_models_csv_from(None), "claude-fable-5");
        assert_eq!(
            resolve_default_models_csv_from(Some("a , , b")),
            "a,b",
            "trims + drops empties"
        );
    }

    // Test seam so the CSV-normalization logic is checkable without mutating the
    // process env (which would race other tests per the crate ENV_TEST_LOCK rule).
    fn resolve_default_models_csv_from(env: Option<&str>) -> String {
        match env {
            Some(csv) if !csv.trim().is_empty() => crate::config::helpers::normalize_csv(csv),
            _ => DEFAULT_MODELS.join(","),
        }
    }
}
