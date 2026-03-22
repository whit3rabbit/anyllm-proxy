# llm-translate-api

Anthropic-to-OpenAI API translation proxy. Accepts requests in the Anthropic Messages API format, translates them to OpenAI Chat Completions, forwards to OpenAI, and translates the response back.

Supports streaming SSE, tool calling (function calling), image/document blocks, and standard error mapping.

## Quick Start

```bash
# Build
cargo build

# Run (requires an OpenAI API key)
OPENAI_API_KEY=sk-... cargo run -p anthropic_openai_proxy
```

The proxy listens on `0.0.0.0:3000` by default.

## Usage

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

The proxy translates the request to OpenAI format, forwards it, and returns an Anthropic-format response.

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_KEY` | (required) | OpenAI API key for upstream calls |
| `OPENAI_BASE_URL` | `https://api.openai.com` | OpenAI base URL (for proxies or compatible APIs) |
| `LISTEN_PORT` | `3000` | Server listen port |
| `BIG_MODEL` | `gpt-4o` | OpenAI model for sonnet/opus requests |
| `SMALL_MODEL` | `gpt-4o-mini` | OpenAI model for haiku requests |
| `RUST_LOG` | `info` | Tracing filter (e.g., `debug`, `anthropic_openai_proxy=trace`) |

Additional variables are available for mTLS client certificates and custom CA certs when connecting to endpoints that require them. See [docs/ENV.md](docs/ENV.md) for the full reference.

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/messages` | Anthropic Messages API (streaming and non-streaming) |
| GET | `/health` | Health check (returns `{"status":"ok"}`) |
| GET | `/metrics` | Request count, success/error counters (JSON) |
| GET | `/v1/models` | Static model list |
| POST | `/v1/messages/count_tokens` | Returns unsupported error |
| POST | `/v1/messages/batches` | Returns unsupported error |

## Features

- **Non-streaming translation**: Full request/response round-trip between Anthropic and OpenAI formats
- **Streaming SSE**: State machine translates OpenAI chunks to Anthropic stream events in real time
- **Tool calling**: Tool definitions, tool_use/tool_result blocks, ID passthrough, JSON string/object conversion
- **Image blocks**: Base64 and URL image content translated between formats
- **Document blocks**: PDFs and documents converted to text notes (full fidelity requires OpenAI Responses API)
- **Error mapping**: HTTP status codes and error shapes translated between APIs
- **Retry with backoff**: 3 retries on 429/5xx with exponential backoff, respects retry-after header
- **SSRF protection**: Validates OPENAI_BASE_URL rejects private IPs, loopback, cloud metadata endpoints
- **Concurrency limits**: Prevents self-DOS under upstream rate limiting
- **Auth enforcement**: Requires x-api-key or Authorization header on API routes

## Architecture

Two-crate workspace:

- **`crates/translator`** (`anthropic_openai_translate`): Pure translation logic, no IO. Stateless mapping functions between Anthropic and OpenAI types.
- **`crates/proxy`** (`anthropic_openai_proxy`): HTTP proxy built on axum + reqwest. Routes, middleware, SSE streaming, OpenAI client with retry.

```
Client (Anthropic format) -> proxy (axum)
  -> translator: anthropic types -> mapping -> openai types
  -> backend: reqwest -> OpenAI Chat Completions
  -> translator: openai types -> mapping -> anthropic types
  -> proxy (axum) -> Client (Anthropic format)
```

## Known Limitations

- OpenAI Responses API backend types are defined but not wired up (proxy uses Chat Completions only)
- Model name mapping is static (hardcoded list), not configurable
- Document blocks are converted to text notes, not preserved as binary
- Anthropic cache token fields are dropped on round-trip (OpenAI has no equivalent)

## Development

```bash
cargo test                           # 169 tests
cargo clippy -- -D warnings          # lint
cargo fmt --check                    # format check
```

## References

- [OpenAI OpenAPI spec](https://github.com/openai/openai-openapi/blob/manual_spec/openapi.yaml) - canonical API specification (very large, ~70k+ lines of YAML). Background: [Simon Willison's notes on the spec](https://simonwillison.net/2024/Dec/22/openai-openapi/).

## License

MIT
