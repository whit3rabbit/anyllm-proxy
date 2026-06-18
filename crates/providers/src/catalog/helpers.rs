use crate::catalog::OwnedProviderDef;
use crate::provider::{AuthKind, ProviderCapabilities, ProviderProtocol, ProviderStatus};

use serde_json::Value;

pub(crate) fn normalize_litellm_model_id(model: &str, provider: &str) -> String {
    model
        .strip_prefix(&format!("{provider}/"))
        .unwrap_or(model)
        .to_string()
}

pub(crate) fn truthy_json_field(data: &serde_json::Map<String, Value>, field: &str) -> bool {
    data.get(field).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn u32_json_field(data: &serde_json::Map<String, Value>, field: &str) -> u32 {
    data.get(field).and_then(value_as_u32).unwrap_or(0)
}

fn value_as_u32(value: &Value) -> Option<u32> {
    if let Some(n) = value.as_u64() {
        return u32::try_from(n).ok();
    }
    value.as_str()?.parse::<u32>().ok()
}

pub(crate) fn default_runtime_provider(provider: &str) -> OwnedProviderDef {
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

pub(crate) fn display_name(provider: &str) -> String {
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

pub(crate) fn default_protocol(provider: &str) -> ProviderProtocol {
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

pub(crate) fn default_auth(protocol: ProviderProtocol) -> AuthKind {
    match protocol {
        ProviderProtocol::AzureOpenAI => AuthKind::AzureApiKey,
        ProviderProtocol::GeminiOpenAI
        | ProviderProtocol::GeminiNative
        | ProviderProtocol::VertexAI => AuthKind::GoogleApiKey,
        ProviderProtocol::BedrockNative => AuthKind::AwsSigV4,
        _ => AuthKind::Bearer,
    }
}

pub(crate) fn guessed_env_vars(
    provider: &str,
    protocol: ProviderProtocol,
    auth: AuthKind,
) -> Vec<String> {
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
use crate::catalog::{CatalogError, CatalogMetadata, ProviderCatalog};
#[cfg(feature = "remote-catalog")]
use std::path::{Path, PathBuf};

#[cfg(feature = "remote-catalog")]
pub(crate) fn same_effective_url(requested: &str, final_url: &str) -> bool {
    match (
        reqwest::Url::parse(requested),
        reqwest::Url::parse(final_url),
    ) {
        (Ok(requested), Ok(final_url)) => requested == final_url,
        _ => requested == final_url,
    }
}

#[cfg(feature = "remote-catalog")]
pub(crate) async fn response_text_limited(
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
pub(crate) fn stale_or_error(
    error: CatalogError,
    options: &crate::catalog::RemoteCatalogOptions,
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
pub(crate) fn cache_json_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("litellm_provider_catalog.json")
}

#[cfg(feature = "remote-catalog")]
pub(crate) fn cache_metadata_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("litellm_provider_catalog.meta.json")
}

#[cfg(feature = "remote-catalog")]
pub(crate) fn read_cache_metadata(cache_dir: &Path) -> Result<CatalogMetadata, CatalogError> {
    let json = std::fs::read_to_string(cache_metadata_path(cache_dir))?;
    serde_json::from_str(&json).map_err(CatalogError::Json)
}

#[cfg(feature = "remote-catalog")]
pub(crate) fn write_cache(
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
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), CatalogError> {
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
pub(crate) fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(feature = "remote-catalog")]
pub(crate) fn unix_now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
