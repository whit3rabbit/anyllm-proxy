//! Observability. `OptimizationReport` is emitted for every request (shadow or live) so
//! a lossy transform stays auditable and reversible.

use crate::types::PolicyVersion;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Off,
    Shadow,
    Live,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Shadow => "shadow",
            Mode::Live => "live",
        }
    }
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "0" | "disabled" => Ok(Mode::Off),
            "shadow" | "dry-run" | "dryrun" => Ok(Mode::Shadow),
            "live" | "on" | "true" | "1" => Ok(Mode::Live),
            other => Err(format!("invalid optimizer mode: {other:?}")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct OptimizationReport {
    pub mode: Mode,
    pub applied: bool,
    pub frontier: usize,
    pub input_tokens_est: u64,
    pub output_tokens_est: u64,
    /// ΔT — tokens removed in newly transitioned messages this request.
    pub removed_tokens_est: u64,
    /// S — original tokens from first transitioned message to end of frozen zone.
    pub rewrite_suffix_tokens: u64,
    pub est_cost_delta_usd: f64,
    pub scorer_ms: u32,
    pub messages_compressed: u16,
    pub messages_skipped_deadline: u16,
    /// Hash of all keep-masks — for determinism auditing (I2).
    pub decisions_hash: u64,
    pub policy_version: PolicyVersion,
    /// Fail-open reason, if any.
    pub failure: Option<String>,
}

impl OptimizationReport {
    /// A report for the fail-open path (forward original, record why).
    pub fn failed_open(mode: Mode, version: PolicyVersion, reason: impl Into<String>) -> Self {
        Self {
            mode,
            applied: false,
            frontier: 0,
            input_tokens_est: 0,
            output_tokens_est: 0,
            removed_tokens_est: 0,
            rewrite_suffix_tokens: 0,
            est_cost_delta_usd: 0.0,
            scorer_ms: 0,
            messages_compressed: 0,
            messages_skipped_deadline: 0,
            decisions_hash: 0,
            policy_version: version,
            failure: Some(reason.into()),
        }
    }
}
