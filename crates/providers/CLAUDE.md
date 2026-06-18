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
