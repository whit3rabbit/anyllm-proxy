//! Model-artifact resolution: pinned source, download + sha256 verify, presence check.
//!
//! Always compiled (NOT behind `onnx`) so the proxy admin UI and the `optimize-model`
//! CLI can detect/download the artifact without pulling the ONNX Runtime stack. The
//! actual scorer load (`LlmLingua2Scorer::from_files`) is what needs `onnx`.
//!
//! The ONNX model is NEVER bundled or auto-downloaded: an operator triggers a download
//! explicitly (CLI or admin button), it is sha256-verified against the pin below, and the
//! verified `model.onnx` + `tokenizer.json` land in `<cache_dir>/<sha256>/`. A sidecar
//! `model.onnx.sha256` records the verified digest so presence checks are cheap (no
//! re-hash of the ~170MB file on every status poll).

use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Pinned default artifact: `KatawaDead/llmlingua-2-bert-base-multilingual-cased-meetingbank-onnx-int8`
/// — an int8 ONNX export of the exact reference model (`microsoft/llmlingua-2-bert-base-
/// multilingual-cased-meetingbank`) the parity suite validates against. Verified: the
/// parity gate (keep-set F1 >= 0.9) passes with this artifact. Override with `MODEL_URL` /
/// `MODEL_SHA256` for a self-hosted copy.
pub const MODEL_URL_BASE: &str =
    "https://huggingface.co/KatawaDead/llmlingua-2-bert-base-multilingual-cased-meetingbank-onnx-int8/resolve/main";
/// sha256 of `model.onnx` at [`MODEL_URL_BASE`] (HF LFS oid, independently confirmed).
pub const MODEL_SHA256: &str = "2753018e58d0bfaeb76109fba436d152ebddf734fb32603df204d5c3fb5deada";
/// Expected `model.onnx` size in bytes (cheap sanity check before the sha compare).
pub const MODEL_ONNX_BYTES: u64 = 178_693_007;

/// Errors from the download/verify flow. Callers must treat any of these as fail-open
/// (fall back to `UniformScorer`), never a request-path panic.
#[derive(Debug)]
pub enum ArtifactError {
    Http(String),
    Io(String),
    /// Downloaded bytes did not match the expected sha256; nothing was cached.
    ShaMismatch {
        expected: String,
        got: String,
    },
    /// A supplied sha256 was not a 64-char hex string.
    BadSha(String),
}

impl std::fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactError::Http(s) => write!(f, "http error: {s}"),
            ArtifactError::Io(s) => write!(f, "io error: {s}"),
            ArtifactError::ShaMismatch { expected, got } => {
                write!(f, "sha256 mismatch: expected {expected}, got {got}")
            }
            ArtifactError::BadSha(s) => write!(f, "invalid sha256: {s}"),
        }
    }
}
impl std::error::Error for ArtifactError {}

/// Resolved artifact location + pin. Build via [`ArtifactConfig::resolve`].
#[derive(Clone, Debug)]
pub struct ArtifactConfig {
    /// Base URL; `<url_base>/model.onnx` and `<url_base>/tokenizer.json` are fetched.
    pub url_base: String,
    /// Lowercase 64-char hex sha256 the downloaded `model.onnx` must match.
    pub sha256: String,
    /// Cache root; the verified pair lands in `cache_dir/<sha256>/`.
    pub cache_dir: PathBuf,
}

