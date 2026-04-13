# SiliconFlow

Popular Chinese inference platform serving open-source and proprietary models via an OpenAI-compatible API.

**LiteLLM prefix:** `siliconflow/`
**Status:** Stub — routes through OpenAI-compatible client
**API base:** `https://api.siliconflow.cn/v1`
**Docs:** https://docs.siliconflow.cn

## Authentication

| Variable | Required | Description |
|---|---|---|
| `SILICONFLOW_API_KEY` | Yes | API key from the SiliconFlow console |

## Quick Start

```bash
BACKEND=siliconflow SILICONFLOW_API_KEY=your-key cargo run -p anyllm_proxy
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
