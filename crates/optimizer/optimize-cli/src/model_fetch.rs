//! `optimize-model` — fetch + sha256-verify the LLMLingua-2 ONNX artifact.
//!
//! Thin CLI over `anyllm_optimize_scorer::artifact` (the same download+verify the proxy
//! admin "Download model" button uses). Nothing runs during `cargo build` or at proxy
//! first-run; an operator invokes this ONCE to download `model.onnx` + `tokenizer.json`,
//! verify the `.onnx` against the pinned (or `MODEL_SHA256`-overridden) sha256, and drop
//! the verified pair into `<cache-dir>/<sha256>/`. `LlmLingua2Scorer::from_files` loads it.
//!
//! Defaults to the pinned artifact (`artifact::MODEL_URL_BASE` / `MODEL_SHA256`); flags and
//! `MODEL_URL` / `MODEL_SHA256` / `MODEL_CACHE_DIR` env vars override.
//!
//! ```text
//! optimize-model                                   # pinned default artifact
//! optimize-model --url https://host/x --sha256 <hex> --cache-dir ./artifacts
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result};
use anyllm_optimize_scorer::artifact::{self, ArtifactConfig};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "optimize-model",
    about = "Download + sha256-verify the LLMLingua-2 ONNX artifact (opt-in prerequisite \
             for the `onnx` scorer; never runs automatically)"
)]
struct Args {
    /// Base URL; `<url>/model.onnx` and `<url>/tokenizer.json` are downloaded. Defaults to
    /// `MODEL_URL`, else the pinned artifact.
    #[arg(long, env = "MODEL_URL", default_value = artifact::MODEL_URL_BASE)]
    url: String,

    /// Expected sha256 hex digest of `model.onnx`; the download is rejected on mismatch.
    /// Defaults to `MODEL_SHA256`, else the pinned digest.
    #[arg(long, env = "MODEL_SHA256", default_value = artifact::MODEL_SHA256)]
    sha256: String,

    /// Cache root; the verified pair lands in `<cache-dir>/<sha256>/`. Defaults to
    /// `MODEL_CACHE_DIR`, else `crates/optimizer/artifacts`.
    #[arg(
        long,
        env = "MODEL_CACHE_DIR",
        default_value = "crates/optimizer/artifacts"
    )]
    cache_dir: PathBuf,

    /// Re-download and re-verify even if a matching cached artifact already exists.
    #[arg(long)]
    force: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let sha256 = args.sha256.trim().to_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        anyhow::bail!(
            "--sha256 must be a 64-char hex digest, got {:?}",
            args.sha256
        );
    }
    let cfg = ArtifactConfig {
        url_base: args.url.trim_end_matches('/').to_string(),
        sha256,
        cache_dir: args.cache_dir,
    };

    if !args.force && cfg.is_present() {
        eprintln!(
            "cached artifact already verified at {}",
            cfg.artifact_dir().display()
        );
        println!("{}", cfg.artifact_dir().display());
        return Ok(());
    }

    eprintln!("downloading {}/model.onnx", cfg.url_base);
    let dir = artifact::download_and_verify(&cfg, args.force)
        .context("download + verify model artifact")?;
    eprintln!("verified model.onnx (sha256 {})", cfg.sha256);
    eprintln!("artifact ready at {}", dir.display());
    // stdout = the resolved dir, so callers can `MODEL_DIR=$(optimize-model ...)`.
    println!("{}", dir.display());
    Ok(())
}
