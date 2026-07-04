//! Record-and-restore repair for Anthropic thinking blocks.
//!
//! Claude Code (and similar clients) can corrupt the `thinking`/
//! `redacted_thinking` blocks in the last assistant message of a replayed
//! conversation — merged text from interleaved streams, dropped
//! `redacted_thinking` blocks the client never persists, reordered blocks.
//! The Anthropic API validates the *latest* assistant message's thinking
//! blocks byte-exactly against their signatures, so any mutation produces a
//! repeating 400 until the user clears context.
//!
//! This proxy sits in front of the real Anthropic API in passthrough mode
//! (`BACKEND=anthropic`) and sees every response before the client can
//! corrupt it. [`record_response`] records each response's content blocks
//! as ground truth; [`repair_request`] verifies and repairs only the last
//! assistant message of each outgoing request against that ground truth,
//! never touching anything before it (so prompt-cache prefixes survive).
//!
//! Opt-in via `ANTHROPIC_THINKING_REPAIR=true`; only wired up for
//! `BackendClient::Anthropic`. In-memory only — see [`ThinkingRepairStore`].

mod recorder;
mod repair;
mod store;

pub use recorder::ThinkingRecorder;
pub use repair::repair_request;
pub use store::ThinkingRepairStore;

use anyllm_translate::anthropic::{ContentBlock, MessageCreateRequest, Role};

/// Record a non-streaming response's content blocks as ground truth.
/// `namespace` scopes the record to the calling backend/tenant (see
/// `ThinkingRepairStore`'s doc comment). Takes `content` by value (rather
/// than borrowing a whole `&MessageResponse`) so a caller that already holds
/// an owned, mutable response can move its content out via
/// `std::mem::take` instead of cloning it when nothing after this call still
/// needs it.
pub async fn record_response(
    store: &ThinkingRepairStore,
    namespace: &str,
    msg_id: &str,
    content: Vec<ContentBlock>,
) {
    store.commit(namespace, msg_id, content).await;
}

/// Content-block `"type"` tags this crate's `ContentBlock` enum models.
/// Anything else deserializes to the lossy `ContentBlock::Unknown` variant
/// and re-serializes as a bare `{"type":"Unknown"}` — never round-trip such
/// a block, or the patched request corrupts it on the wire.
const KNOWN_BLOCK_TYPES: &[&str] = &[
    "text",
    "image",
    "document",
    "tool_use",
    "server_tool_use",
    "tool_result",
    "web_search_tool_result",
    "web_fetch_tool_result",
    "thinking",
    "redacted_thinking",
];

