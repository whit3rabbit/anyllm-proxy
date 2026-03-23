# llm-translate-api

API translation proxy that lets you use any OpenAI-compatible backend (OpenAI, local LLMs, OpenRouter, etc.) through the Anthropic Messages API. Supports streaming SSE, tool calling, image/document blocks, and standard error mapping.

This means tools built for Anthropic (like Claude Code) can talk to any backend that speaks OpenAI's Chat Completions format.

## Use Cases

- **AI coding tools**: Point Cursor, Windsurf, Cline, Aider, or any tool that supports an Anthropic endpoint at the proxy to use OpenAI, Gemini, local models, or OpenRouter instead.
- **Cost optimization**: Route haiku-tier requests to a cheap local model and sonnet/opus requests to a premium API. Mix and match with multi-backend config.
- **Self-hosted / air-gapped**: Organizations that can't send data externally but run OpenAI-compatible endpoints internally (Azure OpenAI, vLLM on-prem). Existing Anthropic-format client code works without changes.
- **Observability**: Centralized proxy with per-request logging (latency, token counts, status, backend), admin dashboard, and WebSocket live feed. Useful even with a single backend.
- **Development and testing**: Run Anthropic SDK integration tests against a local model instead of burning API credits.
- **Migration bridge**: Evaluate switching from Anthropic to OpenAI, Gemini, or open-source models without changing client code. Just swap the base URL.
- **Load balancing / failover**: Define multiple backends in TOML config. Hot-reload the default backend via the admin API without restarting.

## Quick Start

```bash
# Build
cargo build

# Run with OpenAI
OPENAI_API_KEY=sk-... cargo run -p anthropic_openai_proxy
```

The proxy listens on `0.0.0.0:3000`. An admin dashboard starts on `127.0.0.1:3001` (localhost only) with a random token printed to stderr.

Send Anthropic-format requests to the proxy:

```bash
curl -X POST http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: any-value" \
  -d '{
    "model": "claude-sonnet-4-6",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

The proxy translates the request to OpenAI format, forwards it, and returns an Anthropic-format response. The Anthropic model name in the request is mapped to the configured backend model (e.g., `claude-sonnet-4-6` becomes `gpt-4o`).

## Using with Claude Code

Point Claude Code at the proxy instead of the real Anthropic API:

```bash
# Start the proxy (pointing at OpenAI, a local LLM, or OpenRouter)
OPENAI_API_KEY=sk-... cargo run -p anthropic_openai_proxy &

# Run Claude Code against the proxy
ANTHROPIC_BASE_URL=http://localhost:3000 claude
```

### With a local LLM (Ollama, LM Studio, vLLM, etc.)

Any server that exposes an OpenAI-compatible `/v1/chat/completions` endpoint works:

```bash
# Ollama (runs on port 11434 by default)
ollama serve &
ollama pull llama3.1

OPENAI_API_KEY=unused \
OPENAI_BASE_URL=http://localhost:11434 \
BIG_MODEL=llama3.1 \
SMALL_MODEL=llama3.1 \
cargo run -p anthropic_openai_proxy &

ANTHROPIC_BASE_URL=http://localhost:3000 claude
```

```bash
# LM Studio (runs on port 1234 by default)
OPENAI_API_KEY=lm-studio \
OPENAI_BASE_URL=http://localhost:1234 \
BIG_MODEL=your-loaded-model \
SMALL_MODEL=your-loaded-model \
cargo run -p anthropic_openai_proxy
```

```bash
# vLLM
OPENAI_API_KEY=unused \
OPENAI_BASE_URL=http://localhost:8000 \
BIG_MODEL=meta-llama/Llama-3.1-70B-Instruct \
SMALL_MODEL=meta-llama/Llama-3.1-8B-Instruct \
cargo run -p anthropic_openai_proxy
```

### With OpenRouter

OpenRouter gives you access to many models through a single API key:

```bash
OPENAI_API_KEY=sk-or-... \
OPENAI_BASE_URL=https://openrouter.ai/api \
BIG_MODEL=anthropic/claude-sonnet-4-6 \
SMALL_MODEL=anthropic/claude-haiku-4-5-20251001 \
cargo run -p anthropic_openai_proxy &

