# Fixtures

Two kinds of fixture live here.

## 1. Eval inputs (`samples/*.jsonl`)

Input for `optimize-eval`. Each line is either a full request body
(`{"messages":[...]}`) or a bare JSON string (treated as one user message). See
`samples/multiturn.jsonl` for a runnable example.

```bash
cargo run -p anyllm_optimize_cli --bin optimize-eval -- \
  --api openai --input crates/optimizer/fixtures/samples/multiturn.jsonl \
  --base-url http://localhost:11434/v1 --model llama3.1 --offline
```

### Trace classes (M0.1, ROADMAP Phase 0)

Three realistic multi-turn trace classes for the ROI harness (also the Phase-3 parity/eval
corpus, ROADMAP D10):

- `samples/history_heavy.jsonl` — long back-and-forth conversation history (support and
  tutoring dialogs), many verbose turns accumulating before the latest message.
- `samples/rag_heavy.jsonl` — user turns carrying large pasted/retrieved context (doc
  excerpts, contract clauses) ahead of the actual question.
- `samples/tool_heavy.jsonl` — agent tool-call loops: `assistant` `tool_calls` + `tool`
  role results (search/read_file/fetch_page output) interleaved with user/assistant text.

Run any of them the same way as `multiturn.jsonl` above, swapping `--input`. Drop
`--offline` (and point `--base-url` at Ollama or OpenRouter) to get provider-reported
token counts alongside the local estimate.

## 2. Parity fixtures (Phase 3, M3.5)

Recorded Python LLMLingua-2 outputs for the parity suite (keep-set F1 >= 0.9,
ratio within +/-5%), across these categories (ROADMAP D10):

- meeting transcripts (in-domain)
- markdown
- code
- JSON-in-prose
- multilingual
- edge cases (emoji, CJK, mixed scripts)

Format: `parity/<category>/<id>.json` with `{ "input": "...", "ratio": 0.5,
"words": [...], "keep_mask": [true, false, ...], "compressed": "...",
"reference": { "model": "...", "llmlingua_version": "..." } }`. `words` is
the reference implementation's own word tokenization (whitespace + attached
punctuation split), aligned 1:1 with `keep_mask`; a parity test comparing
against the Rust word segmenter must account for tokenization differences
before computing set F1, not assume identical indices.

Regenerate with `scripts/gen_parity_fixtures.py` (requires a Python venv with
`pip install llmlingua`; downloads
`microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank` from the HF
Hub on first run). Not part of the Rust workspace or CI — run manually when
the reference model/library version changes.

The parity test lives at `crates/optimize-scorer/tests/parity.rs`
(`cargo test -p anyllm_optimize_scorer --features onnx`, not `#[ignore]`) and
compares `LlmLingua2Scorer`'s ranking against these fixtures. It requires a
real exported ONNX artifact — set `ANYLLM_OPTIMIZER_TEST_MODEL_DIR` to a
directory containing `model.onnx` + `tokenizer.json`
(`scripts/export_llmlingua2.py`); without it the test panics with
instructions rather than silently passing or skipping. These fixtures are
the reference side of that comparison, not a substitute for the artifact.
