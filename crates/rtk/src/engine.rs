//! Filter selection + single-text compression — port of `filterLoader.ts::matchRtkFilter`
//! and `index.ts::processRtkText` (built-in catalog only; project/global custom
//! filters, renderers, code-strip, grouping, and raw-output retention are deferred).
//!
//! Config is fixed to the OmniRoute `DEFAULT_RTK_CONFIG` defaults relevant here:
//! `maxLinesPerResult=120`, `maxCharsPerResult=12000`, `deduplicateThreshold=3`,
//! `intensity="standard"` (final head/tail = 24). Intensity/config exposure is a
//! later phase.

use crate::dedup::deduplicate_repeated_lines;
use crate::detector::{detect_command_type, CommandDetection};
use crate::filter::RtkFilter;
use crate::line_filter::apply_line_filter;
use crate::smart_truncate::{smart_truncate, SmartTruncateOptions};
use regex::{Regex, RegexBuilder};
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/filters_generated.rs"));

const MAX_LINES_PER_RESULT: usize = 120;
const MAX_CHARS_PER_RESULT: usize = 12000;
const DEDUP_THRESHOLD: usize = 3;
const FINAL_HEAD_TAIL: usize = 24; // standard intensity

/// Built-in filters, parsed once, sorted by priority desc then id asc
/// (mirrors `loadRtkFilters` sort: `b.priority - a.priority || a.id.localeCompare(b.id)`).
pub fn filters() -> &'static Vec<RtkFilter> {
    static FILTERS: OnceLock<Vec<RtkFilter>> = OnceLock::new();
    FILTERS.get_or_init(|| {
        let mut v: Vec<RtkFilter> = FILTER_JSONS
            .iter()
            .filter_map(|j| RtkFilter::from_json(j))
            .collect();
        v.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.id.cmp(&b.id)));
        v
    })
}

fn error_markers_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Case-insensitive to match default_priority_re — "error:", "Error:",
        // and "ERROR:" all count as having error markers so document-like
        // classification is consistent regardless of capitalization.
        RegexBuilder::new(r"Error:|Exception:|Traceback\b")
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

fn default_priority_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r"error|failed|exception|traceback|TS\d{4}|FAIL|✖")
            .case_insensitive(true)
            .build()
            .unwrap()
    })
}

/// Select the best filter for an already-computed detection + raw text.
/// Resolution order matches `matchRtkFilter`: by detected type → by command
/// pattern → by content pattern → generic-output fallback.
fn match_filter_for(detection: &CommandDetection, text: &str) -> Option<&'static RtkFilter> {
    let detected_command = detection.command.clone().unwrap_or_default();
    let fs = filters();
    fs.iter()
        .find(|f| f.command_types.iter().any(|t| t == &detection.r#type))
        .or_else(|| {
            fs.iter().find(|f| {
                !detected_command.is_empty()
                    && f.command_patterns
                        .iter()
                        .any(|p| p.is_match(&detected_command))
            })
        })
        .or_else(|| {
            fs.iter()
                .find(|f| f.match_patterns.iter().any(|p| p.is_match(text)))
        })
        .or_else(|| {
            fs.iter()
                .find(|f| f.command_types.iter().any(|t| t == "generic-output"))
        })
}

/// Public matcher (used by tests) — detects then selects.
pub fn match_filter(text: &str, command: Option<&str>) -> Option<&'static RtkFilter> {
    let detection = detect_command_type(text, command);
    match_filter_for(&detection, text)
}

pub struct ProcessResult {
    pub text: String,
    pub compressed: bool,
}

/// Compress one tool-output string — port of `processRtkText`.
///
/// `skip_filters` (from a non-shell tool) skips filter matching but still runs
/// engine dedup + the final hard cap. A "document-like read" (unknown type, no
/// command, no error markers — e.g. a file read) skips BOTH the filter and the
/// final truncation so its middle is never dropped.
pub fn process_rtk_text(text: &str, command: Option<&str>, skip_filters: bool) -> ProcessResult {
    let detection = detect_command_type(text, command);
    let has_error_markers = error_markers_re().is_match(text);
    // When skip_filters is true (e.g. a Read tool instead of a shell), the caller
    // expects text preservation. Override is_document_like to true so both the
    // filter stage AND the outer smart_truncate are bypassed — the detection type
    // is based on content, not caller intent, and could falsely classify a file
    // read as e.g. git-status output, which would then truncate the file middle.
    let is_document_like = skip_filters
        || (detection.r#type == "unknown" && detection.command.is_none() && !has_error_markers);

    let mut result = text.to_string();
    let mut matched: Option<&RtkFilter> = None;

    if !skip_filters && !is_document_like {
        if let Some(f) = match_filter_for(&detection, text) {
            // effectiveMaxLines(filter.maxLines || maxLinesPerResult, "standard") = base (min 1).
            let base = if f.max_lines == 0 {
                MAX_LINES_PER_RESULT
            } else {
                f.max_lines
            };
            result = apply_line_filter(&result, f, Some(base.max(1)));
            matched = Some(f);
        }
    }

    let deduped = deduplicate_repeated_lines(&result, DEDUP_THRESHOLD);
    if deduped.collapsed > 0 {
        result = deduped.text;
    }

    if !is_document_like {
        let mut priority: Vec<Regex> = vec![default_priority_re().clone()];
        if let Some(f) = matched {
            priority.extend(f.priority_patterns.iter().cloned());
        }
        let truncated = smart_truncate(
            &result,
            &SmartTruncateOptions {
                max_lines: MAX_LINES_PER_RESULT,
                max_chars: MAX_CHARS_PER_RESULT,
                preserve_head: FINAL_HEAD_TAIL,
                preserve_tail: FINAL_HEAD_TAIL,
                priority_patterns: &priority,
            },
        );
        if truncated.truncated {
            result = truncated.text;
        }
    }

    // JS gates on token estimate; byte length is a safe proxy (never swaps in a
    // larger payload).
    let compressed = result.len() < text.len();
    ProcessResult {
        text: result,
        compressed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtins_load() {
        assert_eq!(filters().len(), FILTER_JSONS.len());
        assert_eq!(FILTER_JSONS.len(), 55);
    }

    #[test]
    fn git_status_is_compressed() {
        let input = "On branch main\nChanges not staged for commit:\n  (use \"git add\" to update)\n\tmodified: src/app.ts\nnothing added to commit\n";
        let out = process_rtk_text(input, Some("git status"), false);
        assert!(out.text.contains("On branch main"));
        assert!(out.text.len() <= input.len());
    }

    #[test]
    fn document_read_not_truncated() {
        // Unknown type, no command, no error markers, 300 plain lines -> untouched.
        let doc = (0..300)
            .map(|i| format!("line {i} of a source file"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = process_rtk_text(&doc, None, false);
        assert_eq!(out.text, doc, "document-like read preserved verbatim");
        assert!(!out.compressed);
    }
}
