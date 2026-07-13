// Token counting endpoint and helpers.

use anyllm_translate::anthropic;
use axum::{http::StatusCode, response::IntoResponse, Json};
use std::sync::LazyLock;
use tiktoken_rs::CoreBPE;

use super::state::AnthropicJson;

/// GPT-4o tokenizer (o200k_base), the closest available approximation to
/// Anthropic's tokenizer. This endpoint is inherently approximate since we
/// use tiktoken, not the real Anthropic tokenizer.
static TOKENIZER: LazyLock<CoreBPE> =
    LazyLock::new(|| tiktoken_rs::o200k_base().expect("failed to load o200k_base tokenizer"));

/// Approximate token count for an Anthropic request using the GPT-4o tokenizer.
/// Inherently approximate: uses tiktoken (o200k_base), not the real Anthropic tokenizer.
pub(crate) async fn count_tokens(
    AnthropicJson(body): AnthropicJson<anthropic::MessageCreateRequest>,
) -> axum::response::Response {
    // Offload to blocking threadpool: tokenization is CPU-intensive and
    // would stall the async runtime, blocking other request handlers.
    match tokio::task::spawn_blocking(move || count_request_tokens_sync(&body)).await {
        Ok(token_count) => {
            let mut resp = (
                StatusCode::OK,
                Json(serde_json::json!({ "input_tokens": token_count })),
            )
                .into_response();
            // Token counts use o200k_base (GPT-4o) which may differ significantly
            // from the target model's tokenizer, especially for CJK text.
            resp.headers_mut().insert(
                "x-anyllm-token-counter",
                axum::http::HeaderValue::from_static(
                    "approximate (tiktoken o200k_base); do not use for billing",
                ),
            );
            resp
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "token counting failed" })),
        )
            .into_response(),
    }
}

/// Count tokens across all text segments of an Anthropic request.
/// Counts each segment independently to avoid a single large concatenation.
/// Per-segment counting may differ slightly from concatenated counting at BPE
/// boundaries, but this endpoint is already approximate (tiktoken, not the real
/// Anthropic tokenizer).
pub(crate) fn count_request_tokens_sync(req: &anthropic::MessageCreateRequest) -> usize {
    let mut total = 0;

    if let Some(system) = &req.system {
        match system {
            anthropic::System::Text(t) => total += count_segment(t),
            anthropic::System::Blocks(blocks) => {
                for b in blocks {
                    total += count_segment(&b.text);
                }
            }
        }
    }

    for msg in &req.messages {
        total += count_content(&msg.content);
    }

    if let Some(tools) = &req.tools {
        for tool in tools {
            total += count_segment(&tool.name);
            if let Some(desc) = &tool.description {
                total += count_segment(desc);
            }
            if let Ok(schema) = serde_json::to_string(&tool.input_schema) {
                total += count_segment(&schema);
            }
        }
    }

    total
}

