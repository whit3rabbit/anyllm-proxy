use super::common::now_unix_secs;
use rusqlite::Connection;

/// Prune health_checks rows older than 31 days. Called each write cycle.
pub fn prune_health_checks(conn: &Connection) -> rusqlite::Result<()> {
    let cutoff = now_unix_secs() - 31 * 24 * 3600;
    conn.execute("DELETE FROM health_checks WHERE checked_at < ?1", [cutoff])?;
    Ok(())
}

/// Record one health check result and prune old rows.
pub fn insert_health_check(
    conn: &Connection,
    backend: &str,
    status: &str,
    latency_ms: Option<u64>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO health_checks (backend, checked_at, status, latency_ms) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![backend, now_unix_secs(), status, latency_ms.map(|v| v as i64)],
    )?;
    prune_health_checks(conn)?;
    Ok(())
}

/// Returns the uptime percentage for a backend over the last 30 days.
pub fn backend_uptime_pct(conn: &Connection, backend: &str) -> rusqlite::Result<f64> {
    let cutoff = now_unix_secs() - 30 * 24 * 3600;
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM health_checks WHERE backend = ?1 AND checked_at >= ?2",
            rusqlite::params![backend, cutoff],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if total == 0 {
        return Ok(100.0);
    }
    let up: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM health_checks WHERE backend = ?1 AND checked_at >= ?2 AND status = 'up'",
            rusqlite::params![backend, cutoff],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok((up as f64 / total as f64) * 100.0)
}

/// Returns per-day status for the last 30 days (date string -> 'up'|'down'|'degraded').
pub fn backend_history_30d(
    conn: &Connection,
    backend: &str,
) -> rusqlite::Result<Vec<(String, String)>> {
    let cutoff = now_unix_secs() - 30 * 24 * 3600;
    let mut stmt = conn.prepare(
        "SELECT
            date(checked_at, 'unixepoch') AS day,
            SUM(CASE WHEN status='up' THEN 1 ELSE 0 END) AS ups,
            COUNT(*) AS total
         FROM health_checks
         WHERE backend = ?1 AND checked_at >= ?2
         GROUP BY day
         ORDER BY day ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![backend, cutoff], |r| {
        let day: String = r.get(0)?;
        let ups: i64 = r.get(1)?;
        let total: i64 = r.get(2)?;
        let status = if ups == total {
            "up".to_string()
        } else if ups == 0 {
            "down".to_string()
        } else {
            "degraded".to_string()
        };
        Ok((day, status))
    })?;
    rows.collect()
}

/// Upsert a batch of model IDs for a provider into the cache.
/// Replaces all existing entries for this provider atomically.
pub fn upsert_provider_models_cache(
    conn: &mut Connection,
    provider_id: &str,
    model_ids: &[String],
) -> rusqlite::Result<()> {
    let now = now_unix_secs();
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM provider_models_cache WHERE provider_id = ?1",
        rusqlite::params![provider_id],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO provider_models_cache (provider_id, model_id, fetched_at)
             VALUES (?1, ?2, ?3)",
        )?;
        for model_id in model_ids {
            stmt.execute(rusqlite::params![provider_id, model_id, now])?;
        }
    }
    tx.commit()
}

/// Return all cached model IDs for a provider, sorted.
pub fn list_cached_provider_models(
    conn: &Connection,
    provider_id: &str,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT model_id FROM provider_models_cache WHERE provider_id = ?1 ORDER BY model_id",
    )?;
    let rows = stmt.query_map(rusqlite::params![provider_id], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Return a map of provider_id -> (model_count, last_refreshed) for all providers
/// with cached data. One query instead of one-per-provider.
pub fn get_all_provider_cache_stats(
    conn: &Connection,
) -> rusqlite::Result<std::collections::HashMap<String, (usize, Option<i64>)>> {
    let mut stmt = conn.prepare(
        "SELECT provider_id, COUNT(*), MAX(fetched_at) \
         FROM provider_models_cache GROUP BY provider_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (pid, count, refreshed) = row?;
        map.insert(pid, (count as usize, refreshed));
    }
    Ok(map)
}