ANTHROPIC_BASE_URL=http://localhost:3000 claude
```

You can use any model OpenRouter supports: `google/gemini-2.5-pro`, `meta-llama/llama-3.1-405b-instruct`, `mistralai/mistral-large`, etc.

### With Google Gemini

```bash
BACKEND=gemini \
GEMINI_API_KEY=AIza... \
BIG_MODEL=gemini-2.5-pro \
SMALL_MODEL=gemini-2.5-flash \
cargo run -p anthropic_openai_proxy &

ANTHROPIC_BASE_URL=http://localhost:3000 claude
```

## Multi-Backend Routing

For more complex setups, use a TOML config file to define multiple backends. Each backend gets its own route prefix, and one is designated as the default for unprefixed requests.

Create a `config.toml`:

```toml
listen_port = 3000
default_backend = "openai"

[backends.openai]
kind = "openai"
api_key = "sk-..."
big_model = "gpt-4o"
small_model = "gpt-4o-mini"

[backends.gemini]
kind = "gemini"
api_key = "AIza..."
big_model = "gemini-2.5-pro"
small_model = "gemini-2.5-flash"

[backends.local]
kind = "openai"
api_key = "unused"
base_url = "http://localhost:11434"
big_model = "llama3.1"
small_model = "llama3.1"

[backends.claude]
kind = "anthropic"
api_key = "sk-ant-..."
```

Run with the config file:

```bash
PROXY_CONFIG=config.toml cargo run -p anthropic_openai_proxy
```

This creates routes for each backend:

| Path | Backend |
|------|---------|
| `/v1/messages` | Default backend (openai) |
| `/openai/v1/messages` | OpenAI |
| `/gemini/v1/messages` | Gemini |
| `/local/v1/messages` | Local LLM (Ollama) |
| `/claude/v1/messages` | Anthropic passthrough (no translation) |

The `anthropic` backend kind is a passthrough: requests are forwarded to the real Anthropic API without translation. Useful for A/B testing or fallback routing.

API keys in the TOML can reference environment variables:

```toml
[backends.openai]
kind = "openai"
api_key = "env:OPENAI_API_KEY"
```

## Configuration

### Environment Variables (single backend)

| Variable | Default | Description |
|----------|---------|-------------|
| `BACKEND` | `openai` | Backend provider: `openai`, `vertex`, `gemini`, or `anthropic` |
| `OPENAI_API_KEY` | (required for openai) | API key for upstream calls |
| `OPENAI_BASE_URL` | `https://api.openai.com` | Base URL (change for local LLMs, OpenRouter, etc.) |
| `LISTEN_PORT` | `3000` | Server listen port |
| `BIG_MODEL` | `gpt-4o` | Model for sonnet/opus requests |
| `SMALL_MODEL` | `gpt-4o-mini` | Model for haiku requests |
| `PROXY_API_KEYS` | (unset) | Comma-separated allowed API keys. If unset, any non-empty key is accepted |
| `RUST_LOG` | `info` | Tracing filter (e.g., `debug`, `anthropic_openai_proxy=trace`) |
| `LOG_BODIES` | `false` | Log request/response bodies at debug level |

### TOML Config (multi-backend)

| Variable | Description |
|----------|-------------|
| `PROXY_CONFIG` | Path to TOML config file. When set, env-var-based config is ignored |

### Admin Dashboard

| Variable | Default | Description |
|----------|---------|-------------|
| `ADMIN_PORT` | `3001` | Admin dashboard port (localhost only) |
| `ADMIN_TOKEN` | (auto-generated) | Bearer token for admin API. If unset, printed to stderr at startup |
| `ADMIN_DB_PATH` | `admin.db` | SQLite database for request logs and config overrides |
| `ADMIN_LOG_RETENTION_DAYS` | `7` | Days to keep request log entries before purge |