/// Tokenize a single text segment and return its token count.
fn count_segment(text: &str) -> usize {
    TOKENIZER.encode_with_special_tokens(text).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anthropic::messages::{ContentBlock, InputMessage, Tool, ToolResultContent};
    use anthropic::{Role, System, SystemBlock};
    use anyllm_translate::anthropic;

    fn req(messages: Vec<InputMessage>) -> anthropic::MessageCreateRequest {
        anthropic::MessageCreateRequest {
            model: "claude-sonnet-4-6".into(),
            max_tokens: 100,
            messages,
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

    fn user_text(text: &str) -> InputMessage {
        InputMessage {
            role: Role::User,
            content: anthropic::Content::Text(text.into()),
        }
    }

    fn assistant_blocks(blocks: Vec<ContentBlock>) -> InputMessage {
        InputMessage {
            role: Role::Assistant,
            content: anthropic::Content::Blocks(blocks),
        }
    }

    #[test]
    fn empty_messages_count_zero() {
        assert_eq!(count_request_tokens_sync(&req(vec![])), 0);
    }

    #[test]
    fn simple_text_message_counted() {
        let count = count_request_tokens_sync(&req(vec![user_text("Hello, world!")]));
        assert!(count > 0 && count < 20, "got {count}");
    }

    #[test]
    fn system_text_adds_tokens() {
        let mut r = req(vec![user_text("What is the capital of France?")]);
        r.system = Some(System::Text("You are a helpful assistant.".into()));
        assert!(count_request_tokens_sync(&r) > 5);
    }

    #[test]
    fn system_blocks_are_counted() {
        let mut r = req(vec![user_text("hi")]);
        r.system = Some(System::Blocks(vec![
            SystemBlock {
                block_type: "text".into(),
                text: "Block one.".into(),
                cache_control: None,
            },
            SystemBlock {
                block_type: "text".into(),
                text: "Block two.".into(),
                cache_control: None,
            },
        ]));
        assert!(count_request_tokens_sync(&r) > 5);
    }

    #[test]
    fn tool_definitions_add_tokens() {
        let mut r = req(vec![user_text("Use a tool")]);
        r.tools = Some(vec![Tool {
            name: "get_weather".into(),
            description: Some("Get weather".into()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "location": { "type": "string" } },
                "required": ["location"]
            }),
        }]);
        assert!(count_request_tokens_sync(&r) > 10);
    }

    #[test]
    fn tool_without_description_still_counts() {
        let mut r = req(vec![user_text("Use a tool")]);
        r.tools = Some(vec![Tool {
            name: "get_time".into(),
            description: None,
            input_schema: serde_json::json!({ "type": "object" }),
        }]);
        assert!(count_request_tokens_sync(&r) > 3);
    }

    #[test]
    fn thinking_block_in_content_counted() {
        let r = req(vec![
            user_text("Think step by step"),
            assistant_blocks(vec![
                ContentBlock::Thinking {
                    thinking: "Let me work through this carefully...".into(),
                    signature: Some("sig_abc".into()),
                },
                ContentBlock::Text {
                    text: "Here is the answer.".into(),
                },
            ]),
        ]);
        assert!(count_request_tokens_sync(&r) > 8);
    }

    #[test]
    fn tool_result_with_error_prefixed() {
        let r = req(vec![
            user_text("Run the tool"),
            InputMessage {
                role: Role::Assistant,
                content: anthropic::Content::Blocks(vec![ContentBlock::ToolUse {
                    id: "tu_001".into(),
                    name: "search".into(),
                    input: serde_json::json!({"query": "test"}),
                }]),
            },
            InputMessage {
                role: Role::User,
                content: anthropic::Content::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_001".into(),
                    content: Some(ToolResultContent::Text("Error: permission denied".into())),
                    is_error: Some(true),
                }]),
            },
        ]);
        assert!(count_request_tokens_sync(&r) > 5);
    }

    #[test]
    fn multiple_messages_accumulate_tokens() {
        let single = count_request_tokens_sync(&req(vec![user_text("hello")]));
        let r = req(vec![
            user_text("hello"),
            InputMessage {
                role: Role::Assistant,
                content: anthropic::Content::Text("world".into()),
            },
            user_text("foo bar baz"),
        ]);
        let multi = count_request_tokens_sync(&r);
        assert!(
            multi > single,
            "multiple messages ({multi}) should have more tokens than one ({single})"
        );
    }

    #[test]
    fn image_block_does_not_crash() {
        let r = req(vec![assistant_blocks(vec![
            ContentBlock::Image {
                source: anthropic::messages::ImageSource {
                    source_type: "base64".into(),
                    media_type: Some("image/png".into()),
                    data: Some("AAAA".into()),
                    url: None,
                },
            },
            ContentBlock::Text {
                text: "Here is the image.".into(),
            },
        ])]);
        assert!(count_request_tokens_sync(&r) > 0);
    }
}

fn count_content(content: &anthropic::Content) -> usize {
    match content {
        anthropic::Content::Text(t) => count_segment(t),
        anthropic::Content::Blocks(blocks) => {
            let mut total = 0;
            for block in blocks {
                match block {
                    anthropic::ContentBlock::Text { text } => total += count_segment(text),
                    anthropic::ContentBlock::ToolUse { name, input, .. } => {
                        total += count_segment(name);
                        if let Ok(s) = serde_json::to_string(input) {
                            total += count_segment(&s);
                        }
                    }
                    anthropic::ContentBlock::ToolResult {
                        content: Some(c),
                        is_error,
                        ..
                    } => {
                        // The translation layer prepends "Error: " for error
                        // tool results (message_map.rs), so count that prefix.
                        if *is_error == Some(true) {
                            total += count_segment("Error: ");
                        }
                        match c {
                            anthropic::messages::ToolResultContent::Text(t) => {
                                total += count_segment(t);
                            }
                            anthropic::messages::ToolResultContent::Blocks(inner) => {
                                for b in inner {
                                    if let anthropic::ContentBlock::Text { text } = b {
                                        total += count_segment(text);
                                    }
                                }
                            }
                        }
                    }
                    anthropic::ContentBlock::Thinking { thinking, .. } => {
                        total += count_segment(thinking);
                    }
                    // Images and documents have their own token costs in
                    // the actual APIs, which we can't compute client-side.
                    _ => {}
                }
            }
            total
        }
    }
}
