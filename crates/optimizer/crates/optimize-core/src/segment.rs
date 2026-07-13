//! Structural segmenter (ALGO §5.2). Splits a text buffer into spans that are safe to
//! compress (`Prose`) vs. spans whose *syntax* carries meaning (fenced code, inline code,
//! URLs, tables), which are protected byte-for-byte. Single left-to-right scan, no regex,
//! no substring allocation.
//!
//! Priority order at each scan position (matches ALGO §5.2):
//!   1. Fenced code (``` or ~~~ at start of line) — an unmatched opening fence protects to
//!      end-of-buffer (safe default; guarantees fence-pairing, invariant I6).
//!   2. Table lines (`|` or `+---`, high non-alnum ratio).
//!   3. Inline code (`` `code` ``, matching backtick-run length).
//!   4. URLs (`scheme://non-ws+`).
//!   5. Everything else — Prose.

use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegKind {
    Prose,
    FencedCode,
    InlineCode,
    Url,
    Table,
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub range: Range<usize>,
    pub kind: SegKind,
}

/// Structural split: fenced code, inline code, URLs, and tables are protected as distinct
/// spans; everything else is `Prose`. Never allocates substrings; only records byte ranges.
pub fn segment(text: &str, out: &mut Vec<Segment>) {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut prose_start = 0usize;

    while i < len {
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';

        if at_line_start {
            if let Some(end) = match_fence(text, i) {
                flush_prose(prose_start, i, out);
                out.push(Segment {
                    range: i..end,
                    kind: SegKind::FencedCode,
                });
                i = end;
                prose_start = i;
                continue;
            }
            if let Some(end) = match_table_line(text, i) {
                flush_prose(prose_start, i, out);
                out.push(Segment {
                    range: i..end,
                    kind: SegKind::Table,
                });
                i = end;
                prose_start = i;
                continue;
            }
        }

        if bytes[i] == b'`' {
            if let Some(end) = match_inline_code(text, i) {
                flush_prose(prose_start, i, out);
                out.push(Segment {
                    range: i..end,
                    kind: SegKind::InlineCode,
                });
                i = end;
                prose_start = i;
                continue;
            }
        }

        if bytes[i].is_ascii_alphabetic() {
            if let Some(end) = match_url(text, i) {
                flush_prose(prose_start, i, out);
                out.push(Segment {
                    range: i..end,
                    kind: SegKind::Url,
                });
                i = end;
                prose_start = i;
                continue;
            }
        }

        i += text[i..].chars().next().map(char::len_utf8).unwrap_or(1);
    }

    flush_prose(prose_start, len, out);
    if out.is_empty() {
        // Empty buffer: keep the invariant of at least one segment.
        out.push(Segment {
            range: 0..len,
            kind: SegKind::Prose,
        });
    }
}

fn flush_prose(start: usize, end: usize, out: &mut Vec<Segment>) {
    if end > start {
        out.push(Segment {
            range: start..end,
            kind: SegKind::Prose,
        });
    }
}

/// Matches a fenced-code block starting at `i` (must be start of a line). Returns the byte
/// offset one past the closing fence line, or one past end-of-buffer if unmatched (an
/// unmatched opening fence protects the rest of the buffer — the safe default).
fn match_fence(text: &str, i: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let ch = bytes[i];
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let mut j = i;
    while j < len && bytes[j] == ch {
        j += 1;
    }
    let fence_len = j - i;
    if fence_len < 3 {
        return None;
    }

    let opening_line_end = text[j..].find('\n').map(|o| j + o + 1).unwrap_or(len);
    let mut k = opening_line_end;
    while k < len {
        let next_line_end = text[k..].find('\n').map(|o| k + o + 1).unwrap_or(len);
        let line = text[k..next_line_end].trim_end_matches('\n');
        let trimmed = line.trim();
        if !trimmed.is_empty() && trimmed.len() >= fence_len && trimmed.bytes().all(|b| b == ch) {
            return Some(next_line_end);
        }
        k = next_line_end;
    }
    // No closing fence found: protect to end-of-buffer (safe default).
    Some(len)
}

/// Matches an inline-code span `` `...` `` starting at `i` (a run of 1+ backticks, closed
/// by a run of the SAME length). Never crosses a newline (conservative: falls back to
/// Prose rather than risk over-protecting across paragraphs).
fn match_inline_code(text: &str, i: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut j = i;
    while j < len && bytes[j] == b'`' {
        j += 1;
    }
    let open_len = j - i;

    let mut k = j;
    while k < len {
        match bytes[k] {
            b'\n' => return None,
            b'`' => {
                let run_start = k;
                let mut m = k;
                while m < len && bytes[m] == b'`' {
                    m += 1;
                }
                if m - run_start == open_len {
                    return Some(m);
                }
                k = m;
            }
            _ => k += 1,
        }
    }
    None
}

/// Matches `scheme://non-ws+` starting at `i`. Deleting half a URL is worse than keeping
/// it whole.
fn match_url(text: &str, i: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut j = i;
    while j < len && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'+' | b'-' | b'.')) {
        j += 1;
    }
    if j + 3 > len || &bytes[j..j + 3] != b"://" {
        return None;
    }
    let mut k = j + 3;
    while k < len && !bytes[k].is_ascii_whitespace() {
        k += 1;
    }
    if k == j + 3 {
        return None; // scheme:// with nothing after it isn't a URL worth protecting
    }
    Some(k)
}

