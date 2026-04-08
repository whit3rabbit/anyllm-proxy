# Fireworks AI

Fast inference platform for open-source models, compound AI systems, and fine-tuned model hosting.

**LiteLLM prefix:** `fireworks_ai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.fireworks.ai/api-reference/introduction

## Authentication

| Variable | Required | Description |
|---|---|---|
| `FIREWORKS_API_KEY` | Yes | API key from fireworks.ai/account/api-keys |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=fireworks_ai FIREWORKS_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=fireworks_ai -e FIREWORKS_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-v3p3-70b
    litellm_params:
      model: fireworks_ai/accounts/fireworks/models/llama-v3p3-70b-instruct
      api_key: "env:FIREWORKS_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "accounts/fireworks/models/llama-v3p3-70b-instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "accounts/fireworks/models/llama-v3p3-70b-instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `accounts/fireworks/models/llama-v3p3-70b-instruct` | 131k | Llama 3.3 70B |
| `accounts/fireworks/models/mixtral-8x7b-instruct` | 32k | Mixtral 8x7B MoE |

## Notes

Fireworks AI model IDs use the `accounts/<account>/models/<model-name>` path format. Serverless models are under `accounts/fireworks/models/`. Fine-tuned or privately deployed models use your own account path. The platform also supports compound AI (multi-model pipelines) and function-calling workflows. See https://fireworks.ai/models for the full model catalog.
