//! Claude Code "router" config: maps request tiers to a (managed backend, model)
//! pair. Modeled on ccrouter's Router: an incoming request is classified into one
//! tier (Default/Background/Think/LongContext/WebSearch/Image) and routed to the
//! backend+model configured for that tier.
//!
//! Unlike the model-name-based [`crate::config::route_router`], this keys off
//! *request characteristics* (image content, web-search tool, thinking enabled,
//! token count, haiku/background model). Pure data + classification here; the
//! request-body signal extraction and backend resolution live in
//! `crate::server::state::app_state`.

use serde::{Deserialize, Serialize};

/// Default long-context token threshold (mirrors ccrouter's default).
pub const DEFAULT_CONTEXT_THRESHOLD: u32 = 60_000;

/// A single tier's routing target: which managed backend and which model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierTarget {
    /// Name of a managed backend (see `admin::db::backends`). Empty = unset.
    #[serde(default)]
    pub backend_name: String,
    /// Model name to send upstream (the rename applied for this tier).
    #[serde(default)]
    pub model: String,
    /// Whether this tier is active. A tier only routes when enabled AND
    /// `backend_name` is non-empty.
    #[serde(default)]
    pub enabled: bool,
}

impl TierTarget {
    /// Returns `Some(self)` only when this tier is usable: enabled, with both a
    /// backend and a model set. A tier with no model would send an empty model
    /// upstream, so it counts as unconfigured and falls through.
    fn active(&self) -> Option<&TierTarget> {
        if self.enabled && !self.backend_name.is_empty() && !self.model.is_empty() {
            Some(self)
        } else {
            None
        }
    }
}

/// The whole router config, stored as one JSON blob under config-override key
/// `"router"`. Default is disabled so unconfigured installs are unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterConfig {
    /// Master switch. When false, routing is completely bypassed.
    #[serde(default)]
    pub enabled: bool,
    /// Token count above which a request is classified LongContext.
    #[serde(default = "default_context_threshold")]
    pub context_threshold: u32,
    #[serde(default)]
    pub default: TierTarget,
    #[serde(default)]
    pub background: TierTarget,
    #[serde(default)]
    pub think: TierTarget,
    #[serde(default)]
    pub long_context: TierTarget,
    #[serde(default)]
    pub web_search: TierTarget,
    #[serde(default)]
    pub image: TierTarget,
}

fn default_context_threshold() -> u32 {
    DEFAULT_CONTEXT_THRESHOLD
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            context_threshold: DEFAULT_CONTEXT_THRESHOLD,
            default: TierTarget::default(),
            background: TierTarget::default(),
            think: TierTarget::default(),
            long_context: TierTarget::default(),
            web_search: TierTarget::default(),
            image: TierTarget::default(),
        }
    }
}

/// Classified request characteristics, extracted from the request body by the
/// handler layer. Pure input to [`RouterConfig::pick_tier`].
#[derive(Debug, Clone, Copy, Default)]
pub struct RouterSignals {
    pub has_image: bool,
    pub has_web_search: bool,
    pub thinking: bool,
    pub long_context: bool,
    pub is_background: bool,
}

impl RouterConfig {
    /// Whether the LongContext tier is enabled with a target. Used to skip
    /// (expensive) request token counting when no tier would consume it.
    pub fn long_context_tier_active(&self) -> bool {
        self.enabled && self.long_context.active().is_some()
    }

