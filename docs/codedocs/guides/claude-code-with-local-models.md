---
title: "Claude Code With Local Models"
description: "Run Claude Code or another Anthropic-native tool against Ollama or LM Studio through anyllm-proxy."
---

This guide solves the most common local setup: an Anthropic-native tool on one side and an OpenAI-compatible local model server on the other.

<Steps>
<Step>
### Install and start your local backend

Pick one backend and make sure its OpenAI-compatible endpoint is already running.

" "LM Studio"]}>
<Tab value="Ollama">

```bash
ollama serve
ollama pull qwen2.5-coder:32b
```

</Tab>
<Tab value="LM Studio">

```bash
# Start the local server from the LM Studio app
# Default OpenAI-compatible endpoint:
# http://localhost:1234/v1
```

</Tab>
</Tabs>
</Step>
<Step>
### Create the proxy env file

```bash
OPENAI_API_KEY=unused
OPENAI_BASE_URL=http://localhost:11434/v1
BIG_MODEL=qwen2.5-coder:32b
SMALL_MODEL=qwen2.5-coder:32b
PROXY_API_KEYS=proxy-user
```

If you are using LM Studio, change `OPENAI_BASE_URL` to `http://localhost:1234/v1`.
</Step>
<Step>
### Start the proxy

```bash
anyllm_proxy
```

Expected startup behavior:

```text
anyllm_proxy: data directory: /home/you/.anyllm
anyllm_proxy: loaded 5 variable(s) from env file
```
</Step>
<Step>
### Launch Claude Code through the proxy

```bash
ANTHROPIC_BASE_URL=http://localhost:3000 \
ANTHROPIC_AUTH_TOKEN=proxy-user \
ANTHROPIC_API_KEY="" \
CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
claude
```

You can also let the proxy inject those variables for you:

```bash
anyllm_proxy run claude
```
</Step>
</Steps>

## Complete Example

Test the pipeline before opening Claude Code:

```bash
curl http://localhost:3000/v1/messages \
  -H 'x-api-key: proxy-user' \
  -H 'content-type: application/json' \
  -d '{
    "model": "claude-3-5-sonnet-latest",
    "max_tokens": 64,
    "messages": [{"role": "user", "content": "Reply with local proxy ready"}]
  }'
```

If the request succeeds, Claude Code will use the same path. Internally the proxy accepts the Anthropic `MessageCreateRequest`, maps the model through `ModelMapping` in `crates/proxy/src/config/mod.rs`, translates the payload with `anyllm_translate`, and forwards it through the OpenAI-compatible backend client.

## Why This Works

The local backend never needs to understand Anthropic's schema. `anyllm_proxy` handles the translation and keeps the Anthropic response shape on the outside. That is why tools written specifically for Anthropic's API can still use Ollama or LM Studio as long as the local server already exposes an OpenAI-compatible HTTP surface.
