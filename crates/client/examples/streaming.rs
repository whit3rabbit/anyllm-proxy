//! Streaming example: receive tokens incrementally as they arrive.
//!
//! ```bash
//! CHAT_COMPLETIONS_URL=https://api.openai.com/v1/chat/completions \
//! OPENAI_API_KEY=sk-... \
//! cargo run --example streaming -p anyllm_client
//! ```

use anyllm_client::{Client, ClientError};
use anyllm_translate::anthropic::{Delta, MessageCreateRequest, StreamEvent};
use futures::StreamExt;
use std::io::Write;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), ClientError> {
    let url = std::env::var("CHAT_COMPLETIONS_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string());
    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();

    let client = Client::builder().base_url(&url).api_key(&api_key).build()?;

    let req: MessageCreateRequest = serde_json::from_str(
        r#"{
        "model": "claude-sonnet-4-6",
        "max_tokens": 512,
        "messages": [
            {"role": "user", "content": "Write a haiku about Rust programming."}
        ]
    }"#,
    )
    .expect("static request JSON is valid");

    // messages_stream() returns (stream, rate_limit_headers).
    // The stream yields StreamEvent items translated from the backend's SSE chunks.
    let (mut stream, rate_limits) = client.messages_stream(&req).await?;

    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::ContentBlockDelta { delta, .. } => {
                // TextDelta carries incremental text. Print immediately without buffering.
                if let Delta::TextDelta { text } = delta {
                    print!("{text}");
                    std::io::stdout().flush().ok();
                }
            }
            StreamEvent::MessageDelta { usage, .. } => {
                if let Some(u) = usage {
                    eprintln!("\n[output tokens: {}]", u.output_tokens);
                }
            }
            StreamEvent::MessageStop {} => {
                println!(); // ensure a trailing newline
            }
            _ => {} // MessageStart, ContentBlockStart/Stop, Ping are informational
        }
    }

    if let Some(remaining) = &rate_limits.requests_remaining {
        eprintln!("[requests remaining: {remaining}]");
    }

    Ok(())
}
