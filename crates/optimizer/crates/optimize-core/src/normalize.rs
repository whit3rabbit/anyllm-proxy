//! M2.4 boilerplate/whitespace normalization pass (ALGO §12): collapse
//! redundant whitespace within `Prose` spans of a text buffer.
//!
//! Three rules, applied left-to-right over each `Prose` span (protected spans — fenced/
//! inline code, URLs, tables — are left byte-for-byte untouched, exactly like
//! `compress_message`):
//!   1. A run of 2+ horizontal whitespace (space/tab) NOT touching a newline or the end
//!      of the span collapses to its first character (e.g. a double space after a
//!      sentence becomes a single space).
//!   2. Horizontal whitespace immediately before a newline or the end of the span
//!      (trailing whitespace) is deleted entirely.
//!   3. A run of 3+ consecutive newlines (2+ blank lines) collapses to exactly two
//!      newlines (one blank-line paragraph break is kept; extras are dropped).
//!
//! Extractive only (I5): every edit is a `Delete` of redundant bytes; nothing is ever
//! rewritten, only removed, so the output is always a byte subsequence of the input.
//!
//! Per-message purity (I3): `normalize_buffer` depends ONLY on the one buffer's bytes.
//! The conversation-level `normalize_pass` applies the same eligibility rule as
//! `dedup_pass` (messages `0..frontier`, skipping `Immutable`/`client_cache_marker`), so
//! a frozen message's decision never depends on any other message.
//!
//! Determinism: a single left-to-right byte scan per buffer, no `HashMap`, no ordering
//! ambiguity — edits are produced and returned in ascending byte-range order.

use std::ops::Range;

use crate::edit::{Edit, EditScript};
use crate::segment::{segment, SegKind};
use crate::types::{BufferId, ContentBlock, Conversation, Protection};

/// Plan whitespace-normalization deletes for every `Prose` span of `text`. Edits are
/// returned in ascending, non-overlapping byte-range order (ready to feed straight into
/// `EditScript::new`).
pub fn normalize_buffer(text: &str) -> Vec<Edit> {
    let mut segs = Vec::new();
    segment(text, &mut segs);
    let mut edits = Vec::new();
    for seg in &segs {
        if seg.kind == SegKind::Prose {
            normalize_span(text, seg.range.clone(), &mut edits);
        }
    }
    edits
}

fn normalize_span(text: &str, range: Range<usize>, edits: &mut Vec<Edit>) {
    let bytes = text.as_bytes();
    let mut i = range.start;
    while i < range.end {
        match bytes[i] {
            b' ' | b'\t' => {
                let start = i;
                let mut j = i;
                while j < range.end && matches!(bytes[j], b' ' | b'\t') {
                    j += 1;
                }
                let trailing = j == range.end || bytes[j] == b'\n';
                if trailing {
                    // Trailing whitespace carries no meaning: drop the whole run.
                    if j > start {
                        edits.push(Edit::Delete(start..j));
                    }
                } else if j - start > 1 {
                    // Mid-line run of 2+: keep the first char, drop the rest.
                    edits.push(Edit::Delete(start + 1..j));
                }
                i = j;
            }
            b'\n' => {
                let start = i;
                let mut j = i;
                while j < range.end && bytes[j] == b'\n' {
                    j += 1;
                }
                if j - start > 2 {
                    // Keep at most one blank line (two newlines) between paragraphs.
                    edits.push(Edit::Delete(start + 2..j));
                }
                i = j;
            }
            _ => {
                i += text[i..range.end]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(1);
            }
        }
    }
}

