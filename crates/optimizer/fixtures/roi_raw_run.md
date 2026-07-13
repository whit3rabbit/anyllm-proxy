# ROI harness raw run log (M0.1 traces, live OpenRouter)

Captured for EH-0002 bite 1 (2026-07-12). Command used per class:

```
cargo run -p anyllm_optimize_cli --bin optimize-eval -- \
  --api openai \
  --input crates/optimizer/fixtures/samples/<class>.jsonl \
  --base-url https://openrouter.ai/api/v1 \
  --model "openai/gpt-oss-20b:free" \
  --judge-model "openai/gpt-oss-20b:free" \
  --max-tokens 200 \
  --api-key "$OPENROUTER_API_KEY"
```

`google/gemma-4-31b-it:free` was also tried as `--judge-model` for `rag_heavy` after the
`gpt-oss` judge failed. Other free-tier candidates
(`qwen/qwen3-next-80b-a3b-instruct:free`, `meta-llama/llama-3.2-3b-instruct:free`,
`cognitivecomputations/dolphin-mistral-24b-venice-edition:free`) all returned
`HTTP 429 temporarily rate-limited upstream` (Venice provider, shared free-tier pool) when
probed directly and were not usable.

## history_heavy

```
#      est_raw   est_cmp   est_%  prov_raw  prov_cmp  prov_% resp_sim  judge
0          998       801   19.7%       897       755  15.8%      29%    4/5
1          883       699   20.8%       860       713  17.1%      18%    4/5

TOTAL est raw=1881 comp=1500 saved=381 tokens (~$0.0010 input @ $2.5/Mtok)
QUALITY mean response similarity = 24% over 2 rows
JUDGE   mean equivalence score = 4.00/5 over 2 rows
```

## rag_heavy

```
#      est_raw   est_cmp   est_%  prov_raw  prov_cmp  prov_% resp_sim  judge
0         1519      1519    0.0%      1197      1197   0.0%      30-35%   - (judge failed)
1         1024      1024    0.0%       792     786-792   0.0-0.8%  9-41%  - (judge failed)

TOTAL est raw=2543 comp=2543 saved=0 tokens (~$0.0000 input @ $2.5/Mtok)
QUALITY mean response similarity = 22-35% over 2 rows (varies run to run; provider sampling
is not seeded and the two identical-body requests still draw different completions)
```

Judge failed on every attempt (`gpt-oss-20b:free` and `gemma-4-31b-it:free`): the CLI hardcodes
`max_tokens: 4` for the judge call (`run_judge` in `optimize-cli/src/main.rs`), and both models
returned `"content": null, "finish_reason": "length"` — they spend the 4-token budget without
emitting the digit. This is a harness limitation, not a quality signal; see verdict below.

## tool_heavy

```
#      est_raw   est_cmp   est_%  prov_raw  prov_cmp  prov_% resp_sim  judge
0          937       937    0.0%      1123      1123    0.0%     100%      - (judge failed)
1          987       987    0.0%       908       908    0.0%     100%      - (judge failed)

TOTAL est raw=1924 comp=1924 saved=0 tokens (~$0.0000 input @ $2.5/Mtok)
QUALITY mean response similarity = 100% over 2 rows
```

Judge also failed here (same `max_tokens: 4` issue), but is moot: raw and compressed bodies
are byte-identical (est_% = 0.0%, prov_% = 0.0%), so the two provider replies being 100%
Jaccard-similar is itself the quality proof — there is nothing to grade, no compression was
applied.

## Interpretation (raw savings only; net-cost-with-cache math is bite 2, not computed here)

- `history_heavy`: real, non-trivial local (~20%) and provider-reported (~16-17%) token
  savings, with judge score 4/5 (LLM-graded "close to equivalent") on both rows. The only
  class where FFEC's current STUB segmenter (Prose-span, ALGO §5.2 not yet built) found
  eligible extractive material at horizon=8.
- `rag_heavy` and `tool_heavy`: 0% raw savings — FFEC declines to compress at all (0 edits
  applied) for these two fixture files at the default policy/horizon. Per CLAUDE.md, the
  segmenter is still a STUB that "protects fenced/JSON buffers whole, else one Prose span";
  `rag_heavy`'s pasted context and `tool_heavy`'s tool-call JSON are exactly the content
  types the STUB is conservative about, plus cost-gate/frontier rules protect the latest
  message. Since raw savings are already 0%, these two classes cannot clear the ≥15% net
  bar regardless of cache math (bite 2) or judge score — they fail on the token-savings
  floor before quality or cost-gate accounting even enters.
