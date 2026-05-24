use crate::admin::state::SharedState;
use anyllm_client::http::{build_http_client, HttpClientConfig};
use anyllm_providers::{
    all_providers, get_provider, list_models,
    model::ModelStatus,
    provider::{AuthKind, ProviderProtocol, ProviderStatus},
};
use axum::{extract::Path, extract::State, http::StatusCode, response::IntoResponse, Json};
use std::sync::LazyLock;

static REFRESH_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    build_http_client(&HttpClientConfig {
        connect_timeout: Some(std::time::Duration::from_secs(10)),
        request_timeout: Some(std::time::Duration::from_secs(20)),
        ..HttpClientConfig::new()
    })
});

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
/// Returns all registered providers with metadata from the compile-time registry,
/// enriched with cached model count and last_refreshed timestamp from SQLite.
pub(super) async fn list_providers(State(shared): State<SharedState>) -> impl IntoResponse {
    // One query for all provider cache stats instead of two queries per provider.
    let cache_stats = if let Ok(db_guard) = shared.db.lock() {
        crate::admin::db::get_all_provider_cache_stats(&db_guard).unwrap_or_default()
    } else {
        Default::default()
    };

    let providers: Vec<serde_json::Value> = all_providers()
        .map(|p| {
            let model_count = list_models(p.id).len();
            let (cached_count, last_refreshed) = cache_stats
                .get(p.id)
                .map(|(c, r)| (*c, *r))
                .unwrap_or((0, None));
            serde_json::json!({
                "id":                p.id,
                "display_name":      p.display_name,
                "protocol":          protocol_str(p.protocol),
                "auth":              auth_str(p.auth),
                "status":            provider_status_str(p.status),
                "default_base_url":  p.default_base_url,
                "env_vars":          p.env_vars,
                "litellm_prefix":    p.litellm_prefix,
                "capabilities": {
                    "chat_completions": p.capabilities.chat_completions,
                    "streaming":        p.capabilities.streaming,
                    "tool_use":         p.capabilities.tool_use,
                    "embeddings":       p.capabilities.embeddings,
                    "vision":           p.capabilities.vision,
                    "batch":            p.capabilities.batch,
                },
                "model_count":        model_count,
                "cached_model_count": cached_count,
                "last_refreshed":     last_refreshed,
            })
        })
        .collect();

    Json(serde_json::json!({ "providers": providers })).into_response()
}

/// GET /admin/api/catalog/providers/{id}/models
///
/// Returns all static models for the given provider, enriched with pricing data
/// from the embedded pricing table, plus any cached model IDs from live refreshes.
/// Returns 404 for unknown provider ids.
pub(super) async fn list_provider_models(
    State(shared): State<SharedState>,
    Path(provider_id): Path<String>,
) -> impl IntoResponse {
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

    // Enrich with cached model IDs from live refreshes (best-effort).
    let cached_models: Vec<String> = if let Ok(db_guard) = shared.db.lock() {
        crate::admin::db::list_cached_provider_models(&db_guard, &provider_id).unwrap_or_default()
    } else {
        vec![]
    };

    let has_models = !models.is_empty();
    Json(serde_json::json!({
        "provider_id":    provider_id,
        "has_models":     has_models,
        "models":         models,
        "cached_models":  cached_models,
    }))
    .into_response()
}

/// POST /admin/api/catalog/providers/{id}/refresh
///
/// Calls GET {provider.default_base_url}/v1/models, parses the OpenAI-format
/// model list, and upserts the results into provider_models_cache.
pub(super) async fn refresh_provider_models(
    State(shared): State<SharedState>,
    Path(provider_id): Path<String>,
) -> impl IntoResponse {
    if !super::is_safe_model_name(&provider_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid provider id" })),
        )
            .into_response();
    }

    let provider = match get_provider(&provider_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "provider not found" })),
            )
                .into_response()
        }
    };

    // Only providers that serve chat completions expose a /v1/models endpoint.
    if !provider.capabilities.chat_completions {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "provider does not support model discovery"
            })),
        )
            .into_response();
    }

    // Resolve the API key from the first env var that is set.
    let api_key = provider.env_vars.iter().find_map(|v| std::env::var(v).ok());

    let url = format!(
        "{}/v1/models",
        provider.default_base_url.trim_end_matches('/')
    );
    let mut req = REFRESH_CLIENT.get(&url);
    if let Some(ref key) = api_key {
        req = req.header("Authorization", format!("Bearer {key}"));
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let msg = if e.is_connect() {
                format!("connection refused: {url}")
            } else if e.is_timeout() {
                format!("request timed out: {url}")
            } else {
                format!("request failed: {e}")
            };
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "API key required — set the provider env var and restart, \
                          or configure the key via Settings"
            })),
        )
            .into_response();
    }

    if !resp.status().is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": format!("upstream returned {}", resp.status())
            })),
        )
            .into_response();
    }

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("invalid JSON: {e}") })),
            )
                .into_response()
        }
    };

    let model_ids: Vec<String> = json
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id")?.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let count = model_ids.len();

    // Persist to SQLite via spawn_blocking (transaction needs &mut Connection).
    let db_arc = shared.db.clone();
    let pid = provider_id.clone();
    let ids = model_ids.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let mut conn = db_arc.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = crate::admin::db::upsert_provider_models_cache(&mut conn, &pid, &ids) {
            tracing::warn!(provider = %pid, error = %e, "failed to cache provider models");
        }
    })
    .await;

    tracing::info!(provider = %provider_id, count = count, "refreshed provider model cache");

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "provider_id": provider_id,
            "count":       count,
            "models":      model_ids,
        })),
    )
        .into_response()
}
