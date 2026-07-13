# CLAUDE.md — anyllm-optimizer

Prompt-compression sub-workspace for the anyllm proxy. Read `docs/ALGO.md` (algorithm reference, source of truth)
first. This file is the contributor contract; it holds the standing facts you need every time you touch this crate.
Milestone/changelog history lives in git and `fixtures/roi_results.md` (ROI analysis), not here.

## What this is

**Frozen-Frontier Extractive Compression (FFEC)**: LLMLingua-2 token-importance scoring,
re-worked to be a **pure per-message function** so compressed prompts stay byte-stable
across turns and cooperate with provider prefix caches. A monotone **frontier** bounds
cache invalidation; a **cost gate** refuses to compress when caching already wins; the
whole thing is **fail-open** (any error → forward the original).

All milestones M0–M5 and the proxy integration are implemented; `cargo test --workspace`
(default features) is green. The `onnx` scorer is opt-in and its parity gate is live (needs
a downloaded model artifact, see below).

## Crate map & boundaries

| Crate | Deps | Holds |
|---|---|---|
| `optimize-core` | `smallvec memchr thiserror unicode-segmentation` (NO serde) | IR types, `EditScript`, `frontier`, `select`, `segment`, `compress_message`, cost gate, `HeuristicBudgetCounter`, `optimize()`, traits, invariants |
| `optimize-passes` | core + `serde_json` | OpenAI/Anthropic adapters, `CacheStrategy` impls, pricing, `tool_result` compression |
| `optimize-scorer` | core (+ ONNX deps under `onnx`) | `LlmLingua2Scorer` (stub) |
| `optimize-cli` | core + passes + `reqwest clap anyhow` | `optimize-eval` savings harness |
| `benches` | core + passes + `criterion` | benchmarks |

