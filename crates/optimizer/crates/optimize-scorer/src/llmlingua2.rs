//! LLMLingua-2 ONNX scorer (feature = "onnx"), implemented per ALGO §6.
//!
//! Model: `microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank` (~110M),
//! exported to ONNX and int8-quantized offline (`scripts/export_llmlingua2.py`), shipped
//! as a hash-pinned artifact — never a bundled blob. The exported graph is a mBERT
//! token-classification head: input `input_ids`/`attention_mask`/`token_type_ids`
//! (`[batch, seq]`, int64), output `logits` (`[batch, seq, 2]`, f32). Class index 1 is
//! "preserve" per the reference LLMLingua-2 implementation's label convention; the
//! parity suite (ALGO §12 M3.5, `tests/parity.rs`) is the check that pins this down
//! against the real Python output rather than trusting this comment.
//!
//! Scoring contract (ALGO §6):
//!  - encode WHOLE WORDS (each word tokenized independently, no special tokens) so a
//!    word's subtokens can never straddle a chunk boundary and word<->subtoken
//!    alignment stays exact — no truncation of words, ever;
//!  - greedy-pack words into chunks of <= `max_seq - 2` subtokens (room for CLS/SEP);
//!    prefer breaking right after a word ending in '.' when within
//!    [`PERIOD_BREAK_LOOKBACK`] words of the hard boundary (the paper chunks on
//!    periods where possible);
//!  - word score = MEAN of its subtokens' `softmax(logits)[preserve]`; CLS/SEP
//!    positions are skipped (never attributed to any word);
//!  - chunks are independent and **position-addressed**: each chunk's output is written
//!    into its own `[start, end)` slice of the result vector, so scoring chunks via
//!    `rayon` (or not) can never change the result — determinism invariant #2 in
//!    `CLAUDE.md`.
//!
//! ## Artifact delivery: `MODEL_*` env/config, no bundled blob
//!
//! The quantized `.onnx` + tokenizer files are produced offline by
//! `crates/optimizer/scripts/export_llmlingua2.py` and are NEVER committed to this repo
//! (no `*.onnx` blob in git; see the export script's own header for the export/upload
//! steps). [`LlmLingua2Scorer::from_files`] loads an already-resolved local pair.
//!
//! The download+verify+cache step is an EXPLICIT operator action, not an in-process
//! auto-download: run the `optimize-model` CLI (`optimize-cli/src/model_fetch.rs`), which
//! fetches `<MODEL_URL>/model.onnx` + `tokenizer.json`, checks the `.onnx` against
//! `MODEL_SHA256`, and drops the verified pair into `MODEL_CACHE_DIR/<sha256>/`. Point
//! `from_files` at that dir. Nothing here downloads a model just because the feature
//! compiled in; a bad/missing artifact must fall back to `UniformScorer` per the
//! fail-open invariant, never panic, never trust an unverified file.
//!
//!  - `MODEL_URL` — base URL the artifact (and its `tokenizer.json`) is fetched from.
//!  - `MODEL_SHA256` — the sha256 hex digest the downloaded `.onnx` file MUST match,
//!    printed by the export script as `MODEL_SHA256=<hex>`. This is the same value that
//!    becomes `artifact_hash()` (folded into `PolicyVersion`).
//!  - `MODEL_CACHE_DIR` — local cache directory, keyed by `MODEL_SHA256`.

use std::path::Path;
use std::sync::Mutex;

use anyllm_optimize_core::{ScoreError, TokenScorer, Workspace};
use ort::session::Session;
use ort::value::Tensor;
use rayon::prelude::*;
use tokenizers::Tokenizer;

/// Fallback special-token ids for BERT-family vocabs if the tokenizer's own vocab
/// lookup somehow fails to resolve `[CLS]`/`[SEP]` (defensive only; the exported
/// tokenizer always defines these).
const FALLBACK_CLS_ID: u32 = 101;
const FALLBACK_SEP_ID: u32 = 102;

/// Prefer breaking a chunk right after a '.'-ending word once within this many words of
/// the hard chunk boundary, rather than mid-sentence.
const PERIOD_BREAK_LOOKBACK: usize = 50;