See [docs/ENV.md](docs/ENV.md) for the full reference including mTLS client certificates and Vertex AI options.

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/messages` | Anthropic Messages API (streaming and non-streaming) |
| POST | `/{backend}/v1/messages` | Route to a specific backend (multi-backend mode) |
| GET | `/health` | Health check (`{"status":"ok"}`) |
| GET | `/metrics` | Per-backend request counters (JSON) |
| GET | `/v1/models` | Static model list |

**Admin endpoints** (on `ADMIN_PORT`, localhost only):

| Method | Path | Description |
|--------|------|-------------|
| GET | `/admin/` | Web dashboard (`?token=TOKEN` in URL) |
| GET | `/admin/health` | Admin health check (no auth) |
| GET | `/admin/api/config` | Effective config (env defaults + overrides) |
| PUT | `/admin/api/config` | Update config overrides (hot-reload) |
| GET | `/admin/api/config/overrides` | List SQLite config overrides |
| DELETE | `/admin/api/config/overrides/{key}` | Remove a single override |
| GET | `/admin/api/metrics` | Metrics with latency percentiles |
| GET | `/admin/api/requests` | Paginated request log (`?limit=`, `?offset=`, `?backend=`, `?status=`) |
| GET | `/admin/api/requests/{id}` | Single request detail |
| GET | `/admin/api/backends` | Backends with model mappings and metrics |
| GET | `/admin/ws?token=TOKEN` | WebSocket for live dashboard updates |

## Admin Dashboard

The proxy includes a localhost-only web UI for monitoring and configuration.

```bash
# Start the proxy (dashboard starts automatically)
OPENAI_API_KEY=sk-... cargo run -p anthropic_openai_proxy
# Look for "Admin token: <UUID>" in stderr

# Open the dashboard
open http://127.0.0.1:3001/admin/?token=YOUR_TOKEN_HERE

# Or use the API directly
curl -H "Authorization: Bearer YOUR_TOKEN" http://127.0.0.1:3001/admin/api/metrics
```

**Security:** The admin server binds to `127.0.0.1` only. A random UUID bearer token is required for all routes except `/admin/health`. Set `ADMIN_TOKEN` to use a fixed token.

**Tabs:**

- **Dashboard**: Live request feed via WebSocket, requests/min, error rate, p50/p95 latency, backend status
- **Request Log**: Paginated history with filters (backend, status class), stored in SQLite
- **Settings**: Hot-reload model mappings, log level, log bodies. Persists to SQLite, survives restarts
- **Backends**: Per-backend model mappings and request counters

**Hot-reload example:**

```bash
curl -X PUT http://127.0.0.1:3001/admin/api/config \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"backends": {"openai": {"big_model": "gpt-4-turbo"}}}'
```

## Features

- **Streaming SSE**: State machine translates OpenAI chunks to Anthropic stream events in real time
- **Tool calling**: Tool definitions, tool_use/tool_result blocks, ID passthrough, JSON string/object conversion
- **Image blocks**: Base64 and URL image content translated between formats
- **Document blocks**: PDFs and documents converted to text notes
- **Error mapping**: HTTP status codes and error shapes translated between APIs
- **Retry with backoff**: 3 retries on 429/5xx with exponential backoff, respects retry-after header
- **SSRF protection**: Validates base URLs, rejects private IPs, loopback, cloud metadata endpoints
- **Concurrency limits**: Prevents self-DOS under upstream rate limiting
- **Auth enforcement**: Requires x-api-key or Authorization header on API routes
- **Admin dashboard**: Localhost-only web UI with live traffic, request log, settings, backend status
- **Config hot-reload**: Change model mappings and settings at runtime via admin UI (persisted to SQLite)
- **Multi-backend routing**: Run multiple backends simultaneously with per-backend route prefixes

## Architecture

Two-crate workspace:

- **`crates/translator`** (`anthropic_openai_translate`): Pure translation logic, no IO. Stateless mapping functions between Anthropic and OpenAI types.
- **`crates/proxy`** (`anthropic_openai_proxy`): HTTP proxy built on axum + reqwest. Routes, middleware, SSE streaming, backend clients with retry.

```
Client (Anthropic format) -> proxy (axum)
  -> translator: anthropic types -> mapping -> openai types
  -> backend: reqwest -> upstream API
  -> translator: openai types -> mapping -> anthropic types
  -> proxy (axum) -> Client (Anthropic format)
```

## Known Limitations

- Document blocks are converted to text notes, not preserved as binary
- Anthropic cache token fields are dropped on round-trip (OpenAI has no equivalent)
- Model name mapping is static per config but can be changed at runtime via the admin dashboard
- Tool calling fidelity depends on the backend model's tool support

## Development

```bash
cargo test                           # ~438 tests
cargo clippy -- -D warnings          # lint
cargo fmt --check                    # format check
```

## License

MIT
