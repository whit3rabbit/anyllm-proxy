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

fn normalize_litellm_model_id(model: &str, provider: &str) -> String {
    model
        .strip_prefix(&format!("{provider}/"))
        .unwrap_or(model)
        .to_string()
}

fn truthy_json_field(data: &serde_json::Map<String, Value>, field: &str) -> bool {
    data.get(field).and_then(Value::as_bool).unwrap_or(false)
}

fn u32_json_field(data: &serde_json::Map<String, Value>, field: &str) -> u32 {
    data.get(field).and_then(value_as_u32).unwrap_or(0)
}

fn value_as_u32(value: &Value) -> Option<u32> {
    if let Some(n) = value.as_u64() {
        return u32::try_from(n).ok();
    }
    value.as_str()?.parse::<u32>().ok()
}

fn default_runtime_provider(provider: &str) -> OwnedProviderDef {
    let protocol = default_protocol(provider);
    let auth = default_auth(protocol);
    OwnedProviderDef {
        id: provider.to_string(),
        display_name: display_name(provider),
        default_base_url: String::new(),
        protocol,
        auth,
        status: ProviderStatus::Stub,
        env_vars: guessed_env_vars(provider, protocol, auth),
        litellm_prefix: format!("{provider}/"),
        capabilities: ProviderCapabilities::default(),
    }
}

fn display_name(provider: &str) -> String {
    match provider {
        "ai21" => "AI21".to_string(),
        "aws_polly" => "AWS Polly".to_string(),
        "bedrock" => "AWS Bedrock".to_string(),
        "bedrock_converse" => "AWS Bedrock Converse".to_string(),
        "bedrock_mantle" => "AWS Bedrock Mantle".to_string(),
        "github_copilot" => "GitHub Copilot".to_string(),
        "gmi" => "GMI Cloud".to_string(),
        "oci" => "Oracle Cloud Infrastructure".to_string(),
        "xai" => "xAI".to_string(),
        "zai" => "Z.ai".to_string(),
        _ => provider
            .split(['_', '-'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                if part.len() <= 3 {
                    part.to_ascii_uppercase()
                } else {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(first) => {
                            format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
                        }
                        None => String::new(),
                    }
                }
            })
            .collect::<Vec<String>>()
            .join(" "),
    }
}

fn default_protocol(provider: &str) -> ProviderProtocol {
    if provider == "anthropic" {
        return ProviderProtocol::AnthropicNative;
    }
    if provider == "gemini" || provider == "palm" {
        return ProviderProtocol::GeminiOpenAI;
    }
    if provider == "vertex_ai" || provider.starts_with("vertex_ai-") {
        return ProviderProtocol::VertexAI;
    }
    if provider == "azure" || provider.starts_with("azure") {
        return ProviderProtocol::AzureOpenAI;
    }
    if provider == "bedrock" || provider.starts_with("bedrock") || provider == "amazon_nova" {
        return ProviderProtocol::BedrockNative;
    }
    ProviderProtocol::OpenAICompat
}

fn default_auth(protocol: ProviderProtocol) -> AuthKind {
    match protocol {
        ProviderProtocol::AzureOpenAI => AuthKind::AzureApiKey,
        ProviderProtocol::GeminiOpenAI
        | ProviderProtocol::GeminiNative
        | ProviderProtocol::VertexAI => AuthKind::GoogleApiKey,
        ProviderProtocol::BedrockNative => AuthKind::AwsSigV4,
        _ => AuthKind::Bearer,
    }
}

