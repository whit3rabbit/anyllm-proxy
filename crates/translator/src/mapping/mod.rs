pub mod errors_map;
pub mod message_map;
pub mod responses_message_map;
pub mod responses_streaming_map;
pub mod streaming_map;
pub mod tools_map;
pub mod usage_map;

/// Format an OpenAI refusal string as Anthropic text content.
/// Anthropic has no refusal type, so we surface it as a bracketed text marker.
pub(crate) fn format_refusal(refusal: &str) -> String {
    format!("[Refusal: {}]", refusal)
}
