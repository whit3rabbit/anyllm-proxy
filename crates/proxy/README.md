# anyllm_proxy

HTTP proxy that accepts Anthropic Messages API and OpenAI Chat Completions requests, translates between formats, and forwards to any supported backend (OpenAI, Anthropic, Azure, Vertex, Gemini, Bedrock, or any of the ~90 OpenAI-compatible providers in the catalog). Ships with an optional admin UI, virtual key management, response caching, cost tracking, and a Batch API.

This is the top-level binary crate in the [anyllm-proxy](https://github.com/whit3rabbit/anyllm-proxy) workspace.

## Where it fits

Five-crate workspace:

- `anyllm_translate` - pure format mapping, no I/O.
- `anyllm_providers` - provider and model catalog (~90 providers).
- `anyllm_client` - async HTTP client wrapping translate + transport.
- `anyllm_batch_engine` - batch job queue and worker pool.
- `anyllm_proxy` (this crate) - the axum HTTP server that wires everything together.

If you only need format translation in your own Rust code, depend on `anyllm_translate` or `anyllm_client` directly. Use this crate when you want a standalone server with config files, key management, an admin UI, metrics, and a batch surface.

## Run it

```bash
OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy
# Listens on 0.0.0.0:3000, health at GET /health
```

With the admin UI on a separate port (3001 by default):

```bash
OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy -- --webui
```

Switch backends via `BACKEND=`:

```bash
GROQ_API_KEY=... BACKEND=groq cargo run -p anyllm_proxy
ANTHROPIC_API_KEY=... BACKEND=anthropic cargo run -p anyllm_proxy
```

Any provider id from `anyllm_providers` is accepted as a `BACKEND` value. Full env var reference lives in [`docs/ENV.md`](../../docs/ENV.md).

## Cargo features

| Feature | What it adds |
|---|---|
| `redis` | Distributed rate limiting via Redis sorted sets. |
| `qdrant` | Optional semantic response cache backed by Qdrant. |
| `otel` | OpenTelemetry OTLP trace export. |
| `dangerous-builtin-tools` | Enables BashTool and ReadFileTool in the builtin tool registry. **Do not enable in production without sandboxing.** |

## Modules

| Module | Purpose |
|---|---|
| `server` | Axum HTTP server: routes, middleware (auth, request ID, size/concurrency limits), SSE streaming. |
| `admin` | Localhost-only admin server: config management, request log, WebSocket live updates. |
| `backend` | Backend HTTP clients for OpenAI, Anthropic, Azure, Vertex, Gemini, Bedrock. |
| `config` | Env-based config, TLS client cert setup, URL validation. |
| `batch` | OpenAI-compatible Batch API surface (wraps `anyllm_batch_engine`). |
| `cache` | Response caching: in-memory (moka) plus optional Redis tier. |
| `callbacks` | Webhook callbacks for request completion. |
| `cost` | Per-request cost tracking and model pricing. |
| `env_parser` | Pure env-file parser used by startup bootstrap and the admin import endpoint. |
| `fallback` | Backend fallback chains for transparent failover. |
| `integrations` | Named integration registry (Langfuse, etc.). |
| `metrics` | Request count and success/error tracking, exposed via `GET /metrics`. |
| `ratelimit` | Distributed rate limiting (requires `redis` feature). |
| `tools` | Builtin server-side tool registry. |
| `otel` | OpenTelemetry OTLP export (requires `otel` feature). |

## Install

Published to crates.io alongside the rest of the workspace:

```bash
cargo install anyllm_proxy
```

Docker images are published as `followthewhit3rabbit/anyllm-proxy`. A Debian package can be built with `cargo deb -p anyllm_proxy`.

## Library examples

`anyllm_proxy` is primarily a binary, but several modules are usable as a library when you are building your own server, tool, or admin surface and want to reuse the proxy's well-tested pieces instead of rewriting them. If you only need format mapping or a client, prefer `anyllm_translate` or `anyllm_client` directly.

### 1. Parse a `.env` file without touching the process environment

`env_parser` is pure: it does not read files, mutate `std::env`, or call `set_var`. It accepts a string and returns parsed pairs plus warnings, which is what you want for editor integrations, config previewers, or import endpoints.

```rust
use anyllm_proxy::env_parser::{parse_env_content, escape_for_env_file};

let raw = "OPENAI_API_KEY=sk-...\nBACKEND=groq\n# comment\nGROQ_API_KEY=gsk_...";
let result = parse_env_content(raw);

if !result.hard_errors.is_empty() {
    for err in &result.hard_errors {
        eprintln!("error: {err}");
    }
    return Err("aborting: env file rejected".into());
}

for pair in &result.pairs {
    println!("{}={}", pair.key, pair.value);
}
for warn in &result.warnings {
    eprintln!("warning at line {:?}: {}", warn.line, warn.message);
}

// Round-trip safely back to env file syntax (handles quoting/escaping).
let line = format!("MY_VAR={}", escape_for_env_file("value with spaces"));
```

`hard_errors` being non-empty means the caller must not apply `pairs`. Warnings are informational.

### 2. Compute USD cost from token usage

The `cost` module embeds the LiteLLM pricing table at compile time. Use it to attach cost numbers to your own request logs without re-hosting the JSON.

```rust
use anyllm_proxy::cost::ModelPricing;

let pricing = ModelPricing::load();
if let Some((input_per_m, output_per_m)) = pricing.price_for_model("gpt-4o-mini") {
    println!("input: ${input_per_m}/M tokens, output: ${output_per_m}/M tokens");
}

let total_usd = pricing.cost_for_usage("gpt-4o-mini", 12_345, 678);
println!("call cost: ${total_usd:.6}");
```

To override at runtime (for example, custom enterprise pricing), point `MODEL_PRICING_FILE` at a JSON file with the same shape, then call `ModelPricing::load_with_optional_override(Some(path))`.

### 3. Reuse cache keys and fallback config parsing

Two small, stable pieces that are useful even outside the proxy:

```rust
use anyllm_proxy::cache::{cache_key_for_request, CacheNamespace};
use anyllm_proxy::fallback::config::parse_fallback_config;

// Deterministic cache key for an OpenAI-shaped request body.
let body = serde_json::json!({
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "hi"}],
});
let key = cache_key_for_request(&body, CacheNamespace::OpenAI);

// Parse the proxy's fallback YAML format.
let yaml = r#"
fallback_chains:
  default:
    - name: azure
      env_prefix: AZURE_FALLBACK_
    - name: openai
      env_prefix: OPENAI_FALLBACK_
"#;
let cfg = parse_fallback_config(yaml)?;
```

Larger pieces (`MemoryCache`, `RedisCache`, `SemanticCache`, `FallbackChain`, `Metrics`) are public but are designed around the proxy's own state types. If you need them in a different server, expect to pull more of the surrounding wiring from `crates/proxy/src/server/` rather than treating each as a standalone helper.

### 4. Embed the batch API surface

The proxy's `batch` module wraps `anyllm_batch_engine` with OpenAI-compatible routes. If you want the same `/v1/batches` and `/v1/files` HTTP shape inside your own server, mount the batch handlers and provide a `BatchEngine` in shared state. For a more decoupled integration, depend on `anyllm_batch_engine` directly (see that crate's README for examples).

### 5. Run as a sidecar instead of linking

If you do not need to embed proxy modules into your own binary, the simplest "library use" is to run `anyllm_proxy` as a sidecar on `localhost:3000` and have your application speak Anthropic Messages or OpenAI Chat Completions to it. This is the supported path for non-Rust clients.

## Documentation

- Full architecture: [`docs/proxy-architecture.md`](../../docs/proxy-architecture.md).
- Env vars: [`docs/ENV.md`](../../docs/ENV.md).
- Endpoint inventory: [`docs/ENDPOINTS.md`](../../docs/ENDPOINTS.md).
- Config files: [`docs/CONFIG.md`](../../docs/CONFIG.md).

## Tests

```bash
cargo test -p anyllm_proxy
```

The full workspace suite is roughly 1100 tests; ten are `#[ignore]` because they hit live APIs and need real keys.
