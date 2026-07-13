//! M2.3 dedup pass (ALGO §12): collapse exact-duplicate `ContentBlock::Text`
//! buffers that recur across older (behind-frontier) messages.
//!
//! Extractive only (I5): a duplicate buffer is deleted whole, never rewritten. The FIRST
//! occurrence in conversation order is always kept intact; later byte-identical
//! occurrences are dropped. Scoped to messages `0..frontier` (ALGO's frozen zone) — the
//! same eligibility rule `optimize()` applies to `compress_message` (skip `Immutable` /
//! `client_cache_marker` messages).
//!
//! Direction matters for cache stability (I3, frozen-stability): message `i`'s decision
//! must depend ONLY on messages `0..=i`, never on messages that appear after it. Keeping
//! the *first* occurrence and collapsing *later* ones satisfies this — once message `i`
//! has entered the frozen zone, no amount of new conversation appended after it can ever
//! introduce an *earlier* duplicate that would change `i`'s own decision. The reverse
//! policy (keep the newest, collapse older duplicates) would NOT be safe: whether an
//! early message gets collapsed would depend on whether a not-yet-existing later message
//! turns out to repeat it, so the same frozen message could change bytes across turns as
//! the conversation grows.
//!
//! Determinism: candidates are scanned in a single left-to-right `(message_index,
//! buffer_index)` pass and edits are pushed in that same order. A `HashMap` is used only
//! for O(1) "have we seen this exact text before" point lookups — it is never iterated to
//! decide ordering or content, so decisions cannot depend on hash-bucket layout.

use std::collections::HashMap;

use crate::edit::{Edit, EditScript};
use crate::types::{BufferId, ContentBlock, Conversation, Protection};

/// Plan whole-buffer deletes for `ContentBlock::Text` buffers in messages `0..frontier`
/// whose bytes are byte-for-byte identical to an earlier (lower message-index) buffer
/// also in `0..frontier`. Buffers shorter than `min_len` are never touched (mirrors
/// `compress_message`'s min-length gate; trivial short repeats like "ok" or "thanks"
/// are not worth a dedup edit). Returns one `(message_index, BufferId, EditScript)` per
/// collapsed buffer, already in ascending `(message_index, buffer_index)` order.
pub fn dedup_pass(
    conv: &Conversation,
    frontier: usize,
    min_len: usize,
) -> Vec<(usize, BufferId, EditScript)> {
    let mut seen: HashMap<&str, (usize, usize)> = HashMap::new();
    let mut out = Vec::new();

    for (mi, msg) in conv.messages.iter().enumerate().take(frontier) {
        if msg.protection == Protection::Immutable || msg.client_cache_marker {
            continue;
        }
        for (bi, block) in msg.blocks.iter().enumerate() {
            let ContentBlock::Text(text) = block else {
                continue;
            };
            if text.len() < min_len {
                continue;
            }
            if seen.contains_key(text.as_str()) {
                // Not the first occurrence: collapse this whole buffer. A full-range
                // delete is always in-bounds and on char boundaries (0 and text.len()
                // are always valid UTF-8 boundaries), so this can never fail validate().
                let script = EditScript::new(vec![Edit::Delete(0..text.len())]);
                out.push((mi, BufferId(bi), script));
            } else {
                seen.insert(text.as_str(), (mi, bi));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Message, Role};

    fn msg(text: &str) -> Message {
        Message {
            role: Role::User,
            blocks: vec![ContentBlock::Text(text.into())],
            protection: Protection::Mutable,
            client_cache_marker: false,
        }
    }

    const LONG: &str = "This is a long repeated instruction block that should be collapsed \
        on its second and later occurrences because it is byte-for-byte identical each time.";

    #[test]
    fn collapses_second_and_later_occurrences_only() {
        let conv = Conversation::new(vec![
            msg(LONG),
            msg("unique short-ish filler text"),
            msg(LONG),
            msg(LONG),
        ]);
        let edits = dedup_pass(&conv, conv.len(), 20);
        // Only messages 2 and 3 (the 2nd and 3rd occurrences) are collapsed; message 0
        // (first occurrence) and message 1 (distinct content) are left alone.
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].0, 2);
        assert_eq!(edits[1].0, 3);
        for (_, _, script) in &edits {
            assert_eq!(script.edits, vec![Edit::Delete(0..LONG.len())]);
        }
    }

    #[test]
    fn respects_frontier_boundary() {
        // The duplicate at index 1 sits outside the frontier (only index 0 is eligible),
        // so it must not be touched.
        let conv = Conversation::new(vec![msg(LONG), msg(LONG)]);
        let edits = dedup_pass(&conv, 1, 20);
        assert!(edits.is_empty());
    }

    #[test]
    fn skips_immutable_and_client_cache_marked_messages() {
        let mut immutable = msg(LONG);
        immutable.protection = Protection::Immutable;
        let mut cache_marked = msg(LONG);
        cache_marked.client_cache_marker = true;
        let conv = Conversation::new(vec![msg(LONG), immutable, cache_marked]);
        let edits = dedup_pass(&conv, conv.len(), 20);
        assert!(
            edits.is_empty(),
            "immutable/cache-marked dupes must never be touched"
        );
    }

    #[test]
    fn below_min_len_is_never_deduped() {
        let short = "same short text";
        let conv = Conversation::new(vec![msg(short), msg(short)]);
        let edits = dedup_pass(&conv, conv.len(), 200);
        assert!(edits.is_empty());
    }

    #[test]
    fn deterministic_across_runs() {
        let conv = Conversation::new(vec![msg(LONG), msg(LONG), msg(LONG)]);
        let a = dedup_pass(&conv, conv.len(), 20);
        let b = dedup_pass(&conv, conv.len(), 20);
        assert_eq!(a, b);
    }

    #[test]
    fn earlier_duplicate_decision_is_independent_of_later_messages() {
        // I3-flavored check at the pass level: message 1's collapse decision (it repeats
        // message 0) must be identical whether or not a later message 2 also repeats it.
        let without_extra = Conversation::new(vec![msg(LONG), msg(LONG)]);
        let with_extra = Conversation::new(vec![msg(LONG), msg(LONG), msg(LONG)]);
        let a = dedup_pass(&without_extra, 2, 20);
        let b = dedup_pass(&with_extra, 2, 20);
        assert_eq!(a, b);
    }
}