fn guessed_env_vars(provider: &str, protocol: ProviderProtocol, auth: AuthKind) -> Vec<String> {
    if auth == AuthKind::AwsSigV4 || protocol == ProviderProtocol::BedrockNative {
        return vec![
            "AWS_ACCESS_KEY_ID".to_string(),
            "AWS_SECRET_ACCESS_KEY".to_string(),
            "AWS_REGION".to_string(),
        ];
    }
    if matches!(
        protocol,
        ProviderProtocol::VertexAI
            | ProviderProtocol::GeminiOpenAI
            | ProviderProtocol::GeminiNative
    ) {
        return vec!["GEMINI_API_KEY".to_string()];
    }
    if protocol == ProviderProtocol::AzureOpenAI {
        return vec!["AZURE_OPENAI_API_KEY".to_string()];
    }
    let env_name = provider
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if env_name.is_empty() {
        Vec::new()
    } else {
        vec![format!("{env_name}_API_KEY")]
    }
}

#[cfg(feature = "remote-catalog")]
#[derive(Debug, Clone)]
pub struct RemoteCatalogOptions {
    pub url: String,
    pub cache_dir: Option<PathBuf>,
    pub max_bytes: usize,
    pub stale_on_error: bool,
}

#[cfg(feature = "remote-catalog")]
impl Default for RemoteCatalogOptions {
    fn default() -> Self {
        Self {
            url: LITELLM_CATALOG_URL.to_string(),
            cache_dir: None,
            max_bytes: DEFAULT_MAX_CATALOG_BYTES,
            stale_on_error: false,
        }
    }
}

#[cfg(feature = "remote-catalog")]
impl RemoteCatalogOptions {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    pub fn with_cache_dir(mut self, cache_dir: impl AsRef<Path>) -> Self {
        self.cache_dir = Some(cache_dir.as_ref().to_path_buf());
        self
    }

    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    pub fn with_stale_on_error(mut self, stale_on_error: bool) -> Self {
        self.stale_on_error = stale_on_error;
        self
    }
}

#[cfg(feature = "remote-catalog")]
impl ProviderCatalog {
    pub fn load_litellm_cache(cache_dir: impl AsRef<Path>) -> Result<Self, CatalogError> {
        let cache_dir = cache_dir.as_ref();
        let json = std::fs::read_to_string(cache_json_path(cache_dir))?;
        let mut catalog = Self::from_litellm_json(&json)?;
        if let Ok(metadata_json) = std::fs::read_to_string(cache_metadata_path(cache_dir)) {
            catalog.metadata = serde_json::from_str(&metadata_json)?;
            catalog.refresh_metadata_counts();
        }
        Ok(catalog)
    }

    pub async fn fetch_litellm(
        client: &reqwest::Client,
        cache_dir: Option<&Path>,
    ) -> Result<Self, CatalogError> {
        let options = RemoteCatalogOptions {
            cache_dir: cache_dir.map(Path::to_path_buf),
            ..RemoteCatalogOptions::default()
        };
        Self::fetch_litellm_with_options(client, &options).await
    }

    pub async fn fetch_litellm_with_options(
        client: &reqwest::Client,
        options: &RemoteCatalogOptions,
    ) -> Result<Self, CatalogError> {
        let cache_metadata = options
            .cache_dir
            .as_deref()
            .and_then(|dir| read_cache_metadata(dir).ok())
            .filter(|metadata| metadata.source_url.as_deref() == Some(options.url.as_str()));
        let mut req = client
            .get(&options.url)
            .header(reqwest::header::USER_AGENT, "anyllm-providers/1.0");
        if let Some(etag) = cache_metadata.as_ref().and_then(|m| m.etag.as_ref()) {
            req = req.header(reqwest::header::IF_NONE_MATCH, etag);
        }

        let resp = match req.send().await {
            Ok(resp) => resp,
            Err(e) => return stale_or_error(CatalogError::Http(e), options),
        };
        let final_url = resp.url().as_str().to_string();
        if !same_effective_url(&options.url, &final_url) {
            return stale_or_error(
                CatalogError::Redirect {
                    requested: options.url.clone(),
                    final_url,
                },
                options,
            );
        }

        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            if let Some(cache_dir) = options.cache_dir.as_deref() {
                return Self::load_litellm_cache(cache_dir);
            }
            return Err(CatalogError::CacheMiss);
        }
        if !resp.status().is_success() {
            return stale_or_error(CatalogError::HttpStatus(resp.status().as_u16()), options);
        }

        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = match response_text_limited(resp, options.max_bytes).await {
            Ok(body) => body,
            Err(e) => return stale_or_error(e, options),
        };
        let mut catalog = match Self::from_litellm_json(&body) {
            Ok(catalog) => catalog,
            Err(e) => return stale_or_error(e, options),
        };
        catalog.metadata.source = "remote-litellm".to_string();
        catalog.metadata.source_url = Some(options.url.clone());
        catalog.metadata.etag = etag;
        catalog.metadata.fetched_at_unix_secs = Some(unix_now_secs());
        catalog.refresh_metadata_counts();

