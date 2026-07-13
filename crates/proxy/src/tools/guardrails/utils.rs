use serde_json::{Map, Value};

pub(super) fn is_shell_tool(name: &str) -> bool {
    matches!(
        name,
        "bash" | "shell" | "run_command" | "execute_command" | "terminal" | "exec" | "execute_bash"
    )
}

pub(super) fn is_grep_tool(name: &str) -> bool {
    matches!(name, "grep" | "rg" | "ripgrep" | "st")
}

pub(super) fn is_glob_tool(name: &str) -> bool {
    matches!(name, "glob" | "find_files" | "file_glob")
}

pub(super) fn is_write_or_edit_tool(name: &str) -> bool {
    matches!(
        name,
        "write"
            | "write_file"
            | "edit"
            | "edit_file"
            | "replace"
            | "apply_patch"
            | "create_file"
            | "update_file"
    )
}

pub(super) fn object_args(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

pub(super) fn command_arg(args: &Map<String, Value>) -> Option<&str> {
    string_arg(args, &["command", "cmd", "shell_command", "input"])
}

pub(super) fn string_arg<'a>(args: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    // Only the named keys, in order. Do NOT fall back to an arbitrary string
    // value: that would let an unrelated field be misread as the search symbol
    // or command for an advisory nudge.
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
}

pub(super) fn shell_grep_symbol(command: &str) -> Option<String> {
    let mut parts = command.split_whitespace();
    let binary = strip_shell_quotes(parts.next()?)
        .rsplit('/')
        .next()?
        .to_string();
    if !matches!(binary.as_str(), "rg" | "ripgrep" | "grep" | "st") {
        return None;
    }

    let mut previous_took_value = false;
    for part in parts {
        let token = strip_shell_quotes(part);
        if previous_took_value {
            previous_took_value = false;
            continue;
        }
        if token.starts_with("--") {
            previous_took_value = matches!(
                token.as_str(),
                "--glob" | "--type" | "--context" | "--after-context" | "--before-context"
            );
            continue;
        }
        if token.starts_with('-') {
            continue;
        }
        return symbol_from_search_value(&token);
    }
    None
}

pub(super) fn symbol_from_search_value(value: &str) -> Option<String> {
    let trimmed = strip_shell_quotes(value)
        .trim_matches('/')
        .replace("\\b", "")
        .replace(['^', '$'], "");
    for token in trimmed.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')) {
        if looks_like_symbol_token(token) {
            return Some(token.to_string());
        }
    }
    None
}

fn looks_like_symbol_token(token: &str) -> bool {
    if token.len() < 3 {
        return false;
    }
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        return false;
    }

    let lower = token.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "todo" | "fixme" | "error" | "warning" | "debug" | "test" | "src" | "main"
    ) {
        return false;
    }
    token.contains('_') || token.chars().any(|ch| ch.is_ascii_uppercase())
}

pub(super) fn strip_shell_quotes(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}
