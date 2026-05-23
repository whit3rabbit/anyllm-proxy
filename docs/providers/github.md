# GitHub Models

Azure-hosted OpenAI-compatible inference endpoint accessible with a GitHub personal access token, free tier included.

**LiteLLM prefix:** `github_copilot/`
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.github.com/en/github-models

## Authentication

| Variable | Required | Description |
|---|---|---|
| `GITHUB_TOKEN` | Yes | GitHub personal access token (PAT) with no special scopes required |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=github_copilot GITHUB_TOKEN=your-pat cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=github_copilot -e GITHUB_TOKEN=your-pat -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: github_copilot/gpt-4o
      api_key: "env:GITHUB_TOKEN"
  - model_name: llama-3.3-70b
    litellm_params:
      model: github_copilot/Llama-3.3-70B-Instruct
      api_key: "env:GITHUB_TOKEN"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4o", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4o", "messages": [{"role": "user", "content": "Hello"}]}'
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

| Model ID | Context | Notes |
|---|---|---|
| `gpt-4o` | 128k | OpenAI GPT-4o |
| `gpt-4o-mini` | 128k | OpenAI GPT-4o Mini, low-cost |
| `Llama-3.3-70B-Instruct` | 128k | Meta Llama 3.3 70B |
| `Phi-4` | 16k | Microsoft Phi-4 |
| `Mistral-Nemo` | 128k | Mistral Nemo |
| `text-embedding-3-small` | — | OpenAI embedding model |

## Notes

GitHub Models is backed by Azure AI and uses the endpoint `https://models.inference.ai.azure.com`. A standard GitHub PAT (classic or fine-grained) with no additional scopes is sufficient. The free tier has strict rate limits: check https://docs.github.com/en/github-models/prototyping-with-ai-models#rate-limits for current limits. Not intended for production traffic.
