# Morph

Code-focused LLM optimized for software development tasks.

**LiteLLM prefix:** `morph/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://morphllm.com/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `MORPH_API_KEY` | Yes | API key from morphllm.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=morph MORPH_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=morph -e MORPH_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: morph-v2
    litellm_params:
      model: morph/morph-v2
      api_key: "env:MORPH_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "morph-v2", "max_tokens": 1024, "messages": [{"role": "user", "content": "Write a function to parse JSON"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "morph-v2", "messages": [{"role": "user", "content": "Write a function to parse JSON"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | — |
| Vision | — |
| Batch | — |

## Notable Models

| Model ID | Notes |
|---|---|
| `morph-v2` | Code-specialized model |

## Notes

Morph is designed for code generation and software development workflows. Tool use is supported, making it suitable for agentic coding applications.
