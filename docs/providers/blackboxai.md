# Blackbox AI

LLM chat service at blackbox.ai with an OpenAI-compatible endpoint.

**LiteLLM prefix:** *(not in LiteLLM)*
**Status:** Stub — routes through OpenAI-compatible client
**API base:** `https://api.blackbox.ai/api`
**Docs:** https://docs.blackbox.ai

## Authentication

| Variable | Required | Description |
|---|---|---|
| `BLACKBOXAI_API_KEY` | Yes | API key from the Blackbox AI console |

## Quick Start

```bash
BACKEND=blackboxai BLACKBOXAI_API_KEY=your-key cargo run -p anyllm_proxy
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | — |
| Embeddings | — |
| Vision | ✓ |
| Batch | — |
