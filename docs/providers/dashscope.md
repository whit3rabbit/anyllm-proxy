# Dashscope (Qwen)

Alibaba Cloud's model inference service, hosting the Qwen family of large language and vision models.

**LiteLLM prefix:** `dashscope/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://www.alibabacloud.com/help/en/model-studio/developer-reference/use-qwen-by-calling-api

## Authentication

| Variable | Required | Description |
|---|---|---|
| `DASHSCOPE_API_KEY` | Yes | API key from the Alibaba Cloud DashScope console |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=dashscope DASHSCOPE_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=dashscope -e DASHSCOPE_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: qwen-plus
    litellm_params:
      model: dashscope/qwen-plus
      api_key: "env:DASHSCOPE_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "qwen-plus", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "qwen-plus", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `qwen-turbo` | 8k | Fast, low-cost |
| `qwen-plus` | 128k | Balanced quality and speed |
| `qwen-max` | 8k | Highest quality |
| `qwen-long` | 1M | Ultra-long context |
| `qwen-vl-plus` | — | Vision-language model |

## Notes

The compatible-mode endpoint (`https://dashscope.aliyuncs.com/compatible-mode/v1`) provides OpenAI-format request/response compatibility. Qwen models support both Chinese and English. The `qwen-long` model is suited for document-length contexts up to 1M tokens.
