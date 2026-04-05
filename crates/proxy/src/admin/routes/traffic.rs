// GET /admin/api/traffic?window=N
// Aggregates request_log by route (model_mapped/backend) and time bucket.

use crate::admin::state::{with_db, SharedState};
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct TrafficQuery {
    #[serde(default = "default_window")]
    window: u32,
}

fn default_window() -> u32 {
    6
}

#[derive(Serialize)]
pub struct RouteMetrics {
    path: String,
    requests_per_min: f64,
    error_rate: f64,
    avg_latency_ms: f64,
    p95_latency_ms: i64,
    total_requests: i64,
}

#[derive(Serialize)]
pub struct TrafficSeriesPoint {
    bucket_start: i64,
    path: String,
    requests: i64,
}

#[derive(Serialize)]
pub struct TrafficResponse {
    window_hours: u32,
    routes: Vec<RouteMetrics>,
    series: Vec<TrafficSeriesPoint>,
}

pub(super) async fn get_traffic(
    State(shared): State<SharedState>,
    Query(q): Query<TrafficQuery>,
) -> Json<TrafficResponse> {
    let window_hours = q.window.clamp(1, 168);
    let result = with_db(&shared.db, move |conn| query_traffic(conn, window_hours))
        .await
        .flatten()
        .unwrap_or_else(|| TrafficResponse {
            window_hours,
            routes: vec![],
            series: vec![],
        });
    Json(result)
}

fn query_traffic(conn: &rusqlite::Connection, window_hours: u32) -> Option<TrafficResponse> {
    let since = chrono::Utc::now() - chrono::Duration::hours(window_hours as i64);
    let since_str = since.format("%Y-%m-%dT%H:%M:%S").to_string();
    let window_min = window_hours as f64 * 60.0;

    // Per-route aggregate.
    let mut routes: Vec<RouteMetrics> = {
        let mut stmt = conn
            .prepare(
                "SELECT
                    COALESCE(model_mapped, backend) AS path,
                    COUNT(*) AS total,
                    SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END) AS errors,
                    COUNT(*) * 1.0 / ?1 AS rpm,
                    AVG(latency_ms) AS avg_latency
                 FROM request_log
                 WHERE timestamp >= ?2
                 GROUP BY path
                 ORDER BY total DESC",
            )
            .ok()?;
        let collected: Vec<RouteMetrics> = stmt
            .query_map(rusqlite::params![window_min, since_str], |r| {
                let path: String = r.get(0)?;
                let total: i64 = r.get(1)?;
                let errors: i64 = r.get(2)?;
                let rpm: f64 = r.get(3)?;
                let avg_latency: f64 = r.get::<_, Option<f64>>(4)?.unwrap_or(0.0);
                Ok(RouteMetrics {
                    path,
                    requests_per_min: rpm,
                    error_rate: if total > 0 {
                        errors as f64 / total as f64
                    } else {
                        0.0
                    },
                    avg_latency_ms: avg_latency,
                    p95_latency_ms: 0,
                    total_requests: total,
                })
            })
            .ok()?
            .filter_map(|r| r.ok())
            .collect();
        collected
    };

    // P95 per route via percentile-offset query.
    for r in routes.iter_mut() {
        let path_c = r.path.clone();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT latency_ms FROM request_log
             WHERE timestamp >= ?1 AND COALESCE(model_mapped, backend) = ?2
             ORDER BY latency_ms
             LIMIT 1 OFFSET CAST(0.95 * (
                 SELECT COUNT(*) FROM request_log
                 WHERE timestamp >= ?1 AND COALESCE(model_mapped, backend) = ?2
             ) AS INTEGER)",
        ) {
            if let Ok(p95) = stmt.query_row(
                rusqlite::params![since_str, path_c],
                |row| row.get::<_, i64>(0),
            ) {
                r.p95_latency_ms = p95;
            }
        }
    }

    // Time-bucketed series (5-minute buckets).
    let series: Vec<TrafficSeriesPoint> = {
        let mut stmt = conn
            .prepare(
                "SELECT
                    CAST(strftime('%s', timestamp) AS INTEGER) / 300 * 300 AS bucket,
                    COALESCE(model_mapped, backend) AS path,
                    COUNT(*) AS requests
                 FROM request_log
                 WHERE timestamp >= ?1
                 GROUP BY bucket, path
                 ORDER BY bucket ASC",
            )
            .ok()?;
        let collected: Vec<TrafficSeriesPoint> = stmt
            .query_map(rusqlite::params![since_str], |r| {
                let bucket: i64 = r.get(0)?;
                let path: String = r.get(1)?;
                let requests: i64 = r.get(2)?;
                Ok(TrafficSeriesPoint {
                    bucket_start: bucket,
                    path,
                    requests,
                })
            })
            .ok()?
            .filter_map(|r| r.ok())
            .collect();
        collected
    };

    Some(TrafficResponse {
        window_hours,
        routes,
        series,
    })
}
