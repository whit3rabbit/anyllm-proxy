//! Tool-result compression — the single biggest real-world saving (ROADMAP D8). Never
//! touch JSON *structure*; compress long string *values* through the core text pipeline.
//!
//! ALGO §7:
//!  - parse `raw` with serde_json; on non-JSON, treat the whole thing as one text buffer;
//!  - walk values; for each String leaf longer than `min_len`, run the core §5 pipeline
//!    on the decoded string (ratio = tool_result_value), re-encoding on render;
//!  - keys, numbers, bools, structure: byte-identical;
//!  - if a buffer (the whole thing for non-JSON, or a JSON String leaf) still exceeds
//!    `tool_result_max_tokens` after word-level compression, structural truncation: keep
//!    head 60% + tail 20% (by char count), drop the middle, joined with a deterministic
//!    marker. Applied per-leaf for JSON so structure is never touched (invariant I6).

use anyllm_optimize_core::{
    compress_message as core_compress_message, emit_edits, segment, select_keep, split_words,
    BudgetCounter, BufferId, CompressionPolicy, ContentBlock, Edit, EditScript,
    HeuristicBudgetCounter, Message, OptimizeError, SegKind, TokenScorer, Workspace,
};
use serde_json::Value;

/// Returns `Some(compressed)` if the tool result was shortened, else `None` (unchanged).
///
/// JSON input: only String leaves are ever rewritten (via [`compress_text_buffer`]); keys,
/// numbers, bools, and container structure are byte-identical (invariant I6). Non-JSON
/// input is treated as a single text buffer. Fail-open per buffer: a scorer error or an
/// invalid edit script leaves that buffer untouched rather than erroring the whole result.
pub fn compress_tool_result(
    raw: &str,
    policy: &CompressionPolicy,
    scorer: &dyn TokenScorer,
    ws: &mut Workspace,
) -> Option<String> {
    let ratio = policy.ratios.tool_result_value;
    if ratio >= 1.0 {
        return None;
    }
    let counter = HeuristicBudgetCounter::default();

    match serde_json::from_str::<Value>(raw) {
        Ok(mut value) => {
            let mut changed = false;
            walk_json_strings(
                &mut value,
                ratio,
                policy,
                scorer,
                &counter,
                ws,
                &mut changed,
            );
            if changed {
                serde_json::to_string(&value).ok()
            } else {
                None
            }
        }
        Err(_) => {
            let compressed = compress_text_buffer(raw, ratio, policy, scorer, ws);
            let base = compressed.as_deref().unwrap_or(raw);
            match structural_truncate(base, policy.tool_result_max_tokens, &counter) {
                Some(truncated) => Some(truncated),
                None => compressed,
            }
        }
    }
}

/// Extends core's [`compress_message`](anyllm_optimize_core::compress_message) with
/// value-level `ToolResult` compression (ALGO §7). Text blocks are delegated to core
/// unchanged; a `ToolResult` block that [`compress_tool_result`] shrinks is emitted as
/// one whole-buffer `Edit::Replace`, so it composes with the same
/// `Vec<(BufferId, EditScript)>` shape the orchestrator already consumes from core.
/// Fail-open per block, matching `compress_tool_result`'s own fail-open contract.
pub fn compress_message(
    msg: &Message,
    policy: &CompressionPolicy,
    scorer: &dyn TokenScorer,
    ws: &mut Workspace,
) -> Result<Vec<(BufferId, EditScript)>, OptimizeError> {
    let mut result = core_compress_message(msg, policy, scorer, ws)?;

    for (bi, block) in msg.blocks.iter().enumerate() {
        let ContentBlock::ToolResult { raw } = block else {
            continue;
        };
        let Some(compressed) = compress_tool_result(raw, policy, scorer, ws) else {
            continue;
        };
        let script = EditScript::new(vec![Edit::Replace {
            range: 0..raw.len(),
            text: compressed,
        }]);
        // Fail-open: an invalid script (shouldn't happen for a whole-buffer replace, but
        // validate() is the safety boundary everywhere else in this pipeline too) is
        // silently skipped, not applied.
        if script.validate(raw).is_ok() {
            result.push((BufferId(bi), script));
        }
    }
    result.sort_by_key(|(bi, _)| bi.0);
    Ok(result)
}

