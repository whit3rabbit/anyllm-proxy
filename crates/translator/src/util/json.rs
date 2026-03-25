// JSON helpers for defensive parsing/serialization during API translation.
// OpenAI function arguments may arrive as malformed JSON; these helpers
// ensure we never panic on bad input.

use serde_json::Value;

/// Try to parse a JSON string into a Value. Returns the original string wrapped
/// in Value::String if parsing fails (defensive handling for potentially invalid
/// OpenAI function arguments).
pub fn parse_json_lenient(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string()))
}

/// Strip markdown code fences that local LLMs (DeepSeek, Qwen) sometimes wrap
/// around tool call argument JSON. Handles ```json, ```JSON, and bare ```.
fn strip_markdown_code_fence(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```") {
        // Skip optional language tag on the opening fence line
        let rest = rest
            .strip_prefix("json")
            .or_else(|| rest.strip_prefix("JSON"))
            .unwrap_or(rest);
        // Strip the newline after the opening fence line
        let rest = rest
            .strip_prefix('\n')
            .or_else(|| rest.strip_prefix("\r\n"))
            .unwrap_or(rest);
        // Strip trailing closing fence
        let rest = rest.trim_end();
        let rest = rest.strip_suffix("```").unwrap_or(rest);
        rest.trim()
    } else {
        s
    }
}

/// Parse an OpenAI tool call `arguments` string into a JSON object suitable
/// for Anthropic's `input` field. Unlike `parse_json_lenient`, this guarantees
/// the result is always a JSON object:
/// - Empty/whitespace-only string -> `{}`
/// - Valid JSON object -> the parsed object
/// - Valid JSON non-object (string, number, array, etc.) -> `{"_raw": <value>}`
/// - Invalid JSON -> `{"_raw_error": "<original string>"}`
///
/// Also strips markdown code fences (```json ... ```) that local LLMs
/// (DeepSeek, Qwen, llama-server, ollama) sometimes wrap around arguments.
/// Anthropic's `input` field must always be a JSON object.
pub fn parse_tool_arguments(s: &str) -> Value {
    let trimmed = strip_markdown_code_fence(s.trim());
    if trimmed.is_empty() {
        return Value::Object(serde_json::Map::new());
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(other) => {
            tracing::warn!(
                raw = trimmed,
                "tool arguments parsed as non-object JSON; wrapping in {{\"_raw\": ...}}"
            );
            let mut map = serde_json::Map::new();
            map.insert("_raw".to_string(), other);
            Value::Object(map)
        }
        Err(_) => {
            tracing::warn!(
                raw = trimmed,
                "tool arguments are not valid JSON; wrapping in {{\"_raw_error\": ...}}"
            );
            let mut map = serde_json::Map::new();
            map.insert("_raw_error".to_string(), Value::String(s.to_string()));
            Value::Object(map)
        }
    }
}

