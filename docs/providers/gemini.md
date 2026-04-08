# Google AI Studio (Gemini)

Google AI Studio — Gemini 2.0/2.5 family via OpenAI-compatible endpoint.

**LiteLLM prefix:** `gemini/`  
**Status:** Implemented  
**Docs:** https://ai.google.dev/gemini-api/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `GEMINI_API_KEY` | Yes | API key from https://aistudio.google.com/app/apikey |
| `GEMINI_BASE_URL` | No | Override base URL (default: `https://generativelanguage.googleapis.com/v1beta/openai`) |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=gemini GEMINI_API_KEY=AIza... cargo run -p anyllm_proxy
# or with Docker:
docker run -e BACKEND=gemini -e GEMINI_API_KEY=AIza... -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: gemini-2.5-pro
    litellm_params:
      model: gemini/gemini-2.5-pro
      api_key: "env:GEMINI_API_KEY"
  - model_name: gemini-2.0-flash
    litellm_params:
      model: gemini/gemini-2.0-flash
      api_key: "env:GEMINI_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{
    "model": "gemini-2.0-flash",
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
    "model": "gemini-2.0-flash",
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
| Batch | — |

## Notable Models

| Model ID | Context | Max Output | Notes |
|---|---|---|---|
| `gemini-2.5-pro` | 1,048,576 | 65,536 | Flagship 2.5, extended thinking, vision + tools |
| `gemini-2.5-flash` | 1,048,576 | 65,536 | Efficient 2.5, extended thinking, vision + tools |
| `gemini-2.0-flash` | 1,048,576 | 8,192 | Fast multimodal, vision + tools |
| `gemini-2.0-flash-lite` | 1,048,576 | 8,192 | Lowest cost, vision, no tools |
| `gemini-1.5-pro` | 2,097,152 | 8,192 | 2M context, vision + tools |
| `gemini-1.5-flash` | 1,048,576 | 8,192 | Fast 1.5, vision + tools |
| `gemini-1.5-flash-8b` | 1,048,576 | 8,192 | Smallest 1.5, vision + tools |

## Notes

- The proxy uses the `GeminiOpenAI` protocol, which routes through Google AI Studio's OpenAI-compatible endpoint at `https://generativelanguage.googleapis.com/v1beta/openai`.
- A free tier is available with rate limits. Production use requires a billing-enabled Google Cloud project.
- Set `GEMINI_BASE_URL` to point at a Vertex AI or custom endpoint if needed; the path suffix `/openai` is appended automatically by the client.
- Batch processing is not available via this backend. For batch workloads on Gemini models, use the `vertex_ai` backend.