        if let Some(cache_dir) = options.cache_dir.as_deref() {
            write_cache(cache_dir, &body, &catalog.metadata)?;
        }

        Ok(catalog)
    }
}

#[cfg(feature = "remote-catalog")]
fn same_effective_url(requested: &str, final_url: &str) -> bool {
    match (
        reqwest::Url::parse(requested),
        reqwest::Url::parse(final_url),
    ) {
        (Ok(requested), Ok(final_url)) => requested == final_url,
        _ => requested == final_url,
    }
}

#[cfg(feature = "remote-catalog")]
async fn response_text_limited(
    mut resp: reqwest::Response,
    max_bytes: usize,
) -> Result<String, CatalogError> {
    if let Some(len) = resp.content_length() {
        if len > max_bytes as u64 {
            return Err(CatalogError::TooLarge {
                max_bytes,
                actual_bytes: usize::try_from(len).unwrap_or(usize::MAX),
            });
        }
    }

    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        let next_len = body.len().saturating_add(chunk.len());
        if next_len > max_bytes {
            return Err(CatalogError::TooLarge {
                max_bytes,
                actual_bytes: next_len,
            });
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(CatalogError::Utf8)
}

#[cfg(feature = "remote-catalog")]
fn stale_or_error(
    error: CatalogError,
    options: &RemoteCatalogOptions,
) -> Result<ProviderCatalog, CatalogError> {
    if options.stale_on_error {
        if let Some(cache_dir) = options.cache_dir.as_deref() {
            if let Ok(catalog) = ProviderCatalog::load_litellm_cache(cache_dir) {
                return Ok(catalog);
            }
        }
    }
    Err(error)
}

#[cfg(feature = "remote-catalog")]
fn cache_json_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("litellm_provider_catalog.json")
}

#[cfg(feature = "remote-catalog")]
fn cache_metadata_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("litellm_provider_catalog.meta.json")
}

#[cfg(feature = "remote-catalog")]
fn read_cache_metadata(cache_dir: &Path) -> Result<CatalogMetadata, CatalogError> {
    let json = std::fs::read_to_string(cache_metadata_path(cache_dir))?;
    serde_json::from_str(&json).map_err(CatalogError::Json)
}

#[cfg(feature = "remote-catalog")]
fn write_cache(
    cache_dir: &Path,
    raw_json: &str,
    metadata: &CatalogMetadata,
) -> Result<(), CatalogError> {
    std::fs::create_dir_all(cache_dir)?;
    atomic_write(&cache_json_path(cache_dir), raw_json.as_bytes())?;
    let metadata_json = serde_json::to_vec_pretty(metadata)?;
    atomic_write(&cache_metadata_path(cache_dir), &metadata_json)?;
    Ok(())
}

