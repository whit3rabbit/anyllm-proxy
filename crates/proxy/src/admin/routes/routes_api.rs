// Route handlers for the routes + route_providers CRUD API.
pub use crate::admin::db::{ReorderOutcome, RoutePatch, RouteProviderRow, RouteRow};

use crate::admin::state::SharedState;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::net::SocketAddr;

// ── Response types ────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct RouteResponse {
    id: String,
    name: String,
    description: Option<String>,
    strategy: String,
    rpm: Option<u32>,
    tpm: Option<u64>,
    budget_usd: Option<f64>,
    provider_count: usize,
    created_at: String,
    updated_at: String,
}

#[derive(serde::Serialize)]
struct RouteProviderResponse {
    id: String,
    route_id: String,
    backend_id: String,
    backend_name: String,
    provider_id: String,
    models: Vec<String>,
    priority: i32,
    enabled: bool,
}

// ── Request types ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateRouteRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    pub rpm: Option<u32>,
    pub tpm: Option<u64>,
    pub budget_usd: Option<f64>,
}

fn default_strategy() -> String {
    "failover".into()
}

#[derive(Deserialize)]
pub struct AddRouteProviderRequest {
    pub backend_id: String,
    #[serde(default = "default_models")]
    pub models: Vec<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_models() -> Vec<String> {
    vec!["*".into()]
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub struct UpdateRouteProviderRequest {
    pub models: Option<Vec<String>>,
    pub priority: Option<i32>,
    pub enabled: Option<bool>,
}

#[derive(Deserialize)]
pub struct ReorderRouteProvidersRequest {
    pub provider_ids: Vec<String>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn route_to_response(route: &RouteRow, provider_count: usize) -> RouteResponse {
    RouteResponse {
        id: route.id.clone(),
        name: route.name.clone(),
        description: route.description.clone(),
        strategy: route.strategy.clone(),
        rpm: route.rpm,
        tpm: route.tpm,
        budget_usd: route.budget_usd,
        provider_count,
        created_at: route.created_at.clone(),
        updated_at: route.updated_at.clone(),
    }
}

fn db_error_status(e: &rusqlite::Error) -> StatusCode {
    if let rusqlite::Error::SqliteFailure(ref err, _) = e {
        if err.code == rusqlite::ErrorCode::ConstraintViolation {
            return StatusCode::CONFLICT;
        }
    }
    StatusCode::INTERNAL_SERVER_ERROR
}

fn ok_json<T: serde::Serialize>(val: T) -> axum::response::Response {
    (StatusCode::OK, Json(serde_json::to_value(val).unwrap())).into_response()
}

fn err_json(status: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (status, Json(serde_json::json!({ "error": msg.into() }))).into_response()
}

fn build_provider_responses(
    providers: &[RouteProviderRow],
    backends: &[crate::admin::db::ManagedBackendRow],
) -> Vec<RouteProviderResponse> {
    let backend_map: std::collections::HashMap<&str, &crate::admin::db::ManagedBackendRow> =
        backends.iter().map(|b| (b.id.as_str(), b)).collect();
    providers
        .iter()
        .map(|p| {
            let backend = backend_map.get(p.backend_id.as_str());
            RouteProviderResponse {
                id: p.id.clone(),
                route_id: p.route_id.clone(),
                backend_id: p.backend_id.clone(),
                backend_name: backend.map(|b| b.name.clone()).unwrap_or_default(),
                provider_id: backend.map(|b| b.provider_id.clone()).unwrap_or_default(),
                models: p.models.clone(),
                priority: p.priority,
                enabled: p.enabled,
            }
        })
        .collect()
}

fn audit(shared: &SharedState, source_ip: Option<String>, action: &str, target_type: &str, target_id: String, detail: Option<String>) {
    super::emit_audit(shared, crate::admin::db::AuditEntry {
        id: None,
        timestamp: None,
        action: action.into(),
        target_type: target_type.into(),
        target_id: Some(target_id),
        detail,
        source_ip,
    });
}

// ── Route CRUD ────────────────────────────────────────────────────────────────

pub(super) async fn list_routes(State(shared): State<SharedState>) -> axum::response::Response {
    let result = crate::admin::state::with_db(&shared.db, |conn| {
        let routes = crate::admin::db::list_routes(conn)?;
        let mut resp = Vec::with_capacity(routes.len());
        for r in &routes {
            let count = crate::admin::db::count_route_providers(conn, &r.id).unwrap_or(0);
            resp.push(route_to_response(r, count));
        }
        Ok::<_, rusqlite::Error>(resp)
    })
    .await;

    match result {
        Some(Ok(resp)) => ok_json(serde_json::json!({ "routes": resp })),
        _ => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to list routes"),
    }
}

pub(super) async fn create_route(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<CreateRouteRequest>,
) -> axum::response::Response {
    if body.name.trim().is_empty() {
        return err_json(StatusCode::BAD_REQUEST, "name is required");
    }

    let now = crate::admin::db::now_iso8601();
    let row = RouteRow {
        id: uuid::Uuid::new_v4().to_string(),
        name: body.name.trim().to_string(),
        description: body.description,
        strategy: body.strategy,
        rpm: body.rpm,
        tpm: body.tpm,
        budget_usd: body.budget_usd,
        created_at: now.clone(),
        updated_at: now,
    };

    let row_clone = row.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::insert_route(conn, &row_clone)
    })
    .await;

    match result {
        Some(Ok(())) => {
            audit(&shared, Some(addr.ip().to_string()), "route_created", "route", row.name.clone(), None);
            (StatusCode::CREATED, Json(serde_json::to_value(route_to_response(&row, 0)).unwrap())).into_response()
        }
        Some(Err(e)) => err_json(db_error_status(&e), e.to_string()),
        None => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to create route"),
    }
}

pub(super) async fn update_route(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<RoutePatch>,
) -> axum::response::Response {
    let id_clone = id.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        let updated = crate::admin::db::update_route(conn, &id_clone, &body)?;
        if !updated {
            return Ok::<_, rusqlite::Error>((false, None, 0));
        }
        let route = crate::admin::db::get_route(conn, &id_clone).ok().flatten();
        let count = crate::admin::db::count_route_providers(conn, &id_clone).unwrap_or(0);
        Ok((true, route, count))
    })
    .await;

    match result {
        Some(Ok((true, Some(r), count))) => {
            audit(&shared, Some(addr.ip().to_string()), "route_updated", "route", id, None);
            ok_json(route_to_response(&r, count))
        }
        Some(Ok((true, None, _))) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "route not found after update"),
        Some(Ok((false, _, _))) => err_json(StatusCode::NOT_FOUND, "route not found"),
        Some(Err(e)) => err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        None => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to update route"),
    }
}