/// LLMLingua-2 ONNX scorer.
pub struct LlmLingua2Scorer {
    /// `ort::Session::run` takes `&mut self`; ONNX Runtime's own `Run()` is internally
    /// thread-safe, but the Rust binding isn't, so concurrent chunk scoring serializes
    /// on this mutex. Chunks are still submitted via `rayon` and remain
    /// position-addressed (see module docs) — correctness never depends on the actual
    /// degree of concurrency achieved here. Revisit with a session pool (one per rayon
    /// thread) if profiling against the exit bar (ALGO §12 M3: p99 < 100ms/CPU) shows
    /// this mutex is the bottleneck.
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    /// CLS/SEP-included window size (512 → 510 usable).
    pub max_seq: usize,
    /// Pinned model artifact hash, folded into `PolicyVersion`.
    pub artifact_hash: u64,
}

impl LlmLingua2Scorer {
    /// Load a scorer from an already-resolved local ONNX file + `tokenizer.json` pair.
    /// `artifact_hash` should be derived from the verified artifact (e.g. a fold of its
    /// sha256, per the module docs) by the caller.
    pub fn from_files(
        onnx_path: impl AsRef<Path>,
        tokenizer_path: impl AsRef<Path>,
        artifact_hash: u64,
    ) -> Result<Self, ScoreError> {
        let session = Session::builder()
            .map_err(|e| ScoreError::Backend(format!("session builder: {e}")))?
            .commit_from_file(onnx_path)
            .map_err(|e| ScoreError::Backend(format!("commit_from_file: {e}")))?;
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| ScoreError::Backend(format!("tokenizer load: {e}")))?;
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            max_seq: 512,
            artifact_hash,
        })
    }

    /// In-process env-driven resolution is intentionally NOT provided: fetching is an
    /// explicit operator step via the `optimize-model` CLI (download + sha256 verify into
    /// `MODEL_CACHE_DIR/<sha>/`), after which callers use [`Self::from_files`]. This keeps
    /// "never auto-download a model" a structural guarantee, not a runtime policy. Returns
    /// an error so any caller that reaches here falls back to `UniformScorer` (fail-open).
    pub fn load(_model_path: &str) -> Result<Self, ScoreError> {
        Err(ScoreError::Backend(
            "LlmLingua2Scorer has no in-process auto-download; fetch + verify the artifact \
             with the `optimize-model` CLI, then use LlmLingua2Scorer::from_files"
                .into(),
        ))
    }

    fn score_words_impl(&self, words: &[&str], ws: &mut Workspace) -> Result<Vec<f32>, ScoreError> {
        if words.is_empty() {
            return Ok(Vec::new());
        }

        // 1. Subtokenize every word (no special tokens) into a shared flat scratch
        //    buffer, recording each word's subtoken range via `offsets`. This keeps
        //    allocation to O(1) buffers instead of one `Vec<u32>` per word.
        ws.ids.clear();
        let mut offsets: Vec<usize> = Vec::with_capacity(words.len() + 1);
        offsets.push(0);
        for word in words {
            let enc = self
                .tokenizer
                .encode(*word, false)
                .map_err(|e| ScoreError::Backend(format!("tokenize word: {e}")))?;
            ws.ids.extend(enc.get_ids().iter().copied());
            offsets.push(ws.ids.len());
        }

        // 2. Greedy-pack whole words into chunks of <= (max_seq - 2) subtokens.
        let budget = self.max_seq.saturating_sub(2).max(1);
        let chunks = pack_chunks(words, &offsets, budget);

        // 3. Score chunks (rayon-parallel, position-addressed — see module docs).
        let cls = self
            .tokenizer
            .token_to_id("[CLS]")
            .unwrap_or(FALLBACK_CLS_ID);
        let sep = self
            .tokenizer
            .token_to_id("[SEP]")
            .unwrap_or(FALLBACK_SEP_ID);
        let ids: &[u32] = &ws.ids;

        let per_chunk: Vec<(usize, Vec<f32>)> = chunks
            .par_iter()
            .map(|chunk| -> Result<(usize, Vec<f32>), ScoreError> {
                let word_scores = self.score_chunk(chunk, ids, &offsets, cls, sep)?;
                Ok((chunk.start, word_scores))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut scores = vec![0.0f32; words.len()];
        for (start, word_scores) in per_chunk {
            scores[start..start + word_scores.len()].copy_from_slice(&word_scores);
        }
        Ok(scores)
    }

    /// Runs one chunk through the ONNX session and mean-pools subtoken `p_preserve`
    /// into one score per word in `chunk`.
    fn score_chunk(
        &self,
        chunk: &Chunk,
        ids: &[u32],
        offsets: &[usize],
        cls: u32,
        sep: u32,
    ) -> Result<Vec<f32>, ScoreError> {
        let sub_start = offsets[chunk.start];
        let sub_end = offsets[chunk.end];
        let seq_len = sub_end - sub_start + 2; // + CLS + SEP

        let mut input_ids: Vec<i64> = Vec::with_capacity(seq_len);
        input_ids.push(cls as i64);
        input_ids.extend(ids[sub_start..sub_end].iter().map(|&id| id as i64));
        input_ids.push(sep as i64);
        let attention_mask: Vec<i64> = vec![1; seq_len];
        let token_type_ids: Vec<i64> = vec![0; seq_len];

        let input_ids_t = Tensor::from_array(([1usize, seq_len], input_ids))
            .map_err(|e| ScoreError::Backend(format!("input_ids tensor: {e}")))?;
        let attention_mask_t = Tensor::from_array(([1usize, seq_len], attention_mask))
            .map_err(|e| ScoreError::Backend(format!("attention_mask tensor: {e}")))?;
        let token_type_ids_t = Tensor::from_array(([1usize, seq_len], token_type_ids))
            .map_err(|e| ScoreError::Backend(format!("token_type_ids tensor: {e}")))?;

        let mut session = self
            .session
            .lock()
            .map_err(|_| ScoreError::Backend("scorer session mutex poisoned".into()))?;
        let outputs = session
            .run(ort::inputs![
                "input_ids" => input_ids_t,
                "attention_mask" => attention_mask_t,
                "token_type_ids" => token_type_ids_t,
            ])
            .map_err(|e| ScoreError::Backend(format!("session run: {e}")))?;
        let logits_value = outputs
            .get("logits")
            .ok_or_else(|| ScoreError::Backend("model output missing `logits`".into()))?;
        let (_shape, logits) = logits_value
            .try_extract_tensor::<f32>()
            .map_err(|e| ScoreError::Backend(format!("extract logits: {e}")))?;

        pool_word_scores(chunk, offsets, sub_start, logits)
    }
}

impl TokenScorer for LlmLingua2Scorer {
    fn score_words(&self, words: &[&str], ws: &mut Workspace) -> Result<Vec<f32>, ScoreError> {
        self.score_words_impl(words, ws)
    }

    fn artifact_hash(&self) -> u64 {
        self.artifact_hash
    }
}

/// The production integration point for the real ML scorer (ROADMAP D8 / TASKS M3.6):
/// "`LLMLingua2Pass` applies only to messages behind the frontier, per D8 targets."
///
/// This is a thin, explicitly-named wrapper around [`LlmLingua2Scorer`] so callers that
/// assemble the real pipeline (the CLI harness today, the proxy integration later) have
/// one unambiguous symbol for "the ML scorer, wired for real use" instead of reaching
/// for [`anyllm_optimize_core::UniformScorer`] by default. It needs no frontier/target
/// logic of its own: `optimize()`/`optimize_for_route()`
/// (`anyllm_optimize_core::orchestrator`) already only ever invoke a `TokenScorer` for
/// messages with index `< frontier(n, policy)` (excluding `Protection::Immutable` /
/// `client_cache_marker` messages — i.e. system, the latest turn, and anything the
/// client itself cache-pinned), and `compress_message` only ever scores `Text` and
/// `ToolResult` buffers (`Message::buffer`) — never `ToolUse` args or `Opaque` blocks.
/// Those two structural facts together already ARE the D8 target list: tool results,
/// old RAG/pasted-context blocks in old user messages, and old assistant messages. So
/// passing `&LLMLingua2Pass` where `&UniformScorer` was used before is the whole wiring:
/// the gating is inherited, not reimplemented.
pub struct LLMLingua2Pass {
    scorer: LlmLingua2Scorer,
}

impl LLMLingua2Pass {
    /// Wrap an already-loaded [`LlmLingua2Scorer`].
    pub fn new(scorer: LlmLingua2Scorer) -> Self {
        Self { scorer }
    }

    /// Load from an already-resolved local ONNX file + `tokenizer.json` pair. See
    /// [`LlmLingua2Scorer::from_files`] for the artifact-hash contract.
    pub fn from_files(
        onnx_path: impl AsRef<Path>,
        tokenizer_path: impl AsRef<Path>,
        artifact_hash: u64,
    ) -> Result<Self, ScoreError> {
        Ok(Self::new(LlmLingua2Scorer::from_files(
            onnx_path,
            tokenizer_path,
            artifact_hash,
        )?))
    }
}

impl TokenScorer for LLMLingua2Pass {
    fn score_words(&self, words: &[&str], ws: &mut Workspace) -> Result<Vec<f32>, ScoreError> {
        self.scorer.score_words(words, ws)
    }

    fn artifact_hash(&self) -> u64 {
        self.scorer.artifact_hash()
    }
}

/// One chunk: word index range `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Chunk {
    start: usize,
    end: usize,
}

