//! Word splitting, deterministic top-k selection, and edit emission.
//! LLMLingua-2's importance top-k, made deterministic (quantize before comparing) and
//! structure-safe (force-keeps for punctuation/digits/first word).

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// A word: a byte range in the buffer with no surrounding whitespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub range: Range<usize>,
}

/// Quantize before ANY comparison. Kills float-drift nondeterminism across
/// runtimes/threads. `1e-4` resolution is far below meaningful score differences.
#[inline]
pub fn quantize(p: f32) -> u16 {
    (p.clamp(0.0, 1.0) * 10_000.0) as u16
}

/// Structural force-keep rules (the paper's `force_tokens`, generalized).
#[derive(Clone, Debug)]
pub struct ForceRules {
    pub keep_chars: &'static [char],
    pub keep_digits: bool,
    pub keep_first_word: bool,
}

impl Default for ForceRules {
    fn default() -> Self {
        Self {
            keep_chars: &['\n', '?', '!', ':'],
            keep_digits: true,
            keep_first_word: true,
        }
    }
}

/// Unicode word boundaries; punctuation runs are their own "words"; whitespace is NOT a
/// word (it is glue, handled at edit emission). Byte ranges are into `text`.
pub fn split_words(text: &str, span: Range<usize>, out: &mut Vec<Word>) {
    let slice = &text[span.clone()];
    let mut acc = 0usize;
    for w in slice.split_word_bounds() {
        let start = span.start + acc;
        acc += w.len();
        if !w.chars().all(char::is_whitespace) {
            out.push(Word {
                range: start..span.start + acc,
            });
        }
    }
}

/// Returns a keep-mask over `words`. INVARIANTS: preserves order (it's a mask),
/// `n_keep >= forced count`, ties broken by position (earlier wins). Deterministic.
pub fn select_keep(
    words: &[Word],
    text: &str,
    scores: &[f32],
    ratio: f32,
    force: &ForceRules,
) -> Vec<bool> {
    let n = words.len();
    let mut keep = vec![false; n];
    if n == 0 {
        return keep;
    }

    // 1. forced keeps
    for (i, w) in words.iter().enumerate() {
        let s = &text[w.range.clone()];
        if s.chars().any(|c| force.keep_chars.contains(&c))
            || (force.keep_digits && s.chars().any(|c| c.is_ascii_digit()))
        {
            keep[i] = true;
        }
    }
    if force.keep_first_word {
        keep[0] = true;
    }

    // 2. budget: at least the forced count, at most ceil(ratio * n)
    let forced = keep.iter().filter(|&&k| k).count();
    let n_keep = ((n as f32 * ratio).ceil() as usize).max(forced);

    // 3. rank the rest by (quantized score desc, position asc) — fully deterministic
    let mut order: Vec<usize> = (0..n).filter(|&i| !keep[i]).collect();
    order.sort_by(|&a, &b| {
        quantize(scores[b])
            .cmp(&quantize(scores[a]))
            .then(a.cmp(&b))
    });
    for &i in order.iter().take(n_keep.saturating_sub(forced)) {
        keep[i] = true;
    }
    keep
}

