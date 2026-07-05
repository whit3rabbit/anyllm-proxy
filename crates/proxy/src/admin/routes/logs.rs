use crate::admin::state::SharedState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

/// GET /admin/api/metrics -- current metrics snapshot.
/// Response shape matches the TypeScript `Metrics` interface in admin-ui/src/api/types.ts.
pub(super) async fn get_metrics(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    let mut aggregate = crate::metrics::MetricsSnapshot::default();
    for (_, m) in shared.backend_metrics.iter() {
        let snap = m.snapshot();
        aggregate.requests_total += snap.requests_total;
        aggregate.requests_success += snap.requests_success;
        aggregate.requests_error += snap.requests_error;
        aggregate.streams_started += snap.streams_started;
        aggregate.streams_completed += snap.streams_completed;
        aggregate.streams_failed += snap.streams_failed;
        aggregate.streams_client_disconnected += snap.streams_client_disconnected;
        aggregate.pxpipe_compressed_total += snap.pxpipe_compressed_total;
        aggregate.pxpipe_images_total += snap.pxpipe_images_total;
        aggregate.pxpipe_imaged_chars_total += snap.pxpipe_imaged_chars_total;
    }

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let since_60s = now_secs.saturating_sub(60);

    // Run both DB queries concurrently — both are read-only and independent.
    let (percentiles, rpm_count) = tokio::join!(
        crate::admin::state::with_db(&shared.db, compute_latency_percentiles),
        crate::admin::state::with_db(&shared.db, move |conn| {
            crate::admin::db::count_requests_since(conn, since_60s).unwrap_or(0)
        }),
    );
    let (p50, p95, _p99) = percentiles.unwrap_or((None, None, None));
    let rpm = rpm_count.unwrap_or(0) as f64;

    Json(serde_json::json!({
        "total_requests": aggregate.requests_total,
        "successful_requests": aggregate.requests_success,
        "failed_requests": aggregate.requests_error,
        "requests_per_minute": rpm,
        "p50_latency_ms": p50.unwrap_or(0),
        "p95_latency_ms": p95.unwrap_or(0),
        "error_rate": aggregate.error_rate(),
        "streams_started": aggregate.streams_started,
        "streams_completed": aggregate.streams_completed,
        "streams_failed": aggregate.streams_failed,
        "streams_client_disconnected": aggregate.streams_client_disconnected,
        "pxpipe_compressed_total": aggregate.pxpipe_compressed_total,
        "pxpipe_images_total": aggregate.pxpipe_images_total,
        "pxpipe_imaged_chars_total": aggregate.pxpipe_imaged_chars_total,
    }))
}

/// Query parameters for `GET /admin/api/observability/overview`.
#[derive(serde::Deserialize)]
pub(super) struct ObservabilityQuery {
    /// Accepted as `window` (used by the admin SPA) or `hours` (legacy alias).
    window: Option<u32>,
    hours: Option<u32>,
    backend: Option<String>,
    key_id: Option<i64>,
    timeline_limit: Option<u32>,
    failure_limit: Option<u32>,
}

/// GET /admin/api/observability/overview -- request rollups for the operator dashboard.
/// Response shape matches the TypeScript `ObservabilityResponse` interface.
pub(super) async fn get_observability_overview(
    State(shared): State<SharedState>,
    Query(params): Query<ObservabilityQuery>,
) -> Json<serde_json::Value> {
    let hours = params.window.or(params.hours).unwrap_or(6).clamp(1, 168);
    let timeline_limit = params.timeline_limit.unwrap_or(40).clamp(10, 200);
    let failure_limit = params.failure_limit.unwrap_or(12).clamp(1, 100);
    let backend = params.backend.filter(|v| !v.is_empty() && v.len() <= 128);
    let backend_str = backend.as_deref().unwrap_or("").to_string();
    let key_id = params.key_id;

    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let since = crate::admin::db::epoch_to_iso8601(now_epoch.saturating_sub(hours as u64 * 3600));
    let until = crate::admin::db::now_iso8601();

    let result = crate::admin::state::with_db(&shared.db, move |conn| {
        let series = crate::admin::db::query_request_timeseries(
            conn,
            &since,
            Some(&until),
            backend.as_deref(),
            key_id,
        )?;
        let timeline = crate::admin::db::query_request_timeline(
            conn,
            &since,
            Some(&until),
            backend.as_deref(),
            key_id,
            timeline_limit,
        )?;
        let failures = crate::admin::db::query_failure_breakdown(
            conn,
            &since,
            Some(&until),
            backend.as_deref(),
            key_id,
            failure_limit,
        )?;
        Ok::<_, rusqlite::Error>((series, timeline, failures))
    })
    .await;

    match result {
        Some(Ok((series, timeline, failures))) => {
            let (
                total_requests,
                total_errors,
                total_input_tokens,
                total_output_tokens,
                total_cost_usd,
            ) = series.iter().fold(
                (0u64, 0u64, 0u64, 0u64, 0.0f64),
                |(req, err, inp, out, cost), b| {
                    (
                        req + b.requests_total,
                        err + b.requests_error,
                        inp + b.input_tokens,
                        out + b.output_tokens,
                        cost + b.cost_usd,
                    )
                },
            );
            Json(serde_json::json!({
                "window_hours": hours,
                "backend": backend_str,
                "total_requests": total_requests,
                "total_errors": total_errors,
                "total_input_tokens": total_input_tokens,
                "total_output_tokens": total_output_tokens,
                "total_cost_usd": total_cost_usd,
                "series": series,
                "timeline": timeline,
                "failures": failures,
            }))
        }
        other => {
            if let Some(Err(e)) = other {
                tracing::error!(error = %e, "query observability overview failed");
            }
            Json(serde_json::json!({
                "window_hours": hours,
                "backend": backend_str,
                "total_requests": 0u64,
                "total_errors": 0u64,
                "total_input_tokens": 0u64,
                "total_output_tokens": 0u64,
                "total_cost_usd": 0.0f64,
                "series": serde_json::Value::Array(vec![]),
                "timeline": serde_json::Value::Array(vec![]),
                "failures": serde_json::Value::Array(vec![]),
            }))
        }
    }
}

