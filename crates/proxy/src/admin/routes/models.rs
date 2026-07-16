use crate::admin::state::SharedState;
use anyllm_client::http::{build_http_client, HttpClientConfig};
use anyllm_providers::provider::ProviderProtocol;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;
use std::sync::LazyLock;

/// Shared HTTP client for model discovery with SSRF-safe DNS and short timeout.
static DISCOVER_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    build_http_client(&HttpClientConfig {
        connect_timeout: Some(std::time::Duration::from_secs(10)),
        request_timeout: Some(std::time::Duration::from_secs(15)),
        ..HttpClientConfig::new()
    })
});

/// Local discovery is only for explicit local provider sources such as Ollama and for
/// managed backends whose provider is a local LLM server. Allows loopback + private IPs
/// (localhost/LAN); cloud-metadata IPs stay blocked by the SSRF-safe DNS resolver.
static LOCAL_DISCOVER_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    build_http_client(&HttpClientConfig {
        connect_timeout: Some(std::time::Duration::from_secs(10)),
        request_timeout: Some(std::time::Duration::from_secs(15)),
        ssrf_allow_loopback: true,
        ssrf_allow_private: true,
        ..HttpClientConfig::new()
    })
});

/// GET /admin/api/models -- list all routed model names and deployment counts.
pub(super) async fn list_models(State(shared): State<SharedState>) -> impl IntoResponse {
    if let Some(ref router_lock) = shared.model_router {
        let router = router_lock.read().unwrap_or_else(|e| e.into_inner());
        let models: Vec<serde_json::Value> = router
            .list_models()
            .into_iter()
            .map(|(name, count)| {
                serde_json::json!({
                    "model_name": name,
                    "deployments": count,
                })
            })
            .collect();
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "strategy": format!("{:?}", router.strategy()),
                "models": models,
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "strategy": null,
                "models": [],
                "note": "no model router active (not using LiteLLM config)"
            })),
        )
            .into_response()
    }
}

/// Request body for POST /admin/api/models.
#[derive(serde::Deserialize)]
pub(super) struct AddModelRequest {
    model_name: String,
    backend_name: String,
    actual_model: String,
    #[serde(default)]
    rpm: Option<u32>,
    #[serde(default)]
    tpm: Option<u64>,
    #[serde(default = "default_weight")]
    weight: u32,
}

/// Default weight for a new deployment when none is specified in the request body.
pub(super) fn default_weight() -> u32 {
    1
}

/// POST /admin/api/models -- add a deployment for a model name.
pub(super) async fn add_model(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<AddModelRequest>,
) -> impl IntoResponse {
    let Some(ref router_lock) = shared.model_router else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no model router active"})),
        )
            .into_response();
    };

    // Validate name fields to prevent log injection via control characters.
    for (field, value) in [
        ("model_name", &body.model_name),
        ("backend_name", &body.backend_name),
        ("actual_model", &body.actual_model),
    ] {
        if !super::is_safe_model_name(value) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("invalid {field}: contains disallowed characters")
                })),
            )
                .into_response();
        }
    }

    // Validate that backend_name refers to a configured backend.
    // Check static backends first, then managed backends (SQLite-backed).
    let backend_known = shared.backend_metrics.contains_key(&body.backend_name)
        || shared
            .managed_backends
            .read()
            .map(|m| m.contains_key(&body.backend_name))
            .unwrap_or(false);

    if !backend_known {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("unknown backend: {}", body.backend_name)
            })),
        )
            .into_response();
    }

    let deployment = std::sync::Arc::new(crate::config::model_router::Deployment::with_weight(
        body.backend_name.clone(),
        body.actual_model.clone(),
        body.rpm,
        body.tpm,
        body.weight,
    ));

    let mut router = router_lock.write().unwrap_or_else(|e| e.into_inner());
    router.add_deployment(body.model_name.clone(), deployment);

    // Persist to SQLite so the deployment survives restarts.
    if let Ok(db) = shared.db.lock() {
        if let Err(e) = crate::admin::db::insert_model_deployment(
            &db,
            &body.model_name,
            &body.backend_name,
            &body.actual_model,
            body.rpm,
            body.tpm,
            body.weight,
        ) {
            tracing::warn!(error = %e, "failed to persist model deployment to SQLite");
        }
    }

    tracing::info!(
        model_name = %body.model_name,
        backend = %body.backend_name,
        actual_model = %body.actual_model,
        "added model deployment via admin API"
    );

    super::emit_audit(
        &shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: "model_added".into(),
            target_type: "model".into(),
            target_id: Some(body.model_name.clone()),
            detail: Some(format!(
                "backend={}, actual_model={}",
                body.backend_name, body.actual_model
            )),
            source_ip: Some(addr.ip().to_string()),
        },
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "status": "added",
            "model_name": body.model_name,
            "backend_name": body.backend_name,
            "actual_model": body.actual_model,
        })),
    )
        .into_response()
}

