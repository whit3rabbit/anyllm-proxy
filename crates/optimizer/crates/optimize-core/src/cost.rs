//! The cost gate: pure arithmetic over provider pricing and cache model. Lives in core
//! (no serde) so the orchestrator can gate; `anyllm_optimize_passes` re-exports it and
//! supplies the `Pricing`/`CacheModel` via `CacheStrategy`.
//!
//! Let (all in target-LLM tokens, approximate is fine):
//!   ΔT = tokens removed in newly transitioned messages this request
//!   S  = original tokens from the first transitioned message to end of frozen zone
//!   H  = horizon: expected remaining turns of this conversation
//! apply ⇔ cost(apply) < cost(skip):
//!   cost(apply) = (S − ΔT)·input·write_mult + H·(S − ΔT)·cached_read
//!   cost(skip)  = H·S·cached_read
//! ⇔ (S − ΔT)·input·write_mult < H·ΔT·cached_read

use crate::traits::{CacheModel, Pricing};

/// Decide whether applying the compression is cost-positive given the cache regime.
pub fn should_apply(dt: u64, s: u64, h: u64, p: &Pricing, m: &CacheModel) -> bool {
    match m {
        // Optimizer-managed breakpoints: the recent zone is never cached, so a
        // transition never invalidates. Any real removal wins.
        CacheModel::ExplicitBreakpoints => dt > 0,
        CacheModel::ImplicitPrefix => {
            if dt == 0 {
                return false;
            }
            let rewrite = (s.saturating_sub(dt)) as f64 * p.input * p.cache_write_mult;
            let reads_saved = h as f64 * dt as f64 * p.cached_read;
            rewrite < reads_saved
        }
    }
}

/// Signed net USD delta of applying compression vs skipping it over `h` remaining turns.
/// Positive ⇒ compression saves money; the sign always agrees with `should_apply` (this is
/// the same inequality, rearranged to `cost(skip) − cost(apply)` instead of `rewrite <
/// reads_saved`). Used by the ROI eval harness (EH-0002) to report a per-route dollar
/// number, not just the boolean gate; `OptimizationReport.est_cost_delta_usd` is still a
/// placeholder pending EH-0005 wiring this in for the request path.
///
/// `ImplicitPrefix`: skip never pays a write this turn (the prefix is already cached from
/// a prior turn); apply pays one write of the smaller suffix, then `h` cheaper reads.
///   delta = h·ΔT·cached_read − (S − ΔT)·input·write_mult
///
/// `ExplicitBreakpoints`: the optimizer places a *new* breakpoint at the frontier either
/// way (this batch just transitioned into the frozen zone), so both apply and skip pay one
/// write; only the size differs, and every term is non-negative for ΔT ≥ 0.
///   delta = ΔT·input·write_mult + h·ΔT·cached_read
pub fn net_cost_delta_usd(dt: u64, s: u64, h: u64, p: &Pricing, m: &CacheModel) -> f64 {
    let dt = dt as f64;
    let s = s as f64;
    let h = h as f64;
    let raw_delta = match m {
        CacheModel::ImplicitPrefix => {
            let reads_saved = h * dt * p.cached_read;
            let rewrite = (s - dt).max(0.0) * p.input * p.cache_write_mult;
            reads_saved - rewrite
        }
        CacheModel::ExplicitBreakpoints => {
            dt * p.input * p.cache_write_mult + h * dt * p.cached_read
        }
    };
    raw_delta / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn openai() -> Pricing {
        Pricing {
            input: 1.0,
            cached_read: 0.5,
            cache_write_mult: 1.0,
        }
    }

    #[test]
    fn implicit_applies_for_long_conversation() {
        // 30% removal of S=100 -> dt=30, s=100, H=8: 70 < 8*30*0.5=120 -> apply
        assert!(should_apply(
            30,
            100,
            8,
            &openai(),
            &CacheModel::ImplicitPrefix
        ));
    }

    #[test]
    fn implicit_skips_dying_conversation() {
        // H=2: 70 < 2*30*0.5=30 -> false, correct (rewriting a dying suffix loses money)
        assert!(!should_apply(
            30,
            100,
            2,
            &openai(),
            &CacheModel::ImplicitPrefix
        ));
    }

    #[test]
    fn explicit_applies_on_any_removal() {
        let anthropic = Pricing {
            input: 3.0,
            cached_read: 0.3,
            cache_write_mult: 1.25,
        };
        assert!(should_apply(
            1,
            100,
            1,
            &anthropic,
            &CacheModel::ExplicitBreakpoints
        ));
        assert!(!should_apply(
            0,
            100,
            8,
            &anthropic,
            &CacheModel::ExplicitBreakpoints
        ));
    }

    #[test]
    fn net_delta_sign_matches_should_apply_implicit() {
        // Same fixture as implicit_applies_for_long_conversation: gate says apply, so the
        // dollar delta must be strictly positive.
        let delta = net_cost_delta_usd(30, 100, 8, &openai(), &CacheModel::ImplicitPrefix);
        assert!(delta > 0.0, "expected positive net delta, got {delta}");

        // Same fixture as implicit_skips_dying_conversation: gate says skip, so the dollar
        // delta must be zero or negative.
        let delta = net_cost_delta_usd(30, 100, 2, &openai(), &CacheModel::ImplicitPrefix);
        assert!(delta <= 0.0, "expected non-positive net delta, got {delta}");
    }

    #[test]
    fn net_delta_implicit_known_value() {
        // reads_saved = 8*30*0.5 = 120; rewrite = (100-30)*1.0*1.0 = 70; delta = 50 units
        // = $0.00005 (units are $/Mtok, so divide by 1e6).
        let delta = net_cost_delta_usd(30, 100, 8, &openai(), &CacheModel::ImplicitPrefix);
        assert!((delta - 50.0 / 1_000_000.0).abs() < 1e-12);
    }

    #[test]
    fn net_delta_explicit_always_nonnegative_for_any_removal() {
        let anthropic = Pricing {
            input: 3.0,
            cached_read: 0.3,
            cache_write_mult: 1.25,
        };
        // dt=1, s=100, h=1: matches explicit_applies_on_any_removal's true case.
        let delta = net_cost_delta_usd(1, 100, 1, &anthropic, &CacheModel::ExplicitBreakpoints);
        assert!(delta > 0.0, "expected positive net delta, got {delta}");

        // dt=0: no removal, no savings, regardless of s/h.
        let delta = net_cost_delta_usd(0, 100, 8, &anthropic, &CacheModel::ExplicitBreakpoints);
        assert_eq!(delta, 0.0);
    }
}
