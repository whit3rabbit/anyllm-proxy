# Perplexity AI

Web-search augmented language models with real-time internet access.

**LiteLLM prefix:** `perplexity/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.perplexity.ai/reference/post_chat_completions

## Authentication

| Variable | Required | Description |
|---|---|---|
| `PERPLEXITYAI_API_KEY` | Yes | API key from perplexity.ai/settings/api (also accepted as `PERPLEXITY_API_KEY`) |
| `PERPLEXITY_API_KEY` | Yes | Alias for `PERPLEXITYAI_API_KEY` |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=perplexity PERPLEXITYAI_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=perplexity -e PERPLEXITYAI_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: sonar-pro
    litellm_params:
      model: perplexity/sonar-pro
      api_key: "env:PERPLEXITYAI_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "sonar-pro", "max_tokens": 1024, "messages": [{"role": "user", "content": "What happened in the news today?"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "sonar-pro", "messages": [{"role": "user", "content": "What happened in the news today?"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | — |
| Embeddings | — |
| Vision | — |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `sonar-pro` | 200k | Web search, higher quality |
| `sonar` | 128k | Web search, faster and cheaper |
| `llama-3.1-sonar-large-128k-online` | 128k | Sonar large with web access |
| `llama-3.1-sonar-small-128k-online` | 128k | Sonar small with web access |

## Notes

All Perplexity "online" and "sonar" models include real-time web search. Responses include citations in a `citations` field on the response object — these are passed through as-is from the upstream API. Tool use and function calling are not supported. Perplexity does not offer an embeddings endpoint. System prompts are supported but Perplexity recommends keeping them concise; the models are optimized for user-facing queries, not agent workflows.