/// DELETE /admin/api/models/{name} -- remove all deployments for a model.
pub(super) async fn remove_model(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let Some(ref router_lock) = shared.model_router else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no model router active"})),
        )
            .into_response();
    };

    let mut router = router_lock.write().unwrap_or_else(|e| e.into_inner());
    if router.remove_model(&name) {
        // Remove from SQLite as well.
        if let Ok(db) = shared.db.lock() {
            if let Err(e) = crate::admin::db::delete_model_deployments(&db, &name) {
                tracing::warn!(error = %e, "failed to remove model deployment from SQLite");
            }
        }
        tracing::info!(model_name = %name, "removed model via admin API");
        super::emit_audit(
            &shared,
            crate::admin::db::AuditEntry {
                id: None,
                timestamp: None,
                action: "model_removed".into(),
                target_type: "model".into(),
                target_id: Some(name.clone()),
                detail: None,
                source_ip: Some(addr.ip().to_string()),
            },
        );
        (
            StatusCode::OK,
            Json(serde_json::json!({"status": "removed", "model_name": name})),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "model not found", "model_name": name})),
        )
            .into_response()
    }
}

// ── Model discovery ──────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub(super) struct DiscoverRequest {
    source: String,
    #[serde(default)]
    url: Option<String>,
    /// Provider being configured. When it is a local LLM server (loopback/private
    /// default base URL), SSRF is relaxed for discovery so localhost/LAN targets work.
    #[serde(default)]
    provider_id: Option<String>,
    /// Optional key for a `custom` source that enforces one (e.g. a keyed LM Studio).
    #[serde(default)]
    api_key: Option<String>,
}

#[derive(serde::Serialize)]
struct DiscoverResponse {
    models: Vec<DiscoveredModel>,
    source: String,
    auth_used: bool,
}

#[derive(serde::Serialize)]
struct DiscoveredModel {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

/// POST /admin/api/models/discover -- fetch available models from a provider.
pub(super) async fn discover_models(
    State(shared): State<SharedState>,
    Json(body): Json<DiscoverRequest>,
) -> impl IntoResponse {
    // Look the provider up once; derive local-ness and protocol from the catalog,
    // never from a client flag.
    let provider_def = body
        .provider_id
        .as_deref()
        .and_then(|id| shared.provider_catalog.get_provider(id));
    let provider_is_local = provider_def.is_some_and(|p| p.is_local());
    // Anthropic-native providers list models at /v1/models too, but authenticate with
    // x-api-key + anthropic-version instead of a Bearer token.
    let is_anthropic =
        provider_def.is_some_and(|p| p.protocol == ProviderProtocol::AnthropicNative);

    let (url, api_key) = match resolve_discover_target(&body, provider_is_local) {
        Ok(v) => v,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };

    let auth_used = api_key.is_some();
    let client = if body.source == "ollama" || provider_is_local {
        &*LOCAL_DISCOVER_CLIENT
    } else {
        &*DISCOVER_CLIENT
    };
    let mut req = client.get(&url);
    if let Some(ref key) = api_key {
        if is_anthropic {
            req = req
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01");
        } else {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
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
                "error": "API key required. Configure a key in Settings, then try again."
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
                .into_response();
        }
    };

    // Standard OpenAI format: { "data": [{ "id": "...", "name": "..." }, ...] }
    let mut models: Vec<DiscoveredModel> = json
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m.get("id")?.as_str()?.to_string();
                    // OpenAI uses "name"; Anthropic list-models uses "display_name".
                    let name = m
                        .get("name")
                        .or_else(|| m.get("display_name"))
                        .and_then(|n| n.as_str())
                        .map(String::from);
                    Some(DiscoveredModel { id, name })
                })
                .collect()
        })
        .unwrap_or_default();

    models.sort_unstable_by(|a, b| a.id.cmp(&b.id));

    // Persist discovered ids to provider_models_cache so they survive the request
    // and feed the autorouter datalist (which reads cached_models). Keyed by
    // provider id; mirrors refresh_provider_models. Skip an empty result: the upsert
    // is DELETE-then-INSERT, so persisting zero ids (a 200 with no `data`, or a
    // non-OpenAI JSON shape) would wipe a previously-good cache. Awaited on purpose so
    // the client's follow-up cache refetch observes the write (read-after-write).
    if let Some(pid) = body.provider_id.clone() {
        let ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
        if !ids.is_empty() {
            let db_arc = shared.db.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let mut conn = db_arc.lock().unwrap_or_else(|e| e.into_inner());
                if let Err(e) = crate::admin::db::upsert_provider_models_cache(&mut conn, &pid, &ids)
                {
                    tracing::warn!(provider = %pid, error = %e, "failed to cache discovered models");
                }
            })
            .await;
        }
    }

    (
        StatusCode::OK,
        Json(DiscoverResponse {
            models,
            source: body.source,
            auth_used,
        }),
    )
        .into_response()
}

