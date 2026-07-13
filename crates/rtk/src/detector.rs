//! Command-output classification — port of `commandDetector.ts` +
//! `splitCompositeCommand.ts::lastCommandSegment`.
//!
//! The detector picks a `type` (e.g. `git-status`) that `engine::match_filter`
//! uses to select a filter. Command-prefix detection is case-SENSITIVE (JS
//! `new RegExp` with no flag); command patterns are case-insensitive; content
//! patterns carry per-pattern flags transcribed from the source.

use regex::{Regex, RegexBuilder};
use std::sync::OnceLock;

pub struct CommandDetection {
    pub r#type: String,
    pub command: Option<String>,
    pub confidence: f64,
    #[allow(dead_code)]
    pub category: String,
}

pub(super) struct Detector {
    pub(super) r#type: &'static str,
    pub(super) category: &'static str,
    pub(super) command_patterns: Vec<Regex>,
    pub(super) content_patterns: Vec<Regex>,
}

/// Compile with explicit flags: `i` = case-insensitive, `m` = multiline.
/// Fixed, in-tree patterns — a bad transcription panics (caught by `table_builds`).
pub(super) fn rx(pattern: &str, flags: &str) -> Regex {
    RegexBuilder::new(pattern)
        .case_insensitive(flags.contains('i'))
        .multi_line(flags.contains('m'))
        .build()
        .unwrap_or_else(|e| panic!("rtk detector regex {pattern:?}: {e}"))
}

const COMMAND_PREFIXES: &[&str] = &[
    "git",
    "make",
    "gradle",
    "gradlew",
    "dotnet",
    "terraform",
    "tofu",
    "opentofu",
    "systemctl",
    "npm",
    "pnpm",
    "yarn",
    "vitest",
    "jest",
    "pytest",
    "python",
    "go",
    "cargo",
    "tsc",
    "eslint",
    "webpack",
    "vite",
    "biome",
    "prettier",
    "turbo",
    "nx",
    "playwright",
    "ruff",
    "mypy",
    "pip",
    "uv",
    "poetry",
    "golangci-lint",
    "bundle",
    "rubocop",
    "kubectl",
    "composer",
    "gh",
    "docker",
    "aws",
    "gcloud",
    "ssh",
    "rsync",
    "curl",
    "wget",
    "ls",
    "find",
    "grep",
    "rg",
    "ag",
    "ps",
    "df",
    "du",
];

fn command_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Case-SENSITIVE, mirrors JS `new RegExp("^(?:...)\\b")` with no flags.
    RE.get_or_init(|| Regex::new(&format!(r"^(?:{})\b", COMMAND_PREFIXES.join("|"))).unwrap())
}

mod table;

fn detectors() -> &'static Vec<Detector> {
    static D: OnceLock<Vec<Detector>> = OnceLock::new();
    D.get_or_init(table::build_detectors)
}

/// (type, category, command patterns (all "i"), (content pattern, flags) pairs).
pub(super) type DetectorRow = (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [(&'static str, &'static str)],
);

/// Quote-aware composite-command splitter — port of `lastCommandSegment`.
/// Returns the last top-level `&&`/`||`/`;`-separated segment (trimmed).
pub fn last_command_segment(command: &str) -> String {
    if command.is_empty() {
        return command.to_string();
    }
    let bytes: Vec<char> = command.chars().collect();
    let mut segments: Vec<String> = Vec::new();
    let mut current = 0usize;
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;

    let slice = |a: usize, b: usize| -> String { bytes[a..b].iter().collect() };

    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i];
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_backtick {
            if ch == '`' {
                in_backtick = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if ch == '"' {
                in_double = false;
            }
            if ch == '$' && bytes.get(i + 1) == Some(&'(') {
                depth += 1;
                i += 1;
            }
            i += 1;
            continue;
        }
        if depth > 0 {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
            }
            i += 1;
            continue;
        }
        if ch == '\'' {
            in_single = true;
            i += 1;
            continue;
        }
        if ch == '"' {
            in_double = true;
            i += 1;
            continue;
        }
        if ch == '`' {
            in_backtick = true;
            i += 1;
            continue;
        }
        if ch == '$' && bytes.get(i + 1) == Some(&'(') {
            depth += 1;
            i += 1;
            i += 1;
            continue;
        }
        if ch == '(' {
            depth += 1;
            i += 1;
            continue;
        }
        if ch == '&' && bytes.get(i + 1) == Some(&'&') {
            segments.push(slice(current, i));
            i += 1;
            current = i + 1;
            i += 1;
            continue;
        }
        if ch == '|' && bytes.get(i + 1) == Some(&'|') {
            segments.push(slice(current, i));
            i += 1;
            current = i + 1;
            i += 1;
            continue;
        }
        if ch == ';' {
            segments.push(slice(current, i));
            current = i + 1;
            i += 1;
            continue;
        }
        i += 1;
    }
    segments.push(slice(current, bytes.len()));

    if segments.len() == 1 {
        return command.to_string();
    }
    for seg in segments.iter().rev() {
        let trimmed = seg.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    command.to_string()
}

fn prompt_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\$\s+").unwrap())
}

fn detect_command_from_text(text: &str) -> Option<String> {
    for line in text.split('\n').take(20) {
        let line = line.strip_suffix('\r').unwrap_or(line);
        // trim, then strip a leading "$ " prompt.
        let trimmed = line.trim();
        let trimmed = prompt_re().replace(trimmed, "");
        if trimmed.is_empty() {
            continue;
        }
        if command_prefix_re().is_match(&trimmed) {
            return Some(last_command_segment(&trimmed));
        }
    }
    None
}

pub fn detect_command_type(text: &str, command: Option<&str>) -> CommandDetection {
    let base = command
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .or_else(|| detect_command_from_text(text))
        .unwrap_or_default();
    let detected = last_command_segment(&base);
    let detected_command: Option<String> = if detected.is_empty() {
        None
    } else {
        Some(detected)
    };

    let mut best: Option<CommandDetection> = None;
    for d in detectors() {
        let command_matched = detected_command
            .as_deref()
            .map(|dc| d.command_patterns.iter().any(|p| p.is_match(dc)))
            .unwrap_or(false);
        let content_matches = d
            .content_patterns
            .iter()
            .filter(|p| p.is_match(text))
            .count();
        if !command_matched && content_matches == 0 {
            continue;
        }
        let confidence =
            ((if command_matched { 0.55 } else { 0.0 }) + content_matches as f64 * 0.25).min(1.0);
        let better = best
            .as_ref()
            .map(|b| confidence > b.confidence)
            .unwrap_or(true);
        if better {
            best = Some(CommandDetection {
                r#type: d.r#type.to_string(),
                command: detected_command.clone(),
                confidence,
                category: d.category.to_string(),
            });
        }
    }

    best.unwrap_or_else(|| CommandDetection {
        r#type: "unknown".to_string(),
        command: detected_command.clone(),
        confidence: if detected_command.is_some() {
            0.35
        } else {
            0.1
        },
        category: "generic".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_builds() {
        // Forces every detector regex to compile — catches transcription typos.
        assert_eq!(detectors().len(), 51);
    }

    #[test]
    fn detects_git_status_by_content() {
        let d = detect_command_type("On branch main\nUntracked files:\n", None);
        assert_eq!(d.r#type, "git-status");
    }

    #[test]
    fn last_segment_of_composite() {
        assert_eq!(last_command_segment("cd foo && git status"), "git status");
        assert_eq!(
            last_command_segment("git status | head"),
            "git status | head"
        );
        assert_eq!(last_command_segment("ls"), "ls");
    }
}
