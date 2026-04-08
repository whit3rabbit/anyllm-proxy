# Cohere

Enterprise-focused LLMs with strong retrieval and tool use capabilities, accessed via Cohere's OpenAI-compatibility endpoint.

**LiteLLM prefix:** `cohere_chat/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.cohere.com/v2/docs/compatibility-api

## Authentication

| Variable | Required | Description |
|---|---|---|
| `COHERE_API_KEY` | Yes | API key from dashboard.cohere.com/api-keys |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=cohere_chat COHERE_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=cohere_chat -e COHERE_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: command-r-plus
    litellm_params:
      model: cohere_chat/command-r-plus-08-2024
      api_key: "env:COHERE_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "command-r-plus-08-2024", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "command-r-plus-08-2024", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | ✓ |
| Vision | — |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `command-r-plus-08-2024` | 128k | Highest quality, strong tool use and RAG |
| `command-r-08-2024` | 128k | Balanced, optimized for RAG workflows |
| `command-light` | 4k | Lightweight, lowest latency |

## Notes

This provider uses Cohere's OpenAI compatibility endpoint (`https://api.cohere.com/compatibility/v1`). The native Cohere API format (`cohere_chat` in LiteLLM) is not implemented in this proxy — all requests go through the compatibility layer. Embeddings are available but use Cohere's own model IDs (e.g., `embed-english-v3.0`); confirm model availability via the Cohere dashboard. Vision input is not supported on any current Command model.
