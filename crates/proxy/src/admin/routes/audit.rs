use crate::admin::state::SharedState;
use axum::{
    extract::{Query, State},
    Json,
};

#[derive(serde::Deserialize)]
pub(super) struct AuditQuery {
    limit: Option<u32>,
    offset: Option<u32>,
    action: Option<String>,
    target_type: Option<String>,
    since: Option<String>,
    until: Option<String>,
}

/// GET /admin/api/audit -- paginated audit log.
pub(super) async fn get_audit_log(
    State(shared): State<SharedState>,
    Query(params): Query<AuditQuery>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).min(1000);
    let offset = params.offset.unwrap_or(0);
    let action = params.action.filter(|v| v.len() <= 128);
    let target_type = params.target_type.filter(|v| v.len() <= 128);
    let since = params.since;
    let until = params.until;
    if let Some(param) = super::check_time_range(since.as_deref(), until.as_deref()) {
        return Json(serde_json::json!({
            "error": format!("invalid '{}' value; expected ISO 8601 date or datetime", param),
            "entries": [],
        }));
    }
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::query_audit_log(
            conn,
            limit + 1,
            offset,
            action.as_deref(),
            target_type.as_deref(),
            since.as_deref(),
            until.as_deref(),
        )
    })
    .await
    {
        Some(Ok(mut entries)) => {
            let has_more = entries.len() > limit as usize;
            if has_more {
                entries.truncate(limit as usize);
            }
            Json(serde_json::json!({
                "entries": entries,
                "limit": limit,
                "offset": offset,
                "has_more": has_more,
            }))
        }
        Some(Err(e)) => {
            tracing::error!(error = %e, "query_audit_log failed");
            Json(serde_json::json!({
                "error": "internal database error",
                "entries": [],
            }))
        }
        None => Json(serde_json::json!({
            "error": "task panicked",
            "entries": [],
        })),
    }
}
