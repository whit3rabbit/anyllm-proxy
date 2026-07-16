//! Extract [`RouterSignals`] from a request body for the Claude Code tier
//! router (see [`crate::config::router_config`]). One extractor per input
//! format. Kept out of the handlers so the classification logic is testable in
//! isolation.

use crate::config::router_config::RouterSignals;
use anyllm_translate::anthropic::{Content, ContentBlock, MessageCreateRequest, ThinkingConfig};
use anyllm_translate::openai::{ChatCompletionRequest, ChatContent, ChatContentPart};

/// Whether a model name looks like a background/small model (Claude Code uses
/// `claude-3-5-haiku` for background tasks). Substring match mirrors
/// `ModelMapping::map_model`.
fn is_background_model(model: &str) -> bool {
    crate::config::helpers::contains_ignore_ascii_case(model.as_bytes(), b"haiku")
}

/// Whether a tool name is Anthropic's/OpenAI's server-side web-search tool.
/// Matches the bare name and date-stamped variants (`web_search_20250305`) but
/// NOT unrelated client tools that merely begin with "web_search" (e.g.
/// `web_search_cache`), which the old `starts_with` check misrouted.
fn is_web_search_tool(name: &str) -> bool {
    name == "web_search"
        || name
            .strip_prefix("web_search_")
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// Classify an Anthropic `/v1/messages` request. `long_context` is passed in
/// (rather than computed) so the caller can skip the expensive token count when
/// no LongContext tier is configured -- see [`is_long_context`].
pub(crate) fn anthropic_signals(body: &MessageCreateRequest, long_context: bool) -> RouterSignals {
    let has_image = body.messages.iter().any(|m| match &m.content {
        Content::Blocks(blocks) => blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::Image { .. })),
        Content::Text(_) => false,
    });

    // Web search arrives as a tool definition. `Tool` has no `type` field, so
    // match by name (Anthropic's server tool is named "web_search"/date-stamped).
    let has_web_search = body
        .tools
        .as_ref()
        .is_some_and(|tools| tools.iter().any(|t| is_web_search_tool(&t.name)));

    let thinking = matches!(
        body.thinking,
        Some(ThinkingConfig::Enabled { .. }) | Some(ThinkingConfig::Adaptive { .. })
    );

    let is_background = is_background_model(&body.model);

    RouterSignals {
        has_image,
        has_web_search,
        thinking,
        long_context,
        is_background,
    }
}

/// Whether the request exceeds `threshold` tokens.
///
/// CPU-bound (tiktoken): callers must run it via `spawn_blocking`, not inline
/// on the async runtime (see `routes/messages.rs`). Gated behind an active
/// LongContext tier so it never runs on the default path.
pub(crate) fn is_long_context(body: &MessageCreateRequest, threshold: u32) -> bool {
    crate::server::token_counting::count_request_tokens_sync(body) > threshold as usize
}

/// Classify an OpenAI Chat Completions request. Long-context is not computed on
/// this path (the token counter is Anthropic-shaped); that tier is skipped.
pub(crate) fn openai_signals(body: &ChatCompletionRequest) -> RouterSignals {
    let has_image = body.messages.iter().any(|m| match &m.content {
        Some(ChatContent::Parts(parts)) => parts
            .iter()
            .any(|p| matches!(p, ChatContentPart::ImageUrl { .. })),
        _ => false,
    });

    let has_web_search = body
        .tools
        .as_ref()
        .is_some_and(|tools| tools.iter().any(|t| is_web_search_tool(&t.function.name)));

    // OpenAI reasoning models signal thinking via a non-empty `reasoning_effort`
    // (minimal/low/medium/high) on the *current* request. Ignore explicit
    // "none"/empty so a client that always sends the field isn't misclassified.
    // Do NOT infer thinking from `reasoning_content` in message history: that is
    // a prior assistant turn echoed back, so a plain follow-up in a reasoning
    // conversation would otherwise route to Think on every subsequent request.
    let thinking = body
        .extra
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"));

    let is_background = is_background_model(&body.model);

    RouterSignals {
        has_image,
        has_web_search,
        thinking,
        long_context: false,
        is_background,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anthropic(json: serde_json::Value) -> MessageCreateRequest {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn background_from_haiku_model() {
        let body = anthropic(serde_json::json!({
            "model": "claude-3-5-haiku-20241022",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 16,
        }));
        let s = anthropic_signals(&body, false);
        assert!(s.is_background);
        assert!(!s.has_image);
        assert!(!s.thinking);
    }

    #[test]
    fn image_and_thinking_and_websearch_detected() {
        let body = anthropic(serde_json::json!({
            "model": "claude-sonnet-4",
            "max_tokens": 16,
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "tools": [{"name": "web_search_20250305", "input_schema": {}}],
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "look"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "x"}}
                ]
            }],
        }));
        let s = anthropic_signals(&body, false);
        assert!(s.has_image);
        assert!(s.thinking);
        assert!(s.has_web_search);
        assert!(!s.is_background);
    }

    #[test]
    fn long_context_over_threshold() {
        let big = "word ".repeat(5000);
        let body = anthropic(serde_json::json!({
            "model": "claude-sonnet-4",
            "max_tokens": 16,
            "messages": [{"role": "user", "content": big}],
        }));
        assert!(is_long_context(&body, 10));
        assert!(!is_long_context(&body, 1_000_000));
    }

    #[test]
    fn openai_skips_long_context_but_detects_rest() {
        let body: ChatCompletionRequest = serde_json::from_value(serde_json::json!({
            "model": "claude-3-5-haiku",
            "reasoning_effort": "high",
            "tools": [{"type": "function", "function": {"name": "web_search", "parameters": {}}}],
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "hi"},
                    {"type": "image_url", "image_url": {"url": "http://x/y.png"}}
                ]
            }],
        }))
        .unwrap();
        let s = openai_signals(&body);
        assert!(s.has_image);
        assert!(s.thinking);
        assert!(s.has_web_search);
        assert!(s.is_background);
        assert!(!s.long_context);
    }

    #[test]
    fn web_search_tool_match_is_precise() {
        assert!(is_web_search_tool("web_search"));
        assert!(is_web_search_tool("web_search_20250305"));
        // Unrelated client tools that merely share the prefix must not match.
        assert!(!is_web_search_tool("web_search_cache"));
        assert!(!is_web_search_tool("web_search_"));
        assert!(!is_web_search_tool("my_web_search"));
    }

    #[test]
    fn reasoning_effort_none_is_not_thinking() {
        let body: ChatCompletionRequest = serde_json::from_value(serde_json::json!({
            "model": "gpt-4o",
            "reasoning_effort": "none",
            "messages": [{"role": "user", "content": "hi"}],
        }))
        .unwrap();
        assert!(!openai_signals(&body).thinking);

        let body: ChatCompletionRequest = serde_json::from_value(serde_json::json!({
            "model": "gpt-4o",
            "reasoning_effort": "medium",
            "messages": [{"role": "user", "content": "hi"}],
        }))
        .unwrap();
        assert!(openai_signals(&body).thinking);
    }
}
