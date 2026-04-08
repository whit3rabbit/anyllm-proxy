# DeepSeek

High-performance models with strong coding and reasoning capabilities. R1 supports extended chain-of-thought reasoning.

**LiteLLM prefix:** `deepseek/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://platform.deepseek.com/api-docs/

## Authentication

| Variable | Required | Description |
|---|---|---|
| `DEEPSEEK_API_KEY` | Yes | API key from platform.deepseek.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=deepseek DEEPSEEK_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=deepseek -e DEEPSEEK_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: deepseek-chat
    litellm_params:
      model: deepseek/deepseek-chat
      api_key: "env:DEEPSEEK_API_KEY"
  - model_name: deepseek-r1
    litellm_params:
      model: deepseek/deepseek-reasoner
      api_key: "env:DEEPSEEK_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "deepseek-chat", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "deepseek-chat", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `deepseek-chat` | 64k | General chat and coding, tool use supported |
| `deepseek-reasoner` | 64k | R1 reasoning model, extended thinking, no tool use |

## Notes

`deepseek-reasoner` (R1) produces a `reasoning_content` field in the response containing its chain-of-thought. The proxy maps this field bidirectionally to Anthropic thinking blocks — on the Anthropic Messages API path, the reasoning content surfaces as a `thinking` content block. Tool use is not available on `deepseek-reasoner`. The base URL used by this provider is `https://api.deepseek.com` (no `/v1` suffix in the provider definition; the client appends the path).
