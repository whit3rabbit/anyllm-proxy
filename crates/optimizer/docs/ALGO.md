# anyllm-optimizer — Algorithm reference (source of truth)

This document describes the algorithm **as implemented** in `crates/optimizer`. It is the
normative reference: when code and this doc disagree, the code wins and this doc is the bug.
Section numbers are stable — source comments cite them as `ALGO §N`, so keep them fixed when
editing.

The one algorithm here is *Frozen-Frontier Extractive Compression (FFEC)*: LLMLingua-2's
token-importance scoring, re-worked so compression is a **pure per-message function** that
cooperates with provider prefix caches instead of fighting them.

Taken from LLMLingua-2 (`microsoft/LLMLingua`):
- token-classification importance scoring with a bidirectional encoder (p_preserve per word)
- ≤512-token chunked inference, subword→word probability averaging, words never split
- extractive-only: never reorder, never rewrite, only delete
- force-keep rules for structural tokens (`\n`, punctuation, digits)

Deliberately changed (why in §2):
- global compression ratio → **per-message pure-function compression** (cache stability)
- compress-everything → **frozen frontier with batched advancement** (bounded invalidation)
- unconditional apply → **provider-aware cost gate** (caching discounts can beat compression)
- string in / string out → **IR + edit script + renderer** (auditability, shadow mode, safety)

---

## 0. Non-negotiables

1. **Fail-open is absolute.** Any `Err` or panic anywhere in the pipeline results in the
   original request forwarded byte-identically. `optimize()` wraps the whole pipeline in
   `catch_unwind`; per-message and per-buffer errors are caught and that unit is skipped.
2. No `unsafe`, no global mutable state. All configuration flows through `Policy`.
3. **Determinism is an invariant:** same `(message bytes, PolicyVersion)` → same output bytes
   on any machine, thread, or run. Never iterate a `HashMap` to make a decision; sort by
   `(quantized_score desc, position asc)`; quantize all scores with `quantize()` before
   comparing.

Workspace layout (library crates + tooling):

```
crates/optimizer/
├── crates/
│   ├── optimize-core/    # IR, edits, renderer, frontier, selection, passes, cost gate, optimize()
│   ├── optimize-passes/  # provider adapters, cache strategies, pricing, tool-result compression
│   └── optimize-scorer/  # feature "onnx": LLMLingua-2 scorer (ort + tokenizers)
├── optimize-cli/         # optimize-eval savings harness, optimize-model artifact fetcher
└── benches/
```

Note the deviation from a pure layering: `compress_message`, `segment`, the cost gate
(`should_apply`), `HeuristicBudgetCounter`, and `optimize()` itself live in **core**, not
passes — `optimize()` is core's entry point (§9) and calls them, and passes depends on core,
so putting them in passes would be a circular dep. Passes holds only what needs `serde_json`
(adapters, tool-result) or provider knowledge (strategies, pricing).

---

## 1. The algorithm, end to end

```
FFEC(conversation C with messages m[0..n], policy P, cache strategy S, scorer, budget counter):

 1. IR        := built by the provider adapter from already-parsed request JSON
 2. PROTECT   := adapters mark Immutable: system messages, m[n] (latest), client-marked
                 (cache_control) messages, ToolUse args, Opaque blocks (images/thinking/unknown)
 3. frontier  := F(n)  — deterministic, batched, monotone (§4)
                 messages with index < frontier, not Immutable, not client_cache_marker are ELIGIBLE
 4. for each eligible message mi, oldest-first, until the scorer deadline expires:
        edits[i] := compress_message(mi, P')   — PURE function of mi's bytes (§5)
                 // P' is P, optionally with a per-message ratio pre-planned by the
                 // BudgetPlanner (§5.6). Inside: segment text (§5.2) → for each Prose span,
                 // score words (§6) → select keep-set (§5.4) → emit Delete/Replace edits (§5.5)
        each script is validated against invariants (§3/§10); an invalid script is dropped
        (fail-open per buffer). Deadline-skipped messages get NO edits and stay verbatim.
 5. estimate  := ΔT (tokens removed this turn) and S (frozen-zone tokens) via the BudgetCounter
 6. gate      := should_apply(ΔT, S, horizon, pricing, cache_model) — apply, or ship original (§8)
 7. if P.mode != Live or gate says skip or no edits: emit report, forward ORIGINAL
    else: render(IR, edits) → new body; place the cache breakpoint at the frontier; forward
 8. emit OptimizationReport either way
```

