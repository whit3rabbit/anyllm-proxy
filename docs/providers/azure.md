# Azure OpenAI

Azure OpenAI — OpenAI models deployed in your Azure subscription.

**LiteLLM prefix:** `azure/`  
**Status:** Wired — not live-tested  
**Docs:** https://learn.microsoft.com/en-us/azure/ai-services/openai/reference

## Authentication

| Variable | Required | Description |
|---|---|---|
| `AZURE_OPENAI_API_KEY` | Yes | API key from your Azure OpenAI resource |
| `AZURE_OPENAI_ENDPOINT` | Yes | Resource endpoint, e.g. `https://my-resource.openai.azure.com` |
| `AZURE_OPENAI_DEPLOYMENT` | Yes | Deployment name you created in Azure AI Studio |
| `AZURE_OPENAI_API_VERSION` | No | API version, e.g. `2024-10-21` (default used if unset) |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=azure \
  AZURE_OPENAI_API_KEY=... \
  AZURE_OPENAI_ENDPOINT=https://my-resource.openai.azure.com \
  AZURE_OPENAI_DEPLOYMENT=my-gpt4o-deployment \
  cargo run -p anyllm_proxy
# or with Docker:
docker run \
  -e BACKEND=azure \
  -e AZURE_OPENAI_API_KEY=... \
  -e AZURE_OPENAI_ENDPOINT=https://my-resource.openai.azure.com \
  -e AZURE_OPENAI_DEPLOYMENT=my-gpt4o-deployment \
  -e PROXY_OPEN_RELAY=true \
  -p 3000:3000 \
  followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: azure/my-gpt4o-deployment
      api_base: "https://my-resource.openai.azure.com"
      api_key: "env:AZURE_OPENAI_API_KEY"
      api_version: "2024-10-21"
  - model_name: gpt-4o-mini
    litellm_params:
      model: azure/my-gpt4o-mini-deployment
      api_base: "https://my-resource.openai.azure.com"
      api_key: "env:AZURE_OPENAI_API_KEY"
      api_version: "2024-10-21"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{
    "model": "my-gpt4o-deployment",
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
    "model": "my-gpt4o-deployment",
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

Azure does not have a fixed model list. Available models depend on which base models you have deployed in your Azure AI Studio resource. Common deployments:

| Base Model | Typical Deployment Name | Notes |
|---|---|---|
| GPT-4o | `gpt-4o` or custom | Latest multimodal flagship |
| GPT-4o-mini | `gpt-4o-mini` or custom | Smaller, cheaper option |
| GPT-4 Turbo | `gpt-4-turbo` or custom | Previous-gen flagship |
| text-embedding-3-large | `text-embedding-3-large` | Embeddings |

## Notes

- Azure does not use a fixed base URL. Each Azure OpenAI resource has its own endpoint (`https://<resource-name>.openai.azure.com`). The `api_base` field must be set per model in LiteLLM YAML config.
- When using single-backend mode, `AZURE_OPENAI_ENDPOINT` sets the resource URL and `AZURE_OPENAI_DEPLOYMENT` is used as the deployment/model name for all requests.
- The API version controls which Azure OpenAI REST API version is used. Check the Azure docs for the latest stable version.
- This backend is wired and tested for structure but has not been validated against a live Azure endpoint. Report issues if you encounter problems.
