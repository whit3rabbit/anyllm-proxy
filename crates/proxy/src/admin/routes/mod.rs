//! Admin web server routing logic and request handlers.
//!
//! Exposes REST endpoints and a WebSocket channel for managing configurations,
//! virtual keys, backend catalogs, audits, and real-time logs.

/// Audit log API handlers.
pub mod audit;
/// LiteLLM provider catalog API handlers.
pub mod catalog;
/// Server configuration API handlers.
pub mod config;
/// Environment file export/import API handlers.
pub mod env;
/// Favorite providers API handlers.
pub mod favorites;
/// Helper utilities for route handlers.
pub mod helpers;
/// Virtual API key management handlers.
pub mod keys;
/// Request history logs and stats API handlers.
pub mod logs;
/// Managed backend credentials API handlers.
pub mod managed_backends;
/// MCP server configuration API handlers.
pub mod mcp;
/// Admin route auth and CSRF middlewares.
pub mod middleware;
/// Model routing and discovery API handlers.
pub mod models;
/// Optimizer ONNX-model detect/download API handlers.
pub mod optimizer;
/// Custom API route config handlers.
pub mod routes_api;
/// Uptime/status overview API handlers.
pub mod status;
/// Real-time traffic stats API handlers.
pub mod traffic;
/// Proxy/backends uptime history API handlers.
pub mod uptime;

#[cfg(test)]
mod tests;

use crate::admin::auth::{generate_csrf_token, validate_admin_token};
use crate::admin::state::SharedState;
use crate::admin::ws::ws_handler;
use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    middleware as axum_middleware,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use std::sync::Arc;

pub(super) use helpers::{base64_url_encode, check_time_range, is_safe_model_name};
pub use helpers::{reset_admin_rate_limit, set_admin_rpm};
pub use middleware::validate_csrf;
use middleware::{admin_rate_limit_middleware, reject_cross_origin};