/// Conversation-level pass: apply `normalize_buffer` to every `ContentBlock::Text`
/// buffer of length `>= min_len` in messages `0..frontier`, skipping `Immutable`/
/// `client_cache_marker` messages (same eligibility rule as `dedup_pass` /
/// `compress_message`). Returns one `(message_index, BufferId, EditScript)` per buffer
/// that produced at least one edit, in ascending `(message_index, buffer_index)` order.
pub fn normalize_pass(
    conv: &Conversation,
    frontier: usize,
    min_len: usize,
) -> Vec<(usize, BufferId, EditScript)> {
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
            let edits = normalize_buffer(text);
            if edits.is_empty() {
                continue;
            }
            let script = EditScript::new(edits);
            // Fail-open per buffer: an invalid script is silently skipped, not applied.
            if script.validate(text).is_ok() {
                out.push((mi, BufferId(bi), script));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Message, Role};

    fn apply(text: &str, edits: &[Edit]) -> String {
        let script = EditScript::new(edits.to_vec());
        script.validate(text).expect("edits must validate");
        let mut out = String::new();
        script.apply(text, &mut out);
        out
    }

    fn msg(text: &str) -> Message {
        Message {
            role: Role::User,
            blocks: vec![ContentBlock::Text(text.into())],
            protection: Protection::Mutable,
            client_cache_marker: false,
        }
    }

    #[test]
    fn collapses_mid_line_double_spaces() {
        let text = "one  two   three    four.";
        let edits = normalize_buffer(text);
        let out = apply(text, &edits);
        assert_eq!(out, "one two three four.");
    }

    #[test]
    fn strips_trailing_whitespace_before_newline_and_eof() {
        let text = "line one   \nline two\t\t";
        let edits = normalize_buffer(text);
        let out = apply(text, &edits);
        assert_eq!(out, "line one\nline two");
    }

    #[test]
    fn collapses_blank_line_runs_to_one() {
        let text = "para one.\n\n\n\n\npara two.";
        let edits = normalize_buffer(text);
        let out = apply(text, &edits);
        assert_eq!(out, "para one.\n\npara two.");
    }

    #[test]
    fn single_space_and_single_blank_line_are_untouched() {
        let text = "a normal sentence.\n\nanother normal one.";
        let edits = normalize_buffer(text);
        assert!(edits.is_empty(), "already-clean text should not be edited");
    }

    #[test]
    fn fenced_code_and_table_spans_are_never_touched() {
        // Redundant whitespace INSIDE a fenced block or table line must survive
        // byte-for-byte (I6); only the surrounding Prose may be normalized.
        let text = "intro  text\n```\ncode   with    spaces\n\n\n\nmore\n```\nafter  text";
        let edits = normalize_buffer(text);
        let out = apply(text, &edits);
        let fence_start = text.find("```").unwrap();
        let fence_end = text.rfind("```").unwrap() + 3;
        assert!(
            out.contains(&text[fence_start..fence_end]),
            "fenced block bytes must be unchanged: {out:?}"
        );
        assert_eq!(
            out,
            "intro text\n```\ncode   with    spaces\n\n\n\nmore\n```\nafter text"
        );
    }

    #[test]
    fn output_is_a_subsequence_never_longer_than_input() {
        let text = "  redundant   leading and   trailing whitespace   \n\n\n\n\n  more  ";
        let edits = normalize_buffer(text);
        let out = apply(text, &edits);
        assert!(out.len() <= text.len());
        // Extractive: every char kept in `out` must appear in the same relative order
        // in `text` (cheap check: `out` bytes are a subsequence of `text` bytes).
        let mut ti = text.bytes();
        assert!(out.bytes().all(|b| ti.by_ref().any(|t| t == b)));
    }

    #[test]
    fn deterministic_across_runs() {
        let text = "some  text   with    lots  of   redundant     whitespace  \n\n\n\nhere.";
        let a = normalize_buffer(text);
        let b = normalize_buffer(text);
        assert_eq!(a, b);
    }

    #[test]
    fn conversation_pass_respects_frontier_boundary() {
        let messy = "one  two   three\n\n\n\nfour.";
        let conv = Conversation::new(vec![msg(messy), msg(messy)]);
        let edits = normalize_pass(&conv, 1, 5);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].0, 0);
    }

    #[test]
    fn conversation_pass_skips_immutable_and_client_cache_marked_messages() {
        let messy = "one  two   three\n\n\n\nfour.";
        let mut immutable = msg(messy);
        immutable.protection = Protection::Immutable;
        let mut cache_marked = msg(messy);
        cache_marked.client_cache_marker = true;
        let conv = Conversation::new(vec![immutable, cache_marked]);
        let edits = normalize_pass(&conv, conv.len(), 5);
        assert!(
            edits.is_empty(),
            "immutable/cache-marked messages must never be touched"
        );
    }

    #[test]
    fn conversation_pass_below_min_len_is_never_touched() {
        let short = "a  b";
        let conv = Conversation::new(vec![msg(short)]);
        let edits = normalize_pass(&conv, conv.len(), 200);
        assert!(edits.is_empty());
    }

    #[test]
    fn conversation_pass_deterministic_across_runs_identical_output_bytes() {
        // Acceptance criterion: a message with redundant whitespace shortens
        // deterministically across two runs with identical output bytes.
        let messy = "This   has  lots of  redundant   whitespace and trailing spaces   \n\n\n\n\
                     and it repeats across   multiple    lines with   trailing space  \n\n\n\
                     to make sure it clears the min_len gate for this test.";
        let conv = Conversation::new(vec![msg(messy)]);
        let a = normalize_pass(&conv, conv.len(), 20);
        let b = normalize_pass(&conv, conv.len(), 20);
        assert_eq!(a, b);
        assert!(!a.is_empty(), "fixture should actually produce edits");

        let (_, _, script) = &a[0];
        let mut out_a = String::new();
        script.apply(messy, &mut out_a);
        let mut out_b = String::new();
        script.apply(messy, &mut out_b);
        assert_eq!(out_a, out_b);
        assert!(
            out_a.len() < messy.len(),
            "normalization should shorten the buffer"
        );
    }

    #[test]
    fn per_message_purity_independent_of_other_messages() {
        // I3-flavored check: message 0's normalization decision must be identical
        // whether or not a later message repeats the same redundant whitespace.
        let messy = "one  two   three\n\n\n\nfour.";
        let alone = Conversation::new(vec![msg(messy)]);
        let with_more = Conversation::new(vec![msg(messy), msg(messy)]);
        let a = normalize_pass(&alone, 1, 5);
        let b = normalize_pass(&with_more, 1, 5);
        assert_eq!(a, b);
    }
}
