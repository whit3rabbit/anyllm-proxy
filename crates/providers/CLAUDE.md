# anyllm_providers

Static provider + model metadata catalog (ids, protocols, endpoints, capabilities). No HTTP logic for OpenAI-compatible providers.

See root `../../CLAUDE.md` for workspace-wide commands and conventions.

## Test

```bash
cargo test -p anyllm_providers
```

## Features

- `default = []` — pure const catalog, no deps.
- `runtime-catalog` — pulls in `serde_json` for runtime lookup.
- `remote-catalog` — `runtime-catalog` + `reqwest` for fetching a remote catalog.

## Layout

- `providers/<name>.rs` — one file per provider. ~100+ providers.
- `provider.rs`, `model.rs` — core types.
- `catalog.rs` (+ `catalog/helpers.rs`, `catalog/tests.rs`) — registry assembly; high-churn, change carefully.
- `registry.rs` — wires every provider into the catalog.

## Adding a provider

1. Copy any stub into `providers/<name>.rs`.
2. Register it in `providers/mod.rs` AND `registry.rs`.
3. Done — OpenAI-compatible providers need no HTTP code (the proxy talks to them via the stub OpenAI client).

## Gotchas

- **`ProviderProtocol::Custom` is not a runnable backend.** A provider with `Custom` protocol makes the proxy's `resolve_backend()` return `None` and panic at startup (e.g. `sagemaker`). Use an existing protocol unless you also add backend wiring in the proxy.
- Model pricing is NOT here — it lives in `proxy/assets/model_pricing.json` (auto-updated by `scripts/update_pricing.py`).
- **Registry lookup maps (`PROVIDERS_BY_ID`, `MODELS_BY_PROVIDER`, etc. in `registry.rs`) chain `ALL_*` then `LEGACY_ONLY_*` with `HashMap::insert` (last-writer-wins).** Safe ONLY because legacy ids/prefixes are disjoint from the LiteLLM snapshot. A colliding id or `litellm_prefix` makes the LEGACY entry silently win (the old `ALL.or_else(LEGACY)` made ALL win).
- **`find_by_litellm_prefix`: a bare id (prefix minus `/`) resolves ONLY when it is an alias (`canonical_provider_id(id) != id`).** A non-alias bare id is not a routing prefix and returns None (e.g. `baidu/` != baidu's real prefix `qianfan/`).
- **`litellm_snapshot.rs`'s header says "generated, don't hand-edit" but the tail of the file (after the `ModelDef` array's closing `];`) is hand-maintained.** Anthropic thinking-capability lists (`ANTHROPIC_ADAPTIVE_THINKING_MODELS`, `ANTHROPIC_ADAPTIVE_ONLY_THINKING_MODELS`, `ANTHROPIC_MAX_REASONING_EFFORT_MODELS`, `ANTHROPIC_XHIGH_REASONING_EFFORT_MODELS`) live there and are edited directly, not by the script.
- **"Supports adaptive thinking" ≠ "requires adaptive thinking."** Opus 4.6/Sonnet 4.6 support adaptive but still accept legacy `budget_tokens` (deprecated, not rejected); Fable 5/Opus 4.7/4.8 reject `budget_tokens` outright (400). Two separate registry predicates exist for this: `model_supports_anthropic_adaptive_thinking` vs `model_requires_anthropic_adaptive_thinking` — don't use one where the other is needed.
