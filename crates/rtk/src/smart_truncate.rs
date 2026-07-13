//! Head/tail + priority-line retention — port of `smartTruncate.ts`.
//! Inserts the exact `[rtk:truncated N lines]` marker (conformance-critical).
//!
//! The `apply_line_filter` path only ever passes `max_lines` (no `max_chars`),
//! but the char-limit branch is ported for completeness. `selected.includes`
//! is an O(n) scan per candidate; kept O(n^2) to match JS value-dedup exactly.
//! ponytail: line counts are small (bounded by preserve_head + preserve_tail +
//! priority lines), so the quadratic scan never matters.

use crate::line_filter::split_lines;
use regex::Regex;

pub struct SmartTruncateOptions<'a> {
    pub max_lines: usize,
    pub max_chars: usize,
    pub preserve_head: usize,
    pub preserve_tail: usize,
    pub priority_patterns: &'a [Regex],
}

pub struct SmartTruncateResult {
    pub text: String,
    pub truncated: bool,
    #[allow(dead_code)]
    pub dropped_lines: usize,
}

pub fn smart_truncate(text: &str, opts: &SmartTruncateOptions) -> SmartTruncateResult {
    let lines = split_lines(text);
    let over_line_limit = opts.max_lines > 0 && lines.len() > opts.max_lines;
    let over_char_limit = opts.max_chars > 0 && text.chars().count() > opts.max_chars;
    if !over_line_limit && !over_char_limit {
        return SmartTruncateResult {
            text: text.to_string(),
            truncated: false,
            dropped_lines: 0,
        };
    }

    let preserve_head = opts.preserve_head;
    let preserve_tail = opts.preserve_tail;

    let priority_lines: Vec<&String> = if opts.priority_patterns.is_empty() {
        Vec::new()
    } else {
        lines
            .iter()
            .filter(|line| opts.priority_patterns.iter().any(|p| p.is_match(line)))
            .collect()
    };

    let head: Vec<String> = lines.iter().take(preserve_head).cloned().collect();
    let tail: Vec<String> = if preserve_tail > 0 {
        let start = lines.len().saturating_sub(preserve_tail);
        lines[start..].to_vec()
    } else {
        Vec::new()
    };

    let mut selected: Vec<String> = head.clone();
    for line in &priority_lines {
        if !selected.iter().any(|s| s == *line) {
            selected.push((*line).clone());
        }
    }
    let tail_start = lines.len() - tail.len();
    for (offset, line) in tail.iter().enumerate() {
        let original_index = tail_start + offset;
        if original_index >= preserve_head && !selected.iter().any(|s| s == line) {
            selected.push(line.clone());
        }
    }

    let mut dropped_lines = lines.len().saturating_sub(selected.len());
    let head_len = head.len();
    let mut result_parts: Vec<String> = Vec::with_capacity(selected.len() + 1);
    result_parts.extend(selected.iter().take(head_len).cloned());
    result_parts.push(format!("[rtk:truncated {dropped_lines} lines]"));
    result_parts.extend(selected.iter().skip(head_len).cloned());
    let mut result = result_parts.join("\n");

    // Char-limit second pass (not exercised by the filter path).
    if opts.max_chars > 0 && result.chars().count() > opts.max_chars {
        let lines_before = result.split('\n').count();
        let marker = "\n[rtk:truncated by chars]\n";
        let budget = opts.max_chars.saturating_sub(marker.chars().count());
        if budget == 0 {
            let clipped: String = marker.chars().take(opts.max_chars).collect();
            return SmartTruncateResult {
                text: clipped,
                truncated: true,
                dropped_lines,
            };
        }
        // Mirror JS: headChars = ceil(budget*0.55), tailChars = budget-headChars.
        let head_chars = ((budget as f64) * 0.55).ceil() as usize;
        let tail_chars = budget.saturating_sub(head_chars);
        let chars: Vec<char> = result.chars().collect();
        let head_text: String = chars.iter().take(head_chars).collect();
        let tail_text: String = if tail_chars > 0 {
            chars[chars.len().saturating_sub(tail_chars)..]
                .iter()
                .collect()
        } else {
            String::new()
        };
        result = format!("{head_text}{marker}{tail_text}");
        if result.chars().count() > opts.max_chars {
            result = result.chars().take(opts.max_chars).collect();
        }
        // Char-limit truncation may drop additional lines beyond the line-level
        // selection; update dropped_lines to reflect the total.
        let lines_after = result.split('\n').count();
        dropped_lines = dropped_lines.saturating_add(lines_before.saturating_sub(lines_after));
    }

    SmartTruncateResult {
        text: result,
        truncated: true,
        dropped_lines,
    }
}