/// Greedy-packs `words` into chunks of at most `budget` subtokens each (per
/// `offsets`), never splitting a word across chunks, preferring to break right after a
/// '.'-ending word within [`PERIOD_BREAK_LOOKBACK`] words of the hard boundary.
///
/// `offsets[i]..offsets[i + 1]` is word `i`'s subtoken range; `offsets.len() ==
/// words.len() + 1`.
fn pack_chunks(words: &[&str], offsets: &[usize], budget: usize) -> Vec<Chunk> {
    let n = words.len();
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < n {
        // Max `end` (exclusive) such that words[start..end] fit within `budget`
        // subtokens. Always include at least one word, even if it alone exceeds
        // `budget` — words are never split or dropped.
        let mut end = start + 1;
        while end < n && offsets[end + 1] - offsets[start] <= budget {
            end += 1;
        }

        // Prefer breaking right after a '.'-ending word near the hard boundary. `j` is
        // a candidate `break_at`; word `j - 1` is the last word included if we break
        // there. The range is inclusive of `end` itself so the natural greedy boundary
        // is checked first (and kept, with no need to shrink the chunk) when it
        // already lands right after a period.
        let lookback_floor = end.saturating_sub(PERIOD_BREAK_LOOKBACK).max(start + 1);
        let mut break_at = end;
        for j in (lookback_floor..=end).rev() {
            if words[j - 1].ends_with('.') {
                break_at = j;
                break;
            }
        }

        chunks.push(Chunk {
            start,
            end: break_at,
        });
        start = break_at;
    }
    chunks
}

