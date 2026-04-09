# anyllm-proxy

An API translation proxy that lets Anthropic-based tools (Claude Code, Cursor, Windsurf, Cline) talk to any OpenAI-compatible backend, local LLM, or alternative provider.

**[Releases](https://github.com/whit3rabbit/anyllm-proxy/releases)** | **[ENV Reference](docs/ENV.md)** | **[Config Reference](docs/CONFIG.md)**

---

## Install

**macOS (Homebrew):**
```bash
brew install whit3rabbit/tap/anyllm-proxy
```

**Linux (Debian/Ubuntu):**
```bash
# Download and install the .deb package (amd64 or arm64)
curl -LO https://github.com/whit3rabbit/anyllm-proxy/releases/latest/download/anyllm-proxy_0.9.0-1_amd64.deb
sudo dpkg -i anyllm-proxy_*.deb
sudo systemctl enable --now anyllm-proxy
# Edit /etc/default/anyllm-proxy to set env vars
```

**Binary (all platforms):** Download from the [releases page](https://github.com/whit3rabbit/anyllm-proxy/releases).

<details>
<summary>Other install methods</summary>

```bash
# Cargo (from source)
cargo install anyllm_proxy

# Build from source
cargo build -p anyllm_proxy --release

# Docker
docker run -e OPENAI_API_KEY=sk-... -p 3000:3000 followthewhit3rabbit/anyllm-proxy:latest
```

</details>

---

## Quick Start

Create a `.anyllm.env` config file in `~/.anyllm/` (or the current directory):

```env
OPENAI_API_KEY=unused
OPENAI_BASE_URL=http://localhost:11434/v1
BIG_MODEL=qwen2.5-coder:32b
SMALL_MODEL=qwen2.5-coder:32b
```

Run the proxy (auto-loads `.anyllm.env` from `~/.anyllm/` or the current directory):

```bash
anyllm_proxy
# or: anyllm_proxy --env-file ~/configs/ollama.env
```

Point Claude Code at the proxy:

```bash
ANTHROPIC_BASE_URL=http://localhost:3000 \
ANTHROPIC_AUTH_TOKEN=proxy-user \
ANTHROPIC_API_KEY="" \
CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
claude
```

Or use the `run` subcommand to do the same in one step:

```bash
anyllm_proxy run claude
```

### Simple mode vs. advanced mode

| | Simple mode | Advanced mode |
|---|---|---|
| **Config** | 3 env vars or `.anyllm.env` | `config.toml` / `config.yaml` |
| **Routing** | Single backend | Multi-backend with path prefixes |
| **Admin UI** | `--webui` (guided setup if no config) | `--webui` (full dashboard) |
| **Translation warnings** | Silent (never exposed to clients) | `x-anyllm-degradation` header active |
| **How to enable** | Default | Pass `--webui`, set `PROXY_CONFIG`, or `ANYLLM_DEGRADATION_WARNINGS=true` |

Most users never leave simple mode. Start there.

---

## Backends

### Local LLMs (Ollama, LM Studio, vLLM)

```bash
# Ollama
OPENAI_API_KEY=unused \
OPENAI_BASE_URL=http://localhost:11434/v1 \
BIG_MODEL=qwen2.5-coder:32b \
SMALL_MODEL=qwen2.5-coder:32b \
anyllm_proxy
```

Use the same pattern for **LM Studio** (port `1234`) or **vLLM** (port `8000`) by substituting `OPENAI_BASE_URL`.

If your local LLM rejects `stream_options`, set `OMIT_STREAM_OPTIONS=true`.

### Commercial APIs

**OpenAI:**
```bash
OPENAI_API_KEY=sk-... BIG_MODEL=gpt-4o SMALL_MODEL=gpt-4o-mini anyllm_proxy
```

**OpenRouter:**
```bash
# Using the dedicated provider key (recommended):
BACKEND=openrouter \
OPENROUTER_API_KEY=sk-or-... \
BIG_MODEL=anthropic/claude-3.5-sonnet \
SMALL_MODEL=anthropic/claude-3-haiku \
anyllm_proxy

# Or via the generic OpenAI-compat path:
OPENAI_API_KEY=sk-or-... \
OPENAI_BASE_URL=https://openrouter.ai/api/v1 \
BIG_MODEL=anthropic/claude-3.5-sonnet \
SMALL_MODEL=anthropic/claude-3-haiku \
anyllm_proxy
```

**Google Gemini:**
```bash
BACKEND=gemini GEMINI_API_KEY=AIza... anyllm_proxy
```

**Azure OpenAI:**
```bash
BACKEND=azure \
AZURE_OPENAI_ENDPOINT=https://myresource.openai.azure.com \
AZURE_OPENAI_DEPLOYMENT=my-gpt4o \
AZURE_OPENAI_API_KEY=... \
anyllm_proxy
```

**AWS Bedrock:**
```bash
BACKEND=bedrock \
AWS_REGION=us-east-1 \
AWS_ACCESS_KEY_ID=AKIA... \
AWS_SECRET_ACCESS_KEY=... \
BIG_MODEL=anthropic.claude-3-5-sonnet-20241022-v2:0 \
SMALL_MODEL=anthropic.claude-3-5-haiku-20241022-v1:0 \
anyllm_proxy
```

**Anthropic Passthrough** (no translation, for auth/routing/rate-limiting only):
```bash
BACKEND=anthropic ANTHROPIC_API_KEY=sk-ant-... anyllm_proxy
```

See [docs/ENV.md](docs/ENV.md) for the full variable reference.

---

## Admin Web Interface

Pass `--webui` (or `--admin`) to start the admin dashboard alongside the proxy:

```bash
anyllm_proxy --webui
# Proxy:     http://localhost:3000
# Admin UI:  http://127.0.0.1:3001/admin/?token=$(cat ~/.anyllm/.admin_token)
```

If no backend is configured, the UI opens on the **Settings** tab with a getting-started guide and env file import.

The admin server binds to `127.0.0.1:3001` by default (localhost only). Dashboard tabs:

- **Dashboard:** Live RPM, error rate, P50/P95 latency, per-backend cards, filterable live request feed.
- **Request Log:** Historical log with filters (backend, status, key, date range), paginated, with per-request cost and token detail.
- **Access Control:** Virtual key CRUD -- create, edit (RPM/TPM limits, budget, expiry, model allowlist), revoke without restarting.
- **Backends:** Configured backends and their status.
- **Models:** Discover models from providers (OpenRouter, DeepInfra, Ollama, or configured backend), add/remove deployments. Changes are persisted to SQLite and survive restarts.
- **Audit:** All admin config mutations and key lifecycle events.
- **Settings:** Mutable config (log level, log_bodies, model mappings), read-only env vars (secrets masked), **Import/Export .anyllm.env**. Shows a getting-started guide when no backend is configured.

**Token:** On first start an admin token is auto-generated and written to `~/.anyllm/.admin_token`. Pass it as `?token=` in the URL or `Authorization: Bearer` for API calls. To set a fixed token:

```bash
ADMIN_TOKEN=mysecret anyllm_proxy --webui
# Generate a strong token: openssl rand -hex 32
```

<details>
<summary>Admin env vars and Docker setup</summary>

| Variable | Default | Description |
|---|---|---|
| `ADMIN_PORT` | `3001` | Admin server port |
| `ADMIN_BIND` | `127.0.0.1` | Bind address (`0.0.0.0` in Docker) |
| `ADMIN_TOKEN` | auto-generated | Fixed token (min 32 chars recommended) |
| `ADMIN_TOKEN_PATH` | `~/.anyllm/.admin_token` | Where the auto-generated token is written |
| `ADMIN_DB_PATH` | `~/.anyllm/admin.db` | SQLite database path |
| `ANYLLM_HOME` | `~/.anyllm` | Data directory for all default file paths |
| `ADMIN_LOG_RETENTION_DAYS` | `7` | Request log retention |
| `DISABLE_ADMIN` | -- | Set to `1` to force-disable |
| `WEBUI` / `ADMIN` | -- | Docker entrypoint shorthand for `--webui` |

**Custom port / disable:**

```bash
ADMIN_PORT=4000 anyllm_proxy --webui          # change port
DISABLE_ADMIN=1 anyllm_proxy --webui          # do not start admin even when flag is present
```

**Docker:** The admin server must be reachable from outside the container. Set `ADMIN_BIND=0.0.0.0` and expose the port:

```bash
docker run -e OPENAI_API_KEY=sk-... -e WEBUI=1 -e ADMIN_BIND=0.0.0.0 \
  -p 3000:3000 -p 127.0.0.1:3001:3001 followthewhit3rabbit/anyllm-proxy:latest

# docker-compose (recommended)
docker compose up
# Token: docker compose exec proxy cat /data/.admin_token
```

**CSRF:** State-mutating admin API calls (POST/PUT/DELETE) require an `X-CSRF-Token` header. Fetch a one-time token from `GET /admin/csrf-token` before each mutating request. The SPA handles this automatically; scripts must do it explicitly. Admin endpoints are rate-limited to 10 requests/minute per IP.

</details>

---

## Virtual Key Management

Create short-lived, rate-limited, or budget-capped API keys without restarting the proxy. Requires `--webui`.

```bash
# Create a key with RPM/TPM limits, a monthly budget, and a model allowlist
curl -X POST http://localhost:3001/admin/api/keys \
  -H "Authorization: Bearer $(cat ~/.anyllm/.admin_token)" \
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
```

<details>
<summary>More virtual key operations</summary>

```bash
# Use the key like any other proxy key
curl http://localhost:3000/v1/messages \
  -H "x-api-key: sk-vk..." \
  -d '{"model": "claude-sonnet-4-20250514", "max_tokens": 100, "messages": [...]}'

# Update limits on an existing key (no restart needed)
curl -X PUT http://localhost:3001/admin/api/keys/1 \
  -H "Authorization: Bearer $(cat ~/.anyllm/.admin_token)" \
  -H "Content-Type: application/json" \
  -d '{"rpm_limit": 120, "max_budget_usd": 20.00}'

# Check spend for a key
curl http://localhost:3001/admin/api/keys/1/spend \
  -H "Authorization: Bearer $(cat ~/.anyllm/.admin_token)"

# Revoke immediately (no restart needed)
curl -X DELETE http://localhost:3001/admin/api/keys/1 \
  -H "Authorization: Bearer $(cat ~/.anyllm/.admin_token)"
```

`budget_duration` accepts `daily`, `monthly`, or `lifetime`. `allowed_models` supports exact names and trailing-wildcard patterns (e.g., `claude-*`). A key at 100% of its budget returns 429 with period reset information. Webhook notifications fire at 80%, 95%, and 100% of the budget via `WEBHOOK_URLS`.

</details>

Requests from unauthenticated clients are rejected by default. For local development, set `PROXY_OPEN_RELAY=true` to accept any non-empty key.

**Distributed rate limiting (optional):** Build with `--features redis` and set `REDIS_URL` to share rate limit state across multiple proxy instances. `RATE_LIMIT_FAIL_POLICY=open` (default) allows requests when Redis is unavailable; `closed` rejects them with 503.

---

## Multi-Backend Routing

A single proxy instance can serve all your backends simultaneously. Each backend gets its own URL path.

### TOML config

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
api_key = "env:OPENAI_API_KEY"
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

Additional per-backend fields: `api_format = "chat"` (OpenAI only; `chat` or `responses`), `omit_stream_options = true` (strip `stream_options` for backends that reject it). Top-level `log_bodies = true` enables request/response body logging. Any config value can use `env:VAR_NAME` to read from the environment at startup.

### LiteLLM config (drop-in)

anyllm-proxy accepts LiteLLM `config.yaml` files directly:

```bash
PROXY_CONFIG=config.yaml anyllm_proxy --webui
```

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

general_settings:
  master_key: os.environ/LITELLM_MASTER_KEY
```

Multiple deployments of the same model name are load-balanced with round-robin routing. Deployments at their RPM limit are automatically skipped. The `os.environ/VAR_NAME` syntax is supported alongside `env:VAR_NAME`.

<details>
<summary>LiteLLM env var aliases</summary>

| LiteLLM env var | anyllm-proxy equivalent |
|---|---|
| `LITELLM_MASTER_KEY` | `PROXY_API_KEYS` |
| `LITELLM_CONFIG` | `PROXY_CONFIG` |
| `AZURE_API_KEY` | `AZURE_OPENAI_API_KEY` |
| `AZURE_API_BASE` | `AZURE_OPENAI_ENDPOINT` |
| `AZURE_API_VERSION` | `AZURE_OPENAI_API_VERSION` |
| `AWS_REGION_NAME` | `AWS_REGION` |

See [docs/COMPARISON_LITELLM.md](docs/COMPARISON_LITELLM.md) for a full feature comparison.

</details>

### Multiple separate instances

For completely separate proxy processes (different ports, machines, or containers), keep one `.env` file per deployment:

```bash
anyllm_proxy --env-file ~/proxies/deepseek.env
# Docker-compatible:
docker run --env-file ~/proxies/openai-prod.env -p 3000:3000 anyllm-proxy
```

The admin UI's **Export .env** button (Settings tab) generates a ready-to-edit template from the current configuration.

---

## Config Directory

All data files live in `~/.anyllm/` by default. The directory is created on first run.

```
~/.anyllm/
  admin.db          SQLite (keys, models, audit, env imports)
  .admin_token      Auto-generated admin auth token
  .anyllm.env       Environment file (auto-loaded if present)
  config.yaml       Proxy config (auto-detected if present)
```

Override the directory with `ANYLLM_HOME=/path/to/dir`, or override individual files with `ADMIN_DB_PATH`, `ADMIN_TOKEN_PATH`, `--env-file`, or `PROXY_CONFIG`.

The proxy looks for `.anyllm.env` in three places (first match wins): `--env-file` flag, then the current directory, then `~/.anyllm/`. Similarly, `config.yaml` is auto-detected in `~/.anyllm/` when `PROXY_CONFIG` is not set.

Docker Compose sets explicit paths (`/data/admin.db`, `/data/.admin_token`) so the home directory convention does not apply in containers. See [docs/CONFIG.md](docs/CONFIG.md) for full details.

---

## Features

- **Streaming SSE:** Real-time translation of chunked responses.
- **Tool Calling:** Transparent tool definition and `tool_use`/`tool_result` translation.
- **Image and Document Blocks:** Base64/URL and document block support.
- **OpenAI input:** `POST /v1/chat/completions` accepts OpenAI format and returns OpenAI format, so OpenAI-native clients work unchanged.
- **Embeddings passthrough:** `POST /v1/embeddings` forwarded as-is to the backend. Works with OpenAI, Azure, Vertex, Gemini, and vLLM. Not available when `BACKEND=anthropic`.
- **Degradation header:** `x-anyllm-degradation` is set when features are silently dropped during translation (e.g., `top_k`, `cache_control`, `document_blocks`, `thinking_config`).
- **Model allowlist:** Per-virtual-key restriction by exact model name or `prefix/*` wildcard, enforced pre-request.
- **Budget tracking and spend alerts:** Per-key `max_budget_usd` with daily/monthly/lifetime periods. Webhooks fire at 80%, 95%, and 100%.
- **Audit log:** All admin config mutations and key lifecycle events stored in SQLite.
- **OIDC/JWT authentication:** Set `OIDC_ISSUER_URL` to accept JWT bearer tokens.
- **OpenTelemetry:** Build with `--features otel` for OTLP trace export. Zero overhead when not compiled in.
- **Safety:** SSRF protection (including IPv6 ULA/link-local), concurrency limits, exponential backoff retry, CSRF protection on admin endpoints.

---

## Docker

```bash
# Pull and run
docker run -e OPENAI_API_KEY=sk-... -p 3000:3000 followthewhit3rabbit/anyllm-proxy:latest

# With admin UI
docker run -e OPENAI_API_KEY=sk-... -e WEBUI=1 -e ADMIN_BIND=0.0.0.0 \
  -p 3000:3000 -p 127.0.0.1:3001:3001 followthewhit3rabbit/anyllm-proxy:latest

# docker-compose (recommended)
cp .env.example .env   # set OPENAI_API_KEY
docker compose up
```

<details>
<summary>Smoke tests (no real API key needed)</summary>

```bash
docker compose -f docker-compose.test.yml up -d --build
bash scripts/docker-smoke-test.sh
docker compose -f docker-compose.test.yml down -v
```

</details>

---

<details>
<summary><strong>Using as a Library</strong></summary>

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

**Streaming (OpenAI chunks to Anthropic SSE events):**

```rust
use anyllm_translate::new_stream_translator;

let mut translator = new_stream_translator(model);
// Feed each OpenAI chunk as it arrives:
let events = translator.process_chunk(&chunk);
// After the stream ends:
let final_events = translator.finish();
```

**Reverse direction (OpenAI from Anthropic), for serving OpenAI-native clients:**

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

</details>

---

## License

MIT
