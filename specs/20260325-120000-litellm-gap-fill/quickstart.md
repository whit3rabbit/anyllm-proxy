# Quickstart: LiteLLM Gap Fill Features

## 1. OpenAI Chat Completions Input

After this feature, any OpenAI-native client can use the proxy:

```bash
# Start proxy backed by OpenAI
OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy

# Send an OpenAI-format request (NEW)
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "x-api-key: your-proxy-key" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "messages": [{"role": "user", "content": "Hello"}],
    "max_tokens": 100
  }'

# Response is OpenAI format (not Anthropic)
```

## 2. AWS Bedrock Backend

```bash
BACKEND=bedrock \
AWS_REGION=us-east-1 \
AWS_ACCESS_KEY_ID=AKIA... \
AWS_SECRET_ACCESS_KEY=... \
BIG_MODEL=anthropic.claude-3-5-sonnet-20241022-v2:0 \
SMALL_MODEL=anthropic.claude-3-5-haiku-20241022-v1:0 \
cargo run -p anyllm_proxy
```

## 3. Azure OpenAI Backend

```bash
BACKEND=azure \
AZURE_OPENAI_ENDPOINT=https://myresource.openai.azure.com \
AZURE_OPENAI_DEPLOYMENT=my-gpt4o \
AZURE_OPENAI_API_KEY=... \
cargo run -p anyllm_proxy
```

## 4. Virtual Key Management

```bash
# Create a key via admin API
curl -X POST http://localhost:3001/admin/api/keys \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"description": "dev key", "rpm_limit": 60}'

# Use the returned key
curl http://localhost:3000/v1/messages \
  -H "x-api-key: sk-vk..." \
  -H "Content-Type: application/json" \
  -d '{"model": "claude-sonnet-4-20250514", "max_tokens": 100, "messages": [{"role": "user", "content": "Hi"}]}'

# Revoke it (takes effect immediately)
curl -X DELETE http://localhost:3001/admin/api/keys/1 \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

## 5. OpenTelemetry Export

```bash
# Build with OTEL feature
cargo build -p anyllm_proxy --features otel

# Run with OTEL collector endpoint
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 \
OTEL_SERVICE_NAME=anyllm-proxy \
OPENAI_API_KEY=sk-... \
cargo run -p anyllm_proxy --features otel
```

## 6. Rust Client Library

```rust
use anyllm_client::{ClientBuilder, Tool, ToolChoice};

let client = ClientBuilder::new()
    .base_url("http://localhost:3000")
    .api_key("sk-vk...")
    .timeout(Duration::from_secs(30))
    .max_retries(3)
    .build()?;

// Non-streaming
let response = client.messages(request).await?;

// Streaming (returns impl Stream)
let mut stream = client.messages_stream(request).await?;
while let Some(event) = stream.next().await {
    match event? {
        StreamEvent::ContentBlockDelta { delta, .. } => print!("{}", delta.text()),
        StreamEvent::MessageStop => break,
        _ => {}
    }
}
```
