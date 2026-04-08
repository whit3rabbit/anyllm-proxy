# Cloudflare Workers AI

Cloudflare Workers AI — serverless inference on Cloudflare's global network, running open models at the edge.

**LiteLLM prefix:** `cloudflare/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://developers.cloudflare.com/workers-ai/

## Authentication

| Variable | Required | Description |
|---|---|---|
| `CLOUDFLARE_API_KEY` | Yes | Cloudflare API token with Workers AI permissions |
| `CLOUDFLARE_ACCOUNT_ID` | Yes | Your Cloudflare account ID |

Obtain an API token at https://dash.cloudflare.com/profile/api-tokens. Grant the token `Workers AI:Read` and `Workers AI:Edit` permissions. Your account ID is visible in the Cloudflare dashboard URL and on the account home page.

## Quick Start

### Single-Backend (env vars)

The base URL embeds your account ID. Substitute it before running:

```bash
BACKEND=cloudflare \
  CLOUDFLARE_API_KEY=your-token \
  OPENAI_BASE_URL=https://api.cloudflare.com/client/v4/accounts/<your-account-id>/ai/v1 \
  PROXY_OPEN_RELAY=true \
  cargo run -p anyllm_proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama3-70b-fast
    litellm_params:
      model: cloudflare/@cf/meta/llama-3.3-70b-instruct-fp8-fast
      api_key: "env:CLOUDFLARE_API_KEY"
      api_base: "https://api.cloudflare.com/client/v4/accounts/<your-account-id>/ai/v1"
  - model_name: mistral-7b
    litellm_params:
      model: cloudflare/@cf/mistral/mistral-7b-instruct-v0.2
      api_key: "env:CLOUDFLARE_API_KEY"
      api_base: "https://api.cloudflare.com/client/v4/accounts/<your-account-id>/ai/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
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

| Model ID | Notes |
|---|---|
| `@cf/meta/llama-3.3-70b-instruct-fp8-fast` | Llama 3.3 70B, FP8 quantized, optimized for speed |
| `@cf/mistral/mistral-7b-instruct-v0.2` | Mistral 7B v0.2 |
| `@cf/qwen/qwen1.5-14b-chat-awq` | Qwen 1.5 14B, AWQ quantized |

## Notes

- The API base URL is account-specific. There is no shared base URL. You must substitute your account ID into `https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1`.
- Model IDs use Cloudflare's `@cf/` namespace format (e.g., `@cf/meta/llama-3.3-70b-instruct-fp8-fast`). Pass the full ID including the `@cf/` prefix.
- Tool use is not supported on this backend — Cloudflare Workers AI does not expose a function calling interface through the OpenAI-compatible endpoint.
- Model availability depends on your Cloudflare plan. Some models require a paid Workers AI subscription.
- Full model catalog: https://developers.cloudflare.com/workers-ai/models/