    /// Select the routing target for a request, or `None` to fall through to
    /// normal model-name routing.
    ///
    /// Precedence (highest first): Image, WebSearch, Think, LongContext,
    /// Background, Default. A matched-but-inactive tier is skipped, so an image
    /// request whose Image tier is unconfigured still falls back to Default.
    pub fn pick_tier(&self, s: &RouterSignals) -> Option<&TierTarget> {
        if !self.enabled {
            return None;
        }
        if s.has_image {
            if let Some(t) = self.image.active() {
                return Some(t);
            }
        }
        if s.has_web_search {
            if let Some(t) = self.web_search.active() {
                return Some(t);
            }
        }
        if s.thinking {
            if let Some(t) = self.think.active() {
                return Some(t);
            }
        }
        if s.long_context {
            if let Some(t) = self.long_context.active() {
                return Some(t);
            }
        }
        if s.is_background {
            if let Some(t) = self.background.active() {
                return Some(t);
            }
        }
        self.default.active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(backend: &str) -> TierTarget {
        TierTarget {
            backend_name: backend.into(),
            model: format!("{backend}-model"),
            enabled: true,
        }
    }

    fn full_config() -> RouterConfig {
        RouterConfig {
            enabled: true,
            context_threshold: DEFAULT_CONTEXT_THRESHOLD,
            default: target("def"),
            background: target("bg"),
            think: target("think"),
            long_context: target("long"),
            web_search: target("web"),
            image: target("img"),
        }
    }

    #[test]
    fn disabled_config_never_routes() {
        let mut cfg = full_config();
        cfg.enabled = false;
        let s = RouterSignals {
            has_image: true,
            ..Default::default()
        };
        assert!(cfg.pick_tier(&s).is_none());
    }

    #[test]
    fn precedence_image_beats_all() {
        let cfg = full_config();
        let s = RouterSignals {
            has_image: true,
            has_web_search: true,
            thinking: true,
            long_context: true,
            is_background: true,
        };
        assert_eq!(cfg.pick_tier(&s).unwrap().backend_name, "img");
    }

    #[test]
    fn precedence_order_descends() {
        let cfg = full_config();
        // web_search beats think/long/bg
        let s = RouterSignals {
            has_web_search: true,
            thinking: true,
            long_context: true,
            is_background: true,
            ..Default::default()
        };
        assert_eq!(cfg.pick_tier(&s).unwrap().backend_name, "web");
        // think beats long/bg
        let s = RouterSignals {
            thinking: true,
            long_context: true,
            is_background: true,
            ..Default::default()
        };
        assert_eq!(cfg.pick_tier(&s).unwrap().backend_name, "think");
        // long beats bg
        let s = RouterSignals {
            long_context: true,
            is_background: true,
            ..Default::default()
        };
        assert_eq!(cfg.pick_tier(&s).unwrap().backend_name, "long");
        // bg beats default
        let s = RouterSignals {
            is_background: true,
            ..Default::default()
        };
        assert_eq!(cfg.pick_tier(&s).unwrap().backend_name, "bg");
    }

    #[test]
    fn no_signal_uses_default() {
        let cfg = full_config();
        assert_eq!(
            cfg.pick_tier(&RouterSignals::default())
                .unwrap()
                .backend_name,
            "def"
        );
    }

    #[test]
    fn inactive_tier_falls_back_to_default() {
        let mut cfg = full_config();
        cfg.image.enabled = false;
        let s = RouterSignals {
            has_image: true,
            ..Default::default()
        };
        // Image tier off -> falls through to Default.
        assert_eq!(cfg.pick_tier(&s).unwrap().backend_name, "def");
    }

    #[test]
    fn empty_backend_is_inactive() {
        let mut cfg = full_config();
        cfg.default.backend_name = String::new();
        // No tier matches and default has no backend -> None.
        assert!(cfg.pick_tier(&RouterSignals::default()).is_none());
    }

    #[test]
    fn empty_model_is_inactive() {
        let mut cfg = full_config();
        cfg.background.model = String::new();
        let s = RouterSignals {
            is_background: true,
            ..Default::default()
        };
        // Background tier has no model -> falls through to Default.
        assert_eq!(cfg.pick_tier(&s).unwrap().backend_name, "def");
    }

    #[test]
    fn serde_round_trip_and_defaults() {
        let cfg = full_config();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: RouterConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);

        // Missing threshold falls back to the default.
        let partial: RouterConfig = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert_eq!(partial.context_threshold, DEFAULT_CONTEXT_THRESHOLD);
        assert!(partial.default.backend_name.is_empty());
    }
}
