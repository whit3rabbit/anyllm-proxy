// ID generation utilities for Anthropic-format identifiers.
// Uses UUID v4 (simple/no-hyphen format) with a domain prefix to match the
// {prefix}_{hex} pattern that Anthropic SDKs and clients expect when parsing
// response IDs. See: https://docs.anthropic.com/en/api/messages

/// Generate a message ID in Anthropic format (msg_ prefix + uuid v4 without hyphens).
pub fn generate_message_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4().as_simple())
}

/// Generate a content block ID in Anthropic format.
pub fn generate_content_block_id() -> String {
    format!("block_{}", uuid::Uuid::new_v4().as_simple())
}

/// Generate a tool use ID in Anthropic format (toolu_ prefix).
pub fn generate_tool_use_id() -> String {
    format!("toolu_{}", uuid::Uuid::new_v4().as_simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_id_has_correct_prefix_and_length() {
        let id = generate_message_id();
        assert!(id.starts_with("msg_"), "expected msg_ prefix, got: {id}");
        // "msg_" (4) + 32 hex chars = 36
        assert_eq!(id.len(), 36, "unexpected length: {id}");
    }

    #[test]
    fn content_block_id_has_correct_prefix_and_length() {
        let id = generate_content_block_id();
        assert!(
            id.starts_with("block_"),
            "expected block_ prefix, got: {id}"
        );
        // "block_" (6) + 32 hex chars = 38
        assert_eq!(id.len(), 38, "unexpected length: {id}");
    }

    #[test]
    fn tool_use_id_has_correct_prefix_and_length() {
        let id = generate_tool_use_id();
        assert!(
            id.starts_with("toolu_"),
            "expected toolu_ prefix, got: {id}"
        );
        // "toolu_" (6) + 32 hex chars = 38
        assert_eq!(id.len(), 38, "unexpected length: {id}");
    }

    #[test]
    fn ids_are_unique() {
        let a = generate_message_id();
        let b = generate_message_id();
        assert_ne!(a, b);
    }
}
