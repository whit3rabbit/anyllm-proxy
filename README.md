# anyllm-proxy

An API translation proxy that lets Anthropic-based tools (Claude Code, Cursor, Windsurf, Cline) talk to any OpenAI-compatible backend, local LLM, or alternative provider.

**[Releases](https://github.com/whit3rabbit/anyllm-proxy/releases)** | **[Library Usage](#using-as-a-library)**

---

## Quick Start

Download a binary from the [releases page](https://github.com/whit3rabbit/anyllm-proxy/releases), or install from source:

```bash
cargo install anyllm_proxy
```

Create a `.anyllm.env` config file:

```env
OPENAI_API_KEY=unused
OPENAI_BASE_URL=http://localhost:11434/v1
BIG_MODEL=qwen2.5-coder:32b
SMALL_MODEL=qwen2.5-coder:32b
```

Run the proxy (auto-loads `.anyllm.env` from the current directory):

```bash
anyllm_proxy
# or: anyllm_proxy --env-file ~/configs/ollama.env
```

Point Claude Code at the proxy:

```bash
ANTHROPIC_BASE_URL=http://localhost:3000 claude
```

### Admin Web Interface (optional)

Pass `--webui` (or `--admin`) to also start the admin dashboard on `127.0.0.1:3001`. It shows live request logs, latency, error rates, and lets you hot-reload model mappings without restarting.

```bash
anyllm_proxy --webui
# Proxy API: http://localhost:3000
# Admin UI:  http://127.0.0.1:3001/admin/?token=$(cat .admin_token)
```

The dashboard's Settings tab shows all active environment variables (API keys masked) and has an **Export .env** button that generates a `.anyllm.env` template you can edit and reuse. To use a custom port or a fixed token:

```bash
ADMIN_PORT=4000 ADMIN_TOKEN=mysecret anyllm_proxy --webui
```

To force-disable the admin even when the flag is present (useful in automated environments):

```bash
DISABLE_ADMIN=1 anyllm_proxy --webui   # admin will NOT start
```

### Multiple backends on one proxy (recommended)

A single proxy instance can serve all your backends simultaneously. Each backend gets its own URL path. Use a `config.toml` (see [section 2](#2-multi-routing-and-the-web-interface)):

```toml
# config.toml
listen_port = 3000
default_backend = "local"

[backends.local]
kind = "openai"
api_key = "unused"
base_url = "http://localhost:11434/v1"
big_model = "qwen2.5-coder:32b"
small_model = "qwen2.5-coder:7b"

[backends.openai]
kind = "openai"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
big_model = "gpt-4o"
small_model = "gpt-4o-mini"

[backends.deepseek]
kind = "openai"
api_key = "sk-deepseek-..."
base_url = "https://api.deepseek.com/v1"
big_model = "deepseek-coder"
small_model = "deepseek-chat"
```

```bash
PROXY_CONFIG=config.toml anyllm_proxy --webui
```

All three backends are live at once:

| Path | Backend |
|------|---------|
| `http://localhost:3000/v1/messages` | local (default) |
| `http://localhost:3000/openai/v1/messages` | OpenAI |
| `http://localhost:3000/deepseek/v1/messages` | DeepSeek |

Point different tools at different paths, or switch in Claude Code by changing `ANTHROPIC_BASE_URL`.

### Multiple separate instances (for isolated deployments)

For cases where you want completely separate proxy processes (different ports, different machines, different Docker containers), keep one `.env` file per deployment:

```
~/proxies/
  ollama.env         # local Ollama
  openai-prod.env    # production OpenAI
  deepseek.env       # DeepSeek API
```

Run any one:

```bash
anyllm_proxy --env-file ~/proxies/deepseek.env
```

Docker-compatible — same file works with `--env-file`:

```bash
docker run --env-file ~/proxies/openai-prod.env -p 3000:3000 anyllm-proxy
```

The admin UI's **Export .env** button (Settings tab) generates a ready-to-edit template from the current configuration.

---

## What, Why, and How?

### What is it?
A lightweight, fast Rust-based proxy that accepts Anthropic Messages API requests, translates them to OpenAI Chat Completions format, forwards them to any compliant backend, and translates responses back in real-time. Supports streaming SSE, tool calling, and image/document blocks.

### Why use it?

- **Local AI Coding:** Run Claude Code against local models (Llama 3, DeepSeek, Qwen) without API credits.
- **Broad Compatibility:** Works with open-weights and alternative models including Qwen and DeepSeek.
- **Multi-Backend Routing:** Route `haiku` requests to a fast local model and `opus` requests to external providers, transparently.
- **Observability:** Built-in admin dashboard for request logs, latency, and live config changes.

### How to Build from Source

```bash
cargo build

# Proxy only (default)
cargo run -p anyllm_proxy

# Proxy + admin web UI
cargo run -p anyllm_proxy -- --webui
```

The proxy listens on `0.0.0.0:3000`. The admin dashboard (opt-in via `--webui`) binds to `127.0.0.1:3001`.

---

## 1. Primary Use Case: Claude Code + Local LLMs

### Example: Running with Ollama (DeepSeek / Qwen)

```bash
# 1. Start your local LLM
ollama run qwen2.5-coder:32b &

# 2. Start the translation proxy
OPENAI_API_KEY=unused \
OPENAI_BASE_URL=http://localhost:11434/v1 \
BIG_MODEL=qwen2.5-coder:32b \
SMALL_MODEL=qwen2.5-coder:32b \
cargo run -p anyllm_proxy &

# 3. Use Claude Code targeting the local proxy
ANTHROPIC_BASE_URL=http://localhost:3000 claude
```

Use the same pattern for **LM Studio** (default port `1234`) or **vLLM** (default port `8000`) by substituting `OPENAI_BASE_URL`.

---

## 2. Multi-Routing and the Web Interface

Create a `config.toml` to map different routes to different backends:

```toml
listen_port = 3000
default_backend = "local_qwen"

[backends.local_qwen]
kind = "openai"
api_key = "unused"
base_url = "http://localhost:11434/v1"
big_model = "qwen2.5-coder:32b"
small_model = "qwen2.5-coder:7b"

[backends.deepseek_api]
kind = "openai"
api_key = "sk-deepseek-..."
base_url = "https://api.deepseek.com/v1"
big_model = "deepseek-coder"
small_model = "deepseek-chat"

[backends.openrouter]
kind = "openai"
api_key = "sk-or-..."
base_url = "https://openrouter.ai/api/v1"
big_model = "anthropic/claude-3.5-sonnet"
small_model = "google/gemini-2.5-flash"
```

```bash
PROXY_CONFIG=config.toml anyllm_proxy --webui
```

All backends are live at once on a single port. The path prefix matches the backend name in the config:

| Path | Backend | Notes |
|------|---------|-------|
| `/v1/messages` | `local_qwen` | default |
| `/deepseek_api/v1/messages` | `deepseek_api` | |
| `/openrouter/v1/messages` | `openrouter` | |

Point Claude Code at a specific backend:
```bash
ANTHROPIC_BASE_URL=http://localhost:3000/deepseek_api claude
```

### The Admin Dashboard

Start the proxy with `--webui`, then open:

```bash
open http://127.0.0.1:3001/admin/?token=$(cat .admin_token)
```

---

## 3. Commercial APIs (OpenAI, Gemini, OpenRouter)

**OpenRouter:**
```bash
OPENAI_API_KEY=sk-or-... \
OPENAI_BASE_URL=https://openrouter.ai/api/v1 \
BIG_MODEL=anthropic/claude-3.5-sonnet \
SMALL_MODEL=anthropic/claude-3-haiku \
cargo run -p anyllm_proxy
```

**OpenAI:**
```bash
OPENAI_API_KEY=sk-... \
BIG_MODEL=gpt-4o \
SMALL_MODEL=gpt-4o-mini \
cargo run -p anyllm_proxy
```

**Google Gemini:**
```bash
BACKEND=gemini \
GEMINI_API_KEY=AIza... \
BIG_MODEL=gemini-2.5-pro \
SMALL_MODEL=gemini-2.5-flash \
cargo run -p anyllm_proxy
```

---

## 4. Additional Backends and Features

### Azure OpenAI

```bash
BACKEND=azure \
AZURE_OPENAI_ENDPOINT=https://myresource.openai.azure.com \
AZURE_OPENAI_DEPLOYMENT=my-gpt4o \
AZURE_OPENAI_API_KEY=... \
cargo run -p anyllm_proxy
```

### AWS Bedrock

```bash
BACKEND=bedrock \
AWS_REGION=us-east-1 \
AWS_ACCESS_KEY_ID=AKIA... \
AWS_SECRET_ACCESS_KEY=... \
BIG_MODEL=anthropic.claude-3-5-sonnet-20241022-v2:0 \
SMALL_MODEL=anthropic.claude-3-5-haiku-20241022-v1:0 \
cargo run -p anyllm_proxy
```

### OpenAI Chat Completions Input

The proxy accepts `POST /v1/chat/completions` in OpenAI format and returns OpenAI format. This means any OpenAI-native client (LiteLLM, LangChain, etc.) can route through the proxy unchanged:

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "x-api-key: your-key" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "messages": [{"role": "user", "content": "Hello"}],
    "max_tokens": 100
  }'
```

### Virtual Key Management

Create short-lived or rate-limited API keys without restarting the proxy. Start with `--webui` to enable the admin server, then:

```bash
# Create a key with RPM limit
curl -X POST http://localhost:3001/admin/api/keys \
  -H "Authorization: Bearer $(cat .admin_token)" \
  -H "Content-Type: application/json" \
  -d '{"description": "dev key", "rpm_limit": 60}'
# Response: {"id": 1, "key": "sk-vk...", ...}

# Use the key like any other proxy key
curl http://localhost:3000/v1/messages \
  -H "x-api-key: sk-vk..." \
  -d '{"model": "claude-sonnet-4-20250514", "max_tokens": 100, "messages": [...]}'

# Revoke immediately (no restart needed)
curl -X DELETE http://localhost:3001/admin/api/keys/1 \
  -H "Authorization: Bearer $(cat .admin_token)"
```

### OpenTelemetry Export

```bash
cargo build -p anyllm_proxy --features otel

OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 \
OTEL_SERVICE_NAME=anyllm-proxy \
OPENAI_API_KEY=sk-... \
./target/debug/anyllm_proxy
```

Spans are exported via OTLP HTTP (protobuf). Standard OpenTelemetry SDK environment variables control endpoint, service name, and sampling. The feature adds zero runtime overhead when not compiled in.

---

## Using as a Library

The translation engine is available as standalone Rust crates. See [library-integration.md](docs/library-integration.md) for full examples.

```
crates/translator  (lib, IO-free pure translation)
    |
crates/client      (lib, async HTTP client wrapping translator)
    |
crates/proxy       (bin, full proxy server)
```

| Level | Crate | Use Case |
|---|---|---|
| **Pure translation** | `anyllm_translate` | Stateless type conversion between Anthropic and OpenAI formats. No IO, no HTTP. Bring your own transport. |
| **HTTP client** | `anyllm_client` | `client.messages(req).await` -- send Anthropic requests, get Anthropic responses. Handles translation, HTTP, retry, and streaming internally. |
| **Embedded middleware** | `anyllm_translate` with `middleware` feature | Drop-in Tower Layer or axum Router that adds `/v1/messages` to an existing server. |
| **Full proxy** | `anyllm_proxy` | Multi-backend routing, admin UI, metrics, auth. Everything in this README. |

### Pure Translation (no IO)

```rust
use anyllm_translate::{TranslationConfig, translate_request, translate_response};
use anyllm_translate::anthropic::MessageCreateRequest;

let config = TranslationConfig::builder()
    .model_map("haiku", "gpt-4o-mini")
    .model_map("sonnet", "gpt-4o")
    .build();

let anthropic_req: MessageCreateRequest = serde_json::from_str(&body)?;
let openai_req = translate_request(&anthropic_req, &config)?;
// ... send with your own HTTP client ...
let anthropic_resp = translate_response(&openai_resp, &anthropic_req.model);
```

For streaming:

```rust
use anyllm_translate::new_stream_translator;

let mut translator = new_stream_translator(model);
let events = translator.process_chunk(&chunk);
let final_events = translator.finish();
```

### HTTP Client (translation + transport)

```rust
use anyllm_client::{Client, ClientConfig, Auth};
use anyllm_translate::TranslationConfig;

let client = Client::new(
    ClientConfig::builder()
        .backend_url("https://api.openai.com/v1/chat/completions")
        .auth(Auth::Bearer("sk-...".into()))
        .translation(
            TranslationConfig::builder()
                .model_map("sonnet", "gpt-4o")
                .build()
        )
        .build()
);

let response = client.messages(&anthropic_request).await?;
let (stream, rate_limits) = client.messages_stream(&anthropic_request).await?;
```

### Embedded Middleware (for existing axum apps)

```rust
use anyllm_translate::middleware::{anthropic_compat_router, AnthropicCompatConfig};

let config = AnthropicCompatConfig::builder()
    .backend_url("https://api.openai.com")
    .api_key("sk-...")
    .build();

let app = Router::new()
    .merge(anthropic_compat_router(config))
    .route("/my-other-endpoint", get(handler));
```

---

## Advanced Features

- **Streaming SSE:** Real-time translation of chunked responses.
- **Tool Calling:** Transparent tool definition and `tool_use`/`tool_result` translation.
- **Image & Document Blocks:** Base64/URL and document block support.
- **Observability:** SQLite request logging, metrics endpoint, WebSocket live dashboard.
- **Safety:** SSRF protection, concurrency limits, exponential backoff retry.

## License

MIT
