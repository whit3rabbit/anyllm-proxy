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
/// The default URL used to fetch the remote LiteLLM model catalog.
pub const LITELLM_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

#[cfg(feature = "remote-catalog")]
/// The default limit on downloaded catalog size (4MB) to prevent OOM/DoS.
pub const DEFAULT_MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;

/// An owned representation of a provider definition, suitable for serialization and storage.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnedProviderDef {
    /// The unique identifier of the provider.
    pub id: String,
    /// The user-facing display name of the provider.
    pub display_name: String,
    /// The default base URL for the provider API.
    pub default_base_url: String,
    /// The network protocol used to communicate with the provider.
    pub protocol: ProviderProtocol,
    /// The kind of authentication required by the provider.
    pub auth: AuthKind,
    /// The implementation status of the provider in the proxy.
    pub status: ProviderStatus,
    /// The environment variables containing API keys/secrets for this provider.
    pub env_vars: Vec<String>,
    /// The prefix used by LiteLLM to route requests to this provider.
    pub litellm_prefix: String,
    /// The capability flags supported by this provider.
    pub capabilities: ProviderCapabilities,
}

impl OwnedProviderDef {
    /// Returns true for local LLM servers (Ollama/LM Studio/vLLM/llamafile/...), detected by a
    /// loopback/private default base URL. Used to auto-relax SSRF protection for
    /// admin-configured managed backends pointing at localhost or a LAN address.
    pub fn is_local(&self) -> bool {
        base_url_is_local(&self.default_base_url)
    }
}

/// Detect a local LLM server by its default base URL. Kept as a free fn so both
/// `OwnedProviderDef::is_local` and the catalog assembler (which works with the
/// `&'static str` from `ProviderDef`) share one definition.
///
/// Matches on the parsed host only (not a raw substring) so a hosted provider
/// whose URL merely *contains* `localhost`/`127.0.0.1`/`0.0.0.0` in a subdomain,
/// path, or query is not misclassified as local (this gates SSRF relaxation).
pub fn base_url_is_local(base: &str) -> bool {
    let host = host_from_base_url(base);
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => v4.is_loopback() || v4.is_private() || v4.is_unspecified(),
        Ok(std::net::IpAddr::V6(v6)) => v6.is_loopback() || v6.is_unspecified(),
        Err(_) => false,
    }
}

/// Extract the host from a base URL without pulling in the `url` crate (the
/// providers crate is dependency-light). Handles scheme, userinfo, port, and
/// bracketed IPv6 literals; good enough for the curated `default_base_url` set.
fn host_from_base_url(base: &str) -> &str {
    let after_scheme = base.split_once("://").map_or(base, |(_, rest)| rest);
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let hostport = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if let Some(rest) = hostport.strip_prefix('[') {
        // [ipv6]:port -> ipv6
        return rest.split(']').next().unwrap_or(rest);
    }
    hostport.rsplit_once(':').map_or(hostport, |(host, _)| host)
}

/// An owned representation of a model definition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnedModelDef {
    /// The unique model identifier (e.g. "gpt-4o").
    pub id: String,
    /// The ID of the provider that offers this model.
    pub provider_id: String,
    /// The maximum input/context window size in tokens.
    pub context_window: u32,
    /// The maximum number of output tokens this model can generate.
    pub max_output_tokens: u32,
    /// The capabilities supported by this model (e.g. vision, streaming, tool use).
    pub capabilities: ModelCapabilities,
    /// The current status of this model (e.g. available, deprecated).
    pub status: ModelStatus,
}

/// Metadata about the current state/source of the provider catalog.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CatalogMetadata {
    /// The source of the catalog, e.g. "bundled" or "remote".
    pub source: String,
    /// The URL from which the catalog was fetched, if remote.
    pub source_url: Option<String>,
    /// The ETag of the remote HTTP response, if available.
    pub etag: Option<String>,
    /// The UNIX timestamp when the catalog was fetched.
    pub fetched_at_unix_secs: Option<u64>,
    /// The total number of providers defined in the catalog.
    pub provider_count: usize,
    /// The total number of models defined in the catalog.
    pub model_count: usize,
}

/// A structured catalog of all known backend providers and models.
/// Under the hood, this combines static metadata with optional LiteLLM remote snapshots.
#[derive(Debug, Clone)]
pub struct ProviderCatalog {
    /// Metadata about the catalog's source and fetch state.
    pub metadata: CatalogMetadata,
    /// Map of provider ID to its definition.
    pub providers: BTreeMap<String, OwnedProviderDef>,
    /// Set of provider IDs that should be actively advertised to the user/UI.
    pub advertised_provider_ids: BTreeSet<String>,
    /// Map of provider ID to the list of models it offers.
    pub models_by_provider: BTreeMap<String, Vec<OwnedModelDef>>,
    /// Helper index for matching providers by their LiteLLM prefix.
    pub provider_ids_by_litellm_prefix: BTreeMap<String, String>,
    /// Nested index maps for fast O(1) lookup of models by provider and model ID.
    pub model_indexes_by_provider: BTreeMap<String, BTreeMap<String, usize>>,
}

