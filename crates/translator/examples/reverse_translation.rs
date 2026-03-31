//! Reverse translation example: OpenAI -> Anthropic direction.
//!
//! Demonstrates:
//! - translate_openai_to_anthropic_request: accept an OpenAI Chat Completions request
//! - translate_anthropic_to_openai_response: convert an Anthropic response to OpenAI format
//! - ReverseStreamingTranslator: convert Anthropic SSE events to OpenAI streaming chunks
//!
//! This is the direction used by the proxy's POST /v1/chat/completions endpoint,
//! allowing OpenAI-native clients (LiteLLM, LangChain) to talk to Anthropic backends.
//!
//! ```bash
//! cargo run --example reverse_translation -p anyllm_translate
//! ```

use anyllm_translate::anthropic::{MessageResponse, StreamEvent};
use anyllm_translate::openai::ChatCompletionRequest;
use anyllm_translate::{
    new_reverse_stream_translator, translate_anthropic_to_openai_response,
    translate_openai_to_anthropic_request, TranslationWarnings,
};

fn main() {
    translate_direction();
    println!();
    streaming_direction();
}

// --- Non-streaming: OpenAI request -> Anthropic request -> OpenAI response ---
fn translate_direction() {
    println!("=== Non-streaming reverse translation ===");

    // An OpenAI Chat Completions request received from a client.
    let openai_req: ChatCompletionRequest = serde_json::from_str(
        r#"{
        "model": "gpt-4o",
        "max_tokens": 256,
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Explain monads in one sentence."}
        ]
    }"#,
    )
    .expect("mock OpenAI request is valid");

    // translate_openai_to_anthropic_request: converts for forwarding to Anthropic API.
    // Warnings collect features dropped in translation (e.g. tool_choice "none").
    let mut warnings = TranslationWarnings::default();
    let anthropic_req = translate_openai_to_anthropic_request(&openai_req, &mut warnings)
        .expect("reverse translation should succeed");

    println!("Anthropic model: {}", anthropic_req.model);
    println!("max_tokens: {}", anthropic_req.max_tokens);
    if !warnings.is_empty() {
        println!("warnings: {:?}", warnings);
    }

    // Mock an Anthropic response (normally returned by the upstream Anthropic API).
    let anthropic_resp: MessageResponse = serde_json::from_str(r#"{
        "id": "msg_abc123",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-6",
        "content": [
            {"type": "text", "text": "A monad is a design pattern for sequencing computations that may have context or side effects."}
        ],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 25, "output_tokens": 18}
    }"#)
    .expect("mock Anthropic response is valid");

    // translate_anthropic_to_openai_response: return to the OpenAI-native client.
    let openai_resp = translate_anthropic_to_openai_response(&anthropic_resp, "gpt-4o");
    println!("OpenAI response id: {}", openai_resp.id);
    if let Some(choice) = openai_resp.choices.first() {
        if let Some(anyllm_translate::openai::ChatContent::Text(text)) = &choice.message.content {
            println!("text: {text}");
        }
    }
}

// --- Streaming: Anthropic SSE events -> OpenAI ChatCompletionChunk objects ---
fn streaming_direction() {
    println!("=== Streaming reverse translation ===");

    // Create a translator for a given message ID and model.
    // The proxy generates a synthetic message ID; here we use a fixed value.
    let mut translator =
        new_reverse_stream_translator("chatcmpl-xyz".to_string(), "gpt-4o".to_string());

    // Simulate the Anthropic SSE events that would arrive from the upstream.
    let events: Vec<StreamEvent> = serde_json::from_str(r#"[
        {
            "type": "message_start",
            "message": {
                "id": "msg_abc", "type": "message", "role": "assistant",
                "content": [], "model": "claude-sonnet-4-6",
                "usage": {"input_tokens": 10, "output_tokens": 0}
            }
        },
        {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hello"}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": ", world!"}},
        {"type": "content_block_stop", "index": 0},
        {"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 2}},
        {"type": "message_stop"}
    ]"#)
    .expect("mock events are valid");

    for event in &events {
        let chunks = translator.process_event(event);
        for chunk in chunks {
            // Each chunk is ready to serialize as `data: <json>\n\n` in an SSE stream.
            let serialized = serde_json::to_string(&chunk).unwrap();
            println!("chunk: {serialized}");
        }
    }

    println!("done: {}", translator.is_done());
    if translator.is_done() {
        // Emit `data: [DONE]\n\n` to signal end of stream to the OpenAI client.
        println!("data: [DONE]");
    }
}
