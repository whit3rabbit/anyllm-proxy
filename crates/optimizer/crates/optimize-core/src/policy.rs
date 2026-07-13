//! Policy: all tunables flow through here (no global mutable state). `PolicyVersion`
//! (in `types`) identifies the decision procedure for cache-stability accounting.

use std::collections::HashMap;
use std::time::Duration;

use crate::budget_planner::BudgetPlanner;
use crate::frontier::FrontierPolicy;
use crate::report::Mode;
use crate::select::ForceRules;
use crate::traits::Pricing;
use crate::types::{PolicyVersion, Role};

/// Keep-ratio table by (role, block kind). Start conservative (ROADMAP risk 1).
#[derive(Clone, Debug)]
pub struct RatioTable {
    pub text_user: f32,
    pub text_assistant: f32,
    pub text_system: f32,
    pub tool_result_value: f32,
}

impl Default for RatioTable {
    fn default() -> Self {
        // Conservative defaults: keep most of the text. Tighten per-route later.
        Self {
            text_user: 0.7,
            text_assistant: 0.6,
            text_system: 1.0, // system is Immutable anyway; 1.0 is a belt-and-braces guard
            tool_result_value: 0.4,
        }
    }
}

impl RatioTable {
    /// Ratio for a text block by role.
    pub fn text_ratio(&self, role: Role) -> f32 {
        match role {
            Role::User => self.text_user,
            Role::Assistant => self.text_assistant,
            Role::System => self.text_system,
            Role::Tool => self.tool_result_value,
        }
    }

