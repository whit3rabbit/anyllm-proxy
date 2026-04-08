# Azure AI Foundry

Azure AI Foundry (Serverless API / Models-as-a-Service) — Llama, Mistral, Phi, Cohere, and other third-party models via Azure's pay-as-you-go hosted endpoints.

**LiteLLM prefix:** `azure_ai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://learn.microsoft.com/en-us/azure/ai-foundry/

## Authentication

| Variable | Required | Description |
|---|---|---|
| `AZURE_AI_API_KEY` | Yes | API key for the model deployment |
| `AZURE_AI_API_BASE` | Yes | Deployment endpoint URL, e.g. `https://<resource>.services.ai.azure.com/models` |

Each model deployment in Azure AI Foundry gets its own endpoint URL. Set `AZURE_AI_API_BASE` (or `OPENAI_BASE_URL`) to that URL — there is no global default.

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=azure_ai \
  AZURE_AI_API_KEY=your-key \
  OPENAI_BASE_URL=https://<resource>.services.ai.azure.com/models \
  PROXY_OPEN_RELAY=true \
  cargo run -p anyllm_proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama3-70b
    litellm_params:
      model: azure_ai/Meta-Llama-3-70B-Instruct
      api_key: "env:AZURE_AI_API_KEY"
      api_base: "https://<resource>.services.ai.azure.com/models"
  - model_name: mistral-large
    litellm_params:
      model: azure_ai/Mistral-Large
      api_key: "env:AZURE_AI_API_KEY"
      api_base: "https://<resource>.services.ai.azure.com/models"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "Meta-Llama-3-70B-Instruct",
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
    "model": "Meta-Llama-3-70B-Instruct",
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

## Notes

- Azure AI Foundry Serverless API is distinct from Azure OpenAI Service (`azure` backend). Use the `azure` backend for GPT-4o and other OpenAI models; use `azure_ai` for third-party models (Llama, Mistral, Phi, Cohere, etc.).
- Each deployment has a unique endpoint URL. There is no single base URL shared across all models. Retrieve the endpoint from the Azure AI Foundry portal under the deployment details.
- Model IDs in requests must match the deployment name exactly as it appears in the portal (e.g., `Meta-Llama-3-70B-Instruct`, not `llama-3-70b`).
- This provider uses Bearer token auth. The key is the deployment-specific API key, not an Azure subscription key.
- No models are enumerated in the provider catalog — use the exact deployment name from your Azure portal.