/// Errors that can occur when parsing or fetching the model catalog.
#[derive(Debug)]
pub enum CatalogError {
    /// JSON parsing or serialization errors.
    Json(serde_json::Error),
    /// File I/O or network stream read errors.
    Io(std::io::Error),
    /// Invalid shape or field format in the parsed LiteLLM catalog JSON.
    InvalidFormat(&'static str),
    /// The catalog content is not valid UTF-8.
    Utf8(std::string::FromUtf8Error),
    #[cfg(feature = "remote-catalog")]
    /// HTTP errors when fetching the remote catalog.
    Http(reqwest::Error),
    #[cfg(feature = "remote-catalog")]
    /// The remote server returned a non-200 HTTP status code.
    HttpStatus(u16),
    #[cfg(feature = "remote-catalog")]
    /// The downloaded catalog exceeds the specified size limit.
    TooLarge {
        /// The maximum allowed size in bytes.
        max_bytes: usize,
        /// The actual response size received.
        actual_bytes: usize,
    },
    #[cfg(feature = "remote-catalog")]
    /// The catalog cache is missing.
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
    /// Builds a catalog initialized with the bundled/static provider and model definitions.
    pub fn bundled() -> Self {
        let mut providers = BTreeMap::new();
        let mut advertised_provider_ids = BTreeSet::new();
        let mut models_by_provider = BTreeMap::new();

        for provider in registry::advertised_provider_defs() {
            advertised_provider_ids.insert(provider.id.to_string());
            providers.insert(provider.id.to_string(), OwnedProviderDef::from(*provider));
        }
        for provider in registry::legacy_only_provider_defs() {
            // Local LLM servers (ollama/lm_studio/llamafile/vllm/...) should be listable so the
            // admin UI can offer a "Local LLMs" section. Detect by localhost default base URL.
            if base_url_is_local(provider.default_base_url) {
                advertised_provider_ids.insert(provider.id.to_string());
            }
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
            provider_ids_by_litellm_prefix: BTreeMap::new(),
            model_indexes_by_provider: BTreeMap::new(),
        };
        catalog.refresh_metadata_counts();
        catalog
    }

    /// Parses a LiteLLM catalog JSON string and merges it with the bundled definitions.
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
                        tool_choice: truthy_json_field(data, "supports_tool_choice"),
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

    /// Returns a reference to the catalog's metadata.
    pub fn metadata(&self) -> &CatalogMetadata {
        &self.metadata
    }

    /// Looks up a provider definition by its ID.
    pub fn get_provider(&self, id: &str) -> Option<&OwnedProviderDef> {
        let id = registry::canonical_provider_id(id);
        self.providers.get(id)
    }

    /// Returns an iterator over all advertised provider definitions.
    pub fn all_providers(&self) -> impl Iterator<Item = &OwnedProviderDef> {
        self.advertised_provider_ids
            .iter()
            .filter_map(|id| self.providers.get(id))
    }

    /// Lists all models offered by a given provider.
    pub fn list_models(&self, provider_id: &str) -> &[OwnedModelDef] {
        let provider_id = registry::canonical_provider_id(provider_id);
        self.models_by_provider
            .get(provider_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Looks up a specific model definition by provider ID and model ID.
    pub fn get_model(&self, provider_id: &str, model_id: &str) -> Option<&OwnedModelDef> {
        let provider_id = registry::canonical_provider_id(provider_id);
        let index = self
            .model_indexes_by_provider
            .get(provider_id)?
            .get(model_id)?;
        self.models_by_provider.get(provider_id)?.get(*index)
    }

    /// Resolves the provider protocol kind and base URL for routing.
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

    /// Finds a provider definition based on a LiteLLM prefix string.
    pub fn find_by_litellm_prefix(&self, prefix: &str) -> Option<&OwnedProviderDef> {
        if let Some(provider_id) = self.provider_ids_by_litellm_prefix.get(prefix) {
            return self.providers.get(provider_id);
        }

        let provider_id = prefix.strip_suffix('/')?;
        let canonical = registry::canonical_provider_id(provider_id);
        self.providers.get(canonical)
    }

    fn refresh_metadata_counts(&mut self) {
        self.rebuild_indexes();
        self.metadata.provider_count = self.all_providers().count();
        self.metadata.model_count = self
            .advertised_provider_ids
            .iter()
            .filter_map(|id| self.models_by_provider.get(id))
            .map(Vec::len)
            .sum();
    }

    fn rebuild_indexes(&mut self) {
        self.provider_ids_by_litellm_prefix.clear();
        for (provider_id, provider) in &self.providers {
            if !provider.litellm_prefix.is_empty() {
                self.provider_ids_by_litellm_prefix
                    .insert(provider.litellm_prefix.clone(), provider_id.clone());
            }
        }

        self.model_indexes_by_provider.clear();
        for (provider_id, models) in &self.models_by_provider {
            let mut indexes = BTreeMap::new();
            for (index, model) in models.iter().enumerate() {
                indexes.insert(model.id.clone(), index);
            }
            self.model_indexes_by_provider
                .insert(provider_id.clone(), indexes);
        }
    }
}

#[derive(Default)]
struct ProviderRows {
    modes: BTreeSet<String>,
    tool_use: bool,
    tool_choice: bool,
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
        self.tool_choice = self.tool_choice || truthy_json_field(data, "supports_tool_choice");
        self.vision = self.vision || truthy_json_field(data, "supports_vision");
    }

    fn provider_capabilities(&self, batch: bool) -> ProviderCapabilities {
        ProviderCapabilities {
            chat_completions: self.modes.contains("chat") || self.modes.contains("responses"),
            streaming: self.modes.contains("chat")
                || self.modes.contains("completion")
                || self.modes.contains("responses"),
            tool_use: self.tool_use,
            tool_choice: self.tool_choice,
            embeddings: self.modes.contains("embedding"),
            vision: self.vision,
            batch,
        }
    }
}

#[cfg(test)]
mod tests;
