// GET /admin/api/uptime

use crate::admin::db::{backend_history_30d, backend_uptime_pct};
use crate::admin::state::{with_db, SharedState};
use axum::{extract::State, Json};
use serde::Serialize;

/// One day in the 30-day health history calendar.
#[derive(Serialize)]
pub struct HistoryDay {
    date: String,
    /// `"up"`, `"down"`, or `"unknown"`.
    status: String,
}

/// Proxy process uptime summary.
#[derive(Serialize)]
pub struct ProxyUptimeInfo {
    /// Unix timestamp of admin server startup.
    started_at: u64,
    uptime_pct_30d: f64,
    history: Vec<HistoryDay>,
}

/// Per-backend uptime summary for the Uptime tab.
#[derive(Serialize)]
pub struct BackendUptimeInfo {
    name: String,
    status: String,
    last_checked_at: Option<i64>,
    last_latency_ms: Option<i64>,
    uptime_pct_30d: f64,
    history: Vec<HistoryDay>,
}

/// Response body for `GET /admin/api/uptime`.
#[derive(Serialize)]
pub struct UptimeResponse {
    proxy: ProxyUptimeInfo,
    backends: Vec<BackendUptimeInfo>,
}

/// GET /admin/api/uptime — returns proxy and per-backend uptime stats for the Uptime tab.
pub(super) async fn get_uptime(State(shared): State<SharedState>) -> Json<UptimeResponse> {
    let started_at = shared
        .started_at
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Snapshot backend names from RuntimeConfig (holds the admin-visible backend list).
    let backend_names: Vec<String> = {
        let cfg = shared
            .runtime_config
            .read()
            .unwrap_or_else(|e| e.into_inner());
        cfg.model_mappings.keys().cloned().collect()
    };

    let response = with_db(&shared.db, move |conn| {
        // Aggregate proxy-level history: any day where any backend was down counts as down.
        let proxy_history: Vec<HistoryDay> = conn
            .prepare(
                "SELECT date(checked_at, 'unixepoch') AS day,
                        MIN(CASE WHEN status='down' THEN 0 ELSE 1 END) AS all_up
                 FROM health_checks
                 WHERE checked_at >= strftime('%s','now') - 30*86400
                 GROUP BY day
                 ORDER BY day ASC",
            )
            .ok()
            .and_then(|mut stmt| {
                stmt.query_map([], |r| {
                    Ok(HistoryDay {
                        date: r.get(0)?,
                        status: if r.get::<_, i64>(1)? == 1 {
                            "up".to_string()
                        } else {
                            "down".to_string()
                        },
                    })
                })
                .ok()
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default();

        let backends: Vec<BackendUptimeInfo> = backend_names
            .iter()
            .map(|name| {
                let uptime_pct = backend_uptime_pct(conn, name).unwrap_or(100.0);
                let history = backend_history_30d(conn, name)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(date, status)| HistoryDay { date, status })
                    .collect();

                let (last_checked_at, last_latency_ms, current_status) = conn
                    .query_row(
                        "SELECT checked_at, latency_ms, status FROM health_checks
                         WHERE backend = ?1 ORDER BY checked_at DESC LIMIT 1",
                        [name.as_str()],
                        |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                r.get::<_, Option<i64>>(1)?,
                                r.get::<_, String>(2)?,
                            ))
                        },
                    )
                    .map(|(ts, lat, st)| (Some(ts), lat, st))
                    .unwrap_or((None, None, "unknown".to_string()));

                BackendUptimeInfo {
                    name: name.clone(),
                    status: current_status,
                    last_checked_at,
                    last_latency_ms,
                    uptime_pct_30d: uptime_pct,
                    history,
                }
            })
            .collect();

        let down_days = proxy_history.iter().filter(|d| d.status == "down").count();
        let total_days = proxy_history.len();
        let proxy_uptime_pct = if total_days == 0 {
            100.0
        } else {
            (total_days - down_days) as f64 / total_days as f64 * 100.0
        };

        UptimeResponse {
            proxy: ProxyUptimeInfo {
                started_at,
                uptime_pct_30d: proxy_uptime_pct,
                history: proxy_history,
            },
            backends,
        }
    })
    .await;

    Json(response.unwrap_or_else(|| UptimeResponse {
        proxy: ProxyUptimeInfo {
            started_at,
            uptime_pct_30d: 100.0,
            history: vec![],
        },
        backends: vec![],
    }))
}
