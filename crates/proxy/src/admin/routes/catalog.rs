use anyllm_providers::{
    all_providers, get_provider, list_models,
    model::ModelStatus,
    provider::{AuthKind, ProviderProtocol, ProviderStatus},
};
use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};

fn protocol_str(p: ProviderProtocol) -> &'static str {
    match p {
        ProviderProtocol::OpenAICompat => "openai_compat",
        ProviderProtocol::AzureOpenAI => "azure_openai",
        ProviderProtocol::VertexAI => "vertex_ai",
        ProviderProtocol::GeminiOpenAI => "gemini_openai",
        ProviderProtocol::GeminiNative => "gemini_native",
        ProviderProtocol::AnthropicNative => "anthropic_native",
        ProviderProtocol::BedrockNative => "bedrock_native",
        ProviderProtocol::Custom => "custom",
    }
}

fn auth_str(a: AuthKind) -> &'static str {
    match a {
        AuthKind::Bearer => "bearer",
        AuthKind::GoogleApiKey => "google_api_key",
        AuthKind::AzureApiKey => "azure_api_key",
        AuthKind::AwsSigV4 => "aws_sigv4",
        AuthKind::None => "none",
    }
}

fn provider_status_str(s: ProviderStatus) -> &'static str {
    match s {
        ProviderStatus::Implemented => "implemented",
        ProviderStatus::Wired => "wired",
        ProviderStatus::Stub => "stub",
    }
}

fn model_status_str(s: ModelStatus) -> &'static str {
    match s {
        ModelStatus::Available => "available",
        ModelStatus::Deprecated => "deprecated",
        ModelStatus::Stub => "stub",
    }
}

/// GET /admin/api/catalog/providers
///
/// Returns all registered providers with metadata from the compile-time registry.
/// No SharedState needed — data is all static.
pub(super) async fn list_providers() -> impl IntoResponse {
    let providers: Vec<serde_json::Value> = all_providers()
        .map(|p| {
            let model_count = list_models(p.id).len();
            serde_json::json!({
                "id":               p.id,
                "display_name":     p.display_name,
                "protocol":         protocol_str(p.protocol),
                "auth":             auth_str(p.auth),
                "status":           provider_status_str(p.status),
                "default_base_url": p.default_base_url,
                "env_vars":         p.env_vars,
                "litellm_prefix":   p.litellm_prefix,
                "capabilities": {
                    "chat_completions": p.capabilities.chat_completions,
                    "streaming":        p.capabilities.streaming,
                    "tool_use":         p.capabilities.tool_use,
                    "embeddings":       p.capabilities.embeddings,
                    "vision":           p.capabilities.vision,
                    "batch":            p.capabilities.batch,
                },
                "model_count": model_count,
            })
        })
        .collect();

    Json(serde_json::json!({ "providers": providers })).into_response()
}

/// GET /admin/api/catalog/providers/{id}/models
///
/// Returns all static models for the given provider, enriched with pricing data
/// from the embedded pricing table. Returns 404 for unknown provider ids.
pub(super) async fn list_provider_models(Path(provider_id): Path<String>) -> impl IntoResponse {
    if !super::is_safe_model_name(&provider_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid provider id" })),
        )
            .into_response();
    }

    if get_provider(&provider_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "provider not found" })),
        )
            .into_response();
    }

    let models: Vec<serde_json::Value> = list_models(&provider_id)
        .iter()
        .map(|m| {
            let pricing = crate::cost::price_per_million_for_model(m.id).map(|(inp, out)| {
                serde_json::json!({
                    "input_per_million_tokens":  inp,
                    "output_per_million_tokens": out,
                })
            });
            serde_json::json!({
                "id":                m.id,
                "context_window":    m.context_window,
                "max_output_tokens": m.max_output_tokens,
                "status":            model_status_str(m.status),
                "capabilities": {
                    "streaming":         m.capabilities.streaming,
                    "tool_use":          m.capabilities.tool_use,
                    "vision":            m.capabilities.vision,
                    "extended_thinking": m.capabilities.extended_thinking,
                },
                "pricing": pricing,
            })
        })
        .collect();

    let has_models = !models.is_empty();
    Json(serde_json::json!({
        "provider_id": provider_id,
        "has_models":  has_models,
        "models":      models,
    }))
    .into_response()
}
