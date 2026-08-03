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
    let rows = Arc::new(AnthropicCatalogRows {
        rows: Arc::from(rows),
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

/// Push a model row, deduping by id. Empty ids are skipped. `display_name`
/// mirrors the id: it stays a real, routable name (what Claude Code sends back),
/// and is a large improvement over synthesizing "Claude Sonnet/Opus/Haiku" for
/// backends that are not Anthropic.
fn push_model_row(
    data: &mut Vec<serde_json::Value>,
    seen: &mut HashSet<String>,
    id: &str,
    owned_by: &str,
) {
    if !id.is_empty() && seen.insert(id.to_string()) {
        data.push(serde_json::json!({
            "id": id,
            "object": "model",
            "created": 0,
            "owned_by": owned_by,
            "display_name": id,
        }));
    }
}

/// GET /v1/models -- returns the real routable models the proxy can serve, so
/// Claude Code's gateway model discovery
/// (`CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1`) can populate its `/model`
/// picker with models a user can actually pick and have routed. Sources, in
/// dedup order: autorouter tier targets (operator-typed, covers local/custom
/// models), each enabled managed backend's provider catalog, then LiteLLM
/// `model_list` virtual models. Falls back to the static Anthropic catalog only
/// when none of those produce anything, preserving simple single-backend installs.
pub async fn models(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut data: Vec<serde_json::Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // 1+2. Autorouter tier targets and managed-backend catalog models. Gated on
    //      `router.enabled` because the explicit-pick routing that makes these
    //      directly pickable only runs when the autorouter is on; advertising
    //      them otherwise would list models a pick would not route.
    let router_enabled = {
        let cfg = state
            .runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner());
        cfg.router.enabled
    };
    if router_enabled {
        // Tier targets: clone out so the lock is held only briefly.
        let tier_models: Vec<(String, String)> = {
            let cfg = state
                .runtime_config
                .read()
                .unwrap_or_else(|e| e.into_inner());
            cfg.router
                .active_tiers()
                .map(|(_, t)| (t.model.clone(), t.backend_name.clone()))
                .collect()
        };
        for (model, backend) in &tier_models {
            push_model_row(&mut data, &mut seen, model, backend);
        }

        // Each enabled managed backend's provider catalog models (static catalog
        // only; no DB read so the request path stays DB-free).
        if let Some(shared) = state.shared.as_ref() {
            if let Ok(guard) = shared.managed_backends.read() {
                for (name, (row, _client)) in guard.iter() {
                    if !row.enabled {
                        continue;
                    }
                    for m in state.provider_catalog.list_models(&row.provider_id).iter() {
                        push_model_row(&mut data, &mut seen, m.id.as_str(), name);
                    }
                }
            }
        }
    }

    // 3. LiteLLM model_list virtual models (routed via ModelRouter regardless of
    //    the autorouter, so always advertised).
    if let Some(ref router_lock) = state.model_router {
        let router = router_lock.read().unwrap_or_else(|e| e.into_inner());
        for model_name in router.known_models() {
            if seen.insert(model_name.to_string()) {
                data.push(serde_json::json!({
                    "id": model_name,
                    "object": "model",
                    "created": 0,
                    "owned_by": "organization"
                }));
            }
        }
    }

    // Fallback: no real models configured -> static Anthropic catalog (unchanged
    // behavior for a simple single-backend install with no managed backends and
    // the autorouter off).
    if data.is_empty() {
        let cached_rows = cached_anthropic_catalog_model_rows(&state.provider_catalog);
        let fallback = cached_rows.rows.iter().cloned().collect::<Vec<_>>();
        return Json(serde_json::json!({ "object": "list", "data": fallback }));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_model_row_dedups_and_skips_empty() {
        let mut data = Vec::new();
        let mut seen = HashSet::new();
        push_model_row(&mut data, &mut seen, "gpt-4o", "openai-be");
        push_model_row(&mut data, &mut seen, "gpt-4o", "other-be"); // dup id -> dropped
        push_model_row(&mut data, &mut seen, "", "empty-be"); // empty -> dropped
        push_model_row(&mut data, &mut seen, "deepseek-chat", "deepseek-be");

        assert_eq!(data.len(), 2);
        assert_eq!(data[0]["id"], "gpt-4o");
        assert_eq!(data[0]["owned_by"], "openai-be"); // first writer wins
        assert_eq!(data[0]["display_name"], "gpt-4o");
        assert_eq!(data[1]["id"], "deepseek-chat");
    }
}
