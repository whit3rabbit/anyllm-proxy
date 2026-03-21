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

/// Serialize a JSON Value to a string. Returns "{}" if serialization fails.
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
}
