pub use crate::admin::db::{ReorderOutcome, RoutePatch, RouteProviderRow, RouteRow};
use crate::admin::state::SharedState;
use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;

#[derive(serde::Serialize)]
pub(crate) struct RouteResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub strategy: String,
    pub rpm: Option<u32>,
    pub tpm: Option<u64>,
    pub budget_usd: Option<f64>,
    pub enabled: bool,
    pub guardrail_mode: Option<String>,
    pub pxpipe_compress: Option<bool>,
    pub pxpipe_models: Option<String>,
    pub redact_secrets: Option<bool>,
    pub position: i32,
    pub provider_count: usize,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(serde::Serialize)]
pub(crate) struct RouteProviderResponse {
    pub id: String,
    pub route_id: String,
    pub backend_id: String,
    pub backend_name: String,
    pub provider_id: String,
    pub models: Vec<String>,
    pub priority: i32,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub(crate) struct CreateRouteRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    pub rpm: Option<u32>,
    pub tpm: Option<u64>,
    pub budget_usd: Option<f64>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub guardrail_mode: Option<String>,
    #[serde(default)]
    pub pxpipe_compress: Option<bool>,
    #[serde(default)]
    pub pxpipe_models: Option<String>,
    #[serde(default)]
    pub redact_secrets: Option<bool>,
    #[serde(default)]
    pub position: i32,
}

fn default_strategy() -> String {
    "failover".into()
}

#[derive(Deserialize)]
pub(crate) struct AddRouteProviderRequest {
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
pub(crate) struct UpdateRouteProviderRequest {
    pub models: Option<Vec<String>>,
    pub priority: Option<i32>,
    pub enabled: Option<bool>,
}

#[derive(Deserialize)]
pub(crate) struct ReorderRouteProvidersRequest {
    pub provider_ids: Vec<String>,
}

pub(crate) fn route_to_response(route: &RouteRow, provider_count: usize) -> RouteResponse {
    RouteResponse {
        id: route.id.clone(),
        name: route.name.clone(),
        description: route.description.clone(),
        strategy: route.strategy.clone(),
        rpm: route.rpm,
        tpm: route.tpm,
        budget_usd: route.budget_usd,
        enabled: route.enabled,
        guardrail_mode: route.guardrail_mode.clone(),
        pxpipe_compress: route.pxpipe_compress,
        pxpipe_models: route.pxpipe_models.clone(),
        redact_secrets: route.redact_secrets,
        position: route.position,
        provider_count,
        created_at: route.created_at.clone(),
        updated_at: route.updated_at.clone(),
    }
}

pub(crate) fn db_error_status(e: &rusqlite::Error) -> StatusCode {
    if let rusqlite::Error::SqliteFailure(ref err, _) = e {
        if err.code == rusqlite::ErrorCode::ConstraintViolation {
            return StatusCode::CONFLICT;
        }
    }
    StatusCode::INTERNAL_SERVER_ERROR
}

pub(crate) fn ok_json<T: serde::Serialize>(val: T) -> axum::response::Response {
    (StatusCode::OK, Json(serde_json::to_value(val).unwrap())).into_response()
}

pub(crate) fn err_json(status: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (status, Json(serde_json::json!({ "error": msg.into() }))).into_response()
}

pub(crate) fn build_provider_responses(
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

pub(crate) fn audit(
    shared: &SharedState,
    source_ip: Option<String>,
    action: &str,
    target_type: &str,
    target_id: String,
    detail: Option<String>,
) {
    super::super::emit_audit(
        shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: action.into(),
            target_type: target_type.into(),
            target_id: Some(target_id),
            detail,
            source_ip,
        },
    );
}