/// Compute p50, p95, p99 latency from the last 5 minutes of request log.
pub(super) fn compute_latency_percentiles(
    conn: &rusqlite::Connection,
) -> (Option<u64>, Option<u64>, Option<u64>) {
    // Get latencies from recent requests, sorted.
    let cutoff = crate::admin::db::now_iso8601(); // We want last 5 minutes
    let mut stmt = conn
        .prepare(
            "SELECT latency_ms FROM request_log
             WHERE timestamp > datetime(?1, '-5 minutes')
             ORDER BY latency_ms ASC",
        )
        .ok();

    let latencies: Vec<u64> = stmt
        .as_mut()
        .and_then(|s| {
            s.query_map(rusqlite::params![cutoff], |row| {
                row.get::<_, i64>(0).map(|v| v as u64)
            })
            .ok()
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    if latencies.is_empty() {
        return (None, None, None);
    }

    let p = |pct: f64| -> u64 {
        let idx = ((pct / 100.0) * (latencies.len() as f64 - 1.0)).round() as usize;
        latencies[idx.min(latencies.len() - 1)]
    };

    (Some(p(50.0)), Some(p(95.0)), Some(p(99.0)))
}

/// Query parameters for `GET /admin/api/requests`.
#[derive(serde::Deserialize)]
pub(super) struct RequestsQuery {
    limit: Option<u32>,
    offset: Option<u32>,
    backend: Option<String>,
    since: Option<String>,
    until: Option<String>,
    status: Option<String>,
    key_id: Option<i64>,
}

/// GET /admin/api/requests -- paginated request log.
pub(super) async fn get_requests(
    State(shared): State<SharedState>,
    Query(params): Query<RequestsQuery>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).min(1000);
    let offset = params.offset.unwrap_or(0);

    let backend = params.backend.filter(|v| v.len() <= 128);
    let since = params.since;
    let until = params.until;
    let status = params.status.filter(|v| v.len() <= 32);
    let key_id = params.key_id;
    if let Some(param) = super::check_time_range(since.as_deref(), until.as_deref()) {
        return Json(serde_json::json!({
            "error": format!("invalid '{}' value; expected ISO 8601 date or datetime", param),
            "requests": [],
        }));
    }
    // NOT A BUG: Querying limit+1 rows is the standard "has_more" cursor pattern.
    // has_more = entries.len() > limit is true iff the DB returned the extra row,
    // which means more results exist beyond this page. The extra row is truncated
    // before returning. entries.len() can never exceed limit+1 from the SQL LIMIT.
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::query_request_log(
            conn,
            limit + 1,
            offset,
            backend.as_deref(),
            since.as_deref(),
            until.as_deref(),
            status.as_deref(),
            key_id,
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
                "requests": entries,
                "limit": limit,
                "offset": offset,
                "has_more": has_more,
            }))
        }
        Some(Err(e)) => {
            tracing::error!(error = %e, "query_request_log failed");
            Json(serde_json::json!({
                "error": "internal database error",
                "requests": [],
            }))
        }
        None => Json(serde_json::json!({
            "error": "task panicked",
            "requests": [],
        })),
    }
}

/// GET /admin/api/requests/:id -- single request detail.
pub(super) async fn get_request_by_id(
    State(shared): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match crate::admin::state::with_db(&shared.db, move |conn| {
        crate::admin::db::get_request_by_id(conn, &id)
    })
    .await
    {
        Some(Ok(Some(entry))) => {
            (StatusCode::OK, Json(serde_json::to_value(entry).unwrap())).into_response()
        }
        Some(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "request not found"})),
        )
            .into_response(),
        Some(Err(e)) => {
            tracing::error!(error = %e, "get_request_by_id failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal database error"})),
            )
                .into_response()
        }
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        )
            .into_response(),
    }
}
