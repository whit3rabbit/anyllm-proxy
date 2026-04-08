# OpenAI

GPT-4o, o3, o1 and embeddings. The reference OpenAI-compatible backend.

**LiteLLM prefix:** `openai/`  
**Status:** Implemented  
**Docs:** https://platform.openai.com/docs/api-reference

## Authentication

| Variable | Required | Description |
|---|---|---|
| `OPENAI_API_KEY` | Yes | API key from https://platform.openai.com/api-keys |
| `OPENAI_ORG_ID` | No | Organization ID for org-scoped requests and billing |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=openai OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy
# or with Docker:
docker run -e BACKEND=openai -e OPENAI_API_KEY=sk-... -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: "env:OPENAI_API_KEY"
  - model_name: o3-mini
    litellm_params:
      model: openai/o3-mini
      api_key: "env:OPENAI_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{
    "model": "gpt-4o",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | ✓ |
| Vision | ✓ |
| Batch | ✓ |

## Notable Models

| Model ID | Context | Max Output | Notes |
|---|---|---|---|
| `gpt-4o` | 128k | 16,384 | Flagship multimodal model, vision + tools |
| `gpt-4o-mini` | 128k | 16,384 | Smaller, faster, cheaper 4o variant |
| `gpt-4-turbo` | 128k | 4,096 | Previous-generation turbo, vision + tools |
| `gpt-4` | 8k | 8,192 | Original GPT-4, no vision |
| `gpt-3.5-turbo` | 16k | 4,096 | Fast and cheap, no vision |
| `o1` | 200k | 100,000 | Extended thinking, vision + tools |
| `o1-mini` | 128k | 65,536 | Reasoning-focused, no tools/vision |
| `o3` | 200k | 100,000 | Latest reasoning model, vision + tools |
| `o3-mini` | 200k | 100,000 | Efficient reasoning, tools, no vision |
| `o4-mini` | 200k | 100,000 | Reasoning with vision + tools |
| `text-embedding-3-large` | 8,191 | — | High-quality embeddings |
| `text-embedding-3-small` | 8,191 | — | Efficient embeddings |

## Notes

- Set `OPENAI_ORG_ID` to scope API usage and billing to a specific organization.
- The `o1`, `o3`, `o4-mini` series are reasoning models. They use internal chain-of-thought tokens that count toward billing but are not returned in the response.
- Batch API (`/v1/batches`) is supported for async bulk processing at 50% cost. Use `/v1/messages/batches` (Anthropic format) or pass through directly.
- Embeddings requests route to `/v1/embeddings` and pass through without translation.
