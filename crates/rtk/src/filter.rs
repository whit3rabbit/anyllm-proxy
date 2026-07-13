//! Filter schema (canonical JSON "pack" shape) + normalization to a compiled
//! `RtkFilter`. Faithful port of OmniRoute's `filterSchema.ts::validateRtkFilter`
//! (canonical branch only — every vendored built-in uses the pack shape; the
//! legacy shape is not ported, YAGNI).
//!
//! Regex flags mirror `lineFilter.ts`:
//!   keep/strip/collapse/priority → case-insensitive          (JS "i")
//!   replace                      → case-sensitive             (JS "g")
//!   matchOutput / matcher        → case-insensitive multiline (JS "im")
//!
//! Two fail-open drops, in order, match OmniRoute:
//!   1. ReDoS-prone patterns removed before compile (`is_redos_prone`).
//!   2. Patterns the Rust `regex` crate can't compile (e.g. lookahead in the 3
//!      gh/git-diff/kubectl detection patterns) are skipped silently. JS keeps
//!      those (its engine supports lookahead); losing a detection-only pattern
//!      is harmless — those filters still match via outputTypes / other patterns.

use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use std::sync::OnceLock;

// ──────────────── Raw deserialize shape (canonical pack) ────────────────

#[derive(Debug, Deserialize)]
pub struct InlineTest {
    pub name: String,
    pub input: String,
    pub expected: String,
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReplaceRule {
    pattern: String,
    replacement: String,
}

#[derive(Debug, Deserialize)]
struct MatchOutputRuleRaw {
    pattern: String,
    message: String,
    #[serde(default)]
    unless: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FilterMatch {
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    patterns: Vec<String>,
    #[serde(default)]
    #[serde(rename = "outputTypes")]
    output_types: Vec<String>,
}

fn default_head_tail() -> u32 {
    20
}

#[derive(Debug, Deserialize)]
struct FilterRules {
    #[serde(default)]
    #[serde(rename = "stripAnsi")]
    strip_ansi: bool,
    #[serde(default)]
    replace: Vec<ReplaceRule>,
    #[serde(default)]
    #[serde(rename = "matchOutput")]
    match_output: Vec<MatchOutputRuleRaw>,
    #[serde(default)]
    #[serde(rename = "includePatterns")]
    include_patterns: Vec<String>,
    #[serde(default)]
    #[serde(rename = "dropPatterns")]
    drop_patterns: Vec<String>,
    #[serde(default)]
    #[serde(rename = "collapsePatterns")]
    collapse_patterns: Vec<String>,
    #[serde(default)]
    deduplicate: bool,
    #[serde(default)]
    #[serde(rename = "truncateLineAt")]
    truncate_line_at: u32,
    #[serde(default)]
    #[serde(rename = "maxLines")]
    max_lines: u32,
    #[serde(default = "default_head_tail")]
    #[serde(rename = "headLines")]
    head_lines: u32,
    #[serde(default = "default_head_tail")]
    #[serde(rename = "tailLines")]
    tail_lines: u32,
    #[serde(default)]
    #[serde(rename = "onEmpty")]
    on_empty: String,
    #[serde(default)]
    #[serde(rename = "filterStderr")]
    filter_stderr: bool,
}

impl Default for FilterRules {
    fn default() -> Self {
        FilterRules {
            strip_ansi: false,
            replace: Vec::new(),
            match_output: Vec::new(),
            include_patterns: Vec::new(),
            drop_patterns: Vec::new(),
            collapse_patterns: Vec::new(),
            deduplicate: false,
            truncate_line_at: 0,
            max_lines: 0,
            head_lines: 20,
            tail_lines: 20,
            on_empty: String::new(),
            filter_stderr: false,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct FilterPreserve {
    #[serde(default)]
    #[serde(rename = "errorPatterns")]
    error_patterns: Vec<String>,
    #[serde(default)]
    #[serde(rename = "summaryPatterns")]
    summary_patterns: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct FilterPack {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default = "default_priority")]
    pub priority: i64,
    #[serde(default)]
    r#match: FilterMatch,
    #[serde(default)]
    rules: FilterRules,
    #[serde(default)]
    preserve: FilterPreserve,
    #[serde(default)]
    pub tests: Vec<InlineTest>,
}

fn default_priority() -> i64 {
    50
}

// ──────────────── Compiled / normalized shape ────────────────

pub struct MatchOutputRule {
    pub pattern: Regex,
    pub message: String,
    pub unless: Option<Regex>,
}

/// Normalized + compiled filter. Field names mirror `RtkFilterDefinition`.
pub struct RtkFilter {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub priority: i64,
    /// `match.outputTypes` — detector type ids this filter serves.
    pub command_types: Vec<String>,
    /// `match.commands` compiled "im" — matcher fallback #2.
    pub command_patterns: Vec<Regex>,
    /// `match.patterns` compiled "im" — matcher fallback #3.
    pub match_patterns: Vec<Regex>,
    pub strip_ansi: bool,
    pub replace: Vec<(Regex, String)>,
    pub match_output: Vec<MatchOutputRule>,
    pub keep_patterns: Vec<Regex>,
    pub strip_patterns: Vec<Regex>,
    pub collapse_patterns: Vec<Regex>,
    pub priority_patterns: Vec<Regex>,
    pub truncate_line_at: usize,
    pub on_empty: String,
    pub filter_stderr: bool,
    pub deduplicate: bool,
    pub max_lines: usize,
    pub preserve_head: usize,
    pub preserve_tail: usize,
    pub tests: Vec<InlineTest>,
}

/// Conservative, dependency-free ReDoS guard — port of `isReDoSProne`. Flags a
/// single group containing an unbounded quantifier that is itself quantified
/// (`(a+)+`, `(a*)*`, `([a-z]+)+`, `(a+|b)+`, …) — catastrophic backtracking.
pub fn is_redos_prone(pattern: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\([^()]*(?:[+*]|\{\d+,\})[^()]*\)\s*(?:[+*]|\{\d+,\})").unwrap())
        .is_match(pattern)
}

/// Compile a list of line patterns (case-insensitive). Drops ReDoS-prone and
/// uncompilable patterns, preserving order — mirrors JS `compilePatterns` +
/// `dropReDoSProne`.
fn compile_ci(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter(|p| !is_redos_prone(p))
        .filter_map(|p| RegexBuilder::new(p).case_insensitive(true).build().ok())
        .collect()
}

/// Compile matcher patterns (case-insensitive + multiline, JS "im").
fn compile_im(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter(|p| !is_redos_prone(p))
        .filter_map(|p| {
            RegexBuilder::new(p)
                .case_insensitive(true)
                .multi_line(true)
                .build()
                .ok()
        })
        .collect()
}

impl RtkFilter {
    /// Normalize + compile a canonical pack. Returns `None` only if the filter
    /// has no id (malformed); all other failures degrade fail-open per field.
    pub fn from_pack(pack: FilterPack) -> Option<RtkFilter> {
        if pack.id.is_empty() {
            return None;
        }
        let rules = pack.rules;

        let replace = rules
            .replace
            .into_iter()
            .filter(|r| !is_redos_prone(&r.pattern))
            // JS "g": case-sensitive, applied per line. Literal replacement
            // (no capture-group expansion) — no vendored replacement uses `$n`.
            .filter_map(|r| Regex::new(&r.pattern).ok().map(|re| (re, r.replacement)))
            .collect();

        let match_output = rules
            .match_output
            .into_iter()
            .filter(|r| !is_redos_prone(&r.pattern))
            .filter_map(|r| {
                let pattern = RegexBuilder::new(&r.pattern)
                    .case_insensitive(true)
                    .multi_line(true)
                    .build()
                    .ok()?;
                let unless = match r.unless {
                    Some(u) => {
                        RegexBuilder::new(&u)
                            .case_insensitive(true)
                            .multi_line(true)
                            .build()
                            .ok()
                    }
                    None => None,
                };
                Some(MatchOutputRule {
                    pattern,
                    message: r.message,
                    unless,
                })
            })
            .collect();

        // priorityPatterns = errorPatterns ++ summaryPatterns.
        let mut preserve_patterns = pack.preserve.error_patterns;
        preserve_patterns.extend(pack.preserve.summary_patterns);

        Some(RtkFilter {
            id: pack.id,
            name: if pack.label.is_empty() {
                String::new()
            } else {
                pack.label
            },
            description: pack.description,
            category: pack.category,
            priority: pack.priority,
            command_types: pack.r#match.output_types,
            command_patterns: compile_im(&pack.r#match.commands),
            match_patterns: compile_im(&pack.r#match.patterns),
            strip_ansi: rules.strip_ansi,
            replace,
            match_output,
            keep_patterns: compile_ci(&rules.include_patterns),
            strip_patterns: compile_ci(&rules.drop_patterns),
            collapse_patterns: compile_ci(&rules.collapse_patterns),
            priority_patterns: compile_ci(&preserve_patterns),
            truncate_line_at: rules.truncate_line_at as usize,
            on_empty: rules.on_empty,
            filter_stderr: rules.filter_stderr,
            deduplicate: rules.deduplicate,
            max_lines: rules.max_lines as usize,
            preserve_head: rules.head_lines as usize,
            preserve_tail: rules.tail_lines as usize,
            tests: pack.tests,
        })
    }

    /// Parse + normalize a JSON pack string.
    pub fn from_json(json: &str) -> Option<RtkFilter> {
        let pack: FilterPack = serde_json::from_str(json).ok()?;
        RtkFilter::from_pack(pack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redos_guard_flags_nested_quantifier() {
        assert!(is_redos_prone("(a+)+"));
        assert!(is_redos_prone("([a-z]*)*"));
        assert!(!is_redos_prone("^On branch "));
        assert!(!is_redos_prone(r"\bFAIL\b"));
    }

    #[test]
    fn lookahead_pattern_skipped_not_fatal() {
        // gh's detection pattern uses (?!...) — regex crate rejects it; the
        // filter still loads, just without that one match pattern.
        let json = r#"{"id":"x","label":"X","category":"generic","match":{"patterns":["^gh\\s+pr(?!.*--json)\\b"]},"rules":{}}"#;
        let f = RtkFilter::from_json(json).expect("loads");
        assert!(f.match_patterns.is_empty(), "lookahead pattern dropped");
    }
}
