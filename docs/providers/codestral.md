# Codestral

Mistral's code-focused model endpoint with a dedicated API key, separate from the main Mistral API.

**LiteLLM prefix:** `codestral/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.mistral.ai/capabilities/code_generation/

## Authentication

| Variable | Required | Description |
|---|---|---|
| `CODESTRAL_API_KEY` | Yes | API key obtained from console.mistral.ai (Codestral plan, separate from Mistral API keys) |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=codestral CODESTRAL_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=codestral -e CODESTRAL_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: codestral
    litellm_params:
      model: codestral/codestral-latest
      api_key: "env:CODESTRAL_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "codestral-latest", "max_tokens": 1024, "messages": [{"role": "user", "content": "Write a Python function to reverse a string"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "codestral-latest", "messages": [{"role": "user", "content": "Write a Python function to reverse a string"}]}'
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
| `codestral-latest` | 256k | Always points to the current production Codestral model |
| `codestral-2501` | 256k | January 2025 snapshot |

## Notes

- Codestral API keys are separate from standard Mistral API keys. Both are issued at console.mistral.ai but under different plans.
- Base URL is `https://codestral.mistral.ai/v1`, distinct from `https://api.mistral.ai/v1`.
- Fill-in-the-middle (FIM) completions use the `/fim/completions` endpoint on `codestral.mistral.ai`. This is a non-standard endpoint not routed through the proxy; call it directly if needed.
