//! M3.5 parity suite (ALGO §12): checks `LlmLingua2Scorer`'s word-importance ranking
//! against the recorded Python LLMLingua-2 reference outputs in
//! `crates/optimizer/fixtures/parity/` (see `fixtures/README.md` for the fixture
//! format and category list, and `scripts/gen_parity_fixtures.py` to regenerate).
//!
//! Deliberately NOT `#[ignore]`: this is the M3.5 acceptance check
//! (`cargo test -p anyllm_optimize_scorer --features onnx` runs it, no `#[ignore]`).
//! It requires a real exported ONNX artifact — set `ANYLLM_OPTIMIZER_TEST_MODEL_DIR`
//! to a directory containing `model.onnx` + `tokenizer.json`
//! (`crates/optimizer/scripts/export_llmlingua2.py`). Without that artifact the test
//! panics with instructions rather than silently passing or skipping — a missing
//! artifact must never masquerade as a passing parity check.
//!
//! Scope: this isolates the SCORER's ranking quality against the Python reference
//! (ALGO §12 M3.5: "keep-set F1 ≥ 0.9 vs Python LLMLingua-2 at matched ratios"). It
//! reuses the fixture's own reference word list directly (not the Rust structural
//! segmenter) and mirrors `optimize_core::select_keep`'s deterministic ranking rule
//! (quantized score desc, position asc) without the pipeline's structural force-keep
//! rules, per the tokenization-mismatch caveat in `fixtures/README.md`.

#![cfg(feature = "onnx")]

use std::path::{Path, PathBuf};

use anyllm_optimize_core::{quantize, TokenScorer, Workspace};
use anyllm_optimize_scorer::LlmLingua2Scorer;

/// Minimum keep-set F1 across every fixture (ALGO §12 M3.5).
const MIN_F1: f32 = 0.9;
/// Max allowed absolute deviation of achieved keep ratio from the fixture's ratio.
const RATIO_TOLERANCE: f32 = 0.05;

struct Fixture {
    name: String,
    words: Vec<String>,
    keep_mask: Vec<bool>,
    ratio: f32,
}

fn fixtures_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/optimizer/crates/optimize-scorer
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/parity")
}

fn load_fixtures(dir: &Path) -> Vec<Fixture> {
    let mut out = Vec::new();
    let categories = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read fixtures dir {}: {e}", dir.display()));
    for category in categories {
        let category = category.expect("dir entry");
        if !category.file_type().expect("file type").is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(category.path())
            .unwrap_or_else(|e| panic!("read category dir {}: {e}", category.path().display()));
        for entry in entries {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let raw = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let v: serde_json::Value = serde_json::from_str(&raw)
                .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
            let words: Vec<String> = v["words"]
                .as_array()
                .unwrap_or_else(|| panic!("{}: missing `words` array", path.display()))
                .iter()
                .map(|w| {
                    w.as_str()
                        .unwrap_or_else(|| panic!("{}: `words` entry not a string", path.display()))
                        .to_string()
                })
                .collect();
            let keep_mask: Vec<bool> = v["keep_mask"]
                .as_array()
                .unwrap_or_else(|| panic!("{}: missing `keep_mask` array", path.display()))
                .iter()
                .map(|b| {
                    b.as_bool().unwrap_or_else(|| {
                        panic!("{}: `keep_mask` entry not a bool", path.display())
                    })
                })
                .collect();
            let ratio = v["ratio"]
                .as_f64()
                .unwrap_or_else(|| panic!("{}: missing/invalid `ratio`", path.display()))
                as f32;
            assert_eq!(
                words.len(),
                keep_mask.len(),
                "{}: words/keep_mask length mismatch",
                path.display()
            );
            out.push(Fixture {
                name: format!(
                    "{}/{}",
                    category.file_name().to_string_lossy(),
                    path.file_name().unwrap().to_string_lossy()
                ),
                words,
                keep_mask,
                ratio,
            });
        }
    }
    assert!(
        !out.is_empty(),
        "no parity fixtures found under {}",
        dir.display()
    );
    out
}

