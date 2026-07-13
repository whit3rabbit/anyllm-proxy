//! `anyllm_optimize_scorer` — LLMLingua-2 token-importance scorer (ONNX-backed).
//!
//! Implements token-importance scoring (ALGO §6) using the LLMLingua-2 method
//! with an ONNX-exported bidirectional encoder (mBERT). Subwords are tokenized
//! and scored, then averaged back to word-level preserve probabilities.
//!
//! Enabled via the opt-in `onnx` feature. When `onnx` is disabled or a model artifact is
//! missing, callers fall back to [`anyllm_optimize_core::UniformScorer`], which is
//! re-exported here for convenience.

pub use anyllm_optimize_core::{TokenScorer, UniformScorer};

/// Model-artifact resolution (pin, download, sha256 verify, presence). Always available
/// so detect/download works without the `onnx` build.
pub mod artifact;

#[cfg(feature = "onnx")]
mod llmlingua2;
#[cfg(feature = "onnx")]
pub use llmlingua2::{LLMLingua2Pass, LlmLingua2Scorer};
