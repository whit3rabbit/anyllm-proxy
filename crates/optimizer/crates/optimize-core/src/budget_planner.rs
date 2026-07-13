//! M4.1 budget planner pass (ALGO §12): allocate the compression **ratio** for one
//! message by its age/size/type instead of one flat per-role ratio (LLMLingua-1's one
//! good idea, folded in as a planner pass — see ROADMAP Phase 4).
//!
//! Per-message purity (I3): [`BudgetPlanner::plan_ratio`] is a pure function of ONE
//! message's own `(role, index, byte_len)` plus the static [`RatioTable`] base — nothing
//! else. `index` is the message's absolute position from the start of the conversation
//! (0 = oldest); the client resends full history every turn, so a given message keeps the
//! same `index` for as long as it exists, regardless of how many later turns get appended.
//! That is what keeps this per-message-pure rather than a global-over-all-messages ratio:
//! it must NEVER be computed from `conv.messages.len()`, the current frontier, or any
//! other message's bytes — any of those would make an already-frozen message's planned
//! ratio drift as the conversation grows, breaking I3. `age_step`/`size_step` only ever
//! *tighten* the base per-role ratio, never loosen it, so the planner can only remove
//! more than the flat baseline, never less (I8 ratio-honesty stays satisfied).
//!
//! Extractive only (I5): the planner never emits edits itself — it only computes the
//! `ratio` argument that `compress_message` (still extractive-only, unchanged) feeds
//! into `select_keep`. See `tests/invariants.rs` for the property tests proving both.

use crate::policy::RatioTable;
use crate::types::Role;

/// Tunables for age/size-aware ratio adjustment. All-zero (the `Default`) reproduces the
/// flat per-role `RatioTable` exactly — this pass is opt-in and backward compatible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BudgetPlanner {
    /// Ratio subtracted per message of absolute age (`index`, 0 = oldest). Older
    /// messages get compressed harder. Zero disables age weighting.
    pub age_step: f32,
    /// Ratio subtracted per whole `size_unit`-byte multiple a buffer's total size is
    /// over one unit. Bigger messages get compressed harder (more absolute tokens saved
    /// per edit). Zero disables size weighting.
    pub size_step: f32,
    /// Byte granularity for the size penalty; clamped to >= 1 in `plan_ratio`.
    pub size_unit: usize,
    /// Floor: `plan_ratio` never returns below this, regardless of accumulated penalty.
    pub min_ratio: f32,
}

impl Default for BudgetPlanner {
    fn default() -> Self {
        Self {
            age_step: 0.0,
            size_step: 0.0,
            size_unit: 4000,
            min_ratio: 0.1,
        }
    }
}

impl BudgetPlanner {
    /// Plan the keep-ratio for ONE message from `base` (the per-role table) and that
    /// message's own `(role, index, byte_len)`. Pure: no clocks, no cross-message data,
    /// no dependency on total conversation length. Result is always in
    /// `[min(min_ratio, base_ratio), base_ratio]` — age/size penalties only tighten.
    pub fn plan_ratio(&self, base: &RatioTable, role: Role, index: usize, byte_len: usize) -> f32 {
        let base_ratio = base.text_ratio(role);
        let unit = self.size_unit.max(1);
        let age_penalty = self.age_step * index as f32;
        let size_penalty = self.size_step * (byte_len / unit) as f32;
        let floor = self.min_ratio.min(base_ratio);
        (base_ratio - age_penalty - size_penalty).clamp(floor, base_ratio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> RatioTable {
        RatioTable {
            text_user: 0.8,
            text_assistant: 0.6,
            text_system: 1.0,
            tool_result_value: 0.4,
        }
    }

    #[test]
    fn default_planner_is_a_no_op_reproducing_the_flat_ratio() {
        let planner = BudgetPlanner::default();
        let base = table();
        for (role, expected) in [
            (Role::User, base.text_user),
            (Role::Assistant, base.text_assistant),
            (Role::Tool, base.tool_result_value),
        ] {
            assert_eq!(planner.plan_ratio(&base, role, 0, 0), expected);
            assert_eq!(planner.plan_ratio(&base, role, 500, 1_000_000), expected);
        }
    }

    #[test]
    fn older_messages_are_compressed_harder() {
        let planner = BudgetPlanner {
            age_step: 0.01,
            ..BudgetPlanner::default()
        };
        let base = table();
        let young = planner.plan_ratio(&base, Role::User, 1, 0);
        let old = planner.plan_ratio(&base, Role::User, 20, 0);
        assert!(
            old < young,
            "an older (lower-index-from-recent... higher absolute index from start \
             = further back) message must plan a ratio <= a younger one: old={old} young={young}"
        );
    }

    #[test]
    fn larger_buffers_are_compressed_harder() {
        let planner = BudgetPlanner {
            size_step: 0.05,
            size_unit: 100,
            ..BudgetPlanner::default()
        };
        let base = table();
        let small = planner.plan_ratio(&base, Role::User, 0, 50);
        let big = planner.plan_ratio(&base, Role::User, 0, 5_000);
        assert!(
            big < small,
            "a bigger buffer must plan a ratio <= a smaller one: big={big} small={small}"
        );
    }

    #[test]
    fn never_exceeds_base_ratio() {
        // age_step/size_step are always non-negative in practice, but even a
        // pathological negative config must not let the planned ratio exceed the base
        // (age/size only ever tighten, per I8 ratio-honesty).
        let planner = BudgetPlanner {
            age_step: -1.0,
            size_step: -1.0,
            ..BudgetPlanner::default()
        };
        let base = table();
        let ratio = planner.plan_ratio(&base, Role::User, 50, 100_000);
        assert!(ratio <= base.text_user);
    }

    #[test]
    fn clamps_to_min_ratio_floor() {
        let planner = BudgetPlanner {
            age_step: 1.0,
            min_ratio: 0.15,
            ..BudgetPlanner::default()
        };
        let base = table();
        let ratio = planner.plan_ratio(&base, Role::User, 1000, 0);
        assert_eq!(ratio, 0.15);
    }

    #[test]
    fn plan_ratio_is_pure_same_inputs_same_output() {
        let planner = BudgetPlanner {
            age_step: 0.02,
            size_step: 0.03,
            size_unit: 500,
            min_ratio: 0.1,
        };
        let base = table();
        let a = planner.plan_ratio(&base, Role::Assistant, 7, 3_000);
        let b = planner.plan_ratio(&base, Role::Assistant, 7, 3_000);
        assert_eq!(
            a, b,
            "identical (role, index, byte_len) must plan identically"
        );
    }
}