/// Deterministic top-k selection mirroring `select_keep`'s ranking rule (quantized
/// score desc, position asc) — see module docs for why this test does not call
/// `select_keep` directly (byte-range/force-keep pipeline concerns, not scorer parity).
fn top_k_keep(scores: &[f32], ratio: f32) -> Vec<bool> {
    let n = scores.len();
    let n_keep = ((n as f32 * ratio).ceil() as usize).min(n);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        quantize(scores[b])
            .cmp(&quantize(scores[a]))
            .then(a.cmp(&b))
    });
    let mut keep = vec![false; n];
    for &i in order.iter().take(n_keep) {
        keep[i] = true;
    }
    keep
}

/// Set-F1 between the predicted and reference keep-masks (both aligned 1:1 to the
/// same fixture word list).
fn keep_set_f1(predicted: &[bool], reference: &[bool]) -> f32 {
    assert_eq!(predicted.len(), reference.len());
    let mut tp = 0usize;
    let mut fp = 0usize;
    let mut fn_ = 0usize;
    for (&p, &r) in predicted.iter().zip(reference.iter()) {
        match (p, r) {
            (true, true) => tp += 1,
            (true, false) => fp += 1,
            (false, true) => fn_ += 1,
            (false, false) => {}
        }
    }
    if tp == 0 {
        return if fp == 0 && fn_ == 0 { 1.0 } else { 0.0 };
    }
    let precision = tp as f32 / (tp + fp) as f32;
    let recall = tp as f32 / (tp + fn_) as f32;
    2.0 * precision * recall / (precision + recall)
}

#[test]
fn llmlingua2_scorer_matches_python_reference_parity() {
    let model_dir = std::env::var("ANYLLM_OPTIMIZER_TEST_MODEL_DIR").unwrap_or_else(|_| {
        panic!(
            "ANYLLM_OPTIMIZER_TEST_MODEL_DIR must point to a directory containing \
             model.onnx + tokenizer.json (see crates/optimizer/scripts/export_llmlingua2.py). \
             This parity test is intentionally not #[ignore] (ALGO §12 M3.5 acceptance check) \
             so a missing artifact fails loudly instead of silently skipping."
        )
    });
    let scorer = LlmLingua2Scorer::from_files(
        Path::new(&model_dir).join("model.onnx"),
        Path::new(&model_dir).join("tokenizer.json"),
        0,
    )
    .expect("load LlmLingua2Scorer from ANYLLM_OPTIMIZER_TEST_MODEL_DIR");

    let fixtures = load_fixtures(&fixtures_dir());

    let mut failures = Vec::new();
    for fx in &fixtures {
        let words: Vec<&str> = fx.words.iter().map(String::as_str).collect();
        let mut ws = Workspace::new();
        let scores = scorer
            .score_words(&words, &mut ws)
            .unwrap_or_else(|e| panic!("{}: score_words failed: {e}", fx.name));

        let predicted = top_k_keep(&scores, fx.ratio);
        let f1 = keep_set_f1(&predicted, &fx.keep_mask);
        let achieved_ratio =
            predicted.iter().filter(|&&k| k).count() as f32 / fx.words.len() as f32;
        let ratio_dev = (achieved_ratio - fx.ratio).abs();

        if f1 < MIN_F1 {
            failures.push(format!("{}: keep-set F1 {f1:.4} < {MIN_F1}", fx.name));
        }
        if ratio_dev > RATIO_TOLERANCE {
            failures.push(format!(
                "{}: achieved ratio {achieved_ratio:.4} deviates from reference ratio \
                 {:.4} by more than {RATIO_TOLERANCE}",
                fx.name, fx.ratio
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "parity check failed for {} of {} fixture(s):\n{}",
        failures.len(),
        fixtures.len(),
        failures.join("\n")
    );
}
