# EH-0002 ROI proof — per-class go/no-go results

M0.1 trace classes evaluated against the M0.2/M0.3 exit criterion from `ROADMAP.md`:
**net cost reduction ≥ 15% on multi-turn traces, with cache modeling, under the
frozen-frontier policy — or the route stays off.** Policy is per-route (per class here).

Inputs: bite 1's live-OpenRouter run (raw savings, quality) — full log in
`roi_raw_run.md` — plus bite 2/3's offline `--net-cost` simulation (this doc), both from
the same `optimize()` call (`Policy { mode: Live, horizon: 8, .. }`, `--api openai`,
matching the fixtures' wire format).

## Summary table

| Class | est raw savings | provider raw savings | resp_sim | judge | net-with-cache, OpenAI/ImplicitPrefix (h=8) | net-with-cache, Anthropic/ExplicitBreakpoints (h=8) | **Clears ≥15% net?** |
|---|---|---|---|---|---|---|---|
| `history_heavy` | 19.7% / 20.8% (avg ~20.2%) | 15.8% / 17.1% (avg ~16.5%) | 24% mean (2 rows) | 4.00/5 mean (2 rows) | **6.9%** ($0.001027 of $0.01494 skip-cost) | **25.5%** ($0.002343 of $0.009188 skip-cost) | **NO** on OpenAI/implicit-prefix routes; **YES** on Anthropic/explicit-breakpoint routes |
| `rag_heavy` | 0.0% | 0.0% | 22-35% (non-deterministic; no compression applied, so this reflects provider sampling noise on identical bodies, not quality loss) | judge failed (harness bug, see below) | 0.0% ($0 — gate never applies, ΔT=0) | 0.0% ($0 — gate never applies, ΔT=0) | **NO** |
| `tool_heavy` | 0.0% | 0.0% | 100% (byte-identical bodies, so identical answers are the quality proof, not a graded comparison) | judge failed (moot — nothing to grade) | 0.0% ($0 — gate never applies, ΔT=0) | 0.0% ($0 — gate never applies, ΔT=0) | **NO** |

Net-with-cache % = signed net USD delta (`anyllm_optimize_core::net_cost_delta_usd`,
gated by `should_apply`) divided by the class-total "always skip" cost
(`H·S·cached_read` for ImplicitPrefix, `S·input·write_mult + H·S·cached_read` for
ExplicitBreakpoints), summed over both rows in the class, `H=8`. Positive = compression
saves money relative to never compressing; this is the same "net cost reduction"
percentage the ROADMAP exit criterion is stated in.

## Reproduce

Raw savings + quality (bite 1, needs `OPENROUTER_API_KEY`, see `roi_raw_run.md` for the
exact commands and free-tier judge-model caveats):

```
cargo run -p anyllm_optimize_cli --bin optimize-eval -- --api openai \
  --input crates/optimizer/fixtures/samples/<class>.jsonl \
  --base-url https://openrouter.ai/api/v1 --model "openai/gpt-oss-20b:free" \
  --judge-model "openai/gpt-oss-20b:free" --max-tokens 200 --api-key "$OPENROUTER_API_KEY"
```

Net cost (bite 2/3, offline, no key needed):

```
cargo run -p anyllm_optimize_cli --bin optimize-eval -- --api openai \
  --input crates/optimizer/fixtures/samples/<class>.jsonl --model "n/a" --net-cost
# add `--cost-model anthropic` to see the ExplicitBreakpoints number
```

Offline token-savings-only proxy (no network, confirms the harness path used, per the
acceptance check):

```
cargo run -p anyllm_optimize_cli --bin optimize-eval -- --api openai \
  --input crates/optimizer/fixtures/samples/<class>.jsonl --model "n/a" --offline
```

Raw `--net-cost` output (2026-07-12, this run):

```
=== history_heavy ===
#     frontier      dt       s   apply        net_usd
0           12     197     773    true       0.000530
1           12     184     721    true       0.000498
NET COST (horizon=8, implicit-prefix, input=$2.50/Mtok cached_read=$1.25/Mtok write_mult=1.00x): total ΔT=381 tokens, net_delta=$0.001027

--cost-model anthropic:
0           12     197     773    true       0.001212
1           12     184     721    true       0.001132
NET COST (horizon=8, explicit-breakpoints, input=$3.00/Mtok cached_read=$0.30/Mtok write_mult=1.25x): total ΔT=381 tokens, net_delta=$0.002343

=== rag_heavy ===
#     frontier      dt       s   apply        net_usd
0            0       0       0   false       0.000000
1            0       0       0   false       0.000000
NET COST: total ΔT=0 tokens, net_delta=$0.000000

=== tool_heavy ===
#     frontier      dt       s   apply        net_usd
0            4       0     271   false       0.000000
1            4       0     345   false       0.000000
NET COST: total ΔT=0 tokens, net_delta=$0.000000
```

## Interpretation / go-no-go

**None of the three sampled classes clear the ≥15% net bar on OpenAI-style
implicit-prefix caching** at milestone-1 (STUB segmenter) state:

- `history_heavy` is the only class where FFEC's current stub segmenter (a single
  Prose-span, ALGO §5.2's structural segmenter not yet built) finds eligible extractive
  material — real, non-trivial raw savings (~20% local, ~16-17% provider-confirmed) with
  judge score 4/5 (LLM-graded "close to equivalent"). But under the frozen-frontier cost
  gate with `horizon=8` and OpenAI's implicit-prefix cache model, the realized net saving
  is only **~6.9%** — well short of 15%. The gate still chooses to apply (`should_apply`
  returns true, since `net_cost_delta_usd` is positive), but "positive" and "≥15%" are
  different bars; implicit-prefix pricing means most of the value of *not* touching the
  prefix is already captured by the provider's own cache, so compression's edge is thin
  at this `S`/`ΔT` ratio and horizon.
- The same `history_heavy` trace **does** clear the bar (**~25.5%**) when re-priced under
  Anthropic's explicit-breakpoints cache model (`--cost-model anthropic`): a
  breakpoint-managed route pays one write either way, so the full `ΔT` reduction is pure
  upside rather than partly offset by a implicit-cache "skip" baseline that's already
  cheap. **This means the go/no-go answer is not just per-route-class, it's per
  (route-class × cache-model)** — a class that fails on an OpenAI-backend route can pass
  on an Anthropic-backend route with the same traffic shape.
- `rag_heavy` and `tool_heavy` produce **0% raw savings** (0 edits applied) on both
  fixture files at the default policy/horizon, under *both* cache models — since raw
  savings are already 0%, cache math can't rescue them; they fail on the token-savings
  floor before quality or cost-gate accounting even enters. Per `CLAUDE.md`, the
  segmenter is still a STUB that "protects fenced/JSON buffers whole, else one Prose
  span"; `rag_heavy`'s pasted context and `tool_heavy`'s tool-call JSON are exactly the
  content types the stub is conservative about.

**Verdict:** per `ROADMAP.md`'s exit criterion, **stop the current milestone-1
implementation from shipping Live on any route by default** — no class clears ≥15% net
on the OpenAI/implicit-prefix path that most anyllm traffic actually uses, and the two
classes the roadmap author expected to pass easily (RAG-heavy, tool-heavy) currently
apply zero compression at all. This is not a dead end: `history_heavy`'s ~20%
extractable, judge-verified-safe raw savings and its ~25.5% net win on
explicit-breakpoint routes are real signal that the approach works once (a) the real
structural segmenter (ALGO §5.2, milestone 2) unlocks `rag_heavy`/`tool_heavy`'s
currently-protected content, and (b) the per-route policy is scoped to
explicit-breakpoint (Anthropic-style) backends first rather than shipped uniformly.
Re-run this exact harness after milestone 2 lands before flipping any route's default
away from off.

## Known harness limitation (does not affect the verdict above)

The `--judge-model` path hardcodes `max_tokens: 4` for the judge call (`run_judge` in
`optimize-cli/src/main.rs`). Every free-tier OpenRouter model tried
(`openai/gpt-oss-20b:free`, `google/gemma-4-31b-it:free`) returned
`"content": null, "finish_reason": "length"` on `rag_heavy`/`tool_heavy` rows — it spends
the 4-token budget without emitting the digit. This is a harness limitation, not a
quality signal, and is moot for both affected classes since raw/compressed bodies are
already byte-identical (0% savings) there, so there was nothing to grade in the first
place. `history_heavy` (the only class with a real compressed body to judge) got a valid
score both times.
