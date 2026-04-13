# iFlytek Spark

Chinese LLM from iFlytek with an OpenAI-compatible endpoint (Spark Open API).

**LiteLLM prefix:** `spark/`
**Status:** Stub — routes through OpenAI-compatible client
**API base:** `https://spark-api-open.xf-yun.com/v1`
**Docs:** https://xinghuo.xfyun.cn/sparkapi

## Authentication

| Variable | Required | Description |
|---|---|---|
| `SPARK_API_KEY` | Yes (either) | Spark API key |
| `IFLYTEK_API_KEY` | Yes (either) | Alias accepted by the proxy |

## Quick Start

```bash
BACKEND=iflytek SPARK_API_KEY=your-key cargo run -p anyllm_proxy
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | — |
| Vision | ✓ |
| Batch | — |