/// A line qualifies as `Table` when it contains `|` or `+---` AND its non-alphanumeric
/// character ratio exceeds 0.4 (ALGO §5.2 rule 4). Returns the byte offset one past the
/// line (including its trailing newline, if any).
fn match_table_line(text: &str, i: usize) -> Option<usize> {
    let len = text.len();
    let line_end = text[i..].find('\n').map(|o| i + o + 1).unwrap_or(len);
    let content = text[i..line_end].trim_end_matches('\n');
    if content.is_empty() || !(content.contains('|') || content.contains("+---")) {
        return None;
    }
    let total = content.chars().count();
    if total == 0 {
        return None;
    }
    let non_alnum = content.chars().filter(|c| !c.is_alphanumeric()).count();
    if (non_alnum as f32) / (total as f32) > 0.4 {
        Some(line_end)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(SegKind, &str)> {
        let mut out = Vec::new();
        segment(text, &mut out);
        out.into_iter().map(|s| (s.kind, &text[s.range])).collect()
    }

    /// Reassembling every segment's slice must reproduce the original text exactly, in
    /// order, with no gaps or overlaps.
    fn assert_covers(text: &str, segs: &[Segment]) {
        let mut prev = 0usize;
        for s in segs {
            assert_eq!(s.range.start, prev, "gap/overlap before {:?}", s.range);
            prev = s.range.end;
        }
        assert_eq!(prev, text.len(), "segments do not cover the whole buffer");
    }

    #[test]
    fn prose_when_plain() {
        let mut out = Vec::new();
        segment("just some plain prose here", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, SegKind::Prose);
    }

    #[test]
    fn empty_buffer_yields_one_prose_segment() {
        let mut out = Vec::new();
        segment("", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, SegKind::Prose);
        assert_eq!(out[0].range, 0..0);
    }

    #[test]
    fn matched_fence_protects_only_the_fenced_span() {
        let text = "before\n```\ncode line\n```\nafter";
        let ks = kinds(text);
        assert_eq!(
            ks,
            vec![
                (SegKind::Prose, "before\n"),
                (SegKind::FencedCode, "```\ncode line\n```\n"),
                (SegKind::Prose, "after"),
            ]
        );
    }

    #[test]
    fn unmatched_fence_protects_to_end_of_buffer() {
        let text = "before\n```\nno closing fence here";
        let mut out = Vec::new();
        segment(text, &mut out);
        assert_covers(text, &out);
        // Everything from the opening fence onward is one protected span.
        let fence_start = text.find("```").unwrap();
        assert!(out
            .iter()
            .any(|s| s.kind == SegKind::FencedCode && s.range == (fence_start..text.len())));
    }

    #[test]
    fn tilde_fence_also_protected() {
        let text = "~~~\nrust code\n~~~";
        let ks = kinds(text);
        assert_eq!(ks, vec![(SegKind::FencedCode, text)]);
    }

    #[test]
    fn inline_code_protected_prose_around_it() {
        let text = "run `cargo test` to check";
        let ks = kinds(text);
        assert_eq!(
            ks,
            vec![
                (SegKind::Prose, "run "),
                (SegKind::InlineCode, "`cargo test`"),
                (SegKind::Prose, " to check"),
            ]
        );
    }

    #[test]
    fn unclosed_inline_backtick_falls_back_to_prose() {
        let text = "a stray ` backtick with no partner";
        let mut out = Vec::new();
        segment(text, &mut out);
        assert!(out.iter().all(|s| s.kind == SegKind::Prose));
        assert_covers(text, &out);
    }

    #[test]
    fn url_protected_whole() {
        let text = "see https://example.com/path?q=1 for details";
        let ks = kinds(text);
        assert_eq!(
            ks,
            vec![
                (SegKind::Prose, "see "),
                (SegKind::Url, "https://example.com/path?q=1"),
                (SegKind::Prose, " for details"),
            ]
        );
    }

    #[test]
    fn table_lines_protected() {
        let text = "intro line\n| a | b |\n|---|---|\n| 1 | 2 |\noutro line";
        let mut out = Vec::new();
        segment(text, &mut out);
        assert_covers(text, &out);
        let table_bytes: usize = out
            .iter()
            .filter(|s| s.kind == SegKind::Table)
            .map(|s| s.range.len())
            .sum();
        let expected: usize = "| a | b |\n|---|---|\n| 1 | 2 |\n".len();
        assert_eq!(table_bytes, expected);
        assert_eq!(out.first().unwrap().kind, SegKind::Prose);
        assert_eq!(out.last().unwrap().kind, SegKind::Prose);
    }

    #[test]
    fn mixed_prose_and_fence_only_prose_bytes_are_deletable() {
        // Regression for the acceptance criterion: a mixed buffer must classify the
        // fenced block as protected and everything else as Prose, byte-exact.
        let text = "Please review this:\n```\nfn main() {}\n```\nThanks a lot for your help.";
        let mut out = Vec::new();
        segment(text, &mut out);
        assert_covers(text, &out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].kind, SegKind::Prose);
        assert_eq!(&text[out[0].range.clone()], "Please review this:\n");
        assert_eq!(out[1].kind, SegKind::FencedCode);
        assert_eq!(&text[out[1].range.clone()], "```\nfn main() {}\n```\n");
        assert_eq!(out[2].kind, SegKind::Prose);
        assert_eq!(&text[out[2].range.clone()], "Thanks a lot for your help.");
    }

    #[test]
    fn segments_always_cover_the_buffer_exactly() {
        for t in [
            "",
            "plain",
            "```\nfenced\n```",
            "`inline` and https://x.io/y and | table | row |\n|---|---|",
            "no closing ```fence",
            "line1\n| t | a | b |\n|---|---|---|\nline3",
        ] {
            let mut out = Vec::new();
            segment(t, &mut out);
            assert_covers(t, &out);
        }
    }
}
