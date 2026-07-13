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

struct Detector {
    r#type: &'static str,
    category: &'static str,
    command_patterns: Vec<Regex>,
    content_patterns: Vec<Regex>,
}

/// Compile with explicit flags: `i` = case-insensitive, `m` = multiline.
/// Fixed, in-tree patterns — a bad transcription panics (caught by `table_builds`).
fn rx(pattern: &str, flags: &str) -> Regex {
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

fn detectors() -> &'static Vec<Detector> {
    static D: OnceLock<Vec<Detector>> = OnceLock::new();
    D.get_or_init(build_detectors)
}

/// (type, category, command patterns (all "i"), (content pattern, flags) pairs).
type DetectorRow = (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [(&'static str, &'static str)],
);

fn build_detectors() -> Vec<Detector> {
    let table: &[DetectorRow] = &[
        (
            "git-status",
            "git",
            &[r"^git\s+status\b"],
            &[
                (r"^On branch ", "m"),
                (r"^Changes (?:not staged|to be committed)", "m"),
                (r"^Untracked files:", "m"),
            ],
        ),
        (
            "git-branch",
            "git",
            &[r"^git\s+branch\b", r"^git\s+checkout\b", r"^git\s+switch\b"],
            &[
                (r"^\*\s+\S+", "m"),
                (r"Switched to (?:a new )?branch", "i"),
                (r#"Already on ['"][^'"]+['"]"#, "i"),
            ],
        ),
        (
            "git-diff",
            "git",
            &[r"^git\s+diff\b", r"^git\s+show\b"],
            &[
                (r"^diff --git ", "m"),
                (r"^@@\s+-\d+,\d+\s+\+\d+,\d+\s+@@", "m"),
            ],
        ),
        (
            "git-log",
            "git",
            &[r"^git\s+log\b"],
            &[(r"^commit [0-9a-f]{7,40}", "m"), (r"^Author: ", "m")],
        ),
        (
            "make",
            "build",
            &[r"^make\b"],
            &[
                (r"^make\[\d+\]: (?:Entering|Leaving) directory", "m"),
                (r"make: \*\*\* ", ""),
            ],
        ),
        (
            "gradle",
            "build",
            &[r"^(?:gradle|gradlew|\./gradlew)\b"],
            &[
                (r"^> Task :", "m"),
                (r"^BUILD (?:SUCCESSFUL|FAILED)\b", "m"),
            ],
        ),
        (
            "dotnet",
            "build",
            &[
                r"^dotnet\s+(?:build|test|run|restore|publish|pack|msbuild)\b",
                r"^dotnet\b",
            ],
            &[
                (r"^Build (?:succeeded|FAILED)\b", "m"),
                (r"\b(?:error|warning) CS\d+\b", "m"),
            ],
        ),
        (
            "terraform-plan",
            "infra",
            &[r"^terraform\s+plan\b"],
            &[
                (r"Terraform will perform the following actions:", ""),
                (r"Plan: \d+ to add", "i"),
            ],
        ),
        (
            "tofu-plan",
            "infra",
            &[r"^(?:tofu|opentofu)\s+plan\b"],
            &[
                (r"OpenTofu will perform the following actions:", ""),
                (r"Plan: \d+ to add", "i"),
            ],
        ),
        (
            "systemctl-status",
            "infra",
            &[r"^systemctl\s+status\b"],
            &[
                (r"^\s*Loaded:\s+", "m"),
                (r"^\s*Active:\s+", "m"),
                (r"^●\s+\S+\.service", "m"),
            ],
        ),
        (
            "test-vitest",
            "test",
            &[r"^vitest\b", r"^npm\s+(?:run\s+)?test:vitest\b"],
            &[
                (r"\bvitest\b", "i"),
                (r"^ ✓ ", "m"),
                (r"^ ❯ ", "m"),
                (r"Test Files\s+\d+\s+(?:passed|failed)", "i"),
            ],
        ),
        (
            "test-jest",
            "test",
            &[r"^jest\b", r"^npm\s+(?:run\s+)?test\b"],
            &[
                (r"Test Suites:\s+\d+", "i"),
                (r"Tests:\s+\d+", "i"),
                (r"^PASS\s+", "m"),
                (r"^FAIL\s+", "m"),
            ],
        ),
        (
            "test-pytest",
            "test",
            &[r"^pytest\b", r"^python\s+-m\s+pytest\b"],
            &[
                (r"=+\s+(?:\d+\s+)?(?:passed|failed|errors?)", "i"),
                (r"^E\s+", "m"),
                (r"^FAILED ", "m"),
            ],
        ),
        (
            "test-cargo",
            "test",
            &[r"^cargo\s+test\b", r"^cargo\s+nextest\b"],
            &[
                (r"^running \d+ tests?", "m"),
                (r"^test\s+[\w:.-]+\s+\.\.\.\s+(?:ok|FAILED|ignored)", "m"),
                (r"test result:\s+(?:ok|FAILED)", "i"),
            ],
        ),
        (
            "test-go",
            "test",
            &[r"^go\s+test\b"],
            &[
                (r"^(?:ok|FAIL)\s+[\w./-]+\s+[\d.]+s", "m"),
                (r"^--- FAIL: ", "m"),
                (r"^panic: ", "m"),
            ],
        ),
        (
            "build-typescript",
            "build",
            &[r"^tsc\b", r"^npm\s+run\s+typecheck\b"],
            &[(r"TS\d{4}:", ""), (r"error TS\d{4}", "i")],
        ),
        (
            "build-eslint",
            "build",
            &[r"^eslint\b", r"^npm\s+run\s+lint\b"],
            &[
                (r"\s+\d+:\d+\s+(?:error|warning)\s+", ""),
                (r"✖\s+\d+\s+problems?", ""),
            ],
        ),
        (
            "build-webpack",
            "build",
            &[
                r"^webpack\b",
                r"^npx\s+webpack\b",
                r"^npm\s+run\s+build:webpack\b",
            ],
            &[
                (r"webpack\s+\d", "i"),
                (r"compiled (?:successfully|with \d+ errors?)", "i"),
                (r"asset .+\.js", "i"),
            ],
        ),
        (
            "build-vite",
            "build",
            &[
                r"^vite\s+build\b",
                r"^npm\s+run\s+build\b",
                r"^pnpm\s+build\b",
            ],
            &[
                (r"vite v[\d.]+", "i"),
                (r"✓ built in", "i"),
                (r"transforming \(\d+\)", "i"),
            ],
        ),
        (
            "biome",
            "build",
            &[r"^biome\b", r"^npx\s+biome\b"],
            &[
                (r"lint/[A-Za-z0-9/.-]+", ""),
                (r"Checked \d+ files? in", "i"),
            ],
        ),
        (
            "prettier",
            "build",
            &[r"^prettier\b", r"^npx\s+prettier\b"],
            &[
                (r"^Checking formatting\.\.\.", "m"),
                (r"Code style issues found", "i"),
            ],
        ),
        (
            "turbo",
            "build",
            &[r"^turbo\b", r"^npx\s+turbo\b"],
            &[
                (r"^• Packages in scope:", "m"),
                (r"^Tasks:\s+\d+\s+successful", "m"),
            ],
        ),
        (
            "nx",
            "build",
            &[r"^nx\b", r"^npx\s+nx\b"],
            &[(r"^NX\s+", "m"), (r"^> nx run ", "m")],
        ),
        (
            "playwright",
            "test",
            &[r"^playwright\s+test\b", r"^npx\s+playwright\s+test\b"],
            &[
                (r"Running \d+ tests? using \d+ workers?", "i"),
                (r"^\s+\d+ failed", "m"),
            ],
        ),
        (
            "npm-install",
            "package",
            &[r"^(?:npm|pnpm|yarn)\s+(?:install|add|update)\b"],
            &[
                (r"added \d+ packages", "i"),
                (r"packages are looking for funding", "i"),
                (r"audited \d+ packages", "i"),
            ],
        ),
        (
            "npm-audit",
            "package",
            &[r"^(?:npm|pnpm|yarn)\s+audit\b"],
            &[
                (r"found \d+ vulnerabilities", "i"),
                (r"\b(?:low|moderate|high|critical)\b", "i"),
            ],
        ),
        (
            "ruff",
            "build",
            &[r"^ruff\b", r"^uv\s+run\s+ruff\b"],
            &[
                (r"^[\w./-]+\.py:\d+:\d+:\s+[A-Z]\d+", "m"),
                (r"Found \d+ errors?\.", "i"),
            ],
        ),
        (
            "mypy",
            "build",
            &[r"^mypy\b", r"^python\s+-m\s+mypy\b"],
            &[
                (r"^[\w./-]+\.py:\d+:\s+error:", "m"),
                (r"Found \d+ errors? in \d+ files?", "i"),
            ],
        ),
        (
            "pip",
            "package",
            &[
                r"^pip\s+(?:install|download|uninstall)\b",
                r"^python\s+-m\s+pip\b",
            ],
            &[(r"^Collecting ", "m"), (r"^Successfully installed ", "m")],
        ),
        (
            "uv-sync",
            "package",
            &[r"^uv\s+sync\b", r"^uv\s+pip\s+install\b"],
            &[
                (r"^Resolved \d+ packages?", "m"),
                (r"^Installed \d+ packages?", "m"),
            ],
        ),
        (
            "poetry-install",
            "package",
            &[r"^poetry\s+install\b"],
            &[
                (r"^Installing dependencies from lock file", "m"),
                (r"^Package operations:", "m"),
            ],
        ),
        (
            "golangci-lint",
            "build",
            &[r"^golangci-lint\b"],
            &[(r"^[\w./-]+\.go:\d+:\d+:", "m"), (r"^\d+ issues?:", "m")],
        ),
        (
            "bundle-install",
            "package",
            &[r"^bundle\s+install\b"],
            &[
                (r"^Fetching gem metadata from ", "m"),
                (r"^Bundle complete!", "m"),
            ],
        ),
        (
            "rubocop",
            "build",
            &[r"^rubocop\b", r"^bundle\s+exec\s+rubocop\b"],
            &[
                (r"^Inspecting \d+ files", "m"),
                (r"^[\w./-]+\.rb:\d+:\d+:\s+[A-Z]:", "m"),
            ],
        ),
        (
            "docker-ps",
            "docker",
            &[r"^docker\s+ps\b"],
            &[(r"^CONTAINER ID\s+IMAGE\s+COMMAND", "m")],
        ),
        (
            "docker-logs",
            "docker",
            &[r"^docker\s+logs\b", r"^docker\s+compose\s+logs\b"],
            &[
                (r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}", "m"),
                (r"\b(?:ERROR|WARN|INFO)\b", ""),
                (r"^Attaching to ", "m"),
            ],
        ),
        (
            "aws",
            "cloud",
            &[r"^aws\b"],
            &[
                (r"An error occurred \([A-Za-z0-9]+\) when calling", ""),
                (r"^(?:upload|download): ", "m"),
            ],
        ),
        (
            "gcloud",
            "cloud",
            &[r"^gcloud\b"],
            &[(r"^ERROR: \(gcloud\.", "m"), (r"^Updated property \[", "m")],
        ),
        (
            "ssh",
            "cloud",
            &[r"^ssh\b"],
            &[
                (r"Permission denied \(", ""),
                (r"Host key verification failed", ""),
                (r"Connection timed out", ""),
            ],
        ),
        (
            "rsync",
            "cloud",
            &[r"^rsync\b"],
            &[
                (r"^sending incremental file list", "m"),
                (r"^rsync error:", "m"),
            ],
        ),
        (
            "curl",
            "cloud",
            &[r"^curl\b"],
            &[(r"curl: \(\d+\)", ""), (r"^HTTP/\d(?:\.\d)? \d{3}", "m")],
        ),
        (
            "wget",
            "cloud",
            &[r"^wget\b"],
            &[(r"^--\d{4}-\d{2}-\d{2}", "m"), (r"^ERROR \d{3}:", "m")],
        ),
        (
            "json-output",
            "generic",
            &[r"^jq\b", r"^cat\s+.*\.json\b"],
            &[(r"^\s*[\[{][\s\S]*[\]}]\s*$", "")],
        ),
        (
            "shell-ls",
            "shell",
            &[r"^ls(?:\s+-[A-Za-z]+)?\b"],
            &[
                (r"^total \d+", "m"),
                (r"^\S+\s+\S+\s+\d+\s+\w+\s+\d{1,2}\s+", "m"),
            ],
        ),
        (
            "shell-find",
            "shell",
            &[r"^find\b"],
            &[(r"^(?:\.{1,2}|/|[\w.-]+/).+", "m")],
        ),
        (
            "shell-grep",
            "shell",
            &[r"^(?:grep|rg|ag)\b"],
            &[
                (
                    r"^[\w./-]+\.(?:ts|tsx|js|jsx|py|go|rs|java|rb|md|json|ya?ml|txt):\d*:",
                    "m",
                ),
                (r"^[\w./-]+/[\w./-]+:\d*:", "m"),
            ],
        ),
        (
            "shell-ps",
            "shell",
            &[r"^ps\b"],
            &[(r"^(?:USER\s+PID|\s*PID\s+)", "m")],
        ),
        (
            "shell-df",
            "shell",
            &[r"^df\b"],
            &[(r"^Filesystem\s+.*Use%", "m")],
        ),
        (
            "shell-du",
            "shell",
            &[r"^du\b"],
            &[(r"^\d+(?:\.\d+)?[KMGTP]?\s+\S+", "m")],
        ),
        (
            "error-stacktrace",
            "generic",
            &[],
            &[
                (r"Traceback \(most recent call last\):", ""),
                (r"^\s+at\s+\S+\s+\(.+:\d+:\d+\)", "m"),
                (r"^panic: ", "m"),
                (r"^thread '[^']+' panicked at", "m"),
            ],
        ),
        (
            "generic-error",
            "generic",
            &[],
            &[
                (r"Error:", ""),
                (r"Exception:", ""),
                (r"Traceback \(most recent call last\):", ""),
            ],
        ),
    ];

    table
        .iter()
        .map(|(t, cat, cmds, contents)| Detector {
            r#type: t,
            category: cat,
            command_patterns: cmds.iter().map(|p| rx(p, "i")).collect(),
            content_patterns: contents.iter().map(|(p, f)| rx(p, f)).collect(),
        })
        .collect()
}

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