/// Recursively compress every String leaf in place. Object/array structure, key names,
/// numbers, and bools are never touched — only `Value::String` contents may shrink.
#[allow(clippy::too_many_arguments)]
fn walk_json_strings(
    value: &mut Value,
    ratio: f32,
    policy: &CompressionPolicy,
    scorer: &dyn TokenScorer,
    counter: &dyn BudgetCounter,
    ws: &mut Workspace,
    changed: &mut bool,
) {
    match value {
        Value::String(s) => {
            if let Some(compressed) = compress_text_buffer(s, ratio, policy, scorer, ws) {
                *s = compressed;
                *changed = true;
            }
            // Word-level compression alone may not bring an individual value under the
            // cap (e.g. a huge log dump with little redundant prose). Structural
            // truncation only ever rewrites the String's own content, never the
            // surrounding JSON, so structure stays byte-identical (invariant I6).
            if let Some(truncated) = structural_truncate(s, policy.tool_result_max_tokens, counter)
            {
                *s = truncated;
                *changed = true;
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                walk_json_strings(item, ratio, policy, scorer, counter, ws, changed);
            }
        }
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                walk_json_strings(v, ratio, policy, scorer, counter, ws, changed);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Run the core §5 text pipeline (segment → words → score → keep-set → edits) on one
/// buffer. Returns `None` (unchanged) below `min_len`, on a scoring error, on an invalid
/// edit script, or when no edits were produced — always fail-open per buffer.
fn compress_text_buffer(
    text: &str,
    ratio: f32,
    policy: &CompressionPolicy,
    scorer: &dyn TokenScorer,
    ws: &mut Workspace,
) -> Option<String> {
    if text.len() < policy.min_len {
        return None;
    }

    let mut segs = Vec::new();
    segment(text, &mut segs);

    let mut edits = Vec::new();
    for seg in &segs {
        if seg.kind != SegKind::Prose {
            continue;
        }
        ws.words.clear();
        split_words(text, seg.range.clone(), &mut ws.words);
        if ws.words.is_empty() {
            continue;
        }
        let wstrs: Vec<&str> = ws.words.iter().map(|w| &text[w.range.clone()]).collect();
        let scores = scorer.score_words(&wstrs, ws).ok()?;
        let keep = select_keep(&ws.words, text, &scores, ratio, &policy.force);
        emit_edits(text, &ws.words, &keep, &mut edits);
    }

    if edits.is_empty() {
        return None;
    }
    let script = EditScript::new(edits);
    if script.validate(text).is_err() {
        return None;
    }
    let mut out = String::new();
    script.apply(text, &mut out);
    if out.len() < text.len() {
        Some(out)
    } else {
        None
    }
}

/// ALGO §7 structural truncation: when `text` still exceeds `max_tokens`, keep the head
/// 60% and tail 20% (by char count) and drop the middle, joined by a deterministic marker
/// naming the elided token count. Fail-open: returns `None` when under budget, when the
/// buffer is too short to have a meaningful head/tail split, or on the (unreachable in
/// practice) degenerate empty-text case.
///
/// Splits only at `char_indices` boundaries — never inside a multi-byte codepoint —
/// satisfying invariant I6 (UTF-8 validity, no split graphemes).
fn structural_truncate(
    text: &str,
    max_tokens: usize,
    counter: &dyn BudgetCounter,
) -> Option<String> {
    if counter.count(text) <= max_tokens as u64 {
        return None;
    }

    // Byte offset of the start of each char, plus a trailing sentinel at text.len() so
    // char index `char_count` is a valid lookup too.
    let byte_offsets: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let char_count = byte_offsets.len() - 1;
    if char_count == 0 {
        return None;
    }

    let head_chars = (char_count as f64 * 0.6).floor() as usize;
    let tail_chars = (char_count as f64 * 0.2).floor() as usize;
    // Too short to leave a non-trivial middle to elide.
    if head_chars + tail_chars >= char_count {
        return None;
    }

    let head_end = byte_offsets[head_chars];
    let tail_start = byte_offsets[char_count - tail_chars];
    let head = &text[..head_end];
    let elided = &text[head_end..tail_start];
    let tail = &text[tail_start..];
    let elided_tokens = counter.count(elided);

    let mut out = String::with_capacity(head.len() + tail.len() + 48);
    out.push_str(head);
    out.push_str(&format!(
        "\n\u{2026}[anyllm-optimizer: {elided_tokens} tokens elided]\u{2026}\n"
    ));
    out.push_str(tail);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyllm_optimize_core::{Protection, Role, UniformScorer};
    use proptest::prelude::*;

    /// A JSON String leaf with real value that's long enough to clear `min_len` (200
    /// chars) and redundant enough for the word-level pipeline to actually shrink it.
    fn long_string_value() -> impl Strategy<Value = Value> {
        prop::collection::vec(
            prop::sample::select(
                &[
                    "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "river",
                    "green", "field", "mountain", "toward", "distant", "blue", "again", "across",
                    "wide", "trees",
                ][..],
            ),
            60..90,
        )
        .prop_map(|words| Value::String(words.join(" ")))
    }

    fn short_string_value() -> impl Strategy<Value = Value> {
        "[a-z]{0,12}".prop_map(Value::String)
    }

    fn json_key() -> impl Strategy<Value = String> {
        "[a-zA-Z_][a-zA-Z0-9_]{0,8}"
    }

    /// Arbitrary JSON: objects/arrays nesting a mix of null/bool/number/short-string/
    /// long-string leaves. Bounded depth/size so generation and compression stay fast.
    fn arb_json() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            any::<i32>().prop_map(|n| Value::Number(n.into())),
            short_string_value(),
            long_string_value(),
        ];
        leaf.prop_recursive(3, 32, 5, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
                prop::collection::vec((json_key(), inner), 0..4).prop_map(|kvs| {
                    Value::Object(kvs.into_iter().collect::<serde_json::Map<_, _>>())
                }),
            ]
        })
    }

    /// True iff `a` and `b` have identical JSON structure: same container shapes, same
    /// keys, byte-identical null/bool/number leaves. String leaves are allowed to differ
    /// in content (only their *presence and type* must match) since compression may
    /// rewrite them (invariant I6).
    fn structure_eq(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Number(x), Value::Number(y)) => x == y,
            (Value::String(_), Value::String(_)) => true,
            (Value::Array(x), Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(a, b)| structure_eq(a, b))
            }
            (Value::Object(x), Value::Object(y)) => {
                x.len() == y.len()
                    && x.iter()
                        .all(|(k, v)| y.get(k).is_some_and(|v2| structure_eq(v, v2)))
            }
            _ => false,
        }
    }

    proptest! {
        // I6, acceptance: compressed tool-result JSON still parses, and its structure
        // (container shapes, keys, non-string leaves) is byte-identical to the input;
        // only String leaf *contents* may have shrunk.
        #[test]
        fn compress_tool_result_preserves_json_structure(value in arb_json()) {
            let raw = value.to_string();
            let mut ws = Workspace::new();
            let out = compress_tool_result(&raw, &CompressionPolicy::default(), &UniformScorer, &mut ws);

            if let Some(compressed) = out {
                let parsed: Value = serde_json::from_str(&compressed)
                    .expect("compressed tool result must still be valid JSON");
                prop_assert!(structure_eq(&value, &parsed),
                    "structure changed:\n  before: {value}\n  after:  {parsed}");
            }
        }
    }

    /// A JSON value below `min_len` (200 chars) is real passthrough behavior (nothing
    /// worth compressing), not a stub short-circuit.
    #[test]
    fn short_json_value_below_min_len_is_passthrough() {
        let mut ws = Workspace::new();
        let out = compress_tool_result(
            "{\"result\":\"anything\"}",
            &CompressionPolicy::default(),
            &UniformScorer,
            &mut ws,
        );
        assert!(out.is_none());
    }

    /// A JSON string value well past `min_len` and redundant enough for the word-level
    /// pipeline to shrink: real compression happens (not a stub passthrough), and every
    /// non-string leaf (key, number) is byte-identical before/after (I6).
    #[test]
    fn long_json_string_value_is_really_compressed() {
        let long_value = "the quick brown fox jumps over the lazy dog ".repeat(20);
        let raw = serde_json::json!({
            "keep_me": 42,
            "also_keep": true,
            "result": long_value,
        })
        .to_string();
        let mut ws = Workspace::new();
        let out =
            compress_tool_result(&raw, &CompressionPolicy::default(), &UniformScorer, &mut ws);

        let compressed = out.expect("long redundant value should compress, not pass through");
        assert!(
            compressed.len() < raw.len(),
            "compressed output should be shorter: {} vs {}",
            compressed.len(),
            raw.len()
        );
        let parsed: Value = serde_json::from_str(&compressed).unwrap();
        assert_eq!(parsed["keep_me"], 42);
        assert_eq!(parsed["also_keep"], true);
        assert_ne!(
            parsed["result"].as_str().unwrap(),
            long_value,
            "the string value itself should have shrunk"
        );
    }

    /// Non-JSON `raw` (fails `serde_json::from_str`) is treated as a single text buffer:
    /// the word-level pipeline still runs and can shrink it.
    #[test]
    fn non_json_input_is_compressed_as_single_buffer() {
        let raw = "the quick brown fox jumps over the lazy dog ".repeat(20);
        assert!(
            serde_json::from_str::<Value>(&raw).is_err(),
            "fixture must be non-JSON"
        );

        let mut ws = Workspace::new();
        let out =
            compress_tool_result(&raw, &CompressionPolicy::default(), &UniformScorer, &mut ws);

        let compressed = out.expect("long redundant non-JSON text should compress");
        assert!(compressed.len() < raw.len());
    }

    /// Non-JSON `raw` short enough (below `min_len`) that the word-level pipeline never
    /// even runs: passthrough `None`, same as the JSON short-value case.
    #[test]
    fn non_json_input_below_min_len_is_passthrough() {
        let raw = "not json and also short";
        assert!(
            serde_json::from_str::<Value>(raw).is_err(),
            "fixture must be non-JSON"
        );

        let mut ws = Workspace::new();
        let out = compress_tool_result(raw, &CompressionPolicy::default(), &UniformScorer, &mut ws);
        assert!(out.is_none());
    }

    /// A buffer with no word-level redundancy for `UniformScorer` to exploit (every word
    /// unique) but large enough that even after the ratio-based cut it still exceeds
    /// `tool_result_max_tokens`: structural truncation kicks in and the deterministic
    /// marker is present in the output. Exercises the non-JSON branch of
    /// `compress_tool_result`.
    fn unique_word_text(word_count: usize) -> String {
        (0..word_count)
            .map(|i| format!("tok{i:06}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn non_json_input_over_cap_gets_truncation_marker() {
        // ~10 bytes/word * 3000 words ≈ 30_000 bytes; default ratio 0.4 leaves ~12_000
        // bytes even after word-level compression removes 60%, still well over the
        // default 4000-token (≈14_400 byte) cap after accounting for a partial cut, so
        // structural truncation must trigger.
        let raw = unique_word_text(3000);
        let mut ws = Workspace::new();
        let out =
            compress_tool_result(&raw, &CompressionPolicy::default(), &UniformScorer, &mut ws);

        let compressed = out.expect("oversized unique-word text must be shortened");
        assert!(
            compressed.contains("tokens elided"),
            "expected deterministic truncation marker in output: {compressed}"
        );
    }

    /// Same over-cap scenario but through the JSON path: the marker lands inside the
    /// String leaf's own content only, structure (keys, non-string leaves) stays
    /// byte-identical (I6).
    #[test]
    fn json_string_value_over_cap_gets_truncation_marker() {
        let long_value = unique_word_text(3000);
        let raw = serde_json::json!({
            "keep_me": 42,
            "result": long_value,
        })
        .to_string();
        let mut ws = Workspace::new();
        let out =
            compress_tool_result(&raw, &CompressionPolicy::default(), &UniformScorer, &mut ws);

        let compressed = out.expect("oversized unique-word JSON value must be shortened");
        let parsed: Value = serde_json::from_str(&compressed)
            .expect("compressed tool result must still be valid JSON");
        assert_eq!(parsed["keep_me"], 42);
        assert!(
            parsed["result"].as_str().unwrap().contains("tokens elided"),
            "expected deterministic truncation marker in the String leaf: {}",
            parsed["result"]
        );
    }

    #[test]
    fn compress_message_wires_tool_result_blocks() {
        // A long redundant string value, well past `min_len`, alongside a short one that
        // should stay untouched, and a Text block so we can confirm core's own handling
        // still runs unchanged through the delegate.
        let long_value = "word ".repeat(100);
        let raw = serde_json::json!({
            "keep_me": 42,
            "result": long_value,
            "short": "fine",
        })
        .to_string();
        let msg = Message {
            role: Role::Tool,
            blocks: vec![
                ContentBlock::Text("short prefix, below min_len".into()),
                ContentBlock::ToolResult { raw: raw.clone() },
            ],
            protection: Protection::Mutable,
            client_cache_marker: false,
        };
        let mut ws = Workspace::new();
        let edits =
            compress_message(&msg, &CompressionPolicy::default(), &UniformScorer, &mut ws).unwrap();

        // Only the ToolResult buffer (index 1) produced an edit; the short Text block is
        // below min_len so core's own pipeline skips it.
        assert_eq!(edits.len(), 1);
        let (buf_id, script) = &edits[0];
        assert_eq!(buf_id.0, 1);

        let mut out = String::new();
        script.apply(&raw, &mut out);
        assert!(out.len() < raw.len(), "tool result should have shrunk");

        // Structure survives: same keys, non-string leaves byte-identical (I6).
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["keep_me"], 42);
        assert_eq!(parsed["short"], "fine");
    }
}
