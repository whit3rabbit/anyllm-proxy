//! Non-streaming example: send an Anthropic request and print the response.
//!
//! ```bash
//! CHAT_COMPLETIONS_URL=https://api.openai.com/v1/chat/completions \
//! OPENAI_API_KEY=sk-... \
//! cargo run --example basic -p anyllm_client
//! ```
//!
//! For local Ollama: CHAT_COMPLETIONS_URL=http://localhost:11434/v1/chat/completions

use anyllm_client::{Client, ClientError};
use anyllm_translate::anthropic::MessageCreateRequest;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {e}");
        match e {
            ClientError::Transport(inner) => eprintln!("  transport: {inner}"),
            ClientError::ApiError { status, body, .. } => {
                eprintln!("  backend returned HTTP {status}: {body}")
            }
            ClientError::Translation(inner) => eprintln!("  translation: {inner}"),
            ClientError::Deserialization(msg) => eprintln!("  deserialization: {msg}"),
            ClientError::Sse(inner) => eprintln!("  sse: {inner}"),
        }
        std::process::exit(1);
    }
}

async fn run() -> Result<(), ClientError> {
    let url = std::env::var("CHAT_COMPLETIONS_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string());
    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();

    // ClientBuilder: quick setup with sensible defaults (10s connect, 900s read, 3 retries).
    // For custom TLS/SSRF settings use Client::new(ClientConfig::builder()...).
    let client = Client::builder().base_url(&url).api_key(&api_key).build()?;

    // Construct the request as Anthropic Messages API JSON. The client handles
    // translation to OpenAI Chat Completions format before sending.
    let req: MessageCreateRequest = serde_json::from_str(
        r#"{
        "model": "claude-sonnet-4-6",
        "max_tokens": 256,
        "messages": [
            {"role": "user", "content": "What is 2 + 2? Reply in one sentence."}
        ]
    }"#,
    )
    .expect("static request JSON is valid");

    let response = client.messages(&req).await?;

    println!("stop_reason: {:?}", response.stop_reason);
    println!(
        "usage: input={} output={}",
        response.usage.input_tokens, response.usage.output_tokens
    );

    for block in &response.content {
        if let anyllm_translate::anthropic::ContentBlock::Text { text } = block {
            println!("{text}");
        }
    }

    Ok(())
}