/// True if `content` (the raw, not-yet-typed JSON array for one message) has
/// a block carrying a `cache_control` breakpoint, a `citations` array, or a
/// `"type"` this crate's `ContentBlock` doesn't model — any of these would be
/// silently dropped or corrupted by re-serializing the typed struct back over
/// it (`ContentBlock::Text` has no `citations` field, same gap as
/// `cache_control`).
fn has_unpatchable_block(content: &serde_json::Value) -> bool {
    let Some(blocks) = content.as_array() else {
        return false;
    };
    blocks.iter().any(|b| {
        b.get("cache_control").is_some()
            || b.get("citations").is_some()
            || !b
                .get("type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| KNOWN_BLOCK_TYPES.contains(&t))
    })
}

/// Forward a possibly-repaired request without discarding fields our
/// `MessageCreateRequest`/`ContentBlock` structs don't model (e.g. per-block
/// `cache_control`, or a content-block/tool type newer than this crate knows
/// about). Splices only the repaired message's `content` into the ORIGINAL
/// raw JSON rather than re-serializing the whole typed request, so every
/// other field -- and every other message -- survives byte-for-byte. Fails
/// open (returns the original bytes unchanged) if that message itself has a
/// block `has_unpatchable_block` can't losslessly round-trip.
///
/// `repaired_req` must be the same request `repair_request` was called on
/// (so its `messages` are aligned 1:1 with `original_body`'s `"messages"`
/// array); only its last assistant message's content is used.
pub fn patch_repaired_body(
    original_body: &[u8],
    repaired_req: &MessageCreateRequest,
) -> Result<bytes::Bytes, serde_json::Error> {
    let mut raw: serde_json::Value = serde_json::from_slice(original_body)?;
    let Some(last_idx) = repaired_req
        .messages
        .iter()
        .rposition(|m| m.role == Role::Assistant)
    else {
        return Ok(bytes::Bytes::copy_from_slice(original_body));
    };
    let Some(msg) = raw
        .get_mut("messages")
        .and_then(|m| m.as_array_mut())
        .and_then(|messages| messages.get_mut(last_idx))
    else {
        return Ok(bytes::Bytes::copy_from_slice(original_body));
    };
    // A `cache_control` breakpoint or a block type newer than this crate
    // knows about can't survive a typed round-trip — fail open (forward the
    // original, unrepaired bytes) rather than silently drop or corrupt it.
    if msg.get("content").is_some_and(has_unpatchable_block) {
        return Ok(bytes::Bytes::copy_from_slice(original_body));
    }
    let repaired_content = serde_json::to_value(&repaired_req.messages[last_idx].content)?;
    msg["content"] = repaired_content;
    serde_json::to_vec(&raw).map(bytes::Bytes::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyllm_translate::anthropic::{Content, ContentBlock, InputMessage};

    fn req_with_last_assistant_thinking(text: &str, sig: &str) -> MessageCreateRequest {
        MessageCreateRequest {
            model: "claude-opus-4-5".to_string(),
            max_tokens: 1024,
            messages: vec![
                InputMessage {
                    role: Role::User,
                    content: Content::Text("hi".to_string()),
                },
                InputMessage {
                    role: Role::Assistant,
                    content: Content::Blocks(vec![ContentBlock::Thinking {
                        thinking: text.to_string(),
                        signature: Some(sig.to_string()),
                    }]),
                },
            ],
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            stream: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn patches_content_when_no_unpatchable_block_present() {
        let original = serde_json::json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "garbled", "signature": "sig_1"}
                ]},
            ],
        });
        let repaired = req_with_last_assistant_thinking("restored", "sig_1");

        let patched =
            patch_repaired_body(&serde_json::to_vec(&original).unwrap(), &repaired).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&patched).unwrap();
        assert_eq!(
            value["messages"][1]["content"][0]["thinking"], "restored",
            "content should be patched when nothing blocks the round-trip"
        );
    }

    #[test]
    fn fails_open_when_last_message_has_cache_control() {
        let original = serde_json::json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "garbled", "signature": "sig_1"},
                    {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {}, "cache_control": {"type": "ephemeral"}},
                ]},
            ],
        });
        let original_bytes = serde_json::to_vec(&original).unwrap();
        let repaired = req_with_last_assistant_thinking("restored", "sig_1");

        let patched = patch_repaired_body(&original_bytes, &repaired).unwrap();
        assert_eq!(
            patched.as_ref(),
            original_bytes.as_slice(),
            "cache_control on the touched message must not be dropped; fail open instead"
        );
    }

    #[test]
    fn fails_open_when_last_message_has_citations() {
        let original = serde_json::json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "garbled", "signature": "sig_1"},
                    {"type": "text", "text": "see source", "citations": [{"type": "web_search_result_location", "url": "https://example.com"}]},
                ]},
            ],
        });
        let original_bytes = serde_json::to_vec(&original).unwrap();
        let repaired = req_with_last_assistant_thinking("restored", "sig_1");

        let patched = patch_repaired_body(&original_bytes, &repaired).unwrap();
        assert_eq!(
            patched.as_ref(),
            original_bytes.as_slice(),
            "citations on a sibling text block must not be dropped; fail open instead"
        );
    }

    #[test]
    fn fails_open_when_last_message_has_unknown_block_type() {
        let original = serde_json::json!({
            "model": "claude-opus-4-5",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "garbled", "signature": "sig_1"},
                    {"type": "some_future_block_type", "id": "x1"},
                ]},
            ],
        });
        let original_bytes = serde_json::to_vec(&original).unwrap();
        let repaired = req_with_last_assistant_thinking("restored", "sig_1");

        let patched = patch_repaired_body(&original_bytes, &repaired).unwrap();
        assert_eq!(
            patched.as_ref(),
            original_bytes.as_slice(),
            "an unmodeled block type must not be corrupted to {{\"type\":\"Unknown\"}}; fail open instead"
        );
    }
}