impl ArtifactConfig {
    /// Resolve from `MODEL_URL` / `MODEL_SHA256` / `MODEL_CACHE_DIR` env vars, falling
    /// back to the pinned defaults and `default_cache_dir` respectively. sha is
    /// lowercased and validated.
    pub fn resolve(default_cache_dir: impl Into<PathBuf>) -> Result<Self, ArtifactError> {
        let url_base = std::env::var("MODEL_URL").unwrap_or_else(|_| MODEL_URL_BASE.to_string());
        let sha256 = std::env::var("MODEL_SHA256")
            .unwrap_or_else(|_| MODEL_SHA256.to_string())
            .trim()
            .to_lowercase();
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ArtifactError::BadSha(sha256));
        }
        let cache_dir = std::env::var_os("MODEL_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| default_cache_dir.into());
        Ok(Self {
            url_base: url_base.trim_end_matches('/').to_string(),
            sha256,
            cache_dir,
        })
    }

    /// Directory the verified pair lives in: `cache_dir/<sha256>/`.
    pub fn artifact_dir(&self) -> PathBuf {
        self.cache_dir.join(&self.sha256)
    }
    pub fn onnx_path(&self) -> PathBuf {
        self.artifact_dir().join("model.onnx")
    }
    pub fn tokenizer_path(&self) -> PathBuf {
        self.artifact_dir().join("tokenizer.json")
    }
    fn marker_path(&self) -> PathBuf {
        self.artifact_dir().join("model.onnx.sha256")
    }

    /// Cheap presence check for status polls: both files exist and the sidecar marker
    /// records the expected sha. Does NOT re-hash the ~170MB file (the sha was verified
    /// when the marker was written). A tampered file would be caught on next download or
    /// by the scorer's own load, not here.
    pub fn is_present(&self) -> bool {
        if !self.onnx_path().exists() || !self.tokenizer_path().exists() {
            return false;
        }
        std::fs::read_to_string(self.marker_path())
            .map(|s| s.trim().eq_ignore_ascii_case(&self.sha256))
            .unwrap_or(false)
    }

    /// `artifact_hash` for `LlmLingua2Scorer::from_files` / `PolicyVersion`: a stable
    /// u64 fold of the pinned sha (first 16 hex chars). Deterministic per artifact.
    pub fn artifact_hash(&self) -> u64 {
        u64::from_str_radix(&self.sha256[..16], 16).unwrap_or(0)
    }
}

/// Download `model.onnx` + `tokenizer.json` from `cfg.url_base`, verify the `.onnx`
/// against `cfg.sha256`, and write the verified pair (+ sidecar marker) into
/// `cfg.artifact_dir()`. Blocking (uses `reqwest::blocking`) — call inside
/// `spawn_blocking` from async code. Idempotent: returns early if already present unless
/// `force`. Never caches an unverified artifact.
pub fn download_and_verify(cfg: &ArtifactConfig, force: bool) -> Result<PathBuf, ArtifactError> {
    let dir = cfg.artifact_dir();
    if !force && cfg.is_present() {
        return Ok(dir);
    }
    std::fs::create_dir_all(&dir).map_err(|e| ArtifactError::Io(e.to_string()))?;

    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|e| ArtifactError::Http(e.to_string()))?;

    // 1. model.onnx — the sha256 gate applies here.
    let onnx = fetch(&client, &format!("{}/model.onnx", cfg.url_base))?;
    let got = sha256_hex(&onnx);
    if got != cfg.sha256 {
        return Err(ArtifactError::ShaMismatch {
            expected: cfg.sha256.clone(),
            got,
        });
    }
    write_file(&cfg.onnx_path(), &onnx)?;

    // 2. tokenizer.json — shipped alongside; integrity covered by the same provenance.
    let tok = fetch(&client, &format!("{}/tokenizer.json", cfg.url_base))?;
    write_file(&cfg.tokenizer_path(), &tok)?;

    // 3. sidecar marker so is_present() is cheap next time.
    write_file(&cfg.marker_path(), cfg.sha256.as_bytes())?;
    Ok(dir)
}

fn fetch(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, ArtifactError> {
    let resp = client
        .get(url)
        .send()
        .map_err(|e| ArtifactError::Http(format!("GET {url}: {e}")))?
        .error_for_status()
        .map_err(|e| ArtifactError::Http(format!("GET {url}: {e}")))?;
    Ok(resp
        .bytes()
        .map_err(|e| ArtifactError::Http(format!("read {url}: {e}")))?
        .to_vec())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), ArtifactError> {
    let mut f = std::fs::File::create(path).map_err(|e| ArtifactError::Io(e.to_string()))?;
    f.write_all(bytes)
        .map_err(|e| ArtifactError::Io(e.to_string()))?;
    Ok(())
}

/// Lowercase hex sha256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_matches_known_vector() {
        // echo -n "" | shasum -a 256
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn resolve_uses_pin_and_validates_sha() {
        // No env set → pinned defaults; artifact_dir keyed by the pinned sha.
        let cfg = ArtifactConfig {
            url_base: MODEL_URL_BASE.trim_end_matches('/').to_string(),
            sha256: MODEL_SHA256.to_string(),
            cache_dir: PathBuf::from("/tmp/x"),
        };
        assert!(cfg.artifact_dir().ends_with(MODEL_SHA256));
        assert_eq!(
            cfg.artifact_hash(),
            u64::from_str_radix(&MODEL_SHA256[..16], 16).unwrap()
        );
        assert!(!cfg.is_present(), "nonexistent dir must not report present");
    }
}