    /// Set the ratio for a single role, leaving the others untouched. Used by the
    /// budget planner (M4.1) to apply a per-message-planned ratio for just the role of
    /// the message currently being compressed, without touching the base table's other
    /// entries (which stay available for any other message compressed this same run).
    pub fn set_text_ratio(&mut self, role: Role, value: f32) {
        match role {
            Role::User => self.text_user = value,
            Role::Assistant => self.text_assistant = value,
            Role::System => self.text_system = value,
            Role::Tool => self.tool_result_value = value,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompressionPolicy {
    pub version: PolicyVersion,
    pub ratios: RatioTable,
    pub force: ForceRules,
    /// Only bother compressing a buffer this many chars or longer.
    pub min_len: usize,
    /// Structural truncation threshold for tool results (target-LLM tokens).
    pub tool_result_max_tokens: usize,
    /// Scorer budget for the WHOLE request.
    pub deadline: Duration,
    /// M4.1: optional per-message age/size ratio planner (LLMLingua-1's "position
    /// matters" idea, kept per-message-pure — see `budget_planner.rs`). `None` (the
    /// default) reproduces the flat per-role `ratios` table exactly.
    pub planner: Option<BudgetPlanner>,
}

impl Default for CompressionPolicy {
    fn default() -> Self {
        Self {
            version: PolicyVersion(0),
            ratios: RatioTable::default(),
            force: ForceRules::default(),
            min_len: 200,
            tool_result_max_tokens: 4000,
            deadline: Duration::from_millis(150),
            planner: None,
        }
    }
}

/// Top-level policy handed to `optimize()`.
#[derive(Clone, Debug)]
pub struct Policy {
    pub mode: Mode,
    pub frontier: FrontierPolicy,
    pub compression: CompressionPolicy,
    /// Expected remaining turns of this conversation (cost-gate horizon).
    pub horizon: u64,
    /// M4.3: config-sourced pricing table for the cost gate. `Some` wins over the
    /// `CacheStrategy::pricing()` a caller passes to `optimize()`, so pricing can be
    /// versioned in config (this field, populated via `Pricing::from_config_str` one
    /// layer up) instead of the hardcoded per-provider tables in
    /// `anyllm_optimize_passes::cost_gate` (ROADMAP risk 5). `None` (the default)
    /// preserves the old behavior of always using the strategy's pricing.
    pub pricing_override: Option<Pricing>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            mode: Mode::Shadow,
            frontier: FrontierPolicy::default(),
            compression: CompressionPolicy::default(),
            horizon: 8,
            pricing_override: None,
        }
    }
}

/// Per-route knobs. Any field left `None` inherits the top-level `OptimizationPolicy`
/// default for that route — this is how "unlisted routes fall back to defaults" works.
#[derive(Clone, Debug, Default)]
pub struct RouteOverride {
    pub mode: Option<Mode>,
    pub ratios: Option<RatioTable>,
    /// Per-route pricing override (M4.3). `None` falls back to the top-level
    /// `OptimizationPolicy::pricing`.
    pub pricing: Option<Pricing>,
}

/// Config shape the proxy integration binds to (see `crates/optimizer/CLAUDE.md`
/// "Proxy integration checklist" step 3: `RouteOptions.optimizer_mode` /
/// `effective_optimizer()` precedence). Lets one route class that failed the M0 ROI
/// gate be turned `Off` while others keep running, without touching the top-level
/// defaults other routes rely on.
#[derive(Clone, Debug)]
pub struct OptimizationPolicy {
    pub mode: Mode,
    pub frontier: FrontierPolicy,
    pub ratios: RatioTable,
    /// M4.3: top-level pricing override, config-sourced (`Pricing::from_config_str`).
    /// `None` (the default) means "use whatever `CacheStrategy::pricing()` the caller
    /// passes to `optimize()`" — i.e. the hardcoded per-provider tables in
    /// `anyllm_optimize_passes::cost_gate` keep working unchanged until config supplies
    /// a table.
    pub pricing: Option<Pricing>,
    pub routes: HashMap<String, RouteOverride>,
}

impl Default for OptimizationPolicy {
    fn default() -> Self {
        Self {
            mode: Mode::Shadow,
            frontier: FrontierPolicy::default(),
            ratios: RatioTable::default(),
            pricing: None,
            routes: HashMap::new(),
        }
    }
}

impl OptimizationPolicy {
    /// Resolve the effective `Policy` for `route`. Precedence: a per-route override
    /// field wins when present; any override field left `None`, and any route not in
    /// `routes` at all, falls back to the top-level default. `CompressionPolicy` fields
    /// other than `ratios` (min_len, tool_result_max_tokens, deadline, version) are not
    /// route-overridable yet — they come from `CompressionPolicy::default()`.
    pub fn resolve(&self, route: &str) -> Policy {
        let (mode, ratios, pricing) = match self.routes.get(route) {
            Some(o) => (
                o.mode.unwrap_or(self.mode),
                o.ratios.clone().unwrap_or_else(|| self.ratios.clone()),
                o.pricing.or(self.pricing),
            ),
            None => (self.mode, self.ratios.clone(), self.pricing),
        };
        Policy {
            mode,
            frontier: self.frontier.clone(),
            compression: CompressionPolicy {
                ratios,
                ..CompressionPolicy::default()
            },
            pricing_override: pricing,
            ..Policy::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_applies_route_override_and_falls_back_for_unlisted_routes() {
        let mut routes = HashMap::new();
        routes.insert(
            "batch".to_string(),
            RouteOverride {
                mode: Some(Mode::Off),
                ratios: Some(RatioTable {
                    text_user: 0.1,
                    text_assistant: 0.1,
                    text_system: 1.0,
                    tool_result_value: 0.1,
                }),
                pricing: None,
            },
        );
        let policy = OptimizationPolicy {
            mode: Mode::Live,
            routes,
            ..OptimizationPolicy::default()
        };

        let overridden = policy.resolve("batch");
        assert_eq!(overridden.mode, Mode::Off);
        assert_eq!(overridden.compression.ratios.text_user, 0.1);

        // Unlisted route: falls back to the top-level defaults, not the "batch" override.
        let default_route = policy.resolve("interactive");
        assert_eq!(default_route.mode, Mode::Live);
        assert_eq!(
            default_route.compression.ratios.text_user,
            RatioTable::default().text_user
        );
    }
}
