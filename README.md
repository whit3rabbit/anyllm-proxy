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

### Simple mode vs. advanced mode

| | Simple mode | Advanced mode |
|---|---|---|
| **Config** | 3 env vars or `.anyllm.env` | `config.toml` / `config.yaml` |
| **Routing** | Single backend | Multi-backend with path prefixes |
| **Admin UI** | Not started | `--webui` flag |
| **Translation warnings** | Silent (never exposed to clients) | `x-anyllm-degradation` header active |
| **How to enable** | Default | Pass `--webui`, set `PROXY_CONFIG`, or `ANYLLM_DEGRADATION_WARNINGS=true` |

Most users never leave simple mode. Start there.

Point Claude Code at the proxy:

```bash
ANTHROPIC_BASE_URL=http://localhost:3000 claude
```

### Admin Web Interface (optional)

Pass `--webui` (or `--admin`) to also start the admin dashboard on `127.0.0.1:3001`. The dashboard has the following tabs:

- **Dashboard:** Live RPM, error rate, P50/P95 latency, per-backend cards, and a filterable live request feed.
- **Request Log:** Historical request log with filters (backend, status, key, date range), paginated, with per-request cost and token detail.
- **Settings:** Mutable config (log level, log_bodies, per-backend model mappings), read-only env vars (secrets masked), and **Export .env** to generate a `.anyllm.env` template.
- **Backends:** Configured backends and their settings.
- **Access Control:** Virtual key CRUD — create, edit (RPM/TPM limits, budget, expiry, model allowlist), and revoke keys without restarting.
- **Models:** Add/remove model routing deployments (LiteLLM config mode only).
- **Audit:** Log of all admin config mutations and key lifecycle events.

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

Additional admin env vars: `ADMIN_DB_PATH` (SQLite file, default: `admin.db`), `ADMIN_TOKEN_PATH` (where the generated token is written, default: `.admin_token`), `ADMIN_LOG_RETENTION_DAYS` (request log retention, default: `7`).

## Advanced Mode

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

### Coming from LiteLLM? Drop in your config.yaml

anyllm-proxy accepts LiteLLM `config.yaml` files directly. If you already have a LiteLLM deployment, point the proxy at your existing config:

```bash
PROXY_CONFIG=config.yaml anyllm_proxy --webui
```

A standard LiteLLM config works as-is:

```yaml
# config.yaml (LiteLLM format)
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: azure/gpt-4o-eu
      api_base: https://my-resource.openai.azure.com/
      api_key: os.environ/AZURE_API_KEY
      rpm: 6000
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: os.environ/OPENAI_API_KEY
      rpm: 10000
  - model_name: claude-3-opus
    litellm_params:
      model: anthropic/claude-3-opus-20240229
      api_key: os.environ/ANTHROPIC_API_KEY

general_settings:
  master_key: os.environ/LITELLM_MASTER_KEY
```

Multiple deployments of the same model name are load-balanced with round-robin routing. Deployments at their RPM limit are automatically skipped.

**Env var compatibility:** LiteLLM env var names are accepted as aliases, so you do not need to rename anything:

| LiteLLM env var | anyllm-proxy equivalent | Notes |
|---|---|---|
| `LITELLM_MASTER_KEY` | `PROXY_API_KEYS` | Admin/auth key |
| `LITELLM_CONFIG` | `PROXY_CONFIG` | Config file path |
| `AZURE_API_KEY` | `AZURE_OPENAI_API_KEY` | Azure auth |
| `AZURE_API_BASE` | `AZURE_OPENAI_ENDPOINT` | Azure endpoint |
| `AZURE_API_VERSION` | `AZURE_OPENAI_API_VERSION` | Azure API version |
| `AWS_REGION_NAME` | `AWS_REGION` | Bedrock region |
| `OPENAI_API_KEY` | `OPENAI_API_KEY` | Same name |
| `ANTHROPIC_API_KEY` | `ANTHROPIC_API_KEY` | Same name |

The `os.environ/VAR_NAME` syntax in YAML values is supported alongside anyllm's native `env:VAR_NAME`. See [docs/COMPARISON_LITELLM.md](docs/COMPARISON_LITELLM.md) for a full feature comparison.

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

Additional per-backend fields: `api_format = "chat"` (OpenAI only; `chat` or `responses`), `omit_stream_options = true` (strip `stream_options` for backends that reject it). Top-level `log_bodies = true` enables request/response body logging. Any config value can use `env:VAR_NAME` to read from the environment at startup (e.g., `api_key = "env:OPENAI_API_KEY"`).

Key environment variables for advanced mode:

- `LOG_BODIES`: Enable request/response body logging at debug level (`true` or `1`, default: disabled).
- `ANYLLM_DEGRADATION_WARNINGS`: Set to `true` or `1` to expose `x-anyllm-degradation` response header when translation silently drops features (default: disabled; auto-enabled when `PROXY_CONFIG` is set).

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