#[cfg(feature = "remote-catalog")]
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), CatalogError> {
    let parent = path
        .parent()
        .ok_or(CatalogError::InvalidFormat("cache path must have a parent"))?;
    let tmp = parent.join(format!(
        ".{}.tmp.{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("catalog"),
        std::process::id(),
        unix_now_nanos()
    ));
    {
        let mut file = std::fs::File::create(&tmp)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(feature = "remote-catalog")]
fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(feature = "remote-catalog")]
fn unix_now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
const TEST_FIXTURE: &str = r#"{
      "openai/gpt-fresh": {
        "litellm_provider": "openai",
        "mode": "chat",
        "max_input_tokens": 12345,
        "max_output_tokens": 678,
        "supports_function_calling": true,
        "supports_vision": true,
        "supports_reasoning": true
      },
      "newco/new-model": {
        "litellm_provider": "newco",
        "mode": "chat",
        "max_input_tokens": 4000,
        "max_output_tokens": 500,
        "supports_tool_choice": true,
        "deprecation_date": "2026-01-01"
      },
      "newco/embed-v1": {
        "litellm_provider": "newco",
        "mode": "embedding",
        "max_input_tokens": 8192,
        "max_output_tokens": 0
      },
      "sample_spec": {
        "litellm_provider": "one of https://docs.litellm.ai/docs/providers"
      }
    }"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_matches_static_lookup_shape() {
        let catalog = ProviderCatalog::bundled();

        assert_eq!(
            catalog.all_providers().count(),
            crate::registry::all_providers().count()
        );
        assert_eq!(
            catalog.get_provider("gmi_cloud").unwrap().id,
            crate::registry::get_provider("gmi_cloud").unwrap().id
        );
        assert_eq!(
            catalog.list_models("zhipuai").len(),
            crate::registry::list_models("zhipuai").len()
        );
        assert!(catalog
            .all_providers()
            .all(|provider| provider.id != "lm_studio"));
        assert!(catalog.get_provider("lm_studio").is_some());
    }

    #[test]
    fn litellm_overlay_preserves_known_provider_metadata() {
        let catalog = ProviderCatalog::from_litellm_json(TEST_FIXTURE).unwrap();
        let provider = catalog.get_provider("openai").unwrap();
        let static_provider = crate::registry::get_provider("openai").unwrap();

        assert_eq!(provider.default_base_url, static_provider.default_base_url);
        assert_eq!(provider.protocol, static_provider.protocol);
        assert_eq!(provider.auth, static_provider.auth);
        assert_eq!(provider.env_vars, static_provider.env_vars);
        assert!(provider.capabilities.chat_completions);
        assert!(provider.capabilities.tool_use);
        assert!(provider.capabilities.vision);
    }

    #[test]
    fn litellm_overlay_adds_new_provider_without_base_url() {
        let catalog = ProviderCatalog::from_litellm_json(TEST_FIXTURE).unwrap();
        let provider = catalog.get_provider("newco").unwrap();

        assert_eq!(provider.status, ProviderStatus::Stub);
        assert_eq!(provider.default_base_url, "");
        assert_eq!(provider.env_vars, vec!["NEWCO_API_KEY"]);
        assert_eq!(provider.litellm_prefix, "newco/");
        assert!(provider.capabilities.chat_completions);
        assert!(provider.capabilities.embeddings);
        assert!(provider.capabilities.tool_use);

        let (kind, base_url) = catalog.resolve_backend("newco").unwrap();
        assert_eq!(kind, "openai");
        assert_eq!(base_url, "");
    }

    #[test]
    fn litellm_overlay_normalizes_models_and_capabilities() {
        let catalog = ProviderCatalog::from_litellm_json(TEST_FIXTURE).unwrap();
        let openai_model = catalog.get_model("openai", "gpt-fresh").unwrap();
        assert_eq!(openai_model.context_window, 12345);
        assert_eq!(openai_model.max_output_tokens, 678);
        assert!(openai_model.capabilities.streaming);
        assert!(openai_model.capabilities.tool_use);
        assert!(openai_model.capabilities.vision);
        assert!(openai_model.capabilities.extended_thinking);

        let new_model = catalog.get_model("newco", "new-model").unwrap();
        assert_eq!(new_model.status, ModelStatus::Deprecated);
        assert_eq!(
            catalog.find_by_litellm_prefix("newco/").unwrap().id,
            "newco"
        );
    }
}

