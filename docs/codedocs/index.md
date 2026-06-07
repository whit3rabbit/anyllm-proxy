---
title: "Getting Started"
description: "Start using anyllm-proxy as a translation proxy, Rust workspace, and local-first bridge for Anthropic-compatible tools."
---

`/whit3rabbit/anyllm-proxy` is a Rust workspace that lets Anthropic-compatible clients talk to OpenAI-compatible, Gemini, Bedrock, Anthropic, Azure, Vertex, and local LLM backends through one proxy and a reusable set of crates.

## The Problem

- Anthropic-native tools like Claude Code, Cursor, Windsurf, and Cline often assume Anthropic's Messages API even when your actual backend is OpenAI-compatible, local, or self-hosted.
- Local LLM servers such as Ollama and LM Studio usually expose OpenAI-style endpoints, which means you otherwise need custom adapters, prompt-format glue, and streaming conversion.
- Production deployments need more than translation: key management, per-backend routing, request logs, spend tracking, batch jobs, and admin controls.
- Embedding the same behavior inside a Rust application is difficult if transport, translation, routing, and config parsing are all coupled together.

## The Solution

`anyllm-proxy` splits the problem into five crates: `anyllm_translate` for pure data conversion, `anyllm_client` for Anthropic-shaped HTTP calls, `anyllm_providers` for provider metadata, `anyllm_batch_engine` for durable batch execution, and `anyllm_proxy` for the server, admin UI, and runtime wiring. The binary can run as a sidecar, while the crates can be embedded directly in Rust applications.

```bash
OPENAI_BASE_URL=http://localhost:11434/v1 \
OPENAI_API_KEY=unused \
BIG_MODEL=qwen2.5-coder:32b \
SMALL_MODEL=qwen2.5-coder:32b \
PROXY_OPEN_RELAY=true \
anyllm_proxy
```

```bash
curl http://localhost:3000/v1/messages \
  -H 'x-api-key: local-dev' \
  -H 'content-type: application/json' \
  -d '{
    "model": "claude-3-5-sonnet-latest",
    "max_tokens": 128,
    "messages": [{"role": "user", "content": "Say hello from the proxy"}]
  }'
```

## Installation

" "bun"]}>
<Tab value="npm">

```bash
# anyllm-proxy is not published as a JavaScript package
brew install whit3rabbit/tap/anyllm-proxy
# or
cargo install anyllm_proxy
```

</Tab>
<Tab value="pnpm">

```bash
# anyllm-proxy is a Rust binary, not a pnpm dependency
brew install whit3rabbit/tap/anyllm-proxy
# or
cargo install anyllm_proxy
```

</Tab>
<Tab value="yarn">

```bash
# anyllm-proxy is not shipped through Yarn registries
brew install whit3rabbit/tap/anyllm-proxy
# or
cargo install anyllm_proxy
```

</Tab>
<Tab value="bun">

```bash
# anyllm-proxy is not a Bun package
brew install whit3rabbit/tap/anyllm-proxy
# or
cargo install anyllm_proxy
```

</Tab>
</Tabs>

Supported environments include macOS via Homebrew, Linux via Debian packages or Cargo, Docker images, and direct Rust crate usage inside a Cargo workspace.

## Quick Start

Create `~/.anyllm/.anyllm.env`:

```bash
OPENAI_API_KEY=unused
OPENAI_BASE_URL=http://localhost:11434/v1
BIG_MODEL=qwen2.5-coder:32b
SMALL_MODEL=qwen2.5-coder:32b
PROXY_API_KEYS=proxy-user
```

Start the proxy:

```bash
anyllm_proxy
```

Send one Anthropic-shaped request:

```bash
curl http://localhost:3000/v1/messages \
  -H 'x-api-key: proxy-user' \
  -H 'content-type: application/json' \
  -d '{
    "model": "claude-3-5-sonnet-latest",
    "max_tokens": 64,
    "messages": [{"role": "user", "content": "Reply with the word ready"}]
  }'
```

Expected output shape:

```json
{
  "id": "msg_...",
  "type": "message",
  "role": "assistant",
  "model": "claude-3-5-sonnet-latest",
  "content": [
    {
      "type": "text",
      "text": "ready"
    }
  ],
  "usage": {
    "input_tokens": 0,
    "output_tokens": 0
  }
}
```

## Key Features

- Anthropic Messages to OpenAI Chat Completions, OpenAI Responses, and Gemini native translation.
- Runtime backend switching through env vars, simple YAML, LiteLLM-style YAML, or TOML multi-backend config.
- Optional admin server with request logs, access control, model management, spend tracking, and env import/export.
- Durable batch execution with JSONL validation, SQLite-backed state, and signed webhook delivery.
- Reusable Rust crates for translation, client transport, provider discovery, batch orchestration, and in-process runtime embedding.

<Cards>
  <Card title="Architecture" href="/docs/architecture">See how the five crates and the proxy server fit together.</Card>
  <Card title="Core Concepts" href="/docs/translation-pipeline">Understand translation, routing, config modes, and batch execution.</Card>
  <Card title="API Reference" href="/docs/api-reference/anyllm-translate">Jump to the public Rust API surface and import paths.</Card>
</Cards>