pub(super) async fn delete_route(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let id_clone = id.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::delete_route(conn, &id_clone)
    })
    .await;

    match result {
        Some(Ok(true)) => {
            audit(&shared, Some(addr.ip().to_string()), "route_deleted", "route", id, None);
            ok_json(serde_json::json!({ "ok": true }))
        }
        Some(Ok(false)) => err_json(StatusCode::NOT_FOUND, "route not found"),
        Some(Err(e)) => err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        None => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete route"),
    }
}

// ── Route Providers CRUD ──────────────────────────────────────────────────────

pub(super) async fn list_route_providers_handler(
    State(shared): State<SharedState>,
    Path(route_id): Path<String>,
) -> axum::response::Response {
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        if crate::admin::db::get_route(conn, &route_id).ok().flatten().is_none() {
            return Ok::<_, rusqlite::Error>(None);
        }
        let providers = crate::admin::db::list_route_providers(conn, &route_id)?;
        let backends = crate::admin::db::list_managed_backends(conn).unwrap_or_default();
        Ok(Some(build_provider_responses(&providers, &backends)))
    })
    .await;

    match result {
        Some(Ok(Some(resp))) => ok_json(serde_json::json!({ "providers": resp })),
        Some(Ok(None)) => err_json(StatusCode::NOT_FOUND, "route not found"),
        _ => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to list route providers"),
    }
}