/// Build the admin router.
/// Token is used for auth middleware on all routes except /admin/health.
pub fn admin_router(shared: SharedState, token: Arc<zeroize::Zeroizing<String>>) -> Router {
    // Public routes (no auth).
    let public = Router::new()
        .route("/admin/health", get(health))
        .with_state(shared.clone())
        .layer(axum_middleware::from_fn(admin_rate_limit_middleware));

    // Protected routes (require admin token + localhost origin check).
    let protected = Router::new()
        .route("/admin/csrf-token", get(get_csrf_token))
        .route(
            "/admin/api/config",
            get(config::get_config).put(config::put_config),
        )
        .route(
            "/admin/api/config/overrides",
            get(config::get_config_overrides),
        )
        .route(
            "/admin/api/config/overrides/{key}",
            delete(config::delete_config_override),
        )
        .route("/admin/api/env", get(env::get_env))
        .route("/admin/api/env/import", post(env::import_env))
        .route("/admin/api/env/export", get(env::export_env))
        .route("/admin/api/metrics", get(logs::get_metrics))
        .route(
            "/admin/api/observability/overview",
            get(logs::get_observability_overview),
        )
        .route("/admin/api/requests", get(logs::get_requests))
        .route("/admin/api/requests/{id}", get(logs::get_request_by_id))
        .route("/admin/api/backends", get(get_backends))
        .route(
            "/admin/api/backends/managed",
            get(managed_backends::list).post(managed_backends::create),
        )
        .route(
            "/admin/api/backends/managed/{name}",
            put(managed_backends::update).delete(managed_backends::delete),
        )
        .route(
            "/admin/api/keys",
            post(keys::create_key).get(keys::list_keys),
        )
        .route(
            "/admin/api/keys/{id}",
            put(keys::update_key).delete(keys::revoke_key),
        )
        .route(
            "/admin/api/keys/{id}/spend",
            get(super::spend::get_key_spend),
        )
        .route(
            "/admin/api/models",
            get(models::list_models).post(models::add_model),
        )
        .route("/admin/api/models/discover", post(models::discover_models))
        .route(
            "/admin/api/optimizer/model",
            get(optimizer::get_model_status).post(optimizer::download_model),
        )
        .route("/admin/api/models/{name}", delete(models::remove_model))
        .route("/admin/api/audit", get(audit::get_audit_log))
        .route(
            "/admin/api/mcp-servers",
            get(mcp::list_mcp_servers).post(mcp::add_mcp_server),
        )
        .route(
            "/admin/api/mcp-servers/{name}",
            delete(mcp::remove_mcp_server),
        )
        .route(
            "/admin/api/favorites",
            get(favorites::list).post(favorites::create),
        )
        .route(
            "/admin/api/favorites/{provider_id}",
            delete(favorites::delete),
        )
        .route("/admin/api/catalog/providers", get(catalog::list_providers))
        .route(
            "/admin/api/catalog/providers/{id}/models",
            get(catalog::list_provider_models),
        )
        .route(
            "/admin/api/catalog/providers/{id}/refresh",
            post(catalog::refresh_provider_models),
        )
        .route("/admin/api/status", get(status::get_status))
        .route("/admin/api/traffic", get(traffic::get_traffic))
        .route("/admin/api/uptime", get(uptime::get_uptime))
        // Routes CRUD
        .route(
            "/admin/api/routes",
            get(routes_api::list_routes).post(routes_api::create_route),
        )
        .route(
            "/admin/api/routes/{id}",
            put(routes_api::update_route).delete(routes_api::delete_route),
        )
        .route(
            "/admin/api/routes/{id}/providers",
            get(routes_api::list_route_providers_handler)
                .post(routes_api::add_route_provider_handler),
        )
        .route(
            "/admin/api/routes/{id}/providers/reorder",
            put(routes_api::reorder_route_providers_handler),
        )
        .route(
            "/admin/api/routes/{id}/providers/{provider_id}",
            put(routes_api::update_route_provider_handler)
                .delete(routes_api::remove_route_provider_handler),
        )
        .with_state(shared.clone())
        // Innermost: CSRF check runs after auth succeeds.
        .layer(axum_middleware::from_fn_with_state(
            shared.clone(),
            validate_csrf,
        ))
        .layer(axum_middleware::from_fn_with_state(
            token.clone(),
            validate_admin_token,
        ))
        .layer(axum_middleware::from_fn(reject_cross_origin))
        .layer(axum_middleware::from_fn(admin_rate_limit_middleware));

    // WebSocket: auth via first message since browsers can't set headers on WS.
    // Origin check applied here too to prevent cross-site WebSocket hijacking.
    let ws_state = (shared.clone(), token.clone());
    let ws_route = Router::new()
        .route("/admin/ws", get(ws_handler))
        .with_state(ws_state)
        .layer(axum_middleware::from_fn(reject_cross_origin));

    // SPA serving (no auth required; the browser prompts for the admin token).
    let spa_route = Router::new()
        .route("/admin/", get(serve_spa))
        .route("/admin", get(serve_spa))
        .route(
            "/",
            get(|| async { axum::response::Redirect::permanent("/admin/") }),
        );

    // Merge all routes.
    public
        .merge(protected)
        .merge(ws_route)
        .merge(spa_route)
        .layer(DefaultBodyLimit::max(1_048_576))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

/// GET /admin/csrf-token
///
/// Returns a fresh CSRF token as JSON and sets it in a non-HttpOnly cookie.
/// The admin SPA reads the cookie in JS and includes it as `X-CSRF-Token` on
/// POST/PUT/DELETE requests (double-submit cookie pattern).
///
/// Security architecture note:
/// This route requires the admin Bearer token and localhost origin/host checks.
/// CSRF protects authenticated mutations, not the login form itself.
/// If TLS is ever added to the admin server, also add `Secure` to Set-Cookie.
async fn get_csrf_token(State(shared): State<SharedState>) -> axum::response::Response {
    let token = generate_csrf_token();
    // moka Cache enforces max_capacity(1,000) and time_to_live(24h) automatically.
    // Eviction is handled by the cache; no manual cap check needed.
    shared.issued_csrf_tokens.insert(token.clone(), ());
    let body = serde_json::json!({"csrf_token": token});
    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        // SameSite=Strict prevents the cookie being sent on cross-site requests.
        // Not httpOnly so the admin SPA JS can read and send it back as a header.
        // Secure flag intentionally omitted: admin binds to 127.0.0.1 over plain HTTP,
        // so setting Secure would prevent the browser from sending the cookie at all.
        // If TLS is added to the admin server, Secure must be added here.
        .header(
            "set-cookie",
            format!("csrf_token={token}; Path=/admin; SameSite=Strict; Max-Age=86400"),
        )
        .body(axum::body::Body::from(
            serde_json::to_string(&body).unwrap(),
        ))
        .unwrap()
        .into_response()
}

/// Serve the embedded SPA HTML with a per-request CSP nonce.
static SPA_HTML: &str = include_str!("../../../admin-ui/dist/index.html");

async fn serve_spa() -> axum::response::Response {
    // Generate a per-request nonce (128-bit, base64url-encoded).
    let mut nonce_bytes = [0u8; 16];
    getrandom::fill(&mut nonce_bytes).expect("getrandom");
    let nonce = base64_url_encode(&nonce_bytes);

    // Replace the placeholder in the embedded HTML with the actual nonce.
    let html = SPA_HTML.replace("__CSP_NONCE__", &nonce);

    let csp = format!(
        "default-src 'self'; script-src 'self' 'nonce-{nonce}'; \
         style-src 'self' 'nonce-{nonce}' https://fonts.bunny.net; \
         font-src https://fonts.bunny.net; \
         connect-src 'self' ws: wss:; img-src 'self' data:; \
         frame-ancestors 'none'"
    );

    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .header("content-security-policy", csp)
        .header("x-frame-options", "DENY")
        .header("referrer-policy", "no-referrer")
        .body(axum::body::Body::from(html))
        .unwrap()
        .into_response()
}

/// GET /admin/api/backends -- list configured backends with status.
pub(super) async fn get_backends(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    let config = shared
        .runtime_config
        .read()
        .unwrap_or_else(|e| e.into_inner());

    let mut backends = Vec::new();
    for (name, mapping) in &config.model_mappings {
        let metrics = shared
            .backend_metrics
            .get(name)
            .map(|m| m.snapshot())
            .unwrap_or_default();

        backends.push(serde_json::json!({
            "name": name,
            "big_model": mapping.big_model,
            "small_model": mapping.small_model,
            "metrics": {
                "requests_total": metrics.requests_total,
                "requests_success": metrics.requests_success,
                "requests_error": metrics.requests_error,
            }
        }));
    }

    Json(serde_json::json!({ "backends": backends }))
}

/// Fire-and-forget audit log write. Failures are logged but never block the caller.
/// Recompile the route dispatch table from the DB and swap it in atomically.
/// Call after any route / route-provider / managed-backend mutation so the
/// live `RouteRouter` reflects the change without a restart. No-op if the
/// route router is absent (test states).
pub(crate) async fn rebuild_route_router(shared: &SharedState) {
    let Some(rr_lock) = shared.route_router.clone() else {
        return;
    };
    let built = crate::admin::state::with_db(&shared.db, |conn| {
        crate::config::route_router::RouteRouter::build_from_db(conn)
    })
    .await;
    match built {
        Some(Ok(rr)) => {
            *rr_lock.write().unwrap_or_else(|e| e.into_inner()) = rr;
        }
        Some(Err(e)) => tracing::warn!(error = %e, "failed to rebuild route router"),
        None => {}
    }
}

pub(crate) fn emit_audit(shared: &SharedState, entry: crate::admin::db::AuditEntry) {
    let db = shared.db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = crate::admin::db::insert_audit_entry(&conn, &entry) {
            tracing::warn!(error = %e, action = %entry.action, "failed to write audit log");
        }
    });
}
