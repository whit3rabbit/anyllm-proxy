use crate::error::TranslateError;

/// What to do when an Anthropic feature has no backend equivalent
/// (e.g., cache_control, thinking blocks, metadata).
#[derive(Debug, Clone, PartialEq)]
pub enum LossyBehavior {
    /// Drop unsupported features silently.
    Silent,
    /// Log a warning via `tracing::warn` (current default behavior).
    Warn,
    /// Return a `TranslateError::Translation` instead of dropping.
    Error,
}

/// Configuration for the translation layer.
///
/// Controls model name mapping and behavior when Anthropic features
/// have no equivalent in the target API.
#[derive(Debug, Clone)]
pub struct TranslationConfig {
    /// Ordered list of (substring, target_model) pairs for model name mapping.
    /// Case-insensitive substring match; first hit wins.
    pub model_map: Vec<(String, String)>,

    /// How to handle Anthropic features with no backend equivalent.
    pub lossy_behavior: LossyBehavior,

    /// If true, models not matching any entry pass through unchanged.
    /// If false, unmatched models produce `TranslateError::UnknownModel`.
    pub passthrough_unknown_models: bool,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            model_map: Vec::new(),
            lossy_behavior: LossyBehavior::Warn,
            passthrough_unknown_models: true,
        }
    }
}

impl TranslationConfig {
    /// Start building a `TranslationConfig`.
    pub fn builder() -> TranslationConfigBuilder {
        TranslationConfigBuilder {
            config: Self::default(),
        }
    }

    /// Map an Anthropic model name to a backend model name using the configured rules.
    ///
    /// Performs case-insensitive substring matching in insertion order.
    /// Returns the first match, or passthrough/error depending on config.
    pub fn map_model(&self, model: &str) -> Result<String, TranslateError> {
        let model_bytes = model.as_bytes();
        for (pattern, target) in &self.model_map {
            if contains_ignore_ascii_case(model_bytes, pattern.as_bytes()) {
                return Ok(target.clone());
            }
        }
        if self.passthrough_unknown_models {
            Ok(model.to_string())
        } else {
            Err(TranslateError::UnknownModel(model.to_string()))
        }
    }
}

/// Builder for `TranslationConfig`.
#[derive(Debug, Clone)]
pub struct TranslationConfigBuilder {
    config: TranslationConfig,
}

impl TranslationConfigBuilder {
    /// Add a model mapping rule: if the Anthropic model name contains `pattern`
    /// (case-insensitive), map it to `target`.
    pub fn model_map(mut self, pattern: impl Into<String>, target: impl Into<String>) -> Self {
        self.config.model_map.push((pattern.into(), target.into()));
        self
    }

    /// Set the lossy behavior for unsupported features.
    pub fn lossy_behavior(mut self, behavior: LossyBehavior) -> Self {
        self.config.lossy_behavior = behavior;
        self
    }

    /// Set whether unknown models pass through unchanged or produce an error.
    pub fn passthrough_unknown_models(mut self, passthrough: bool) -> Self {
        self.config.passthrough_unknown_models = passthrough;
        self
    }

    /// Build the `TranslationConfig`.
    pub fn build(self) -> TranslationConfig {
        self.config
    }
}

fn contains_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_passthrough() {
        let config = TranslationConfig::default();
        assert_eq!(
            config.map_model("claude-sonnet-4-6").unwrap(),
            "claude-sonnet-4-6"
        );
    }

    #[test]
    fn model_map_substring_match() {
        let config = TranslationConfig::builder()
            .model_map("haiku", "gpt-4o-mini")
            .model_map("sonnet", "gpt-4o")
            .model_map("opus", "gpt-4o")
            .build();

        assert_eq!(config.map_model("claude-haiku-4-5").unwrap(), "gpt-4o-mini");
        assert_eq!(config.map_model("claude-sonnet-4-6").unwrap(), "gpt-4o");
        assert_eq!(config.map_model("claude-opus-4-6").unwrap(), "gpt-4o");
    }

    #[test]
    fn model_map_case_insensitive() {
        let config = TranslationConfig::builder()
            .model_map("sonnet", "gpt-4o")
            .build();

        assert_eq!(config.map_model("Claude-SONNET-4-6").unwrap(), "gpt-4o");
    }

    #[test]
    fn model_map_first_match_wins() {
        let config = TranslationConfig::builder()
            .model_map("claude", "first")
            .model_map("sonnet", "second")
            .build();

        // "claude-sonnet-4-6" contains both, but "claude" rule is first
        assert_eq!(config.map_model("claude-sonnet-4-6").unwrap(), "first");
    }

    #[test]
    fn unknown_model_passthrough() {
        let config = TranslationConfig::builder()
            .model_map("sonnet", "gpt-4o")
            .build();

        assert_eq!(config.map_model("custom-model").unwrap(), "custom-model");
    }

    #[test]
    fn unknown_model_error_when_strict() {
        let config = TranslationConfig::builder()
            .model_map("sonnet", "gpt-4o")
            .passthrough_unknown_models(false)
            .build();

        let err = config.map_model("custom-model").unwrap_err();
        assert!(matches!(err, TranslateError::UnknownModel(_)));
    }

    #[test]
    fn default_lossy_behavior_is_warn() {
        let config = TranslationConfig::default();
        assert_eq!(config.lossy_behavior, LossyBehavior::Warn);
    }

    #[test]
    fn builder_sets_lossy_behavior() {
        let config = TranslationConfig::builder()
            .lossy_behavior(LossyBehavior::Silent)
            .build();
        assert_eq!(config.lossy_behavior, LossyBehavior::Silent);
    }
}
