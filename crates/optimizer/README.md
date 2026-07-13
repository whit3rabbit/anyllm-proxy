# anyllm-optimizer

Prompt compression for the anyllm proxy. Implements **Frozen-Frontier Extractive
Compression (FFEC)**: LLMLingua-2's token-importance scoring, but re-worked so it
**cooperates with provider prefix caches instead of fighting them**.

The one-sentence design: *a deterministic, per-message, frozen-once extractive
compressor that cooperates with provider prefix caches, with LLMLingua-2 as one scorer
behind a trait, and a cost model that refuses to compress when caching already won.*

See [`fixtures/roi_results.md`](fixtures/roi_results.md) for the ROI analysis and [`docs/ALGO.md`](docs/ALGO.md) for the
algorithm reference (source of truth for what is implemented).

## Why not just run LLMLingua-2?

Every major provider cache is an **exact-prefix cache**: any byte change at position *i*
invalidates everything from *i* onward. LLMLingua-2 compresses with a global ratio τ over
the whole prompt, so every new turn shifts the cutoff and re-renders old messages
differently → **~0% cache hits**. With Anthropic's 0.1x cached-read pricing, that can make
a naive compressor cost *more* than no compression.

FFEC fixes this with three rules:

1. **Compression is a pure function of one message's bytes** (no global ratio, no
   cross-message context) → recompressing a message always yields identical bytes → the
   proxy stays stateless and cache-stable.
2. **Messages transition exactly once** (`verbatim → compressed(frozen)`), at a
   monotone **frontier** (last K turns always verbatim).
3. **Batch frontier advances** (hysteresis) and place the cache breakpoint *at* the
   frontier, so the frozen region is exactly the cached region.

A **cost gate** then refuses to apply compression when the cache already wins, and
everything is **fail-open**: any error forwards the original request untouched.

## Crates

| Crate | Role |
|---|---|
| `crates/optimize-core` | Pure algorithm over an IR: edit scripts, frontier, selection, cost gate, `optimize()`, invariants. No IO, no serde, no ML. |
| `crates/optimize-passes` | Provider adapters (OpenAI/Anthropic JSON ⇄ IR), cache strategies, pricing, JSON-value tool-result compression. |
| `crates/optimize-scorer` | LLMLingua-2 ONNX scorer behind `feature = "onnx"` (opt-in; needs a downloaded artifact). |
| `optimize-cli` | `optimize-eval`: token-usage / savings harness. |
| `benches` | Criterion benchmarks over the corpus. |

## Build & test

```bash
cargo build  -p anyllm_optimize_core -p anyllm_optimize_passes -p anyllm_optimize_cli
cargo test   -p anyllm_optimize_core -p anyllm_optimize_passes    # invariants + adapters
cargo clippy -p anyllm_optimize_core -p anyllm_optimize_passes -- -D warnings
cargo bench  -p anyllm_optimize_benches                           # optimizer overhead
```

## Measuring real savings (`optimize-eval`)

Runs FFEC in Live mode, then sends both the raw and compressed body to any
OpenAI-compatible or Anthropic-compatible endpoint and compares local estimate vs
provider-reported token usage.

```bash
# Offline: local estimate only (no network).
cargo run -p anyllm_optimize_cli --bin optimize-eval -- \
  --api openai --input crates/optimizer/fixtures/samples/longchat.jsonl \
  --model llama3.1 --offline

# Against a local Ollama:
cargo run -p anyllm_optimize_cli --bin optimize-eval -- \
  --api openai --base-url http://localhost:11434/v1 --model llama3.1 \
  --input crates/optimizer/fixtures/samples/longchat.jsonl

# Against OpenRouter (ground-truth provider token counts):
OPENROUTER_API_KEY=sk-... cargo run -p anyllm_optimize_cli --bin optimize-eval -- \
  --api openai --base-url https://openrouter.ai/api/v1 --model openai/gpt-4o-mini \
  --input crates/optimizer/fixtures/samples/longchat.jsonl
```

Add `--stream` for streaming requests (usage is read from the final SSE chunk;
OpenAI gets `stream_options.include_usage`, Anthropic reads `message_start` usage).
Compare two backends by running twice with different `--base-url` and diffing tables.

**Response quality** (the go/no-go signal — savings mean nothing if the answer
changes). When not `--offline`, the harness sends both bodies, captures both responses,
and reports `resp_sim` (word-Jaccard similarity between the raw and compressed answers).
Add `--show-responses` to dump both, or `--judge-model <m>` to have an OpenAI-compatible
model score 1-5 how well the compressed answer preserves the raw one:

```bash
cargo run -p anyllm_optimize_cli --bin optimize-eval -- \
  --api openai --base-url http://localhost:11434/v1 --model llama3.1 \
  --input crates/optimizer/fixtures/samples/longchat.jsonl \
  --judge-model llama3.1 --show-responses
```

## Streaming and OpenAI vs Anthropic

- **Streaming**: FFEC transforms the *outbound request* prompt; the streamed *response*
  is never touched. A `stream:true` request has the same body shape, so compression is
  unaffected. The eval harness handles reading usage off both streaming shapes.
- **OpenAI vs Anthropic**: reconciled in two adapters. OpenAI system is a `role:"system"`
  message; Anthropic system is a top-level field. OpenAI is implicit-prefix cache;
  Anthropic uses explicit `cache_control` breakpoints (client markers → Immutable). Tool
  args (both) and thinking blocks (Anthropic) are never edited.

## Invariants (property-tested from milestone 1)

Fail-open · protected regions byte-identical · determinism · frozen stability · order
preserved · UTF-8 / JSON / fence validity · monotone frontier · budget honesty.
See `crates/optimize-core/tests/invariants.rs`.

## Status

Milestones M0–M5 (ALGO §12) implemented; `cargo test --workspace` green. Core + safety +
adapters + shadow/live orchestration, structural segmenter, dedup + whitespace normalize,
JSON-value tool-result compression, per-route policy, budget planner, and the LLMLingua-2
ONNX scorer are all real (see [`docs/ALGO.md`](docs/ALGO.md) §12 and `CLAUDE.md`). **Wired
into the proxy** (`OPTIMIZER_MODE=off|shadow|live`, admin-toggleable); the runtime path is
`UniformScorer` unless an operator opts a route into the ONNX tier. `dedup_pass`,
`normalize_pass`, and tool-result compression are implemented and exported but not yet
composed into `optimize()` itself (ALGO §1) — the core path compresses Text-block prose.

### ONNX scorer (opt-in)

The ML scorer is behind `--features onnx` and needs a ~110MB model artifact that is never
bundled or auto-downloaded. Produce it offline (`scripts/export_llmlingua2.py`), upload it,
then fetch + sha256-verify it explicitly:

```
MODEL_URL=https://host/llmlingua2 MODEL_SHA256=<hex> \
  cargo run -p anyllm_optimize_cli --bin optimize-model
# -> writes a verified model.onnx + tokenizer.json into MODEL_CACHE_DIR/<sha256>/
```

`LlmLingua2Scorer::from_files` loads that pair; the parity gate (keep-set F1 >= 0.9) runs
via `ANYLLM_OPTIMIZER_TEST_MODEL_DIR` (`cargo test -p anyllm_optimize_scorer --features onnx`).
