# Volcano Engine

ByteDance's Volcano Engine Ark platform, hosting Doubao and other models via an OpenAI-compatible API.

**LiteLLM prefix:** `volcengine/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://www.volcengine.com/docs/82379

## Authentication

| Variable | Required | Description |
|---|---|---|
| `VOLCENGINE_API_KEY` | Yes | API key obtained from console.volcengine.com/ark |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=volcengine VOLCENGINE_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=volcengine -e VOLCENGINE_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: doubao-pro-32k
    litellm_params:
      model: volcengine/ep-xxxxxxxxxx
      api_key: "env:VOLCENGINE_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "ep-xxxxxxxxxx", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "ep-xxxxxxxxxx", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | — |
| Vision | ✓ |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| Doubao-pro-32k | 32k | High-capability Doubao model; accessed via endpoint ID |
| Doubao-lite-32k | 32k | Lower cost Doubao variant; accessed via endpoint ID |

## Notes

- Volcano Engine Ark uses **endpoint IDs** (format: `ep-xxxxxxxxxx`) rather than model name strings. You create an endpoint in the Ark console, selecting the underlying model, and then use that endpoint ID as the model parameter.
- Base URL is `https://ark.cn-beijing.volces.com/api/v3`.
- The platform console is in Chinese; account registration may require a Chinese phone number or business verification.
