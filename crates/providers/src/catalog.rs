use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};
use crate::registry;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
#[cfg(feature = "remote-catalog")]
use std::path::{Path, PathBuf};

pub(crate) mod helpers;
use helpers::*;

#[cfg(feature = "remote-catalog")]
pub const LITELLM_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

#[cfg(feature = "remote-catalog")]
pub const DEFAULT_MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnedProviderDef {
    pub id: String,
    pub display_name: String,
    pub default_base_url: String,
    pub protocol: ProviderProtocol,
    pub auth: AuthKind,
    pub status: ProviderStatus,
    pub env_vars: Vec<String>,
    pub litellm_prefix: String,
    pub capabilities: ProviderCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnedModelDef {
    pub id: String,
    pub provider_id: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub capabilities: ModelCapabilities,
    pub status: ModelStatus,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CatalogMetadata {
    pub source: String,
    pub source_url: Option<String>,
    pub etag: Option<String>,
    pub fetched_at_unix_secs: Option<u64>,
    pub provider_count: usize,
    pub model_count: usize,
}

#[derive(Debug, Clone)]
pub struct ProviderCatalog {
    metadata: CatalogMetadata,
    providers: BTreeMap<String, OwnedProviderDef>,
    advertised_provider_ids: BTreeSet<String>,
    models_by_provider: BTreeMap<String, Vec<OwnedModelDef>>,
}

#[derive(Debug)]
pub enum CatalogError {
    Json(serde_json::Error),
    Io(std::io::Error),
    InvalidFormat(&'static str),
    Utf8(std::string::FromUtf8Error),
    #[cfg(feature = "remote-catalog")]
    Http(reqwest::Error),
    #[cfg(feature = "remote-catalog")]
    HttpStatus(u16),
    #[cfg(feature = "remote-catalog")]
    TooLarge {
        max_bytes: usize,
        actual_bytes: usize,
    },
    #[cfg(feature = "remote-catalog")]
    CacheMiss,
    #[cfg(feature = "remote-catalog")]
    Redirect {
        requested: String,
        final_url: String,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid LiteLLM catalog JSON: {e}"),
            Self::Io(e) => write!(f, "catalog I/O failed: {e}"),
            Self::InvalidFormat(msg) => write!(f, "invalid LiteLLM catalog format: {msg}"),
            Self::Utf8(e) => write!(f, "LiteLLM catalog was not UTF-8: {e}"),
            #[cfg(feature = "remote-catalog")]
            Self::Http(e) => write!(f, "catalog HTTP request failed: {e}"),
            #[cfg(feature = "remote-catalog")]
            Self::HttpStatus(status) => write!(f, "catalog HTTP request returned {status}"),
            #[cfg(feature = "remote-catalog")]
            Self::TooLarge {
                max_bytes,
                actual_bytes,
            } => write!(
                f,
                "catalog response exceeded {max_bytes} bytes: {actual_bytes} bytes"
            ),
            #[cfg(feature = "remote-catalog")]
            Self::CacheMiss => write!(f, "catalog cache is missing"),
            #[cfg(feature = "remote-catalog")]
            Self::Redirect {
                requested,
                final_url,
            } => write!(
                f,
                "catalog request redirected from {requested} to {final_url}"
            ),
        }
    }
}

impl std::error::Error for CatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Utf8(e) => Some(e),
            #[cfg(feature = "remote-catalog")]
            Self::Http(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for CatalogError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<std::io::Error> for CatalogError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<std::string::FromUtf8Error> for CatalogError {
    fn from(value: std::string::FromUtf8Error) -> Self {
        Self::Utf8(value)
    }
}

#[cfg(feature = "remote-catalog")]
impl From<reqwest::Error> for CatalogError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

impl From<&ProviderDef> for OwnedProviderDef {
    fn from(value: &ProviderDef) -> Self {
        Self {
            id: value.id.to_string(),
            display_name: value.display_name.to_string(),
            default_base_url: value.default_base_url.to_string(),
            protocol: value.protocol,
            auth: value.auth,
            status: value.status,
            env_vars: value.env_vars.iter().map(|v| (*v).to_string()).collect(),
            litellm_prefix: value.litellm_prefix.to_string(),
            capabilities: value.capabilities,
        }
    }
}

impl From<&ModelDef> for OwnedModelDef {
    fn from(value: &ModelDef) -> Self {
        Self {
            id: value.id.to_string(),
            provider_id: value.provider_id.to_string(),
            context_window: value.context_window,
            max_output_tokens: value.max_output_tokens,
            capabilities: value.capabilities,
            status: value.status,
        }
    }
}

impl ProviderCatalog {
    pub fn bundled() -> Self {
        let mut providers = BTreeMap::new();
        let mut advertised_provider_ids = BTreeSet::new();
        let mut models_by_provider = BTreeMap::new();

        for provider in registry::advertised_provider_defs() {
            advertised_provider_ids.insert(provider.id.to_string());
            providers.insert(provider.id.to_string(), OwnedProviderDef::from(*provider));
        }
        for provider in registry::legacy_only_provider_defs() {
            providers.insert(provider.id.to_string(), OwnedProviderDef::from(*provider));
        }

        for (provider_id, models) in registry::advertised_model_groups()
            .iter()
            .chain(registry::legacy_only_model_groups().iter())
        {
            models_by_provider.insert(
                (*provider_id).to_string(),
                models.iter().map(OwnedModelDef::from).collect(),
            );
        }

        let mut catalog = Self {
            metadata: CatalogMetadata {
                source: "bundled".to_string(),
                source_url: None,
                etag: None,
                fetched_at_unix_secs: None,
                provider_count: 0,
                model_count: 0,
            },
            providers,
            advertised_provider_ids,
            models_by_provider,
        };
        catalog.refresh_metadata_counts();
        catalog
    }

    pub fn from_litellm_json(json: &str) -> Result<Self, CatalogError> {
        let raw: Value = serde_json::from_str(json)?;
        let root = raw
            .as_object()
            .ok_or(CatalogError::InvalidFormat("root must be a JSON object"))?;
        let mut catalog = Self::bundled();
        let mut rows_by_provider: BTreeMap<String, ProviderRows> = BTreeMap::new();

        for (model_key, data) in root {
            let Some(data) = data.as_object() else {
                continue;
            };
            let Some(provider_id) = data.get("litellm_provider").and_then(Value::as_str) else {
                continue;
            };
            if provider_id.is_empty() || provider_id.starts_with("one of ") {
                continue;
            }

            let mode = data.get("mode").and_then(Value::as_str).unwrap_or_default();
            let model_id = normalize_litellm_model_id(model_key, provider_id);
            let rows = rows_by_provider.entry(provider_id.to_string()).or_default();
            rows.observe(mode, data);
            rows.models.insert(
                model_id.clone(),
                OwnedModelDef {
                    id: model_id,
                    provider_id: provider_id.to_string(),
                    context_window: u32_json_field(data, "max_input_tokens"),
                    max_output_tokens: u32_json_field(data, "max_output_tokens"),
                    capabilities: ModelCapabilities {
                        streaming: matches!(mode, "chat" | "completion" | "responses"),
                        tool_use: truthy_json_field(data, "supports_function_calling")
                            || truthy_json_field(data, "supports_tool_choice"),
                        vision: truthy_json_field(data, "supports_vision"),
                        extended_thinking: truthy_json_field(data, "supports_reasoning")
                            || truthy_json_field(data, "supports_reasoning_content"),
                    },
                    status: if truthy_json_field(data, "is_deprecated")
                        || data.get("deprecation_date").is_some()
                    {
                        ModelStatus::Deprecated
                    } else {
                        ModelStatus::Available
                    },
                },
            );
        }

        for (provider_id, rows) in rows_by_provider {
            let mut provider = catalog
                .providers
                .get(&provider_id)
                .cloned()
                .unwrap_or_else(|| default_runtime_provider(&provider_id));

            provider.litellm_prefix = format!("{provider_id}/");
            let batch = provider.capabilities.batch;
            provider.capabilities = rows.provider_capabilities(batch);

            catalog.advertised_provider_ids.insert(provider_id.clone());
            catalog.providers.insert(provider_id.clone(), provider);
            catalog.models_by_provider.insert(
                provider_id,
                rows.models.into_values().collect::<Vec<OwnedModelDef>>(),
            );
        }

        catalog.metadata.source = "litellm-json".to_string();
        catalog.metadata.source_url = None;
        catalog.metadata.etag = None;
        catalog.metadata.fetched_at_unix_secs = None;
        catalog.refresh_metadata_counts();
        Ok(catalog)
    }

    pub fn metadata(&self) -> &CatalogMetadata {
        &self.metadata
    }

    pub fn get_provider(&self, id: &str) -> Option<&OwnedProviderDef> {
        let id = registry::canonical_provider_id(id);
        self.providers.get(id)
    }

    pub fn all_providers(&self) -> impl Iterator<Item = &OwnedProviderDef> {
        self.advertised_provider_ids
            .iter()
            .filter_map(|id| self.providers.get(id))
    }

    pub fn list_models(&self, provider_id: &str) -> &[OwnedModelDef] {
        let provider_id = registry::canonical_provider_id(provider_id);
        self.models_by_provider
            .get(provider_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn get_model(&self, provider_id: &str, model_id: &str) -> Option<&OwnedModelDef> {
        self.list_models(provider_id)
            .iter()
            .find(|model| model.id == model_id)
    }

    pub fn resolve_backend(&self, provider_id: &str) -> Option<(&'static str, &str)> {
        let p = self.get_provider(provider_id)?;
        let kind = match p.protocol {
            ProviderProtocol::OpenAICompat => "openai",
            ProviderProtocol::AzureOpenAI => "azure",
            ProviderProtocol::VertexAI => "vertex",
            ProviderProtocol::GeminiOpenAI => "gemini",
            ProviderProtocol::GeminiNative => "gemini",
            ProviderProtocol::AnthropicNative => "anthropic",
            ProviderProtocol::BedrockNative => "bedrock",
            ProviderProtocol::Custom => return None,
        };
        Some((kind, p.default_base_url.as_str()))
    }

    pub fn find_by_litellm_prefix(&self, prefix: &str) -> Option<&OwnedProviderDef> {
        let direct = self
            .providers
            .values()
            .find(|p| !p.litellm_prefix.is_empty() && prefix == p.litellm_prefix);
        if direct.is_some() {
            return direct;
        }

        let provider_id = prefix.strip_suffix('/')?;
        let canonical = registry::canonical_provider_id(provider_id);
        self.providers.get(canonical)
    }

    fn refresh_metadata_counts(&mut self) {
        self.metadata.provider_count = self.all_providers().count();
        self.metadata.model_count = self
            .advertised_provider_ids
            .iter()
            .filter_map(|id| self.models_by_provider.get(id))
            .map(Vec::len)
            .sum();
    }
}

#[derive(Default)]
struct ProviderRows {
    modes: BTreeSet<String>,
    tool_use: bool,
    vision: bool,
    models: BTreeMap<String, OwnedModelDef>,
}

impl ProviderRows {
    fn observe(&mut self, mode: &str, data: &serde_json::Map<String, Value>) {
        if !mode.is_empty() {
            self.modes.insert(mode.to_string());
        }
        self.tool_use = self.tool_use
            || truthy_json_field(data, "supports_function_calling")
            || truthy_json_field(data, "supports_tool_choice");
        self.vision = self.vision || truthy_json_field(data, "supports_vision");
    }

    fn provider_capabilities(&self, batch: bool) -> ProviderCapabilities {
        ProviderCapabilities {
            chat_completions: self.modes.contains("chat") || self.modes.contains("responses"),
            streaming: self.modes.contains("chat")
                || self.modes.contains("completion")
                || self.modes.contains("responses"),
            tool_use: self.tool_use,
            embeddings: self.modes.contains("embedding"),
            vision: self.vision,
            batch,
        }
    }
}

#[cfg(test)]
mod tests;
