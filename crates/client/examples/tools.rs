//! Tool calling example: define a tool, let the model call it, handle the result.
//!
//! ```bash
//! CHAT_COMPLETIONS_URL=https://api.openai.com/v1/chat/completions \
//! OPENAI_API_KEY=sk-... \
//! cargo run --example tools -p anyllm_client
//! ```

use anyllm_client::{Client, ClientError, ToolBuilder, ToolChoiceBuilder};
use anyllm_translate::anthropic::{ContentBlock, MessageCreateRequest, StopReason};
use serde_json::json;

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

    // Define a tool with ToolBuilder. The input_schema is standard JSON Schema.
    let weather_tool = ToolBuilder::new("get_weather")
        .description("Get the current weather for a location")
        .input_schema(json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "City and country, e.g. 'Paris, France'"
                },
                "unit": {
                    "type": "string",
                    "enum": ["celsius", "fahrenheit"],
                    "description": "Temperature unit"
                }
            },
            "required": ["location"]
        }))
        .build();

    // Construct the request with the tool attached.
    // tool_choice: auto lets the model decide; use ToolChoiceBuilder::specific("get_weather")
    // to force it to call a particular tool.
    let req: MessageCreateRequest = serde_json::from_value(json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 512,
        "messages": [
            {"role": "user", "content": "What is the weather like in Tokyo right now?"}
        ],
        "tools": [serde_json::to_value(&weather_tool).unwrap()],
        "tool_choice": serde_json::to_value(ToolChoiceBuilder::auto()).unwrap()
    }))
    .expect("request JSON is valid");

    let response = client.messages(&req).await?;

    println!("stop_reason: {:?}", response.stop_reason);

    for block in &response.content {
        match block {
            ContentBlock::Text { text } => {
                println!("text: {text}");
            }
            ContentBlock::ToolUse { id, name, input } => {
                println!("tool_use: name={name} id={id}");
                println!("  input: {}", serde_json::to_string_pretty(input).unwrap());

                // In a real application: execute the tool here, then send the result
                // back in a follow-up request as a ContentBlock::ToolResult.
                let _tool_result = call_weather_tool(input);
                println!("  (tool execution would happen here)");
            }
            _ => {}
        }
    }

    // If stop_reason is ToolUse, send the tool result in a follow-up turn.
    if matches!(response.stop_reason, Some(StopReason::ToolUse)) {
        println!("\nNext step: send tool result back in a follow-up messages() call.");
    }

    Ok(())
}

// Stub simulating a real tool execution.
fn call_weather_tool(input: &serde_json::Value) -> String {
    let location = input
        .get("location")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    format!("Weather in {location}: 22°C, partly cloudy")
}
