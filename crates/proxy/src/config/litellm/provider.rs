use super::types::LiteLLMParams;
use crate::config::single::validate_gcp_identifier;
use crate::config::{resolve_env_value, strip_v1_suffix, BackendKind};

/// Parse LiteLLM's "provider/model_name" format.
/// No prefix defaults to OpenAI (matches LiteLLM behavior).
/// Returns (kind, model_name, stub_provider) where stub_provider is set for
/// registry-resolved OpenAI-compatible providers so callers can use their default URL.
pub(super) fn parse_provider_model(
    model: &str,
) -> (
    BackendKind,
    String,
    Option<&'static anyllm_providers::ProviderDef>,
) {
    let (provider, model_name) = model.split_once('/').unwrap_or(("openai", model));
    let mut stub_provider: Option<&'static anyllm_providers::ProviderDef> = None;
    let kind = match provider.to_ascii_lowercase().as_str() {
        "openai" => BackendKind::OpenAI,
        "azure" => BackendKind::AzureOpenAI,
        "vertex_ai" | "vertex" => BackendKind::Vertex,
        "gemini" => BackendKind::Gemini,
        "anthropic" => {
            stub_provider = anyllm_providers::get_provider("anthropic");
            BackendKind::Anthropic
        }
        "bedrock" => BackendKind::Bedrock,
        other => {
            let prefix_with_slash = format!("{other}/");
            if let Some(p) = anyllm_providers::find_by_litellm_prefix(&prefix_with_slash) {
                let resolved = match anyllm_providers::resolve_backend(p.id) {
                    Some(("openai", _)) => {
                        stub_provider = Some(p);
                        BackendKind::OpenAI
                    }
                    Some(("anthropic", _)) => BackendKind::Anthropic,
                    Some(("gemini", _)) => BackendKind::Gemini,
                    Some(("vertex", _)) => BackendKind::Vertex,
                    Some(("azure", _)) => BackendKind::AzureOpenAI,
                    Some(("bedrock", _)) => BackendKind::Bedrock,
                    _ => {
                        tracing::warn!(provider = %other, "provider found in registry but protocol not mappable, treating as openai-compatible");
                        stub_provider = Some(p);
                        BackendKind::OpenAI
                    }
                };
                resolved
            } else {
                tracing::warn!(
                    provider = %other,
                    "unknown LiteLLM provider, treating as openai-compatible"
                );
                BackendKind::OpenAI
            }
        }
    };
    (kind, model_name.to_string(), stub_provider)
}

pub(super) fn provider_id_for_litellm_model(
    model: &str,
    kind: &BackendKind,
    stub_provider: Option<&'static anyllm_providers::ProviderDef>,
) -> String {
    if let Some(provider) = stub_provider {
        return provider.id.to_string();
    }

    let raw_provider = model
        .split_once('/')
        .map(|(provider, _)| provider)
        .unwrap_or("openai")
        .to_ascii_lowercase();

    match kind {
        BackendKind::OpenAI => raw_provider,
        BackendKind::AzureOpenAI => "azure".to_string(),
        BackendKind::Vertex => "vertex_ai".to_string(),
        BackendKind::Gemini => "gemini".to_string(),
        BackendKind::Anthropic => "anthropic".to_string(),
        BackendKind::Bedrock => "bedrock".to_string(),
    }
}

/// Determine the base URL for a deployment, applying provider-specific defaults.
pub(super) fn resolve_base_url(
    kind: &BackendKind,
    params: &LiteLLMParams,
    stub_provider: Option<&'static anyllm_providers::ProviderDef>,
    actual_model: &str,
) -> String {
    if let Some(ref url) = params.api_base {
        let resolved =
            resolve_env_value(url).unwrap_or_else(|e| panic!("model_list api_base: {e}"));
        if *kind == BackendKind::AzureOpenAI {
            let api_version = params.api_version.as_deref().unwrap_or("2024-10-21");
            if !resolved.contains("/openai/deployments/") {
                let deployment = azure_deployment_from_model(actual_model);
                return format!(
                    "{}/openai/deployments/{deployment}/chat/completions?api-version={api_version}",
                    resolved.trim_end_matches('/'),
                );
            }
            if !resolved.contains("api-version=") {
                let sep = if resolved.contains('?') { '&' } else { '?' };
                return format!("{resolved}{sep}api-version={api_version}");
            }
            return resolved;
        }
        return resolved;
    }
    match kind {
        BackendKind::OpenAI => {
            let url = if let Some(provider) = stub_provider {
                if provider.default_base_url.is_empty() {
                    panic!(
                        "model_list provider '{}' requires api_base because it has no safe global API base URL",
                        provider.id
                    );
                }
                provider.default_base_url
            } else {
                "https://api.openai.com"
            };
            strip_v1_suffix(url).to_string()
        }
        BackendKind::Gemini => {
            "https://generativelanguage.googleapis.com/v1beta/openai".to_string()
        }
        BackendKind::Anthropic => std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
        BackendKind::Bedrock => params
            .aws_region_name
            .as_deref()
            .map(|v| v.to_string())
            .or_else(|| std::env::var("AWS_REGION").ok())
            .unwrap_or_else(|| "us-east-1".to_string()),
        BackendKind::AzureOpenAI => {
            panic!("api_base is required for azure deployments in model_list")
        }
        BackendKind::Vertex => {
            let project = params.vertex_project.as_deref().unwrap_or_else(|| {
                panic!("vertex_project is required for vertex deployments in model_list")
            });
            let location = params.vertex_location.as_deref().unwrap_or_else(|| {
                panic!("vertex_location is required for vertex deployments in model_list")
            });
            validate_gcp_identifier("vertex_project", project);
            validate_gcp_identifier("vertex_location", location);
            format!(
                "https://{location}-aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/endpoints/openapi"
            )
        }
    }
}

pub(super) fn azure_deployment_from_model(model: &str) -> &str {
    for marker in ["o_series/", "gpt5_series/"] {
        if let Some(deployment) = model.strip_prefix(marker) {
            if !deployment.is_empty() {
                return deployment;
            }
        }
    }
    model
}
