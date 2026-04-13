# Baidu ERNIE (Qianfan)

Baidu's ERNIE LLM platform. Authentication uses AK/SK credentials (not a simple Bearer token); set both `QIANFAN_AK` and `QIANFAN_SK`.

**LiteLLM prefix:** `qianfan/`
**Status:** Stub — routes through OpenAI-compatible client
**API base:** `https://aip.baidubce.com/rpc/2.0/ai_custom/v1/wenxinworkshop`
**Docs:** https://cloud.baidu.com/doc/WENXINWORKSHOP

## Authentication

| Variable | Required | Description |
|---|---|---|
| `QIANFAN_AK` | Yes | Qianfan Access Key |
| `QIANFAN_SK` | Yes | Qianfan Secret Key |

## Quick Start

```bash
BACKEND=baidu QIANFAN_AK=your-ak QIANFAN_SK=your-sk cargo run -p anyllm_proxy
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

Baidu uses AK/SK OAuth token exchange, not a plain Bearer token. The stub registers the env vars but the OAuth exchange is not implemented — routing to this backend will fail until the auth layer is wired.
