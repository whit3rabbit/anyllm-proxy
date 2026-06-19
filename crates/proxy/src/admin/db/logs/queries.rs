use rusqlite::Connection;
use std::collections::HashMap;

use super::failure_normalization::{
    first_failure_line, normalize_failure_group_key_from_line, truncate_for_display,
};
use super::{
    row_to_request_log, ObservabilityBucket, ObservabilityFailureItem, ObservabilityTimelineItem,
};
use crate::admin::db::common::epoch_to_iso8601_ms;
use crate::admin::state::RequestLogEntry;

/// Typed status code filter -- prevents SQL injection by construction.
/// Only valid patterns are representable; invalid input is rejected at parse time.
pub enum StatusFilter {
    Exact(u16),
    Class2xx,
    Class4xx,
    Class5xx,
}

impl StatusFilter {
    pub(super) fn parse(s: &str) -> Option<Self> {
        match s {
            "2xx" => Some(Self::Class2xx),
            "4xx" => Some(Self::Class4xx),
            "5xx" => Some(Self::Class5xx),
            other => other.parse::<u16>().ok().map(Self::Exact),
        }
    }

    fn apply_to_query(&self, sql: &mut String, params: &mut Vec<Box<dyn rusqlite::types::ToSql>>) {
        match self {
            Self::Exact(code) => {
                sql.push_str(" AND status_code = ?");
                params.push(Box::new(*code as i64));
            }
            Self::Class2xx => sql.push_str(" AND status_code >= 200 AND status_code < 300"),
            Self::Class4xx => sql.push_str(" AND status_code >= 400 AND status_code < 500"),
            Self::Class5xx => sql.push_str(" AND status_code >= 500 AND status_code < 600"),
        }
    }
}

/// Query the request log with optional filters. Returns rows newest-first.
#[allow(clippy::too_many_arguments)]
pub fn query_request_log(
    conn: &Connection,
    limit: u32,
    offset: u32,
    backend: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
    status_filter: Option<&str>,
    key_id: Option<i64>,
) -> rusqlite::Result<Vec<RequestLogEntry>> {
    let mut sql = String::from(
        "SELECT request_id, timestamp, backend, model_requested, model_mapped,
                status_code, latency_ms, input_tokens, output_tokens, is_streaming, error_message,
                error_kind, key_id, cost_usd
         FROM request_log WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(b) = backend {
        sql.push_str(" AND backend = ?");
        param_values.push(Box::new(b.to_string()));
    }
    if let Some(s) = since {
        sql.push_str(" AND timestamp >= ?");
        param_values.push(Box::new(s.to_string()));
    }
    if let Some(u) = until {
        sql.push_str(" AND timestamp <= ?");
        param_values.push(Box::new(u.to_string()));
    }
    if let Some(sf) = status_filter {
        if let Some(parsed) = StatusFilter::parse(sf) {
            parsed.apply_to_query(&mut sql, &mut param_values);
        }
        // Invalid filter silently ignored
    }
    if let Some(kid) = key_id {
        sql.push_str(" AND key_id = ?");
        param_values.push(Box::new(kid));
    }
    sql.push_str(" ORDER BY id DESC LIMIT ? OFFSET ?");
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), row_to_request_log)?;

    rows.collect()
}

/// Append the optional `until`, `backend`, and `key_id` WHERE clauses shared by all
/// observability queries. `params` must already contain the `since` binding as `?1`.
fn append_common_filters(
    sql: &mut String,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    until: Option<&str>,
    backend: Option<&str>,
    key_id: Option<i64>,
) {
    if let Some(u) = until {
        sql.push_str(" AND timestamp <= ?");
        params.push(Box::new(u.to_string()));
    }
    if let Some(b) = backend {
        sql.push_str(" AND backend = ?");
        params.push(Box::new(b.to_string()));
    }
    if let Some(kid) = key_id {
        sql.push_str(" AND key_id = ?");
        params.push(Box::new(kid));
    }
}

/// Aggregate request log into 1-minute buckets for the timeseries chart.
pub fn query_request_timeseries(
    conn: &Connection,
    since: &str,
    until: Option<&str>,
    backend: Option<&str>,
    key_id: Option<i64>,
) -> rusqlite::Result<Vec<ObservabilityBucket>> {
    let mut sql = String::from(
        "SELECT strftime('%Y-%m-%dT%H:%M:00Z', timestamp) AS bucket_start,
                COUNT(*) AS requests_total,
                SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END) AS requests_error,
                SUM(COALESCE(input_tokens, 0)) AS input_tokens,
                SUM(COALESCE(output_tokens, 0)) AS output_tokens,
                SUM(COALESCE(cost_usd, 0.0)) AS cost_usd
         FROM request_log
         WHERE timestamp >= ?",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(since.to_string())];

    append_common_filters(&mut sql, &mut param_values, until, backend, key_id);

    sql.push_str(" GROUP BY bucket_start ORDER BY bucket_start ASC");

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|value| value.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(ObservabilityBucket {
            bucket_start: row.get(0)?,
            requests_total: row.get::<_, i64>(1)?.max(0) as u64,
            requests_error: row.get::<_, i64>(2)?.max(0) as u64,
            input_tokens: row.get::<_, i64>(3)?.max(0) as u64,
            output_tokens: row.get::<_, i64>(4)?.max(0) as u64,
            cost_usd: row.get::<_, f64>(5).unwrap_or(0.0),
        })
    })?;
    rows.collect()
}

