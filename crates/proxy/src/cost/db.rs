// SQLite persistence for per-key cost tracking.
//
// Accumulates spend and token counts on the virtual_api_key table.
// Reads are used by the admin spend endpoint.

use rusqlite::{params, Connection};

/// Increment spend counters for a virtual key in SQLite.
pub fn accumulate_spend(
    conn: &Connection,
    key_id: i64,
    cost_usd: f64,
    input_tokens: u64,
    output_tokens: u64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE virtual_api_key SET
         total_spend = total_spend + ?1,
         total_requests = total_requests + 1,
         total_tokens = total_tokens + ?2 + ?3,
         total_input_tokens = total_input_tokens + ?2,
         total_output_tokens = total_output_tokens + ?3,
         period_spend_usd = period_spend_usd + ?1
         WHERE id = ?4",
        params![cost_usd, input_tokens as i64, output_tokens as i64, key_id],
    )?;
    Ok(())
}

/// Atomically reset the period budget in SQLite when a new budget period begins.
/// Called when `check_and_reset_period` triggers a rollover; must run before
/// `accumulate_spend` so the running total starts from zero for the new period.
pub fn reset_period_spend(
    conn: &Connection,
    key_id: i64,
    new_period_start: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE virtual_api_key SET period_spend_usd = 0.0, period_start = ?1 WHERE id = ?2",
        params![new_period_start, key_id],
    )?;
    Ok(())
}

/// Per-key spend summary returned by the admin endpoint.
#[derive(Debug, serde::Serialize)]
pub struct KeySpend {
    pub key_id: i64,
    pub key_prefix: String,
    pub total_cost_usd: f64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub request_count: i64,
    pub period_cost_usd: f64,
    pub period_start: Option<String>,
    pub budget_duration: Option<String>,
    pub max_budget_usd: Option<f64>,
}

/// Fetch spend data for a single virtual key.
pub fn get_key_spend(conn: &Connection, key_id: i64) -> rusqlite::Result<Option<KeySpend>> {
    let mut stmt = conn.prepare(
        "SELECT id, key_prefix, total_spend, total_input_tokens, total_output_tokens,
                total_requests, period_spend_usd, max_budget_usd, period_start, budget_duration
         FROM virtual_api_key WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![key_id], |row| {
        Ok(KeySpend {
            key_id: row.get(0)?,
            key_prefix: row.get(1)?,
            total_cost_usd: row.get::<_, f64>(2).unwrap_or(0.0),
            total_input_tokens: row.get::<_, i64>(3).unwrap_or(0),
            total_output_tokens: row.get::<_, i64>(4).unwrap_or(0),
            request_count: row.get::<_, i64>(5).unwrap_or(0),
            period_cost_usd: row.get::<_, f64>(6).unwrap_or(0.0),
            max_budget_usd: row.get::<_, Option<f64>>(7).unwrap_or(None),
            period_start: row.get::<_, Option<String>>(8).unwrap_or(None),
            budget_duration: row.get::<_, Option<String>>(9).unwrap_or(None),
        })
    })?;
    rows.next().transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::admin::db::init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn accumulate_spend_increments_totals() {
        let conn = test_db();
        // Insert a key
        let id = crate::admin::db::insert_virtual_key(
            &conn,
            &crate::admin::db::InsertVirtualKeyParams {
                key_hash: "abc123abc123abc123abc123abc123abc123abc123abc123abc123abc123abcd",
                key_prefix: "sk-vkabc",
                description: Some("test"),
                expires_at: None,
                rpm_limit: None,
                tpm_limit: None,
                spend_limit: Some(100.0),
                role: "developer",
                max_budget_usd: Some(100.0),
                budget_duration: None,
                allowed_models: None,
            },
        )
        .unwrap();

        // Accumulate first request
        accumulate_spend(&conn, id, 0.05, 1000, 500).unwrap();

        let spend = get_key_spend(&conn, id).unwrap().unwrap();
        assert_eq!(spend.key_id, id);
        assert!((spend.total_cost_usd - 0.05).abs() < 1e-10);
        assert_eq!(spend.total_input_tokens, 1000);
        assert_eq!(spend.total_output_tokens, 500);
        assert_eq!(spend.request_count, 1);

        // Accumulate second request
        accumulate_spend(&conn, id, 0.03, 800, 200).unwrap();

        let spend = get_key_spend(&conn, id).unwrap().unwrap();
        assert!((spend.total_cost_usd - 0.08).abs() < 1e-10);
        assert_eq!(spend.total_input_tokens, 1800);
        assert_eq!(spend.total_output_tokens, 700);
        assert_eq!(spend.request_count, 2);
        assert!((spend.period_cost_usd - 0.08).abs() < 1e-10);
        assert!((spend.max_budget_usd.unwrap() - 100.0).abs() < 1e-10);
    }

    #[test]
    fn get_key_spend_not_found() {
        let conn = test_db();
        let result = get_key_spend(&conn, 9999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn reset_period_spend_zeroes_and_updates_start() {
        let conn = test_db();
        let id = crate::admin::db::insert_virtual_key(
            &conn,
            &crate::admin::db::InsertVirtualKeyParams {
                key_hash: "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd",
                key_prefix: "sk-vkaabb",
                description: Some("period-reset-test"),
                expires_at: None,
                rpm_limit: None,
                tpm_limit: None,
                spend_limit: None,
                role: "developer",
                max_budget_usd: Some(10.0),
                budget_duration: Some("monthly"),
                allowed_models: None,
            },
        )
        .unwrap();

        // Accumulate spend in the old period
        accumulate_spend(&conn, id, 7.50, 1000, 500).unwrap();
        let spend = get_key_spend(&conn, id).unwrap().unwrap();
        assert!((spend.period_cost_usd - 7.50).abs() < 1e-10);

        // Simulate period rollover
        reset_period_spend(&conn, id, "2026-04-01T00:00:00Z").unwrap();
        let spend = get_key_spend(&conn, id).unwrap().unwrap();
        assert!((spend.period_cost_usd - 0.0).abs() < 1e-10);
        assert_eq!(spend.period_start.as_deref(), Some("2026-04-01T00:00:00Z"));

        // Accumulate in the new period: must be 1.25, NOT 7.50 + 1.25
        accumulate_spend(&conn, id, 1.25, 200, 100).unwrap();
        let spend = get_key_spend(&conn, id).unwrap().unwrap();
        assert!((spend.period_cost_usd - 1.25).abs() < 1e-10);
        // total_spend is cumulative across periods
        assert!((spend.total_cost_usd - 8.75).abs() < 1e-10);
    }
}