/// Map the source name to a (URL, optional API key) pair.
///
/// `allow_local` is true when the provider being configured is a local LLM server; it
/// relaxes the private/loopback SSRF rejection for the `custom` source (the scheme check
/// stays, and the local discover client still blocks cloud-metadata IPs).
fn resolve_discover_target(
    body: &DiscoverRequest,
    allow_local: bool,
) -> Result<(String, Option<String>), String> {
    match body.source.as_str() {
        "openrouter" => Ok(("https://openrouter.ai/api/v1/models".into(), None)),
        "deepinfra" => Ok(("https://api.deepinfra.com/v1/openai/models".into(), None)),
        "ollama" => {
            let base = std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434".into());
            let base = base.trim_end_matches('/');
            // Ollama exposes /v1/models when running in OpenAI-compat mode,
            // but the native endpoint is /api/tags. Try /v1/models first.
            Ok((format!("{base}/v1/models"), None))
        }
        "configured" => {
            let base = std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com".into());
            let base = base.trim_end_matches('/');
            let key = std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|k| !k.is_empty());
            Ok((format!("{base}/v1/models"), key))
        }
        "custom" => {
            let url = body
                .url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .ok_or("url is required for custom source")?;
            let url = url.trim_end_matches('/');
            if allow_local {
                // Local provider: allow loopback/LAN targets but NOT public hosts, so
                // `allow_local` can't turn discovery into a general outbound fetch/SSRF.
                // Cloud-metadata + link-local stay blocked here and again by the resolver.
                let parsed = url::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
                match parsed.scheme() {
                    "http" | "https" => {}
                    other => {
                        return Err(format!("scheme '{other}' not allowed, use http or https"))
                    }
                }
                ensure_local_discover_host(&parsed)?;
            } else {
                crate::config::validate_base_url(url)?;
            }
            // Append the models path, but don't double up: catalog default base URLs
            // for local providers already end in /v1 (e.g. http://host:4444/v1), so a
            // blind /v1/models append produced /v1/v1/models.
            let url = if url.ends_with("/models") {
                url.to_string()
            } else if url.ends_with("/v1") {
                format!("{url}/models")
            } else {
                format!("{url}/v1/models")
            };
            Ok((url, body.api_key.clone()))
        }
        other => Err(format!("unknown source: {other}")),
    }
}

/// For the local-provider discover path, require the target host be loopback,
/// an RFC 1918 LAN IP, or literally `localhost`. Public IPs and other host names
/// are rejected so `allow_local` cannot be abused as a general outbound fetch.
/// Metadata/link-local IPs are rejected here (and again by the connect resolver).
fn ensure_local_discover_host(parsed: &url::Url) -> Result<(), String> {
    use anyllm_client::http::is_blocked_ip;
    use std::net::IpAddr;
    let local_ip_ok = |ip: IpAddr| -> bool {
        let is_local = match ip {
            IpAddr::V4(v4) => v4.is_loopback() || v4.is_private(),
            IpAddr::V6(v6) => v6.is_loopback(),
        };
        // is_blocked_ip(_, true, true) still rejects metadata/link-local/unspecified.
        is_local && !is_blocked_ip(ip, true, true)
    };
    match parsed.host() {
        Some(url::Host::Ipv4(v4)) if local_ip_ok(IpAddr::V4(v4)) => Ok(()),
        Some(url::Host::Ipv6(v6)) if local_ip_ok(IpAddr::V6(v6)) => Ok(()),
        Some(url::Host::Domain(d)) if d.eq_ignore_ascii_case("localhost") => Ok(()),
        _ => Err("local discovery requires a loopback/LAN IP or 'localhost'".into()),
    }
}

#[cfg(test)]
#[path = "models/tests.rs"]
mod tests;