The dashboard tabs are described under [Admin Web Interface](#admin-web-interface-optional) above. When using a LiteLLM config, the **Models** tab lets you add/remove deployments without editing the config file. All config mutations (model changes, key creation/revocation) are recorded in the **Audit** tab.

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

### Anthropic Passthrough

Forwards requests to the Anthropic API with no format translation. Use this when the upstream is already Anthropic and you only need auth, routing, or rate limiting from the proxy.

```bash
BACKEND=anthropic \
ANTHROPIC_API_KEY=sk-ant-... \
cargo run -p anyllm_proxy
```

`ANTHROPIC_BASE_URL` overrides the upstream URL (default: `https://api.anthropic.com`). Note: `POST /v1/embeddings` is not available on this backend.

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

Create short-lived, rate-limited, or budget-capped API keys without restarting the proxy. Start with `--webui` to enable the admin server, then:

```bash
# Create a key with RPM/TPM limits, a monthly budget, and a model allowlist
curl -X POST http://localhost:3001/admin/api/keys \
  -H "Authorization: Bearer $(cat .admin_token)" \
  -H "Content-Type: application/json" \
  -d '{
    "description": "dev key",
    "rpm_limit": 60,
    "tpm_limit": 100000,
    "max_budget_usd": 10.00,
    "budget_duration": "monthly",
    "expires_at": "2026-12-31T00:00:00Z",
    "allowed_models": ["claude-*", "gpt-4o"]
  }'
# Response: {"id": 1, "key": "sk-vk...", ...}

# Use the key like any other proxy key
curl http://localhost:3000/v1/messages \
  -H "x-api-key: sk-vk..." \
  -d '{"model": "claude-sonnet-4-20250514", "max_tokens": 100, "messages": [...]}'

# Update limits on an existing key (no restart needed)
curl -X PUT http://localhost:3001/admin/api/keys/1 \
  -H "Authorization: Bearer $(cat .admin_token)" \
  -H "Content-Type: application/json" \
  -d '{"rpm_limit": 120, "max_budget_usd": 20.00}'

# Check spend for a key
curl http://localhost:3001/admin/api/keys/1/spend \
  -H "Authorization: Bearer $(cat .admin_token)"

# Revoke immediately (no restart needed)
curl -X DELETE http://localhost:3001/admin/api/keys/1 \
  -H "Authorization: Bearer $(cat .admin_token)"
```

`budget_duration` accepts `daily`, `monthly`, or `lifetime`. `allowed_models` supports exact names and `prefix/*` wildcards. A key at 100% of its budget returns 429 with period reset information. Webhook notifications fire at 80%, 95%, and 100% of the budget via `WEBHOOK_URLS`.

Requests from unauthenticated clients are rejected by default. For local development, set `PROXY_OPEN_RELAY=true` to accept any non-empty key (insecure, never use in production).

**Distributed rate limiting (optional):** Build with `--features redis` and set `REDIS_URL=redis://localhost:6379` to use Redis-backed rate limiting across multiple proxy instances. In-process rate limits are per-instance only. `RATE_LIMIT_FAIL_POLICY=open` (default) allows requests when Redis is unavailable; `closed` rejects them with 503.

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

The translation engine is available as standalone Rust crates.

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
| **Embedded middleware** | `anyllm_translate` with `middleware` feature | Drop-in axum Router that adds `/v1/messages` to an existing server. |
| **Full proxy** | `anyllm_proxy` | Multi-backend routing, admin UI, metrics, auth. Everything in this README. |

### Adding as a dependency

```toml
[dependencies]
# HTTP client (includes translation)
anyllm_client = { git = "https://github.com/whit3rabbit/anyllm-proxy" }

# Translation only (no HTTP, no async)
anyllm_translate = { git = "https://github.com/whit3rabbit/anyllm-proxy" }

# With axum middleware support
anyllm_translate = { git = "https://github.com/whit3rabbit/anyllm-proxy", features = ["middleware"] }
```

### HTTP Client (translation + transport)

The simplest path. Send Anthropic requests, get Anthropic responses. Translation, retry, and SSE streaming are handled internally.

```rust
use anyllm_client::{Client, ClientError};
use anyllm_translate::anthropic::MessageCreateRequest;

let client = Client::builder()
    .base_url("https://api.openai.com/v1/chat/completions")
    .api_key("sk-...")
    .build()?;

let req: MessageCreateRequest = serde_json::from_str(r#"{
    "model": "claude-sonnet-4-6",
    "max_tokens": 256,
    "messages": [{"role": "user", "content": "Hello"}]
}"#)?;

let response = client.messages(&req).await?;
```

For custom TLS, SSRF protection, or per-model mapping, use `ClientConfig::builder()`:

```rust
use anyllm_client::{Client, ClientConfig, Auth};
use anyllm_translate::TranslationConfig;

let client = Client::new(
    ClientConfig::builder()
        .backend_url("https://api.openai.com/v1/chat/completions")
        .auth(Auth::Bearer("sk-...".into()))
        .translation(
            TranslationConfig::builder()
                .model_map("claude-sonnet-4-6", "gpt-4o")
                .model_map("claude-haiku-4-5", "gpt-4o-mini")
                .build()
        )
        .build()
);
```

**Error handling:**

```rust
match client.messages(&req).await {
    Ok(resp) => { /* ... */ }
    Err(ClientError::ApiError { status, body, .. }) => eprintln!("HTTP {status}: {body}"),
    Err(ClientError::Transport(e)) => eprintln!("network: {e}"),
    Err(ClientError::Translation(e)) => eprintln!("translation: {e}"),
    Err(e) => eprintln!("{e}"),
}
```

**Streaming:**

```rust
use anyllm_translate::anthropic::{Delta, StreamEvent};
use futures::StreamExt;

let (mut stream, _rate_limits) = client.messages_stream(&req).await?;
while let Some(event) = stream.next().await {
    if let StreamEvent::ContentBlockDelta { delta: Delta::TextDelta { text }, .. } = event? {
        print!("{text}");
    }
}
```

**Tool calling:**

```rust
use anyllm_client::{ToolBuilder, ToolChoiceBuilder};
use serde_json::json;

let tool = ToolBuilder::new("get_weather")
    .description("Get the current weather for a location")
    .input_schema(json!({
        "type": "object",
        "properties": {"location": {"type": "string"}},
        "required": ["location"]
    }))
    .build();
// Attach tool to MessageCreateRequest via serde_json, then call client.messages().
```

Runnable examples: `cargo run --example basic -p anyllm_client`, `streaming`, `tools`.

### Pure Translation (no IO)

Use when you want to bring your own HTTP client or embed translation in a non-async context.

```rust
use anyllm_translate::{TranslationConfig, translate_request, translate_response};
use anyllm_translate::anthropic::MessageCreateRequest;

let config = TranslationConfig::builder()
    .model_map("claude-sonnet-4-6", "gpt-4o")
    .build();

let anthropic_req: MessageCreateRequest = serde_json::from_str(&body)?;
let openai_req = translate_request(&anthropic_req, &config)?;
// ... send openai_req with your HTTP client ...
let anthropic_resp = translate_response(&openai_resp, &anthropic_req.model);
```

**Streaming (OpenAI chunks → Anthropic SSE events):**

```rust
use anyllm_translate::new_stream_translator;

let mut translator = new_stream_translator(model);
// Feed each OpenAI chunk as it arrives:
let events = translator.process_chunk(&chunk);
// After the stream ends:
let final_events = translator.finish();
```

**Reverse direction (OpenAI ← Anthropic), for serving OpenAI-native clients:**

```rust
use anyllm_translate::{
    translate_openai_to_anthropic_request,
    translate_anthropic_to_openai_response,
    new_reverse_stream_translator,
    TranslationWarnings,
};

let mut warnings = TranslationWarnings::default();
let anthropic_req = translate_openai_to_anthropic_request(&openai_req, &mut warnings)?;
// ... forward to Anthropic API ...
let openai_resp = translate_anthropic_to_openai_response(&anthropic_resp, "gpt-4o");
```

Runnable examples: `cargo run --example translate_request -p anyllm_translate`, `reverse_translation`.

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

For cross-language bindings (FFI, WASM, PyO3), see [docs/library-integration.md](docs/library-integration.md).

---

## Advanced Features

- **Streaming SSE:** Real-time translation of chunked responses.
- **Tool Calling:** Transparent tool definition and `tool_use`/`tool_result` translation.
- **Image & Document Blocks:** Base64/URL and document block support.
- **Embeddings passthrough:** `POST /v1/embeddings` forwarded as-is to the backend (no translation). Works with OpenAI, Azure, Vertex, Gemini, and vLLM. Not available when `BACKEND=anthropic`.
- **Degradation header:** `x-anyllm-degradation` is set on responses when features are silently dropped during translation (e.g., `top_k`, `cache_control`, `document_blocks`, `thinking_config`).
- **Model allowlist:** Per-virtual-key restriction by exact model name or `prefix/*` wildcard, enforced pre-request.
- **Budget tracking and spend alerts:** Per-key `max_budget_usd` with daily/monthly/lifetime periods. Webhook notifications (via `WEBHOOK_URLS`) fire at 80%, 95%, and 100% of the budget.
- **Audit log:** All admin config mutations and key lifecycle events stored in SQLite, queryable via `GET /admin/api/audit`.
- **OIDC/JWT authentication:** Set `OIDC_ISSUER_URL` (and optionally `OIDC_AUDIENCE`) to accept JWT bearer tokens for proxy authentication.
- **Observability:** SQLite request logging, metrics endpoint, WebSocket live dashboard.
- **Safety:** SSRF protection (including IPv6 ULA/link-local), concurrency limits, exponential backoff retry, CSRF protection on admin endpoints.

## License

MIT