#[cfg(all(test, feature = "remote-catalog"))]
mod remote_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct MockResponse {
        status: &'static str,
        headers: Vec<(&'static str, &'static str)>,
        body: String,
    }

    async fn mock_server(responses: Vec<MockResponse>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let request_log = Arc::clone(&requests);
        tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                request_log
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).to_string());
                let mut raw = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    response.status,
                    response.body.len()
                );
                for (name, value) in response.headers {
                    raw.push_str(name);
                    raw.push_str(": ");
                    raw.push_str(value);
                    raw.push_str("\r\n");
                }
                raw.push_str("\r\n");
                raw.push_str(&response.body);
                stream.write_all(raw.as_bytes()).await.unwrap();
            }
        });
        (url, requests)
    }

    fn temp_cache_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anyllm_providers_{name}_{}_{}",
            std::process::id(),
            unix_now_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn remote_200_writes_cache_and_304_uses_it() {
        let (url, requests) = mock_server(vec![
            MockResponse {
                status: "200 OK",
                headers: vec![("ETag", "\"abc\"")],
                body: TEST_FIXTURE.to_string(),
            },
            MockResponse {
                status: "304 Not Modified",
                headers: vec![],
                body: String::new(),
            },
        ])
        .await;
        let cache_dir = temp_cache_dir("etag");
        let options = RemoteCatalogOptions::new(url).with_cache_dir(&cache_dir);
        let client = http_client();

        let first = ProviderCatalog::fetch_litellm_with_options(&client, &options)
            .await
            .unwrap();
        assert_eq!(first.metadata().etag.as_deref(), Some("\"abc\""));
        assert!(cache_json_path(&cache_dir).exists());
        assert!(cache_metadata_path(&cache_dir).exists());

        let second = ProviderCatalog::fetch_litellm_with_options(&client, &options)
            .await
            .unwrap();
        assert!(second.get_provider("newco").is_some());
        assert!(requests.lock().unwrap()[1]
            .to_ascii_lowercase()
            .contains("if-none-match: \"abc\""));

        let _ = std::fs::remove_dir_all(cache_dir);
    }

    #[tokio::test]
    async fn remote_rejects_oversized_response() {
        let body = format!("{TEST_FIXTURE} ");
        let (url, _) = mock_server(vec![MockResponse {
            status: "200 OK",
            headers: vec![],
            body,
        }])
        .await;
        let options = RemoteCatalogOptions::new(url).with_max_bytes(8);
        let client = http_client();

        let err = ProviderCatalog::fetch_litellm_with_options(&client, &options)
            .await
            .unwrap_err();
        assert!(matches!(err, CatalogError::TooLarge { .. }));
    }

    #[tokio::test]
    async fn invalid_json_falls_back_only_when_requested() {
        let (url, _) = mock_server(vec![
            MockResponse {
                status: "200 OK",
                headers: vec![],
                body: TEST_FIXTURE.to_string(),
            },
            MockResponse {
                status: "200 OK",
                headers: vec![],
                body: "{not json".to_string(),
            },
            MockResponse {
                status: "200 OK",
                headers: vec![],
                body: "{not json".to_string(),
            },
        ])
        .await;
        let cache_dir = temp_cache_dir("invalid_json");
        let client = http_client();
        let options = RemoteCatalogOptions::new(url).with_cache_dir(&cache_dir);

        ProviderCatalog::fetch_litellm_with_options(&client, &options)
            .await
            .unwrap();
        let err = ProviderCatalog::fetch_litellm_with_options(&client, &options)
            .await
            .unwrap_err();
        assert!(matches!(err, CatalogError::Json(_)));

        let stale_options = options.with_stale_on_error(true);
        let stale = ProviderCatalog::fetch_litellm_with_options(&client, &stale_options)
            .await
            .unwrap();
        assert!(stale.get_provider("newco").is_some());

        let _ = std::fs::remove_dir_all(cache_dir);
    }
}