Statelessness: the client resends full original history every turn. Because step 4 is a pure
function of one message's bytes, re-compression on turn k+1 reproduces turn k's bytes exactly
— no state store, no coordination, cache-stable by construction.

**Not composed into `optimize()` yet.** `dedup_pass` (§5.7), `normalize_pass` (§5.7), and
tool-result value compression (`optimize-passes::compress_message` / `compress_tool_result`,
§7) are implemented, tested, and exported, but the core `optimize()` orchestrator currently
calls only core's `compress_message` (Text-block prose). A caller that wants dedup/normalize
or tool-result compression composes those passes itself. Wiring them into `optimize()` behind
the same frontier/deadline gating is a follow-up.

---

## 2. Why per-message + frontier (the load-bearing decision)

Provider caches are exact-prefix caches. LLMLingua-2's ratio τ is computed over the whole
prompt, so each new turn shifts the global top-⌈τN⌉ cutoff and re-renders old messages
differently → 0% cache hits. Per-message pure compression makes old messages byte-stable; the
frontier makes the *set* of compressed messages change only in batches. Two provider regimes
follow:

- **Anthropic (explicit breakpoints):** the optimizer owns breakpoint placement and puts the
  deepest breakpoint at the frontier. Everything before it: frozen bytes, cache-read every
  turn. Everything after: never cached, free to be verbatim. When the frontier advances, newly
  frozen messages are compressed *at the moment they first enter the cached region* → zero
  cache invalidation, and the cache write itself is smaller. Compression is nearly pure win
  here (quality risk aside). Modeled as `CacheModel::ExplicitBreakpoints`.
- **OpenAI/Gemini implicit caching:** the recent zone gets auto-cached too, so a frontier
  advance invalidates the suffix once. Hysteresis (batch size K) bounds this to one partial
  re-write per K turns, and the cost gate (§8) decides if it pays. Modeled as
  `CacheModel::ImplicitPrefix`.

---

## 3. optimize-core: IR, edits, renderer

`types.rs`:

- `Role` — System / User / Assistant / Tool. Anthropic's top-level system prompt is
  synthesized into a `Role::System` message by the adapter so protection is uniform.
- `Protection` — `Mutable` (eligible), `Frozen` (informational: compressed on an earlier turn,
  recomputes identically; treated exactly like Mutable), `Immutable` (never touched).
- `Conversation { messages: Vec<Message> }`.
- `Message { role, blocks: Vec<ContentBlock>, protection, client_cache_marker: bool }`.
  `client_cache_marker` = the client set its own `cache_control` here; never touch, never move.
- `ContentBlock` — `Text(String)` (compressible), `ToolResult { raw }` (value-level
  compression only, §7), `ToolUse { raw }` and `Opaque { raw }` (both immutable passthrough).
  `Message::buffer(BufferId)` returns `Some(&str)` only for `Text`/`ToolResult`.
- `BufferId(usize)` — index of a compressible buffer within a message's `blocks`.
- `PolicyVersion(u64)` — identifies the decision procedure. Any change to model weights, rules,
  ratios, or selection logic MUST bump it; operators expect one cache re-write when it changes.

`edit.rs` — edits are byte ranges into ONE text buffer (a Text block or one JSON string value).
Extractive only:

```rust
pub enum Edit {
    Delete(Range<usize>),
    Replace { range: Range<usize>, text: String }, // only for structural truncation markers
}
pub struct EditScript { pub edits: Vec<Edit> } // sorted by start, non-overlapping
```

