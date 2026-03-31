//! Pure translation example: convert between Anthropic and OpenAI formats with no IO.
//!
//! This demonstrates the `anyllm_translate` crate in isolation. No HTTP, no async.
//! Useful when you want to bring your own HTTP client or test translation logic directly.
//!
//! ```bash
//! cargo run --example translate_request -p anyllm_translate
//! ```

use anyllm_translate::anthropic::MessageCreateRequest;
use anyllm_translate::openai::ChatCompletionResponse;
use anyllm_translate::{
    compute_request_warnings, translate_request, translate_response, TranslationConfig,
};

fn main() {
    // --- 1. Configure model mapping ---
    // Map Anthropic model names to backend model names.
    // Any unmapped model name is passed through unchanged.
    let config = TranslationConfig::builder()
        .model_map("claude-haiku-4-5", "gpt-4o-mini")
        .model_map("claude-sonnet-4-6", "gpt-4o")
        .model_map("claude-opus-4-6", "gpt-4o")
        .build();

    // --- 2. Build an Anthropic request ---
    let req: MessageCreateRequest = serde_json::from_str(
        r#"{
        "model": "claude-sonnet-4-6",
        "max_tokens": 256,
        "system": "You are a helpful assistant.",
        "messages": [
            {"role": "user", "content": "What is Rust?"}
        ]
    }"#,
    )
    .expect("static request JSON is valid");

    // --- 3. Check for lossy features before translating ---
    // compute_request_warnings identifies features that will be silently dropped
    // (e.g. top_k, thinking_config, document blocks). Use to set x-anyllm-degradation.
    let warnings = compute_request_warnings(&req);
    if !warnings.is_empty() {
        eprintln!("translation warnings: {:?}", warnings);
    }

    // --- 4. Translate Anthropic -> OpenAI ---
    let openai_req = translate_request(&req, &config).expect("translation should succeed");
    println!("OpenAI model: {}", openai_req.model); // "gpt-4o"
    assert_eq!(openai_req.model, "gpt-4o");

    // Inspect how the system prompt was converted (Anthropic -> OpenAI developer role).
    if let Some(first_msg) = openai_req.messages.first() {
        println!("first OpenAI message role: {:?}", first_msg.role);
    }
    println!(
        "OpenAI request: {}",
        serde_json::to_string_pretty(&openai_req).unwrap()
    );

    // --- 5. Mock an OpenAI response (normally from your HTTP client) ---
    let openai_resp: ChatCompletionResponse = serde_json::from_str(r#"{
        "id": "chatcmpl-abc123",
        "object": "chat.completion",
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Rust is a systems programming language focused on safety and performance."
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 20,
            "completion_tokens": 15,
            "total_tokens": 35
        }
    }"#)
    .expect("mock response JSON is valid");

    // --- 6. Translate OpenAI response -> Anthropic ---
    // Pass the original Anthropic model name so it appears in the response.
    let anthropic_resp = translate_response(&openai_resp, &req.model);
    println!("\nAnthropic response model: {}", anthropic_resp.model);
    println!("stop_reason: {:?}", anthropic_resp.stop_reason);
    for block in &anthropic_resp.content {
        if let anyllm_translate::anthropic::ContentBlock::Text { text } = block {
            println!("text: {text}");
        }
    }
}
