pub fn first_failure_line(message: Option<&str>) -> String {
    collapse_whitespace(
        message
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("Unknown failure"),
    )
}

fn collapse_whitespace(input: &str) -> String {
    let mut collapsed = String::with_capacity(input.len());
    let mut previous_was_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                collapsed.push(' ');
                previous_was_space = true;
            }
        } else {
            collapsed.push(ch);
            previous_was_space = false;
        }
    }
    collapsed.trim().to_string()
}

pub fn truncate_for_display(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let truncated: String = value.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{truncated}...")
}

pub fn normalize_failure_group_key_from_line(first_line: &str) -> String {
    let lowercase = first_line.to_ascii_lowercase();
    let tokens = lowercase
        .split_whitespace()
        .filter_map(|token| {
            let trimmed = token
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_');
            if trimmed.is_empty() {
                None
            } else {
                Some(normalize_failure_token(trimmed))
            }
        })
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        "<empty>".to_string()
    } else {
        tokens.join(" ")
    }
}

fn normalize_failure_token(token: &str) -> String {
    if looks_like_id(token) {
        "<id>".to_string()
    } else if looks_like_numberish(token) {
        "<num>".to_string()
    } else {
        token.to_string()
    }
}

fn looks_like_numberish(token: &str) -> bool {
    fn is_numericish(input: &str) -> bool {
        !input.is_empty()
            && input
                .chars()
                .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | ':' | '/' | '%'))
    }

    if is_numericish(token) {
        return true;
    }

    for suffix in ["ms", "s", "sec", "secs"] {
        if let Some(prefix) = token.strip_suffix(suffix) {
            return is_numericish(prefix);
        }
    }

    false
}

fn looks_like_id(token: &str) -> bool {
    let lowercase = token.to_ascii_lowercase();
    if [
        "req_",
        "msg_",
        "run_",
        "resp_",
        "call_",
        "toolu_",
        "chatcmpl-",
        "cmpl-",
    ]
    .iter()
    .any(|prefix| lowercase.starts_with(prefix))
    {
        return true;
    }

    let compact = lowercase.replace('-', "");
    if compact.len() >= 24 && compact.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return true;
    }

    // Single pass: check all three conditions simultaneously.
    lowercase.len() >= 16 && {
        let mut has_alpha = false;
        let mut has_digit = false;
        let all_valid = lowercase.chars().all(|ch| {
            if ch.is_ascii_alphabetic() {
                has_alpha = true;
            } else if ch.is_ascii_digit() {
                has_digit = true;
            }
            ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'
        });
        all_valid && has_alpha && has_digit
    }
}