**Architecture note (deviation from ALGO's file table):** `compress_message`, `segment`,
the cost gate (`should_apply`), and `HeuristicBudgetCounter` live in **core**, not passes.
`optimize()` is the core entry point (ALGO §9) and must call them; passes depends on core,
so putting them in passes would be a circular dep. Passes keeps only what needs `serde_json`
(adapters, tool-result) or provider knowledge (strategies, pricing). The *richer* structural
segmenter and JSON-value tool-result compression are the passes-side upgrades of the core
stubs.

## Invariants a contributor must not break

1. **Fail-open** — any `Err`/panic ⇒ output ≡ input. `optimize()` wraps the pipeline in
   `catch_unwind`. No panics in the request path.
2. **Determinism** — same `(message bytes, PolicyVersion)` ⇒ same output bytes, any
   machine/thread/run. Never iterate a `HashMap` to make a decision; `quantize()` scores
   before comparing; tie-break by position. Bump `PolicyVersion` on any rule/model change.
3. **Per-message purity** — `compress_message` depends ONLY on one message's bytes + policy.
   No cross-message context, no global ratio, no clocks. This is what keeps caches stable.
4. **Monotone frontier** — `frontier(n+1) >= frontier(n)`.
5. **Extractive only** — never reorder, never paraphrase, only delete (+ marker Replace for
   truncation). Output words are a subsequence of input words.
6. **Validity** — UTF-8, no split graphemes, never break JSON structure in tool blocks,
   never break fence pairing (the segmenter protects fenced buffers whole).
7. **Protected regions untouched** — system, tool schemas, ToolUse args, thinking/Opaque
   blocks, the latest message, client `cache_control` regions.

Property tests: `optimize-core/tests/invariants.rs` (I2/I3/I4/I7/I8) + unit tests (I1/I5/I6)
+ `optimize-passes/tests/pipeline.rs` (I6 through the adapters). Write invariant tests
before adding a scorer.

## Streaming & tool-loop rules (proxy integration)

FFEC is **stream-agnostic**: it transforms the outbound request; the streamed response is
never read. A `stream:true` request has the same body shape. Two rules:

- Apply the optimizer to the **client-sent history on request entry only** — NOT to
  proxy-appended tool-loop turns (`chat_completions/stream/generic/tool_loop.rs`,
  `routes/messages.rs`). This keeps the frontier monotone and never compresses a message
  the model just produced this turn. (Latest turn + ToolUse args are Immutable anyway.)
- Reading usage off streams is an eval-harness concern (`optimize-cli`), not the crate's.

Proxy integration is wired (mirrors rtk/forge-guardrails): `crates/proxy/src/optimizer.rs`,
`server/state.rs`, `admin/state.rs`, `admin/routes/config.rs`, `handler.rs`, `messages.rs`,
`metrics/mod.rs`. The proxy scorer is `UniformScorer` (heuristic path). `RuntimeConfig` field
`optimizer_mode: String` follows the 6-site checklist (2 not compiler-caught). Env seed:
`OPTIMIZER_MODE=off|shadow|live`. Ship order stays Shadow → measure cache-hit-rate delta →
Live per opt-in route; never invert the cache-stability rules above for a better ratio.

## ONNX model artifact (opt-in, never bundled, never auto-downloaded)

The `onnx` feature declares `ort`/`tokenizers`/`ndarray`/`rayon` (`ort`'s `download-binaries`
fetches the onnxruntime binary at *build* time — under `target/`, gitignored). The ~170MB
quantized model is NOT in the build.

**Pinned artifact** (`optimize-scorer/src/artifact.rs`): `KatawaDead/llmlingua-2-bert-base-
multilingual-cased-meetingbank-onnx-int8`, `model.onnx` sha256 `2753018e…deada` — an int8
ONNX of the exact reference model; the parity gate (F1 ≥ 0.9) passes with it. Override via
`MODEL_URL`/`MODEL_SHA256`; cache dir `MODEL_CACHE_DIR` (proxy default `<ANYLLM_HOME>/models`).

**One shared download+verify** in `optimize-scorer::artifact` (always compiled, NOT behind
`onnx`): `ArtifactConfig::resolve` → `download_and_verify` (sha256 gate) → verified pair +
`model.onnx.sha256` sidecar so `is_present()` is a cheap stat, no re-hash. Two callers reuse
it: the `optimize-model` CLI (`optimize-cli/src/model_fetch.rs`) and the proxy admin
(behind the proxy's `optimizer-onnx` feature): `POST /admin/api/optimizer/model` runs it in a
`spawn_blocking` task, `GET` reports `{compiled_in, present, downloading, …}`; the admin UI
gates the mode toggle on presence and shows a Download button when compiled-in-but-absent.

`LlmLingua2Scorer::from_files` loads the verified pair. In the proxy, `OptimizerEngine`
(`crates/proxy/src/optimizer.rs`) holds an `Arc<RwLock<Option<Arc<dyn TokenScorer>>>>` slot:
loaded eagerly at startup if present, else lazily on the first non-`Off` request after a
download (`ensure_scorer_loaded`), shared across per-request clones. Fail-open: a missing/bad
artifact leaves the slot empty and live-mode uses `UniformScorer`. `LlmLingua2Scorer::load`
(in-process auto-download) is intentionally NOT provided — the CLI/admin button is the only
fetch path, so "never auto-download" is structural. Parity suite needs the artifact via
`ANYLLM_OPTIMIZER_TEST_MODEL_DIR`.

```
cargo run -p anyllm_optimize_cli --bin optimize-model          # pinned artifact
MODEL_URL=https://host/x MODEL_SHA256=<hex> \                  # self-hosted override
  cargo run -p anyllm_optimize_cli --bin optimize-model
```

## Do NOT build (ALGO §11)

Causal-LM/perplexity scoring · global ratio across messages · paraphrase/abstractive
rewrite · a conversation state store · tokenizer perfectionism for budgets · compression
of system/schemas/ToolUse args/thinking/latest-turn/unknown blocks.
