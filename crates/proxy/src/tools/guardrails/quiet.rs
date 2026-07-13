use super::utils::*;
use super::ToolGuardrailNudge;
use crate::tools::execution::ToolCall;

pub(super) fn quiet_command_nudge(call: &ToolCall) -> Option<ToolGuardrailNudge> {
    let tool_name = call.name.to_ascii_lowercase();
    if !is_shell_tool(&tool_name) {
        return None;
    }
    let command = command_arg(object_args(&call.input)?)?.trim();
    let suggestion = quiet_command_suggestion(command)?;
    let fingerprint = format!("quiet:{}:{command}:{suggestion}", call.name);
    Some(ToolGuardrailNudge {
        call_id: call.id.clone(),
        kind: "quiet_command",
        content: format!(
            "The requested shell command is likely to produce noisy output. Prefer `{suggestion}`. Repeat the original command only if verbose output is required."
        ),
        fingerprint,
    })
}

fn quiet_command_suggestion(command: &str) -> Option<String> {
    if command_has_prefix(command, "git log") && !contains_word(command, "--oneline") {
        return Some(insert_after_prefix(command, "git log", "--oneline"));
    }
    for prefix in ["cargo build", "cargo check", "cargo clippy", "cargo test"] {
        if command_has_prefix(command, prefix) && !contains_word(command, "--quiet") {
            return Some(insert_after_prefix(command, prefix, "--quiet"));
        }
    }
    if command_has_prefix(command, "pytest") && !contains_word(command, "-q") {
        return Some(insert_after_prefix(command, "pytest", "-q"));
    }
    if command_has_prefix(command, "npm install") && !contains_word(command, "--silent") {
        return Some(insert_after_prefix(command, "npm install", "--silent"));
    }
    if command_has_prefix(command, "pip install") && !contains_word(command, "--quiet") {
        return Some(insert_after_prefix(command, "pip install", "--quiet"));
    }
    if command_has_prefix(command, "docker build") && !contains_word(command, "--progress=quiet") {
        return Some(insert_after_prefix(
            command,
            "docker build",
            "--progress=quiet",
        ));
    }
    if command_has_prefix(command, "curl") && !contains_word(command, "-s") {
        return Some(insert_after_prefix(command, "curl", "-s"));
    }
    if command_has_prefix(command, "make") && !contains_word(command, "-s") {
        return Some(insert_after_prefix(command, "make", "-s"));
    }
    if command_has_prefix(command, "tree") && !contains_word(command, "-I") {
        return Some(insert_after_prefix(
            command,
            "tree",
            "-I \"node_modules|.git|target|dist|build\"",
        ));
    }
    None
}

fn command_has_prefix(command: &str, prefix: &str) -> bool {
    command == prefix || command.starts_with(&format!("{prefix} "))
}

fn contains_word(command: &str, word: &str) -> bool {
    command.split_whitespace().any(|part| part == word)
}

fn insert_after_prefix(command: &str, prefix: &str, insertion: &str) -> String {
    let rest = command[prefix.len()..].trim_start();
    if rest.is_empty() {
        format!("{prefix} {insertion}")
    } else {
        format!("{prefix} {insertion} {rest}")
    }
}
