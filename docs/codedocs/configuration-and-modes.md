---
title: "Configuration And Modes"
description: "Learn the config precedence rules, environment variables, and the differences between simple mode, simple YAML, LiteLLM YAML, and TOML multi-backend configs."
---

Configuration is the operator-facing abstraction in `anyllm-proxy`. It exists because the project supports a low-friction `.anyllm.env` flow for local use and a more explicit config-file flow for multi-backend, routed, and admin-managed deployments.

## What It Solves

- Local users want three or four variables and a working proxy.
- Teams need repeatable config files, secret indirection, and named backends.
- The admin UI needs runtime-mutated state without forcing a full process restart.

`crates/proxy/src/main.rs` and `crates/proxy/src/config/mod.rs` implement that precedence model.

## How It Relates To Other Concepts

- Configuration creates `ModelMapping`, `Config`, `BackendConfig`, and `MultiConfig`, which are then consumed by [Routing And Backends](/docs/routing-and-backends).
- It determines whether degradation warnings are exposed for the [Translation Pipeline](/docs/translation-pipeline).
- It also decides whether the admin server, tool engine, or batch storage have the inputs they need.

## How It Works Internally

Startup in `crates/proxy/src/main.rs` happens in a strict order:

1. resolve the data directory and locate `.anyllm.env`
2. load file-based env vars before Tokio starts
3. compute LiteLLM env aliases
4. apply env vars previously imported into SQLite through the admin UI
5. auto-detect `config.yaml` into `PROXY_CONFIG` when present
6. if the config is LiteLLM YAML, extract `general_settings.master_key` early
7. only then build the async runtime

`MultiConfig::load` in `crates/proxy/src/config/mod.rs` uses the following detection rules:

- `PROXY_CONFIG` ending in `.yaml` or `.yml` with a top-level `models:` key -> simple native YAML
- `PROXY_CONFIG` ending in `.yaml` or `.yml` with `model_list:` -> LiteLLM-compatible YAML
- any other `PROXY_CONFIG` file -> TOML multi-backend config
- no `PROXY_CONFIG` -> env-only `Config::from_env()`

## Basic Usage

Minimal `.anyllm.env`:

```bash
OPENAI_API_KEY=sk-...
BIG_MODEL=gpt-4o
SMALL_MODEL=gpt-4o-mini
PROXY_API_KEYS=proxy-user
```

This lands in the `Config::from_env` branch and produces a single backend, one `ModelMapping`, and a default `LISTEN_PORT` of `3000`.

## Advanced Usage

Simple YAML with named models and tool config:

```yaml
listen_port: 3000
log_bodies: false
routing_strategy: weighted
models:
  - name: claude-3-5-haiku-latest
    model: llama-3.1-8b-instant
    provider: groq
    api_key: env:GROQ_API_KEY
    api_base: https://api.groq.com/openai/v1
    weight: 3
  - name: claude-3-5-haiku-latest
    model: qwen2.5-coder:32b
    provider: openai
    api_key: unused
    api_base: http://localhost:11434/v1
    weight: 1
tool_execution:
  max_iterations: 6
  tool_timeout_secs: 30
  guardrails: standard
  max_write_payload_bytes: 65536
```

`parse_simple_yaml` turns that file into a `MultiConfig`, a `ModelRouter`, and a `ToolStartupConfig`. The server can then build named backends, route by virtual model name, and optionally initialize server-side tool execution from the same document.

`tool_execution.guardrails: standard` enables Forge-style advisory tool-call guardrails inside the tool loop. The proxy can nudge noisy shell commands, oversized write/edit payloads, and grep/glob symbol lookups when an LSP-style tool is available. `FORGE_TOOL_CALL_POLICY=standard` can also enable the same preset when a tool engine is already configured.

`tool_execution` and `guardrails` are only read from this simple native YAML format (the `models:` root key) or from `FORGE_TOOL_CALL_POLICY`. The LiteLLM-compatible `model_list:` format has no tool sections at all: `MultiConfig::load()` hard-codes `tool_config: None` for that branch, so a `tool_execution`/`guardrails` block written into a LiteLLM YAML file is silently ignored. This is intentional (not a bug) — see `crates/proxy/src/config/multi/loader.rs`.

<Callout type="warn">Config precedence is easy to misunderstand. Shell env vars still win over `.anyllm.env`, `.anyllm.env` wins over admin-imported env vars from SQLite, and `PROXY_CONFIG` changes the runtime mode entirely. If a global `OPENAI_API_KEY` is set in your shell, it can override the provider-specific key fallback for stub backends like Groq or OpenRouter.</Callout>

<Accordions>
<Accordion title="Simple Mode vs Config File Mode">
Simple mode is defined by the absence of `PROXY_CONFIG`. It is intentionally optimized for a single backend and a pair of model aliases, which makes local development fast and keeps the startup path obvious. Config file mode expands the feature set significantly by enabling named backends, model routers, managed callbacks, tool sections, and degradation headers by default. The trade-off is cognitive load: once `PROXY_CONFIG` is present, you are operating the proxy as infrastructure rather than as a small local helper.
</Accordion>
<Accordion title="Env Files vs Admin Imports">
The env parser in `crates/proxy/src/env_parser.rs` is pure and safe to call anywhere, which is why the admin import endpoint can validate content before writing it to SQLite. Imported values are convenient because they survive restarts and can be changed from the admin UI, but they intentionally have lower precedence than explicit env files. That makes `.anyllm.env` the source of truth when you want reproducible local behavior, while admin imports are better suited for interactive operations or bootstrap flows. In practice, keep secrets and deployment-critical values in env files or your process manager, and use admin imports for controlled overrides and migrations.
</Accordion>
</Accordions>
