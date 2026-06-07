---
title: "Multi-Backend Routing"
description: "Configure multiple upstream providers, virtual model names, and deployment-level routing with simple YAML."
---

This guide shows how to expose one stable model name while routing across more than one upstream provider.

<Steps>
<Step>
### Create a simple YAML config

Save this as `~/.anyllm/config.yaml`:

```yaml
listen_port: 3000
routing_strategy: weighted
models:
  - name: claude-3-5-sonnet-latest
    model: gpt-4o
    provider: openai
    api_key: env:OPENAI_API_KEY
    api_base: https://api.openai.com
    weight: 2
    rpm: 120
  - name: claude-3-5-sonnet-latest
    model: llama-3.3-70b-versatile
    provider: groq
    api_key: env:GROQ_API_KEY
    api_base: https://api.groq.com/openai/v1
    weight: 1
    rpm: 300
```
</Step>
<Step>
### Export the secrets and start the proxy

```bash
export OPENAI_API_KEY=sk-...
export GROQ_API_KEY=gsk_...
export PROXY_API_KEYS=proxy-user
export PROXY_CONFIG=$HOME/.anyllm/config.yaml

anyllm_proxy
```

This goes through `MultiConfig::load`, which detects the `models:` root key and calls `parse_simple_yaml`.
</Step>
<Step>
### Send requests against the virtual model

```bash
curl http://localhost:3000/v1/messages \
  -H 'x-api-key: proxy-user' \
  -H 'content-type: application/json' \
  -d '{
    "model": "claude-3-5-sonnet-latest",
    "max_tokens": 128,
    "messages": [{"role": "user", "content": "Which backend handled this?"}]
  }'
```

The client still sees the virtual model name, while the router chooses the actual backend model.
</Step>
</Steps>

## Changing Strategies

The same config can switch strategies by changing `routing_strategy`:

```yaml
routing_strategy: latency-based
```

Available values are parsed in `crates/proxy/src/config/simple.rs` and executed by `ModelRouter` in `crates/proxy/src/config/model_router.rs`. Use `round-robin` when you want stable distribution, `latency-based` when tail latency matters more than strict fairness, and `weighted` when you want one backend to absorb a larger percentage of traffic.

## Operational Notes

- RPM and TPM limits are tracked per deployment in memory, not through upstream health checks.
- The router only applies when a config file creates a model list; env-only mode uses `BIG_MODEL` and `SMALL_MODEL` instead.
- If every deployment for a model is at its RPM limit, the proxy returns a 429 with an Anthropic-shaped rate-limit error.

This makes the config a good fit for staged migrations. You can introduce a new provider behind an existing virtual model name, give it a small weight, observe latency and error behavior through the admin UI, and then increase its share without asking clients to change their requested model string. That is exactly the use case the `Deployment` and `RoutingStrategy` types were built for in `crates/proxy/src/config/model_router.rs`.