pub(super) async fn add_route_provider_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(route_id): Path<String>,
    Json(body): Json<AddRouteProviderRequest>,
) -> axum::response::Response {
    let route_id_clone = route_id.clone();
    let backend_id = body.backend_id.clone();
    let models = body.models.clone();
    let priority = body.priority;
    let enabled = body.enabled;

    let backend_id_for_db = backend_id.clone();
    let backend_id_check = backend_id.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        if crate::admin::db::get_route(conn, &route_id_clone).ok().flatten().is_none() {
            return Ok::<_, rusqlite::Error>(Err::<(), String>("route not found".into()));
        }
        if !crate::admin::db::managed_backend_exists(conn, &backend_id_check)? {
            return Ok(Err::<(), String>("backend not found".into()));
        }
        crate::admin::db::add_route_provider(conn, &route_id_clone, &backend_id_for_db, &models, priority, enabled)?;
        Ok(Ok::<(), String>(()))
    })
    .await;

    match result {
        Some(Ok(Ok(()))) => {
            audit(&shared, Some(addr.ip().to_string()), "route_provider_added", "route_provider", route_id, Some(format!("backend_id={}", backend_id)));
            (StatusCode::CREATED, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        Some(Ok(Err(msg))) => err_json(if msg == "route not found" { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST }, msg),
        Some(Err(e)) => err_json(db_error_status(&e), e.to_string()),
        None => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to add route provider"),
    }
}

pub(super) async fn update_route_provider_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path((route_id, provider_id)): Path<(String, String)>,
    Json(body): Json<UpdateRouteProviderRequest>,
) -> axum::response::Response {
    let provider_id_clone = provider_id.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::update_route_provider(
            conn,
            &provider_id_clone,
            body.models.as_deref(),
            body.priority,
            body.enabled,
        )
    })
    .await;

    match result {
        Some(Ok(true)) => {
            audit(&shared, Some(addr.ip().to_string()), "route_provider_updated", "route_provider", provider_id, Some(format!("route_id={}", route_id)));
            ok_json(serde_json::json!({ "ok": true }))
        }
        Some(Ok(false)) => err_json(StatusCode::NOT_FOUND, "route provider not found"),
        Some(Err(e)) => err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        None => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to update route provider"),
    }
}

pub(super) async fn reorder_route_providers_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path(route_id): Path<String>,
    Json(body): Json<ReorderRouteProvidersRequest>,
) -> axum::response::Response {
    let route_id_clone = route_id.clone();
    let ordered = body.provider_ids.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        if crate::admin::db::get_route(conn, &route_id_clone)
            .ok()
            .flatten()
            .is_none()
        {
            return Ok::<_, rusqlite::Error>(None);
        }
        let outcome = crate::admin::db::reorder_route_providers(conn, &route_id_clone, &ordered)?;
        match outcome {
            ReorderOutcome::Mismatch => Ok(Some(Err::<_, ()>(()))),
            ReorderOutcome::Ok(rows) => {
                let backends = crate::admin::db::list_managed_backends(conn).unwrap_or_default();
                Ok(Some(Ok(build_provider_responses(&rows, &backends))))
            }
        }
    })
    .await;

    match result {
        Some(Ok(Some(Ok(providers)))) => {
            audit(
                &shared,
                Some(addr.ip().to_string()),
                "route_providers_reordered",
                "route",
                route_id,
                Some(format!("count={}", providers.len())),
            );
            ok_json(serde_json::json!({ "providers": providers }))
        }
        Some(Ok(Some(Err(())))) => err_json(
            StatusCode::BAD_REQUEST,
            "provider_ids must match the route's current provider set exactly",
        ),
        Some(Ok(None)) => err_json(StatusCode::NOT_FOUND, "route not found"),
        Some(Err(e)) => err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        None => err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to reorder route providers",
        ),
    }
}

pub(super) async fn remove_route_provider_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Path((_route_id, provider_id)): Path<(String, String)>,
) -> axum::response::Response {
    let provider_id_clone = provider_id.clone();
    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::remove_route_provider(conn, &provider_id_clone)
    })
    .await;

    match result {
        Some(Ok(true)) => {
            audit(&shared, Some(addr.ip().to_string()), "route_provider_removed", "route_provider", provider_id, None);
            ok_json(serde_json::json!({ "ok": true }))
        }
        Some(Ok(false)) => err_json(StatusCode::NOT_FOUND, "route provider not found"),
        Some(Err(e)) => err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        None => err_json(StatusCode::INTERNAL_SERVER_ERROR, "Failed to remove route provider"),
    }
}
