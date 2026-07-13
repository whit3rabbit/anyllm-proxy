//! The filter executor — byte-for-byte port of `lineFilter.ts::applyLineFilter`.
//! Stage order (must not change; the vendored `tests[]` assert exact output):
//!   stripAnsi -> filterStderr -> replace -> matchOutput short-circuit
//!   -> strip -> keep -> collapse -> truncateLineAt -> per-filter dedup
//!   -> smartTruncate -> onEmpty fallback

use crate::dedup::deduplicate_repeated_lines;
use crate::filter::RtkFilter;
use crate::smart_truncate::{smart_truncate, SmartTruncateOptions};
use regex::{NoExpand, Regex};
use std::collections::HashSet;
use std::sync::OnceLock;

/// Split on `\r?\n` (matches JS `text.split(/\r?\n/)`): split on `\n`, then drop
/// a single trailing `\r` from each piece. A lone `\r` (no `\n`) is preserved.
pub fn split_lines(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect()
}

/// Borrowing variant used by the deduplicator (avoids allocating owned Strings
/// just to compare runs). Returns `&str` slices where the trailing `\r` is
/// trimmed; a `\r\n` input yields the same logical lines as `split_lines`.
pub fn split_lines_ref(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

fn ansi_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap())
}

fn stderr_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // JS: /^\s*(?:stderr|err)\s*(?:\||:)\s*/i
    RE.get_or_init(|| {
        regex::RegexBuilder::new(r"^\s*(?:stderr|err)\s*(?:\||:)\s*")
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

fn strip_ansi(line: &str) -> String {
    ansi_regex().replace_all(line, NoExpand("")).into_owned()
}

fn normalize_stderr_prefix(line: &str) -> String {
    stderr_regex().replace(line, NoExpand("")).into_owned()
}

/// Unicode-safe per-line truncation — port of `truncateUnicodeSafe` (operates
/// on code points via `chars()`, matching JS `Array.from`).
fn truncate_unicode_safe(line: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return line.to_string();
    }
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= max_chars {
        return line.to_string();
    }
    if max_chars <= 3 {
        return chars.into_iter().take(max_chars).collect();
    }
    let kept: String = chars.iter().take(max_chars - 3).collect();
    format!("{kept}...")
}

/// Run one filter over `text`, returning the compressed text. Mirrors
/// `applyLineFilter(...).text`. `max_lines_override` replaces the filter's own
/// `maxLines` for the final `smartTruncate` (the engine passes an intensity-
/// scaled budget; the conformance suite passes `None` to match `verify.ts`).
pub fn apply_line_filter(
    text: &str,
    filter: &RtkFilter,
    max_lines_override: Option<usize>,
) -> String {
    let mut lines = split_lines(text);

    if filter.strip_ansi {
        lines = lines.iter().map(|l| strip_ansi(l)).collect();
    }

    if filter.filter_stderr {
        lines = lines.iter().map(|l| normalize_stderr_prefix(l)).collect();
    }

    for (pattern, replacement) in &filter.replace {
        lines = lines
            .iter()
            .map(|l| pattern.replace_all(l, NoExpand(replacement)).into_owned())
            .collect();
    }

    if !filter.match_output.is_empty() {
        let blob = lines.join("\n");
        for rule in &filter.match_output {
            if !rule.pattern.is_match(&blob) {
                continue;
            }
            if let Some(unless) = &rule.unless {
                if unless.is_match(&blob) {
                    continue;
                }
            }
            return rule.message.clone();
        }
    }

    if !filter.strip_patterns.is_empty() {
        lines.retain(|l| !filter.strip_patterns.iter().any(|p| p.is_match(l)));
    }

    if !filter.keep_patterns.is_empty() {
        let kept: Vec<String> = lines
            .iter()
            .filter(|l| filter.keep_patterns.iter().any(|p| p.is_match(l)))
            .cloned()
            .collect();
        if !kept.is_empty() {
            lines = kept;
        }
    }

    if !filter.collapse_patterns.is_empty() {
        let mut seen: HashSet<String> = HashSet::new();
        lines.retain(|line| {
            if !filter.collapse_patterns.iter().any(|p| p.is_match(line)) {
                return true;
            }
            let key = line.trim().to_string();
            seen.insert(key)
        });
    }

    if filter.truncate_line_at > 0 {
        lines = lines
            .iter()
            .map(|l| truncate_unicode_safe(l, filter.truncate_line_at))
            .collect();
    }

    if filter.deduplicate {
        let deduped = deduplicate_repeated_lines(&lines.join("\n"), 3);
        if deduped.collapsed > 0 {
            lines = split_lines(&deduped.text);
        }
    }

    let truncated = smart_truncate(
        &lines.join("\n"),
        &SmartTruncateOptions {
            max_lines: max_lines_override.unwrap_or(filter.max_lines),
            max_chars: 0,
            preserve_head: filter.preserve_head,
            preserve_tail: filter.preserve_tail,
            priority_patterns: &filter.priority_patterns,
        },
    );

    if truncated.text.trim().is_empty() && !filter.on_empty.is_empty() {
        filter.on_empty.clone()
    } else {
        truncated.text
    }
}