/// Convert a keep-mask to Delete/Replace edits over `text`.
///
/// - A run of consecutive dropped words is deleted as one edit.
/// - When a kept word follows the run, the run consumes the whitespace gap up to it (so
///   no double space remains); if that gap contains a newline, the whole run is
///   Replaced with a single "\n" to keep paragraph/line structure readable.
/// - When the run reaches the end of the word list, it consumes the PRECEDING gap
///   instead (never the trailing content, which may belong to another segment).
///
/// Emits sorted, non-overlapping edits appended to `out`.
pub fn emit_edits(text: &str, words: &[Word], keep: &[bool], out: &mut Vec<super::edit::Edit>) {
    use super::edit::Edit;
    let n = words.len();
    debug_assert_eq!(keep.len(), n);
    let mut i = 0usize;
    while i < n {
        if keep[i] {
            i += 1;
            continue;
        }
        let a = i;
        while i < n && !keep[i] {
            i += 1;
        }
        let b = i - 1; // last dropped index in this run

        if i < n {
            // A kept word follows at index `i`. Consume the gap between b and i.
            let gap_start = words[b].range.end;
            let gap_end = words[i].range.start;
            let gap_has_newline =
                memchr::memchr(b'\n', &text.as_bytes()[gap_start..gap_end]).is_some();
            if gap_has_newline {
                // Preserve one newline: eat the PRECEDING gap so no trailing space is left
                // before it (e.g. "keep drop\nkeep2" -> "keep\nkeep2").
                let del_start = if a > 0 {
                    words[a - 1].range.end
                } else {
                    words[a].range.start
                };
                out.push(Edit::Replace {
                    range: del_start..gap_end,
                    text: "\n".to_string(),
                });
            } else {
                // Eat the FOLLOWING gap so exactly one space remains before the next word.
                out.push(Edit::Delete(words[a].range.start..gap_end));
            }
        } else {
            // Run reaches the last word: eat the preceding gap if any, keep tail intact.
            let del_start = if a > 0 {
                words[a - 1].range.end
            } else {
                words[a].range.start
            };
            let del_end = words[b].range.end;
            out.push(Edit::Delete(del_start..del_end));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::edit::{Edit, EditScript};
    use super::*;

    fn words_of(text: &str) -> Vec<Word> {
        let mut v = Vec::new();
        split_words(text, 0..text.len(), &mut v);
        v
    }

    #[test]
    fn split_skips_whitespace() {
        let words = words_of("the  quick\nfox");
        let strs: Vec<&str> = words
            .iter()
            .map(|w| &"the  quick\nfox"[w.range.clone()])
            .collect();
        assert_eq!(strs, vec!["the", "quick", "fox"]);
    }

    #[test]
    fn select_respects_forced_and_ratio() {
        let text = "alpha beta gamma delta 42";
        let words = words_of(text);
        // uniform scores; ratio 0.4 of 5 words = ceil(2.0)=2, but "42" (digit) and first
        // word "alpha" are forced -> at least 2 kept.
        let scores = vec![0.5; words.len()];
        let keep = select_keep(&words, text, &scores, 0.4, &ForceRules::default());
        let kept = keep.iter().filter(|&&k| k).count();
        assert!(kept >= 2);
        assert!(keep[0]); // first word forced
                          // the digit word "42" is last -> forced
        assert!(*keep.last().unwrap());
    }

    #[test]
    fn emit_produces_valid_subsequence() {
        let text = "one two three four five";
        let words = words_of(text);
        // drop "two" and "four"
        let keep = vec![true, false, true, false, true];
        let mut edits = Vec::new();
        emit_edits(text, &words, &keep, &mut edits);
        let script = EditScript::new(edits);
        assert!(script.validate(text).is_ok());
        let mut out = String::new();
        script.apply(text, &mut out);
        assert_eq!(out, "one three five");
    }

    #[test]
    fn emit_preserves_newline() {
        let text = "keep drop\nkeep2";
        let words = words_of(text);
        let keep = vec![true, false, true];
        let mut edits = Vec::new();
        emit_edits(text, &words, &keep, &mut edits);
        let script = EditScript::new(edits);
        assert!(script.validate(text).is_ok());
        let mut out = String::new();
        script.apply(text, &mut out);
        assert_eq!(out, "keep\nkeep2");
    }

    #[test]
    fn emit_tail_run_eats_preceding_gap() {
        let text = "keep drop1 drop2";
        let words = words_of(text);
        let keep = vec![true, false, false];
        let mut edits = Vec::new();
        emit_edits(text, &words, &keep, &mut edits);
        // one coalesced delete for the tail run
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0], Edit::Delete(4..16));
        let script = EditScript::new(edits);
        let mut out = String::new();
        script.apply(text, &mut out);
        assert_eq!(out, "keep");
    }
}