`EditScript::validate(src)` is the safety boundary — it rejects the whole script on any
overlap, out-of-bounds range, or non-char-boundary split. `apply(src, out)` walks the edits
once, copying the gaps. Every pipeline stage validates before pushing a script (fail-open per
buffer).

---

## 4. Frontier: deterministic, monotone, batched

```rust
pub struct FrontierPolicy { pub keep_recent: usize, pub batch_k: usize } // default 4, 4

pub fn frontier(n_messages: usize, p: &FrontierPolicy) -> usize {
    let eligible_end = n_messages.saturating_sub(p.keep_recent);
    let k = p.batch_k.max(1);
    eligible_end - (eligible_end % k)
}
```

`F(n)` is a pure function of message count: the last `keep_recent` messages are always
verbatim, and the eligible boundary is floored to a multiple of `batch_k` (hysteresis). It is
monotone (`F(n+1) >= F(n)`) and `batch_k = 0` is safe (`.max(1)`). Bigger K = fewer
invalidation events on implicit-cache providers, slower savings ramp; on Anthropic K can be
small (2). Protection rules dominate eligibility — system messages are Immutable regardless of
where the frontier sits.

---

## 5. compress_message: the pure function

```rust
pub fn compress_message(
    msg: &Message, policy: &CompressionPolicy, scorer: &dyn TokenScorer, ws: &mut Workspace,
) -> Result<Vec<(BufferId, EditScript)>, OptimizeError>
```

PURE: output depends only on the message's block bytes and the policy — no cross-message
context, no clocks, no randomness, no global ratio. Returns one `EditScript` per compressible
buffer. System-role messages and any buffer shorter than `policy.min_len` (default 200) are
skipped; a per-role ratio `>= 1.0` short-circuits. Core's `compress_message` handles `Text`
blocks only; `optimize-passes::compress_message` wraps it and adds `ToolResult` blocks (§7).

`CompressionPolicy` fields: `version`, `ratios` (RatioTable), `force` (ForceRules), `min_len`
(200), `tool_result_max_tokens` (4000), `deadline` (150ms, scorer budget for the whole
request), `planner: Option<BudgetPlanner>` (§5.6, `None` by default). `RatioTable` defaults:
User 0.7, Assistant 0.6, System 1.0 (Immutable anyway), tool-result value 0.4.

### 5.1 Text-block pipeline

```
text ──► segment (§5.2) ──► for each Prose span:
             split_words (§5.3) ──► score (§6) ──► select_keep (§5.4) ──► emit_edits (§5.5)
         non-Prose spans: untouched byte-for-byte
```

### 5.2 Segmentation — structural, not language-aware

Protect anything whose *syntax* carries meaning; compress only prose. `segment()` is a single
left-to-right scan, no regex, no substring allocation, and always covers the whole buffer
exactly (an empty buffer yields one empty Prose segment). `SegKind`: `Prose`, `FencedCode`,
`InlineCode`, `Url`, `Table`. Priority order at each position (as implemented):

