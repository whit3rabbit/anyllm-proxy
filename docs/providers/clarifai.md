# Clarifai

Multimodal AI platform hosting models from multiple providers including OpenAI, Anthropic, and Meta under a unified API.

**LiteLLM prefix:** `clarifai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.clarifai.com/api-guide/api-overview

## Authentication

| Variable | Required | Description |
|---|---|---|
| `CLARIFAI_API_KEY` | Yes | Personal access token from clarifai.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=clarifai CLARIFAI_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=clarifai -e CLARIFAI_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: gpt-4o-via-clarifai
    litellm_params:
      model: clarifai/openai/gpt-4o
      api_key: "env:CLARIFAI_API_KEY"
  - model_name: claude-3-5-sonnet-via-clarifai
    litellm_params:
      model: clarifai/anthropic/claude-3-5-sonnet
      api_key: "env:CLARIFAI_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "openai/gpt-4o", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "openai/gpt-4o", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | — |
| Embeddings | ✓ |
| Vision | ✓ |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `openai/gpt-4o` | 128k | GPT-4o hosted via Clarifai |
| `anthropic/claude-3-5-sonnet` | 200k | Claude 3.5 Sonnet via Clarifai |
| `meta/llama-3_1-70b-instruct` | 128k | Llama 3.1 70B |

## Notes

Model IDs on Clarifai follow a `provider/model-name` convention that differs from most other platforms. Tool use is not supported through the OpenAI-compatible interface even for models that natively support it. Vision support is model-dependent. Clarifai also provides computer vision, embeddings, and workflow tooling beyond LLM inference.
