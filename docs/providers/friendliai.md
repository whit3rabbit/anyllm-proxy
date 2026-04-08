# FriendliAI

Serverless LLM inference platform with support for popular open-source models.

**LiteLLM prefix:** `friendliai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://friendli.ai/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `FRIENDLIAI_TOKEN` | Yes | API token from suite.friendli.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=friendliai FRIENDLIAI_TOKEN=your-token cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=friendliai -e FRIENDLIAI_TOKEN=your-token -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.1-70b
    litellm_params:
      model: friendliai/meta-llama-3.1-70b-instruct
      api_key: "env:FRIENDLIAI_TOKEN"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "meta-llama-3.1-70b-instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "meta-llama-3.1-70b-instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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

| Model ID | Context | Notes |
|---|---|---|
| `meta-llama-3.1-405b-instruct` | 128k | Largest Llama 3.1, highest quality |
| `meta-llama-3.1-70b-instruct` | 128k | Balanced quality and speed |
| `meta-llama-3.1-8b-instruct` | 128k | Fast, low-cost |
| `mixtral-8x7b-instruct-v0-1` | 32k | Mixtral MoE |

## Notes

FriendliAI offers both serverless endpoints (default base URL) and dedicated endpoints for reserved capacity. For dedicated endpoints, override `api_base` in your LiteLLM config with your assigned endpoint URL. The `FRIENDLIAI_TOKEN` is used as a Bearer token.
