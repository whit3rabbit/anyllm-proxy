//! Serde structs mapping the LiteLLM `config.yaml` schema, plus the parsed-result
//! type returned by the conversion functions in [`super::parser`].

use serde::Deserialize;

use crate::config::model_router::ModelRouter;
use crate::config::MultiConfig;

/// Root structure of a LiteLLM `config.yaml` file.
#[derive(Deserialize)]
pub(super) struct LiteLLMConfig {
    #[serde(default)]
    pub(super) model_list: Vec<LiteLLMModelEntry>,
    #[serde(default)]
    pub(super) litellm_settings: Option<LiteLLMSettings>,
    #[serde(default)]
    pub(super) router_settings: Option<RouterSettings>,
    #[serde(default)]
    pub(super) general_settings: Option<GeneralSettings>,
}

#[derive(Deserialize)]
pub(super) struct LiteLLMModelEntry {
    pub(super) model_name: String,
    pub(super) litellm_params: LiteLLMParams,
}

#[derive(Deserialize)]
pub(super) struct LiteLLMParams {
    pub(super) model: String,
    pub(super) api_base: Option<String>,
    pub(super) api_key: Option<String>,
    pub(super) rpm: Option<u32>,
    pub(super) tpm: Option<u64>,
    pub(super) weight: Option<u32>,
    // Azure-specific
    pub(super) api_version: Option<String>,
    // Vertex-specific
    pub(super) vertex_project: Option<String>,
    pub(super) vertex_location: Option<String>,
    // Bedrock-specific
    pub(super) aws_access_key_id: Option<String>,
    pub(super) aws_secret_access_key: Option<String>,
    pub(super) aws_region_name: Option<String>,
    // Catch unknown fields silently (LiteLLM has many we don't support).
    #[serde(flatten)]
    pub(super) _extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
pub(super) struct LiteLLMSettings {
    #[serde(default)]
    pub(super) num_retries: Option<u32>,
    #[serde(default)]
    pub(super) request_timeout: Option<u64>,
    #[serde(default)]
    pub(super) callbacks: Vec<String>,
    #[serde(flatten)]
    pub(super) _extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
pub(super) struct RouterSettings {
    #[serde(default)]
    pub(super) routing_strategy: Option<String>,
    #[serde(flatten)]
    pub(super) _extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
pub(super) struct GeneralSettings {
    pub(super) master_key: Option<String>,
    #[serde(flatten)]
    pub(super) _extra: serde_json::Map<String, serde_json::Value>,
}

/// Parsed result from a LiteLLM config file.
pub struct LiteLLMParsed {
    pub multi_config: MultiConfig,
    pub router: ModelRouter,
    /// Webhook callback URLs from litellm_settings.callbacks (non-named entries).
    pub callback_urls: Vec<String>,
    /// True when "langfuse" appears in litellm_settings.callbacks.
    pub langfuse_requested: bool,
    /// Resolved `general_settings.master_key`, if present.
    /// Caller should apply as PROXY_API_KEYS if that var is not already set.
    pub master_key: Option<String>,
}
