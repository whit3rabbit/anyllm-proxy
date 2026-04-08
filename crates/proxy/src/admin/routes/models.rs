use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;
use std::sync::LazyLock;

/// Shared HTTP client for model discovery (lightweight, short timeout).
static DISCOVER_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("failed to build discover HTTP client")
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
pub(super) async fn discover_models(Json(body): Json<DiscoverRequest>) -> impl IntoResponse {
    let (url, api_key) = match resolve_discover_target(&body) {
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
    let mut req = DISCOVER_CLIENT.get(&url);
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
                    let name = m.get("name").and_then(|n| n.as_str()).map(String::from);
                    Some(DiscoveredModel { id, name })
                })
                .collect()
        })
        .unwrap_or_default();

    models.sort_unstable_by(|a, b| a.id.cmp(&b.id));

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
fn resolve_discover_target(body: &DiscoverRequest) -> Result<(String, Option<String>), String> {
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
                .filter(|u| !u.is_empty())
                .ok_or("url is required for custom source")?;
            let url = url.trim_end_matches('/');
            // If the URL already ends with /models, use as-is; otherwise append.
            let url = if url.ends_with("/models") {
                url.to_string()
            } else {
                format!("{url}/v1/models")
            };
            // Security: url is user-supplied (host + protocol fully controlled).
            // SSRF risk is gated by: (a) admin Bearer token required, (b) admin server
            // binds to 127.0.0.1 by default, (c) DISCOVER_CLIENT follows no redirects.
            Ok((url, None))
        }
        other => Err(format!("unknown source: {other}")),
    }
}