/// Fetch individual request entries for the waterfall timeline view (newest first).
pub fn query_request_timeline(
    conn: &Connection,
    since: &str,
    until: Option<&str>,
    backend: Option<&str>,
    key_id: Option<i64>,
    limit: u32,
) -> rusqlite::Result<Vec<ObservabilityTimelineItem>> {
    let mut sql = String::from(
        "SELECT request_id, timestamp, backend, model_requested, model_mapped, status_code,
                latency_ms, input_tokens, output_tokens, is_streaming, error_message,
                error_kind, key_id, cost_usd, CAST(strftime('%s', timestamp) AS INTEGER) * 1000
         FROM request_log
         WHERE timestamp >= ?",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(since.to_string())];

    append_common_filters(&mut sql, &mut param_values, until, backend, key_id);

    sql.push_str(" ORDER BY timestamp DESC LIMIT ?");
    param_values.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|value| value.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let finished_at_ms = row.get::<_, i64>(14)?.max(0) as u64;
        let latency_ms = row.get::<_, i64>(6)?.max(0) as u64;
        let started_at_ms = finished_at_ms.saturating_sub(latency_ms);
        let model_requested: Option<String> = row.get(3)?;
        let model_mapped: Option<String> = row.get(4)?;
        Ok(ObservabilityTimelineItem {
            request_id: row.get(0)?,
            started_at: epoch_to_iso8601_ms(started_at_ms),
            finished_at: epoch_to_iso8601_ms(finished_at_ms),
            backend: row.get(2)?,
            model: model_mapped.or(model_requested),
            status_code: row.get::<_, i64>(5)?.max(0) as u16,
            latency_ms,
            input_tokens: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
            output_tokens: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
            is_streaming: row.get::<_, i64>(9)? != 0,
            error_message: row.get(10)?,
            error_kind: row.get(11)?,
            key_id: row.get(12)?,
            cost_usd: row.get(13)?,
        })
    })?;
    rows.collect()
}

/// Group recent failures by (error_kind, backend, model, status_code) for the failure-breakdown panel.
pub fn query_failure_breakdown(
    conn: &Connection,
    since: &str,
    until: Option<&str>,
    backend: Option<&str>,
    key_id: Option<i64>,
    limit: u32,
) -> rusqlite::Result<Vec<ObservabilityFailureItem>> {
    let mut sql = String::from(
        "SELECT timestamp, backend, model_requested, model_mapped, status_code,
                latency_ms, error_message, error_kind
         FROM request_log
         WHERE timestamp >= ? AND status_code >= 400",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(since.to_string())];

    append_common_filters(&mut sql, &mut param_values, until, backend, key_id);

    // Fetch at most 2000 rows before Rust-side aggregation.
    sql.push_str(" ORDER BY timestamp DESC LIMIT 2000");

    #[derive(Debug)]
    struct FailureAggregate {
        error_kind: Option<String>,
        backend: String,
        model: Option<String>,
        status_code: u16,
        count: u64,
        latest_seen: String,
        total_latency_ms: u64,
        summary: String,
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|value| value.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let model_requested: Option<String> = row.get(2)?;
        let model_mapped: Option<String> = row.get(3)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            model_mapped.or(model_requested),
            row.get::<_, i64>(4)?.max(0) as u16,
            row.get::<_, i64>(5)?.max(0) as u64,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;

    let mut grouped = HashMap::<String, FailureAggregate>::new();
    for row in rows {
        let (timestamp, backend_name, model, status_code, latency_ms, error_message, error_kind) =
            row?;
        let first_line = first_failure_line(error_message.as_deref());
        let summary = truncate_for_display(&first_line, 120);
        let normalized = normalize_failure_group_key_from_line(&first_line);
        let group_key = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            backend_name,
            status_code,
            model.clone().unwrap_or_default(),
            error_kind.clone().unwrap_or_default(),
            normalized
        );

        let entry = grouped
            .entry(group_key)
            .or_insert_with(|| FailureAggregate {
                error_kind: error_kind.clone(),
                backend: backend_name.clone(),
                model: model.clone(),
                status_code,
                count: 0,
                latest_seen: timestamp.clone(),
                total_latency_ms: 0,
                summary: summary.clone(),
            });
        entry.count += 1;
        entry.total_latency_ms = entry.total_latency_ms.saturating_add(latency_ms);
        if timestamp >= entry.latest_seen {
            entry.latest_seen = timestamp;
            entry.summary = summary;
        }
    }

    let mut failures = grouped
        .into_values()
        .map(|aggregate| ObservabilityFailureItem {
            error_kind: aggregate.error_kind,
            backend: aggregate.backend,
            model: aggregate.model,
            status_code: aggregate.status_code,
            count: aggregate.count,
            latest_seen: aggregate.latest_seen,
            avg_latency_ms: aggregate
                .total_latency_ms
                .checked_div(aggregate.count)
                .unwrap_or(0),
            summary: aggregate.summary,
        })
        .collect::<Vec<_>>();

    failures.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| right.latest_seen.cmp(&left.latest_seen))
            .then_with(|| left.summary.cmp(&right.summary))
    });
    failures.truncate(limit as usize);
    Ok(failures)
}
