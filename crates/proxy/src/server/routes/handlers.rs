use super::helpers::{backend_error_to_response, enforce_model_allowlist_from_json_body};
use crate::server::state::AppState;
use anyllm_providers::ProviderCatalog;
use axum::extract::State;
use axum::response::{IntoResponse, Json, Response};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, Weak};

struct CachedAnthropicCatalogRows {
    catalog: Weak<ProviderCatalog>,
    rows: Arc<AnthropicCatalogRows>,
}

struct AnthropicCatalogRows {
    rows: Arc<[serde_json::Value]>,
    ids: Arc<HashSet<String>>,
}

static ANTHROPIC_CATALOG_ROWS_CACHE: LazyLock<Mutex<HashMap<usize, CachedAnthropicCatalogRows>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn anthropic_catalog_model_rows(catalog: &ProviderCatalog) -> Vec<serde_json::Value> {
    catalog
        .list_models("anthropic")
        .iter()
        .map(|model| {
            serde_json::json!({
                "id": model.id.as_str(),
                "object": "model",
                "created": 0,
                "owned_by": "anthropic",
                "display_name": claude_display_name(&model.id),
            })
        })
        .collect()
}

fn cached_anthropic_catalog_model_rows(
    catalog: &Arc<ProviderCatalog>,
) -> Arc<AnthropicCatalogRows> {
    let key = Arc::as_ptr(catalog) as usize;
    {
        let cache = ANTHROPIC_CATALOG_ROWS_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.get(&key) {
            if let Some(cached_catalog) = entry.catalog.upgrade() {
                if Arc::ptr_eq(&cached_catalog, catalog) {
                    return entry.rows.clone();
                }
            }
        }
    }

    // Build outside the lock so a miss doesn't serialize concurrent /v1/models
    // callers behind the row construction.
    let rows = anthropic_catalog_model_rows(catalog);
    let ids = rows
        .iter()
        .filter_map(|model| model["id"].as_str().map(str::to_string))
        .collect();
    let rows = Arc::new(AnthropicCatalogRows {
        rows: Arc::from(rows),
        ids: Arc::new(ids),
    });
    let mut cache = ANTHROPIC_CATALOG_ROWS_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Drop entries whose catalog has been freed so the pointer-keyed map can't
    // accumulate dead entries across catalog replacements.
    cache.retain(|_, entry| entry.catalog.upgrade().is_some());
    cache.insert(
        key,
        CachedAnthropicCatalogRows {
            catalog: Arc::downgrade(catalog),
            rows: rows.clone(),
        },
    );
    rows
}

fn claude_display_name(model_id: &str) -> String {
    let name = model_id
        .strip_prefix("claude-")
        .unwrap_or(model_id)
        .split('-')
        .take_while(|part| part.len() != 8 || !part.chars().all(|c| c.is_ascii_digit()))
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    if first.is_ascii_alphabetic() {
                        format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
                    } else {
                        format!("{}{}", first, chars.as_str())
                    }
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("Claude {name}")
}

/// GET /v1/models -- returns catalog Claude models merged with model_list entries.
pub async fn models(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cached_rows = cached_anthropic_catalog_model_rows(&state.provider_catalog);
    let mut data = cached_rows.rows.iter().cloned().collect::<Vec<_>>();

    // Merge models from the model router (LiteLLM model_list config).
    if let Some(ref router_lock) = state.model_router {
        let router = router_lock.read().unwrap_or_else(|e| e.into_inner());
        for model_name in router.known_models() {
            if !cached_rows.ids.contains(model_name) {
                data.push(serde_json::json!({
                    "id": model_name,
                    "object": "model",
                    "created": 0,
                    "owned_by": "organization"
                }));
            }
        }
    }

    Json(serde_json::json!({
        "object": "list",
        "data": data,
    }))
}

pub async fn health() -> impl IntoResponse {
    ([("content-type", "application/json")], r#"{"status":"ok"}"#)
}

/// Shared passthrough logic: extract content-type, forward to backend, relay response.
pub async fn passthrough_to_backend(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    body: axum::body::Bytes,
    path: &str,
) -> Response {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");

    let body =
        match super::super::secret_redaction::redact_body(state.redact_secrets(), headers, body)
            .await
        {
            Ok(body) => body,
            Err(err) => return super::super::secret_redaction::error_response(err),
        };

    match state
        .backend
        .raw_passthrough(path, body, content_type)
        .await
    {
        Ok((status, resp_headers, resp_body)) => {
            let mut response = (status, resp_body).into_response();
            for (k, v) in &resp_headers {
                response.headers_mut().insert(k, v.clone());
            }
            response
        }
        Err(e) => backend_error_to_response(e),
    }
}

pub async fn embeddings(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<super::super::middleware::VirtualKeyContext>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Some(resp) = enforce_model_allowlist_from_json_body(vk_ctx.as_ref(), &body) {
        return resp;
    }
    passthrough_to_backend(&state, &headers, body, "/v1/embeddings").await
}

pub async fn rerank(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<super::super::middleware::VirtualKeyContext>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Some(resp) = enforce_model_allowlist_from_json_body(vk_ctx.as_ref(), &body) {
        return resp;
    }
    passthrough_to_backend(&state, &headers, body, "/v1/rerank").await
}

pub async fn v2_rerank(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<super::super::middleware::VirtualKeyContext>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Some(resp) = enforce_model_allowlist_from_json_body(vk_ctx.as_ref(), &body) {
        return resp;
    }
    passthrough_to_backend(&state, &headers, body, "/v2/rerank").await
}

pub async fn completions(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<super::super::middleware::VirtualKeyContext>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Some(resp) = enforce_model_allowlist_from_json_body(vk_ctx.as_ref(), &body) {
        return resp;
    }
    passthrough_to_backend(&state, &headers, body, "/v1/completions").await
}