/// Serialize a JSON Value to a string. Returns "{}" if serialization fails.
/// The fallback ensures tool call arguments always have a valid JSON string
/// even if the Value contains types serde_json cannot serialize (shouldn't
/// happen in practice, but defensive).
pub fn value_to_json_string(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_valid_object() {
        let v = parse_json_lenient(r#"{"key": "value"}"#);
        assert_eq!(v, json!({"key": "value"}));
    }

    #[test]
    fn parse_valid_nested_object() {
        let v = parse_json_lenient(r#"{"a": {"b": [1, 2, 3]}}"#);
        assert_eq!(v, json!({"a": {"b": [1, 2, 3]}}));
    }

    #[test]
    fn parse_null() {
        let v = parse_json_lenient("null");
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn parse_invalid_json_returns_string() {
        let v = parse_json_lenient("not json at all");
        assert_eq!(v, Value::String("not json at all".to_string()));
    }

    #[test]
    fn parse_empty_string_returns_string() {
        let v = parse_json_lenient("");
        assert_eq!(v, Value::String(String::new()));
    }

    #[test]
    fn value_to_json_string_object() {
        let v = json!({"foo": "bar"});
        let s = value_to_json_string(&v);
        assert_eq!(s, r#"{"foo":"bar"}"#);
    }

    #[test]
    fn value_to_json_string_null() {
        let s = value_to_json_string(&Value::Null);
        assert_eq!(s, "null");
    }

    // --- strip_markdown_code_fence ---

    #[test]
    fn strip_fence_json_tag() {
        let input = "```json\n{\"key\": \"val\"}\n```";
        assert_eq!(strip_markdown_code_fence(input), r#"{"key": "val"}"#);
    }

    #[test]
    fn strip_fence_bare() {
        let input = "```\n{\"key\": \"val\"}\n```";
        assert_eq!(strip_markdown_code_fence(input), r#"{"key": "val"}"#);
    }

    #[test]
    fn strip_fence_json_uppercase() {
        let input = "```JSON\n{\"a\": 1}\n```";
        assert_eq!(strip_markdown_code_fence(input), r#"{"a": 1}"#);
    }

    #[test]
    fn strip_fence_trailing_whitespace() {
        let input = "```json\n{\"a\": 1}\n```\n  ";
        assert_eq!(strip_markdown_code_fence(input), r#"{"a": 1}"#);
    }

    #[test]
    fn strip_fence_no_closing() {
        // Missing closing fence: return content after opening fence
        let input = "```json\n{\"a\": 1}";
        assert_eq!(strip_markdown_code_fence(input), r#"{"a": 1}"#);
    }

    #[test]
    fn no_fence_passthrough() {
        let input = r#"{"key": "val"}"#;
        assert_eq!(strip_markdown_code_fence(input), input);
    }

    #[test]
    fn backticks_inside_json_string_not_stripped() {
        // Backticks inside a JSON string value should not trigger stripping
        let input = r#"{"code": "use `foo`"}"#;
        assert_eq!(strip_markdown_code_fence(input), input);
    }

    // --- parse_tool_arguments ---

    #[test]
    fn parse_tool_arguments_empty_string() {
        let v = parse_tool_arguments("");
        assert_eq!(v, json!({}));
    }

    #[test]
    fn parse_tool_arguments_whitespace_only() {
        let v = parse_tool_arguments("   \n  ");
        assert_eq!(v, json!({}));
    }

    #[test]
    fn parse_tool_arguments_valid_object() {
        let v = parse_tool_arguments(r#"{"file_path": "/tmp/test.rs", "limit": 100}"#);
        assert_eq!(v, json!({"file_path": "/tmp/test.rs", "limit": 100}));
    }

    #[test]
    fn parse_tool_arguments_nested_object() {
        let v = parse_tool_arguments(r#"{"a": {"b": [1, 2]}}"#);
        assert_eq!(v, json!({"a": {"b": [1, 2]}}));
    }

    #[test]
    fn parse_tool_arguments_bare_string() {
        let v = parse_tool_arguments(r#""hello""#);
        assert_eq!(v, json!({"_raw": "hello"}));
    }

    #[test]
    fn parse_tool_arguments_bare_number() {
        let v = parse_tool_arguments("42");
        assert_eq!(v, json!({"_raw": 42}));
    }

    #[test]
    fn parse_tool_arguments_bare_array() {
        let v = parse_tool_arguments("[1, 2, 3]");
        assert_eq!(v, json!({"_raw": [1, 2, 3]}));
    }

    #[test]
    fn parse_tool_arguments_invalid_json() {
        let v = parse_tool_arguments("not json {at all");
        assert_eq!(v, json!({"_raw_error": "not json {at all"}));
    }

    #[test]
    fn parse_tool_arguments_null() {
        let v = parse_tool_arguments("null");
        assert_eq!(v, json!({"_raw": null}));
    }

    #[test]
    fn parse_tool_arguments_markdown_json_fence() {
        let v = parse_tool_arguments("```json\n{\"file_path\": \"test.rs\"}\n```");
        assert_eq!(v, json!({"file_path": "test.rs"}));
    }

    #[test]
    fn parse_tool_arguments_markdown_bare_fence() {
        let v = parse_tool_arguments("```\n{\"file_path\": \"test.rs\"}\n```");
        assert_eq!(v, json!({"file_path": "test.rs"}));
    }

    #[test]
    fn parse_tool_arguments_markdown_fence_trailing_whitespace() {
        let v = parse_tool_arguments("```json\n{\"a\": 1}\n```\n  ");
        assert_eq!(v, json!({"a": 1}));
    }

    #[test]
    fn parse_tool_arguments_backticks_in_json_value() {
        // Backticks inside a JSON string value are not code fences
        let v = parse_tool_arguments(r#"{"code": "use `foo`"}"#);
        assert_eq!(v, json!({"code": "use `foo`"}));
    }
}
