use crate::admin::state::SharedState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

/// GET /admin/api/metrics -- current metrics snapshot.
pub(super) async fn get_metrics(State(shared): State<SharedState>) -> Json<serde_json::Value> {
    let mut backends = serde_json::Map::new();
    let mut aggregate = crate::metrics::MetricsSnapshot::default();

    for (name, m) in shared.backend_metrics.iter() {
        let snap = m.snapshot();
        aggregate.requests_total += snap.requests_total;
        aggregate.requests_success += snap.requests_success;
        aggregate.requests_error += snap.requests_error;
        aggregate.streams_started += snap.streams_started;
        aggregate.streams_completed += snap.streams_completed;
        aggregate.streams_failed += snap.streams_failed;
        aggregate.streams_client_disconnected += snap.streams_client_disconnected;
        backends.insert(
            name.clone(),
            serde_json::to_value(&snap).unwrap_or_default(),
        );
    }

    let (p50, p95, p99) = crate::admin::state::with_db(&shared.db, compute_latency_percentiles)
        .await
        .unwrap_or((None, None, None));

    Json(serde_json::json!({
        "backends": backends,
        "total": {
            "requests_total": aggregate.requests_total,
            "requests_success": aggregate.requests_success,
            "requests_error": aggregate.requests_error,
            "streams_started": aggregate.streams_started,
            "streams_completed": aggregate.streams_completed,
            "streams_failed": aggregate.streams_failed,
            "streams_client_disconnected": aggregate.streams_client_disconnected,
        },
        "latency_p50_ms": p50,
        "latency_p95_ms": p95,
        "latency_p99_ms": p99,
        "error_rate": aggregate.error_rate(),
    }))
}

#[derive(serde::Deserialize)]
pub(super) struct ObservabilityQuery {
    hours: Option<u32>,
    backend: Option<String>,
    key_id: Option<i64>,
    timeline_limit: Option<u32>,
    failure_limit: Option<u32>,
}

/// GET /admin/api/observability/overview -- request rollups for the operator dashboard.
pub(super) async fn get_observability_overview(
    State(shared): State<SharedState>,
    Query(params): Query<ObservabilityQuery>,
) -> Json<serde_json::Value> {
    let hours = params.hours.unwrap_or(6).clamp(1, 168);
    let timeline_limit = params.timeline_limit.unwrap_or(40).clamp(10, 200);
    let failure_limit = params.failure_limit.unwrap_or(12).clamp(1, 100);
    let backend = params
        .backend
        .filter(|value| !value.is_empty() && value.len() <= 128);
    let key_id = params.key_id;

    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let since = crate::admin::db::epoch_to_iso8601(now_epoch.saturating_sub(hours as u64 * 3600));
    let until = crate::admin::db::now_iso8601();
    let until_display = until.clone();

    match crate::admin::state::with_db(&shared.db, move |conn| {
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
    .await
    {
        Some(Ok((series, timeline, failures))) => {
            let totals = series.iter().fold(
                (0u64, 0u64, 0u64, 0u64, 0.0f64),
                |(requests_total, requests_error, input_tokens, output_tokens, cost_usd),
                 bucket| {
                    (
                        requests_total + bucket.requests_total,
                        requests_error + bucket.requests_error,
                        input_tokens + bucket.input_tokens,
                        output_tokens + bucket.output_tokens,
                        cost_usd + bucket.cost_usd,
                    )
                },
            );

            Json(serde_json::json!({
                "window_hours": hours,
                "generated_at": until_display,
                "totals": {
                    "requests_total": totals.0,
                    "requests_error": totals.1,
                    "input_tokens": totals.2,
                    "output_tokens": totals.3,
                    "cost_usd": totals.4,
                    "error_rate": if totals.0 == 0 {
                        0.0
                    } else {
                        totals.1 as f64 / totals.0 as f64
                    },
                },
                "series": series,
                "timeline": timeline,
                "failures": failures,
            }))
        }
        Some(Err(e)) => {
            tracing::error!(error = %e, "query observability overview failed");
            Json(serde_json::json!({
                "error": "internal database error",
                "series": [],
                "timeline": [],
                "failures": [],
            }))
        }
        None => Json(serde_json::json!({
            "error": "task panicked",
            "series": [],
            "timeline": [],
            "failures": [],
        })),
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