/// Softmaxes `logits` (`[chunk_seq_len, 2]`, row-major, CLS/SEP included) into
/// per-position `p_preserve` (class index 1), skips the CLS/SEP positions, and mean-
/// pools each word's subtoken positions into one score per word in `chunk`.
fn pool_word_scores(
    chunk: &Chunk,
    offsets: &[usize],
    sub_start: usize,
    logits: &[f32],
) -> Result<Vec<f32>, ScoreError> {
    let num_positions = logits.len() / 2;
    if logits.len() != num_positions * 2 {
        return Err(ScoreError::Backend(
            "logits tensor last dim is not 2 (expected binary preserve/discard head)".into(),
        ));
    }
    let mut p_preserve = vec![0.0f32; num_positions];
    for i in 0..num_positions {
        let l0 = logits[i * 2];
        let l1 = logits[i * 2 + 1];
        let m = l0.max(l1);
        let e0 = (l0 - m).exp();
        let e1 = (l1 - m).exp();
        p_preserve[i] = e1 / (e0 + e1);
    }

    let mut out = Vec::with_capacity(chunk.end - chunk.start);
    for j in chunk.start..chunk.end {
        // +1 shifts past the leading CLS position; skip CLS/SEP entirely (never
        // attributed to any word).
        let local_start = offsets[j] - sub_start + 1;
        let local_end = offsets[j + 1] - sub_start + 1;
        if local_start >= local_end || local_end > p_preserve.len() {
            // A word that tokenized to zero subtokens (shouldn't happen for the mBERT
            // vocab) — fail open with a mid-importance score rather than panic.
            out.push(0.5);
            continue;
        }
        let sum: f32 = p_preserve[local_start..local_end].iter().sum();
        out.push(sum / (local_end - local_start) as f32);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_chunks_keeps_words_whole_and_within_budget() {
        let words: Vec<&str> = (0..20).map(|_| "word").collect();
        // "word" tokenizes to a fixed id count in this fake offsets table: 3 subtokens
        // each, so budget=10 forces multiple chunks.
        let mut offsets = vec![0usize];
        for _ in 0..words.len() {
            offsets.push(offsets.last().unwrap() + 3);
        }
        let chunks = pack_chunks(&words, &offsets, 10);
        // Every word covered exactly once, in order, no gaps.
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks.last().unwrap().end, words.len());
        for w in chunks.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        for c in &chunks {
            let subtokens = offsets[c.end] - offsets[c.start];
            assert!(subtokens <= 10, "chunk exceeds budget: {subtokens}");
            assert!(c.end > c.start, "chunk must contain at least one word");
        }
    }

    #[test]
    fn pack_chunks_prefers_breaking_after_period_near_boundary() {
        // 5 words per "sentence", each word 2 subtokens; budget=11 fits 5 words (10)
        // but not 6 (12). The period after word index 4 (0-based) should be preferred
        // over a mid-sentence hard break at the 5-word boundary.
        let words = ["a", "b.", "c", "d", "e.", "f", "g", "h", "i", "j."];
        let mut offsets = vec![0usize];
        for _ in 0..words.len() {
            offsets.push(offsets.last().unwrap() + 2);
        }
        let chunks = pack_chunks(&words, &offsets, 11);
        // First chunk should end right after "e." (index 5), not mid-run.
        assert_eq!(chunks[0].end, 5);
        assert_eq!(words[chunks[0].end - 1], "e.");
    }

    #[test]
    fn pack_chunks_never_splits_a_single_oversized_word() {
        // A word alone larger than the budget still gets its own chunk rather than
        // being split or dropped.
        let words = ["hugeword", "a", "b"];
        let offsets = vec![0usize, 50, 51, 52];
        let chunks = pack_chunks(&words, &offsets, 10);
        assert_eq!(chunks[0], Chunk { start: 0, end: 1 });
    }

    #[test]
    fn pool_word_scores_skips_cls_sep_and_means_subtokens() {
        // chunk covers words [0, 2); word 0 has 1 subtoken, word 1 has 2 subtokens.
        // logits positions: [CLS, w0, w1a, w1b, SEP] -> 5 positions, 2 classes each.
        let chunk = Chunk { start: 0, end: 2 };
        let offsets = [0usize, 1, 3];
        // class1 - class0 chosen so p_preserve is easy to reason about:
        // CLS: irrelevant (skipped); w0: p=1.0; w1a: p=0.0; w1b: p=1.0; SEP: irrelevant.
        let logits: Vec<f32> = vec![
            0.0, 0.0, // CLS (skipped)
            -10.0, 10.0, // w0 -> p_preserve ~= 1.0
            10.0, -10.0, // w1a -> p_preserve ~= 0.0
            -10.0, 10.0, // w1b -> p_preserve ~= 1.0
            0.0, 0.0, // SEP (skipped)
        ];
        let scores = pool_word_scores(&chunk, &offsets, 0, &logits).unwrap();
        assert_eq!(scores.len(), 2);
        assert!((scores[0] - 1.0).abs() < 1e-3, "word0: {}", scores[0]);
        assert!(
            (scores[1] - 0.5).abs() < 1e-3,
            "word1 mean(0,1): {}",
            scores[1]
        );
    }

    /// End-to-end determinism + inference test — requires a real exported artifact
    /// (see module docs / `scripts/export_llmlingua2.py`), which this sandbox cannot
    /// download. Set `ANYLLM_OPTIMIZER_TEST_MODEL_DIR` to a directory containing
    /// `model.onnx` + `tokenizer.json` to run it locally:
    ///   ANYLLM_OPTIMIZER_TEST_MODEL_DIR=/path/to/dir cargo test -p anyllm_optimize_scorer \
    ///     --features onnx -- --ignored rayon_determinism
    #[test]
    #[ignore = "requires a downloaded ONNX artifact; see doc comment"]
    fn rayon_determinism_matches_serial() {
        let dir = std::env::var("ANYLLM_OPTIMIZER_TEST_MODEL_DIR").expect(
            "set ANYLLM_OPTIMIZER_TEST_MODEL_DIR to a dir with model.onnx + tokenizer.json",
        );
        let scorer = LlmLingua2Scorer::from_files(
            Path::new(&dir).join("model.onnx"),
            Path::new(&dir).join("tokenizer.json"),
            0xA11EF,
        )
        .expect("load scorer from local artifact");

        let text = "The quick brown fox jumps over the lazy dog. ".repeat(40);
        let words: Vec<&str> = text.split_whitespace().collect();

        let serial_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        let parallel_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(8)
            .build()
            .unwrap();

        let mut ws1 = Workspace::new();
        let serial = serial_pool
            .install(|| scorer.score_words(&words, &mut ws1))
            .expect("serial score");

        let mut ws2 = Workspace::new();
        let parallel = parallel_pool
            .install(|| scorer.score_words(&words, &mut ws2))
            .expect("parallel score");

        assert_eq!(
            serial, parallel,
            "rayon parallelism changed scoring results"
        );
    }
}