1. **Fenced code** (` ``` ` or `~~~`, 3+ of the same char at line start) — protected including
   the fence lines, closed by a matching-or-longer fence line. **An unmatched opening fence
   protects to end-of-buffer** (safe default; guarantees fence-pairing, invariant I6).
2. **Table lines** (at line start, containing `|` or `+---` AND non-alphanumeric ratio > 0.4).
3. **Inline code** (`` `code` ``, closed by a backtick run of the same length; never crosses a
   newline — falls back to Prose rather than over-protect).
4. **URLs** (`scheme://non-ws+`; deleting half a URL is worse than keeping it whole).
5. Everything else — **Prose**.

### 5.3 Word split (must match scorer aggregation)

`split_words` uses `unicode_segmentation`'s `split_word_bounds`: punctuation runs are their own
"words", whitespace is NOT a word (it's glue handled at edit emission). A `Word` is a byte
range into the buffer with no surrounding whitespace.

### 5.4 Selection — LLMLingua-2's top-k, made deterministic and structure-safe

`quantize(p: f32) -> u16 = (p.clamp(0,1) * 10_000) as u16` — quantize before ANY comparison, to
kill float-drift nondeterminism across runtimes/threads.

`ForceRules` (defaults): `keep_chars = ['\n', '?', '!', ':']`, `keep_digits = true`,
`keep_first_word = true` (the first word anchors reconstruction).

`select_keep(words, text, scores, ratio, force) -> Vec<bool>`:
1. force-keep any word containing a `keep_char` or (if `keep_digits`) an ASCII digit; force-keep
   word 0 if `keep_first_word`.
2. budget `n_keep = max(ceil(ratio·n), forced_count)`.
3. rank the non-forced words by `(quantized score desc, position asc)` and keep the top
   `n_keep − forced` — fully deterministic, ties broken by earlier position.

### 5.5 Edit emission — deleting words and their glue

`emit_edits(text, words, keep, out)` converts the keep-mask to `Delete`/`Replace` edits:
- a run of consecutive dropped words is coalesced into one edit;
- a dropped run followed by a kept word consumes the FOLLOWING whitespace gap so exactly one
  space remains — **unless that gap contains a `\n`**, in which case the run is `Replace`d with
  a single `"\n"` so line/paragraph structure survives;
- a run that reaches the last word consumes the PRECEDING gap instead (never trailing content,
  which may belong to another segment).

### 5.6 BudgetPlanner (optional per-message ratio)

`BudgetPlanner::plan_ratio(base, role, index, byte_len)` tightens the per-role ratio by a
message's absolute age (`index`, 0 = oldest) and byte size — LLMLingua-1's "position matters"
idea, kept per-message-pure. It is a pure function of ONE message's own `(role, index,
byte_len)` plus the static `RatioTable`; it must NEVER read `conv.messages.len()`, the frontier,
or any other message (that would let a frozen message's ratio drift as history grows, breaking
I3). Penalties only ever *tighten* (never loosen) below the base ratio, floored at `min_ratio`,
so ratio-honesty (I8) holds. The `Default` (all-zero steps) reproduces the flat per-role table
exactly, so it is opt-in and backward compatible. The orchestrator applies it by handing
`compress_message` a per-message policy clone with just that role's ratio pre-planned;
`compress_message` itself stays unmodified.

### 5.7 Deterministic passes (dedup, normalize)

Two standalone extractive passes, per-message-pure, scoped to `0..frontier` and skipping
`Immutable`/`client_cache_marker` messages (same eligibility as `compress_message`):

- `dedup_pass` — collapses exact-duplicate `Text` buffers that recur across behind-frontier
  messages. The FIRST occurrence is kept; later byte-identical ones are whole-buffer deleted.
  Keeping the first (not the newest) is what makes it cache-safe: a frozen message's decision
  can't change when later turns are appended. A `HashMap` is used for point lookups only, never
  iterated to decide ordering.
- `normalize_pass` / `normalize_buffer` — collapses redundant whitespace within Prose spans:
  mid-line runs of 2+ spaces/tabs → one, trailing horizontal whitespace before a newline/EOF →
  deleted, runs of 3+ newlines → two. Protected spans (code/URL/table) are untouched.

Both are exported but not yet composed into `optimize()` (see §1).

---

## 6. optimize-scorer: LLMLingua-2 scoring (ONNX, feature = "onnx")

Model: `microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank` (~110M), exported to
ONNX and int8-quantized offline (`scripts/export_llmlingua2.py`), shipped as a hash-pinned
downloadable artifact (~110MB). The exported graph is an mBERT token-classification head:
inputs `input_ids`/`attention_mask`/`token_type_ids`, output `logits [batch, seq, 2]`; class
index 1 is "preserve".

`TokenScorer` trait: `score_words(&[&str]) -> Vec<f32>` (one `p_preserve` per word, same length;
deterministic for identical input on the same `artifact_hash()`), and `artifact_hash()` (folded
into `PolicyVersion` so a model swap forces a deliberate cache re-write). `UniformScorer` is the
fallback: every word scores 0.5, so selection degenerates to forced-keeps + first-k. It is used
for the no-ML path and for fail-open — never silently for "better" results. **The proxy runtime
path is `UniformScorer` unless an operator opts a route into the ONNX tier.**

`LlmLingua2Scorer` scoring (per the paper):
- encode WHOLE WORDS (each tokenized independently, no special tokens) so a word's subtokens
  never straddle a chunk boundary — no truncation of words, ever;
- greedy-pack words into chunks of ≤ `max_seq − 2` subtokens (512 → 510 usable), preferring to
  break right after a `.`-ending word within `PERIOD_BREAK_LOOKBACK` (50) words of the boundary;
- word score = MEAN of its subtokens' `softmax(logits)[preserve]`; CLS/SEP positions skipped;
- chunks are independent and **position-addressed** — each chunk writes into its own slice of
  the result vector, so `rayon` parallelism (or none) can never change the result. `ort`'s
  `Session::run` takes `&mut self`, so concurrent chunks serialize on a `Mutex<Session>`;
  correctness never depends on the achieved concurrency.

Deadline handling lives in the orchestrator (§9), not the scorer: messages are scored
oldest-first (behind the frontier = highest-value, most-stable). If the deadline expires
mid-request, already-scored messages keep their edits; remaining messages get **no edits this
turn** (not `UniformScorer` edits) so they stay verbatim, preserving each message's single
verbatim→compressed transition for a later turn.

Artifact delivery is an explicit operator step, never an in-process auto-download.
`LlmLingua2Scorer::from_files` loads an already-resolved local `model.onnx` + `tokenizer.json`
pair; `LlmLingua2Scorer::load` deliberately returns an error so any caller reaching it falls
back to `UniformScorer`. The `optimize-model` CLI fetches `<MODEL_URL>/model.onnx` +
`tokenizer.json`, verifies the `.onnx` against `MODEL_SHA256`, and caches the pair under
`MODEL_CACHE_DIR/<sha256>/`. `LLMLingua2Pass` is a thin named wrapper for "the ML scorer wired
for real use"; it needs no frontier logic of its own because `optimize()` only ever scores
behind-frontier, non-Immutable `Text`/`ToolResult` buffers already.

---

## 7. Tool-result compression (optimize-passes)

The single biggest real-world saving. `compress_tool_result(raw, policy, scorer, ws)` never
touches JSON *structure*; it compresses long string *values* through the §5 pipeline:

- parse `raw` with `serde_json`; on parse error, treat the whole thing as one text buffer;
- for each `Value::String` leaf longer than `min_len`: run the §5 text pipeline on the decoded
  string (`ratio = tool_result_value`), re-encoded on serialize;
- keys, numbers, bools, and container structure stay byte-identical (invariant I6);
- if a buffer (the whole non-JSON string, or an individual JSON String leaf) still exceeds
  `tool_result_max_tokens` after word-level compression, **structural truncation**: keep the
  head 60% + tail 20% by char count, drop the middle, joined with a deterministic marker
  `\n…[anyllm-optimizer: {N} tokens elided]…\n`. Splits only on char boundaries (I6). Applied
  per-leaf for JSON so structure is never touched.

`optimize-passes::compress_message` delegates Text blocks to core's `compress_message`, then
adds one whole-buffer `Edit::Replace` per shrunk `ToolResult` block, keeping the same
`Vec<(BufferId, EditScript)>` shape and the same per-buffer fail-open contract. ToolUse args are
Immutable — the model produced them and may replay them; a byte change can break downstream tool
execution or provider-side validation.

---

## 8. The cost gate (optimize-core, re-exported from optimize-passes)

```rust
pub struct Pricing { pub input: f64, pub cached_read: f64, pub cache_write_mult: f64 } // $/Mtok
pub enum CacheModel { ExplicitBreakpoints, ImplicitPrefix }

pub fn should_apply(dt: u64, s: u64, h: u64, p: &Pricing, m: &CacheModel) -> bool
pub fn net_cost_delta_usd(dt: u64, s: u64, h: u64, p: &Pricing, m: &CacheModel) -> f64
```

With ΔT = tokens removed in newly transitioned messages this turn, S = original tokens in the
frozen zone, H = horizon (expected remaining turns, default 8):

- `ExplicitBreakpoints`: the recent zone is never cached, so a transition never invalidates —
  apply ⇔ `ΔT > 0`.
- `ImplicitPrefix`: a frontier advance rewrites the suffix once — apply ⇔
  `(S − ΔT)·input·write_mult < H·ΔT·cached_read` (rewrite cost < reads saved), and never when
  `ΔT = 0`.

`net_cost_delta_usd` is the same inequality rearranged to a signed dollar figure (positive ⇒
compression saves money); its sign always agrees with `should_apply`. The orchestrator reports
exactly `0.0` when `ΔT = 0` (no edits ⇒ nothing changes either way) rather than the raw
formula's rewrite-cost artifact.

Worked intuition: OpenAI (input 2.5, read 1.25, write 1.0), 30% removal, H=8 → apply; the same
at H=2 → skip (rewriting a cached suffix for a dying conversation loses money). Anthropic
(explicit breakpoints) → always apply on any removal.

Pricing lives in `optimize-passes::cost_gate` as per-provider placeholder tables
(`openai_pricing`/`anthropic_pricing`/`gemini_pricing`), surfaced via each `CacheStrategy`.
Because those are placeholders that change, `Pricing::from_config_str` parses a dependency-free
`key=value` table (`optimize-core` has no serde) so pricing can be versioned in config; a
`Policy.pricing_override` (or per-route override) then wins over the strategy's own table.
`DisabledStrategy` uses zero pricing so the gate always skips. The `BudgetCounter` for ΔT/S is
approximate by design (`HeuristicBudgetCounter`, ~bytes/3.6); the CLI harness can swap a
tiktoken counter for reporting. Report all counts as estimates.

`CacheStrategy` (impls in passes) is `{ pricing(), model(), breakpoint_at(frontier) }`.
Anthropic returns `Some(frontier)` (deepest breakpoint at the frontier); OpenAI/Gemini return
`None` (implicit prefix, no breakpoint).

---

## 9. Orchestrator (entry point in optimize-core)

```rust
pub enum Mode { Off, Shadow, Live }
pub struct OptimizeOutcome { pub rendered: Option<RenderedConversation>, pub report: OptimizationReport }

pub fn optimize(conv, policy: &Policy, strategy: &dyn CacheStrategy,
                scorer: &dyn TokenScorer, budget: &dyn BudgetCounter, ws) -> OptimizeOutcome
pub fn optimize_for_route(conv, opt_policy: &OptimizationPolicy, route: &str, ...) -> OptimizeOutcome
```

`optimize()` wraps `run_inner` in `catch_unwind`; on any `Err` or panic it returns
`rendered: None` plus a `failed_open` report. `run_inner`:

1. `f = frontier(n)`. Start a wall-clock `Deadline` from `policy.compression.deadline`.
2. For each eligible message oldest-first (skip Immutable / `client_cache_marker`; on deadline
   expiry, count it as `messages_skipped_deadline` and leave it verbatim): optionally plan its
   ratio (§5.6), call `compress_message`, validate each returned script against the live
   buffer, and collect the valid ones.
3. Estimate S (frozen-zone tokens via the BudgetCounter) and ΔT (sum of `count(orig) −
   count(applied)` over the collected scripts).
4. Resolve pricing (`policy.pricing_override` else `strategy.pricing()`), then
   `apply = !edits.is_empty() && should_apply(ΔT, S, horizon, pricing, strategy.model())`.
5. Build the `OptimizationReport` (see below). If `mode != Live` or `!apply`, return
   `rendered: None`. Otherwise `render(conv, edits, strategy.breakpoint_at(f))` and return it.

`optimize_for_route` resolves an `OptimizationPolicy` down to a per-route `Policy` first (see
§9.1), then calls `optimize`. Determinism auditing: `decisions_hash` is a `DefaultHasher` fold
of every edit (kind + ranges + replacement text), stable across runs/threads/machines.

`OptimizationReport` (emitted for every request, shadow or live): `mode`, `applied`, `frontier`,
`input_tokens_est`, `output_tokens_est`, `removed_tokens_est` (ΔT), `rewrite_suffix_tokens` (S),
`est_cost_delta_usd`, `scorer_ms` (currently always 0 — timing not yet plumbed),
`messages_compressed`, `messages_skipped_deadline`, `decisions_hash`, `policy_version`,
`failure`.

### 9.1 Per-route policy

`OptimizationPolicy { mode, frontier, ratios, pricing, routes: HashMap<String, RouteOverride> }`
is the config-facing shape the proxy binds to. `resolve(route)` precedence: a per-route override
field (`mode`, `ratios`, `pricing`) wins when present; any `None` field, and any route not in
`routes`, falls back to the top-level default. This lets one route class that failed the ROI
gate be turned `Off` while others keep running. Only `mode`/`ratios`/`pricing` are
route-overridable today; other `CompressionPolicy` fields come from `CompressionPolicy::default`.

---

## 10. Invariants (property-tested — `optimize-core/tests/invariants.rs`, unit tests, `optimize-passes/tests/pipeline.rs`)

- **I1 fail-open** — corrupt anything → output ≡ input (asserted at the proxy layer too).
- **I2 determinism** — `optimize(x)` yields the same `decisions_hash` across threads and runs.
- **I3 frozen stability** — extending a conversation never changes the bytes of messages already
  behind the frontier.
- **I4 monotone frontier** — `frontier(n+1) >= frontier(n)`.
- **I5 protected bytes identical** — system / latest / client-marked / ToolUse / Opaque unchanged.
- **I6 validity** — output is UTF-8; fences balanced iff input balanced; every ToolResult that
  parsed as JSON still parses with identical structure (keys, arity, non-string leaves
  byte-identical).
- **I7 extractive** — every non-marker output word occurs in the input in the same order
  (subsequence check).
- **I8 ratio honesty** — kept words ≥ forced count and ≤ `ceil(ratio·n)` + forced.

Write invariant tests before adding a scorer.

---

## 11. What NOT to build

- No causal-LM / perplexity scoring (LLMLingua-1's fine stage) — wrong latency class.
- No global ratio across messages — breaks I3, the core invariant.
- No paraphrase/abstractive rewriting — breaks I7 and the paper's faithfulness rules.
- No conversation state store / DB — statelessness is the design, not a limitation.
- No tokenizer perfectionism for budgets — estimates are fine; only the scorer's own tokenizer
  must be exact.
- No compression of system prompts, tool schemas, ToolUse args, thinking blocks, the latest
  user message, or anything unrecognized.

---

## 12. Implementation status

All milestones M0–M5 and the proxy integration are implemented; `cargo test --workspace`
(default features) is green. The `onnx` scorer is opt-in and its parity gate (keep-set F1 ≥ 0.9
vs Python LLMLingua-2, `optimize-scorer/tests/parity.rs`) is a live check needing the downloaded
artifact via `ANYLLM_OPTIMIZER_TEST_MODEL_DIR`. See the git history for the milestone history,
`fixtures/roi_results.md` for the per-route ROI analysis, and `../CLAUDE.md` for the contributor contract.

Known gaps to keep this doc honest:
- `dedup_pass`, `normalize_pass`, and tool-result value compression are implemented and tested
  but not yet composed into `optimize()` (§1, §5.7, §7).
- `OptimizationReport.scorer_ms` is always 0 (timing not plumbed).
- The request-path `est_cost_delta_usd` uses the heuristic `BudgetCounter`; the tiktoken counter
  is harness-only.
