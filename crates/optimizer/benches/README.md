# anyllm-optimizer benchmarks

Criterion benchmarks for the no-ML (heuristic-scorer) FFEC path. They exist to guard the
Phase-1 exit target — **p99 optimizer overhead well under 1 ms for interactive prompt
sizes** — and to document the perf envelope across the ROADMAP §7 corpus classes.

## Run

```bash
# from crates/optimizer/
cargo bench -p anyllm_optimize_benches --bench optimize

# quick pass (shorter, noisier):
cargo bench -p anyllm_optimize_benches --bench optimize -- \
  --warm-up-time 1 --measurement-time 3 --sample-size 20
```

Pass `--bench optimize` explicitly: the crate's `lib` target is also a bench harness and
will reject criterion flags otherwise.

Criterion writes raw per-sample data and HTML reports (with the p99 tail) under
`target/criterion/<group>/<id>/`. Open `report/index.html` for the full distribution;
`raw.csv` has every sample if you want to compute an exact p99 yourself.

## Corpus classes (ROADMAP §7)

Builders live in `src/lib.rs`. Each shapes a `Conversation` like one real traffic class:

| Class | Builder | Shape |
|---|---|---|
| short chat | `prose_conversation(4)` | 4 short prose turns |
| 100-turn convo | `prose_conversation(100)` | 100 prose turns |
| RAG (~1 MB) | `rag_conversation(1024)` | 4 retrieval turns, each a large retrieved-context buffer, + live question |
| tool-heavy | `tool_conversation(16)` | 16 rounds of ask / `ToolUse` / `ToolResult` |
| JSON | `json_conversation(16)` | 16 JSON `ToolResult` blocks |
| markdown | `markdown_conversation(20)` | prose + tables + fenced blocks per turn |
| code | `code_conversation(20)` | prose + fenced Rust blocks per turn |

Latest turn is `Immutable` per the latest-message rule; older turns are `Mutable`.

## Perf envelope

Measured on the developer machine (Apple Silicon, `bench` profile, heuristic
`UniformScorer` — no ONNX model loaded). These are criterion mean times with the 95%
confidence interval; **treat the absolute numbers as a same-machine regression baseline,
not a spec** — rerun locally to establish your own baseline before reading a regression.

| Class | Approx input | Mean time (95% CI) | Approx throughput |
|---|---|---|---|
| short chat (prose/4) | ~0.4 KB | 21 ns [21.0–21.1 ns] | no-op (nothing eligible) |
| convo (prose/20) | ~4 KB | 122 µs [121–123 µs] | ~30 M tok/s |
| convo (prose/100) | ~20 KB | 724 µs [722–727 µs] | ~15 M tok/s |
| RAG ~1 MB | ~1 MB | 7.5 ms [7.46–7.59 ms] | ~35 M tok/s |
| tool-heavy | ~9 KB | 127 µs [126–127 µs] | ~55 M tok/s |
| JSON | ~6 KB | 290 ns [288–293 ns] | core segmenter protects JSON buffers whole (near no-op) |
| markdown | ~30 KB | 484 µs [482–487 µs] | ~30 M tok/s |
| code | ~26 KB | 239 µs [238–240 µs] | ~55 M tok/s |

Throughput is `input_bytes / mean_time` at ~4 bytes/token — order-of-magnitude only.

Notes on the envelope:

- **p99 / p50 tail:** criterion's HTML report and `raw.csv` (under `target/criterion/`)
  carry the full sample distribution; the table above reports the mean + CI. For a hard
  p99 gate, read `raw.csv` after a run — the no-ML path shows no heavy tail here (CI width
  is sub-percent).
- **Resident memory:** the benched path is heuristic-only, so resident set is dominated by
  the input conversation, not the scorer. The **XLM-R model tier is a stub** (`optimize-scorer`,
  `unimplemented!()` behind `feature = "onnx"`); once it lands, re-measure resident memory
  with the model loaded (target: model + arena, not per-request growth) and add a
  model-resident row here. It is intentionally absent, not overlooked.
- **JSON / short-chat near-no-ops:** the core `segment` stub protects fenced and JSON
  buffers whole, and short chats have nothing eligible past the protected latest turn — so
  `optimize()` short-circuits. Field-level JSON `ToolResult` compression lives in
  `optimize-passes` (`tool_result.rs`), a separate path not exercised by this core bench.
