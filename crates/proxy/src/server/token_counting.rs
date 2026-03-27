// Token counting endpoint and helpers.

use anyllm_translate::anthropic;
use axum::{http::StatusCode, response::IntoResponse, Json};
use std::sync::LazyLock;
use tiktoken_rs::CoreBPE;

use super::routes::AnthropicJson;

/// GPT-4o tokenizer (o200k_base), the closest available approximation to
/// Anthropic's tokenizer. This endpoint is inherently approximate since we
/// use tiktoken, not the real Anthropic tokenizer.
static TOKENIZER: LazyLock<CoreBPE> =
    LazyLock::new(|| tiktoken_rs::o200k_base().expect("failed to load o200k_base tokenizer"));

pub(crate) async fn count_tokens(
    AnthropicJson(body): AnthropicJson<anthropic::MessageCreateRequest>,
) -> axum::response::Response {
    // Offload to blocking threadpool: tokenization is CPU-intensive and
    // would stall the async runtime, blocking other request handlers.
    match tokio::task::spawn_blocking(move || count_request_tokens(&body)).await {
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
fn count_request_tokens(req: &anthropic::MessageCreateRequest) -> usize {
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
