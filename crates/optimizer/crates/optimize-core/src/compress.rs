//! `compress_message`: the pure function at the heart of FFEC.
//!
//! Output depends ONLY on the message's block bytes and the policy — no cross-message
//! context, no clocks, no randomness, no global ratio. Recompressing the same message
//! therefore yields identical bytes across turns (cache stability, invariant I3).

use crate::edit::EditScript;
use crate::error::OptimizeError;
use crate::policy::CompressionPolicy;
use crate::segment::{segment, SegKind};
use crate::select::{emit_edits, select_keep, split_words};
use crate::traits::TokenScorer;
use crate::types::{BufferId, ContentBlock, Message, Role};
use crate::workspace::Workspace;

/// Returns one `EditScript` per compressible buffer in the message. Immutable buffers
/// (ToolUse/Opaque) and system-role text are never touched. ToolResult value-level
/// compression lives in `anyllm_optimize_passes`; here only Text blocks are handled.
pub fn compress_message(
    msg: &Message,
    policy: &CompressionPolicy,
    scorer: &dyn TokenScorer,
    ws: &mut Workspace,
) -> Result<Vec<(BufferId, EditScript)>, OptimizeError> {
    let mut result = Vec::new();
    if msg.role == Role::System {
        return Ok(result); // system is Immutable; belt-and-braces
    }
    let ratio = policy.ratios.text_ratio(msg.role);
    if ratio >= 1.0 {
        return Ok(result);
    }

    for (bi, block) in msg.blocks.iter().enumerate() {
        let ContentBlock::Text(text) = block else {
            continue;
        };
        if text.len() < policy.min_len {
            continue;
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
            let scores = scorer.score_words(&wstrs, ws)?;
            let keep = select_keep(&ws.words, text, &scores, ratio, &policy.force);
            emit_edits(text, &ws.words, &keep, &mut edits);
        }

        if edits.is_empty() {
            continue;
        }
        let script = EditScript::new(edits);
        // Fail-open per buffer: an invalid script is silently skipped, not applied.
        if script.validate(text).is_ok() {
            result.push((BufferId(bi), script));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::UniformScorer;

    fn user_msg(text: &str) -> Message {
        Message {
            role: Role::User,
            blocks: vec![ContentBlock::Text(text.into())],
            protection: crate::types::Protection::Mutable,
            client_cache_marker: false,
        }
    }

    #[test]
    fn compresses_long_prose_deterministically() {
        let text = "The quick brown fox jumps over the lazy dog and then it runs away very \
                    quickly across the wide green field toward the distant blue mountains beyond \
                    the winding river and the tall dark trees under a bright and cloudless sky."
            .to_string();
        let msg = user_msg(&text);
        let policy = CompressionPolicy::default();
        let mut ws = Workspace::new();
        let a = compress_message(&msg, &policy, &UniformScorer, &mut ws).unwrap();
        let b = compress_message(&msg, &policy, &UniformScorer, &mut ws).unwrap();
        assert_eq!(a, b, "compression must be deterministic");
        assert_eq!(a.len(), 1);
        // it actually removed something
        assert!(a[0].1.bytes_removed() > 0);
        // and the result is a valid subsequence
        let mut out = String::new();
        a[0].1.apply(&text, &mut out);
        assert!(out.len() < text.len());
    }

    #[test]
    fn skips_short_buffers() {
        let msg = user_msg("too short");
        let mut ws = Workspace::new();
        let r =
            compress_message(&msg, &CompressionPolicy::default(), &UniformScorer, &mut ws).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn fenced_code_block_is_byte_identical_while_prose_compresses() {
        // Mixed buffer: long prose around a fenced code block. The fence must survive
        // byte-for-byte (I6); only the surrounding Prose words may be dropped.
        let text = format!(
            "Please review this carefully and let me know exactly what you think about it \
             because it really matters a great deal to the whole team right now:\n```\n{}\n``` \
             end of the message, thanks so much in advance for taking the time to review this.",
            "x = 1;\n".repeat(20)
        );
        let msg = user_msg(&text);
        let mut ws = Workspace::new();
        let r =
            compress_message(&msg, &CompressionPolicy::default(), &UniformScorer, &mut ws).unwrap();
        assert_eq!(r.len(), 1, "the surrounding prose should compress");

        let mut out = String::new();
        r[0].1.apply(&text, &mut out);
        assert!(
            out.len() < text.len(),
            "prose words should have been dropped"
        );

        let fence_start = text.find("```").unwrap();
        let fence_end = text.rfind("```").unwrap() + 3;
        let fenced_src = &text[fence_start..fence_end];
        assert!(
            out.contains(fenced_src),
            "fenced block bytes must be unchanged: {out:?}"
        );
    }

    #[test]
    fn unmatched_fence_still_protects_whole_buffer() {
        let text = format!(
            "here is code:\n```\n{}\nno closing fence",
            "x = 1;".repeat(60)
        );
        let msg = user_msg(&text);
        let mut ws = Workspace::new();
        let r =
            compress_message(&msg, &CompressionPolicy::default(), &UniformScorer, &mut ws).unwrap();
        // The only Prose span is the short "here is code:\n" prefix, below min_len's reach
        // for meaningful compression but still processed; assert the fence contents are
        // never touched regardless.
        if let Some((_, script)) = r.first() {
            let mut out = String::new();
            script.apply(&text, &mut out);
            // Only the short prefix before the fence can have shrunk; the fence and
            // everything after it (unmatched -> protected to end-of-buffer) must be an
            // untouched suffix of the original.
            let fence_start = text.find("```").unwrap();
            let suffix_len = text.len() - fence_start;
            assert_eq!(&out[out.len() - suffix_len..], &text[fence_start..]);
        }
    }
}
