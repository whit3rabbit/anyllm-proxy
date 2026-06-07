use super::common::now_iso8601;
use crate::admin::keys::VirtualKeyRow;
use rusqlite::{params, Connection};

/// Parameters for creating a new virtual key.
pub struct InsertVirtualKeyParams<'a> {
    pub key_hash: &'a str,
    pub key_prefix: &'a str,
    pub description: Option<&'a str>,
    pub expires_at: Option<&'a str>,
    pub rpm_limit: Option<u32>,
    pub tpm_limit: Option<u32>,
    pub spend_limit: Option<f64>,
    pub role: &'a str,
    pub max_budget_usd: Option<f64>,
    pub budget_duration: Option<&'a str>,
    pub allowed_models: Option<String>,
    pub allowed_routes: Option<String>,
}

/// Insert a new virtual API key.
pub fn insert_virtual_key(conn: &Connection, p: &InsertVirtualKeyParams) -> rusqlite::Result<i64> {
    let now = now_iso8601();
    conn.execute(
        "INSERT INTO virtual_api_key (key_hash, key_prefix, description, created_at, expires_at, \
         rpm_limit, tpm_limit, spend_limit, role, max_budget_usd, budget_duration, period_start, \
         allowed_models, allowed_routes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            p.key_hash,
            p.key_prefix,
            p.description,
            now,
            p.expires_at,
            p.rpm_limit.map(|v| v as i64),
            p.tpm_limit.map(|v| v as i64),
            p.spend_limit,
            p.role,
            p.max_budget_usd,
            p.budget_duration,
            // Set period_start to now if budget_duration is set
            p.budget_duration.map(|_| &now),
            p.allowed_models,
            p.allowed_routes,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Map a SQLite row to a VirtualKeyRow.
fn row_to_virtual_key(row: &rusqlite::Row) -> rusqlite::Result<VirtualKeyRow> {
    Ok(VirtualKeyRow {
        id: row.get(0)?,
        key_hash: row.get(1)?,
        key_prefix: row.get(2)?,
        description: row.get(3)?,
        created_at: row.get(4)?,
        expires_at: row.get(5)?,
        revoked_at: row.get(6)?,
        rpm_limit: row.get::<_, Option<i64>>(7)?.map(|v| v as u32),
        tpm_limit: row.get::<_, Option<i64>>(8)?.map(|v| v as u32),
        spend_limit: row.get(9)?,
        total_spend: row.get::<_, f64>(10).unwrap_or(0.0),
        total_requests: row.get::<_, i64>(11).unwrap_or(0),
        total_tokens: row.get::<_, i64>(12).unwrap_or(0),
        role: row
            .get::<_, String>(13)
            .unwrap_or_else(|_| "developer".into()),
        max_budget_usd: row.get(14).unwrap_or(None),
        budget_duration: row.get(15).unwrap_or(None),
        period_start: row.get(16).unwrap_or(None),
        period_spend_usd: row.get::<_, f64>(17).unwrap_or(0.0),
        total_input_tokens: row.get::<_, i64>(18).unwrap_or(0),
        total_output_tokens: row.get::<_, i64>(19).unwrap_or(0),
        allowed_models: row
            .get::<_, Option<String>>(20)
            .unwrap_or(None)
            .and_then(|s| serde_json::from_str(&s).ok()),
        allowed_routes: row
            .get::<_, Option<String>>(21)
            .unwrap_or(None)
            .and_then(|s| serde_json::from_str(&s).ok()),
    })
}

const VIRTUAL_KEY_COLUMNS: &str =
    "id, key_hash, key_prefix, description, created_at, expires_at, revoked_at, \
     rpm_limit, tpm_limit, spend_limit, total_spend, total_requests, total_tokens, \
     role, max_budget_usd, budget_duration, period_start, period_spend_usd, \
     total_input_tokens, total_output_tokens, allowed_models, allowed_routes";

/// List all virtual keys (active, expired, revoked).
pub fn list_virtual_keys(conn: &Connection) -> rusqlite::Result<Vec<VirtualKeyRow>> {
    let sql = format!("SELECT {VIRTUAL_KEY_COLUMNS} FROM virtual_api_key ORDER BY id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_virtual_key)?;
    rows.collect()
}

/// Revoke a virtual key by setting revoked_at. Returns the row if found.
pub fn revoke_virtual_key(conn: &Connection, id: i64) -> rusqlite::Result<Option<VirtualKeyRow>> {
    let now = now_iso8601();
    let updated = conn.execute(
        "UPDATE virtual_api_key SET revoked_at = ?1 WHERE id = ?2 AND revoked_at IS NULL",
        params![now, id],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    let sql = format!("SELECT {VIRTUAL_KEY_COLUMNS} FROM virtual_api_key WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_row(params![id], |row| Ok(Some(row_to_virtual_key(row)?)))
}

/// Parameters for updating an existing virtual key (all fields are optional; None = clear).
pub struct UpdateVirtualKeyParams<'a> {
    pub description: Option<&'a str>,
    pub expires_at: Option<&'a str>,
    pub rpm_limit: Option<u32>,
    pub tpm_limit: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub budget_duration: Option<&'a str>,
    pub allowed_models: Option<String>,
    pub allowed_routes: Option<String>,
}

/// Update an existing virtual key. Returns the updated row, or None if not found / revoked.
/// When `budget_duration` is provided, the budget period is reset (period_start = NULL,
/// period_spend_usd = 0) so the new window starts fresh.
pub fn update_virtual_key(
    conn: &Connection,
    id: i64,
    p: &UpdateVirtualKeyParams,
) -> rusqlite::Result<Option<VirtualKeyRow>> {
    // When changing budget_duration, reset the spend period so the new window starts clean.
    let mut sql = String::from(
        "UPDATE virtual_api_key
         SET description = ?2, expires_at = ?3, rpm_limit = ?4, tpm_limit = ?5,
             max_budget_usd = ?6, budget_duration = ?7, allowed_models = ?8,
             allowed_routes = ?9",
    );
    if p.budget_duration.is_some() {
        sql.push_str(", period_start = NULL, period_spend_usd = 0.0");
    }
    sql.push_str(" WHERE id = ?1 AND revoked_at IS NULL");
    let updated = conn.execute(
        &sql,
        params![
            id,
            p.description,
            p.expires_at,
            p.rpm_limit.map(|v| v as i64),
            p.tpm_limit.map(|v| v as i64),
            p.max_budget_usd,
            p.budget_duration,
            p.allowed_models,
            p.allowed_routes,
        ],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    let sql = format!("SELECT {VIRTUAL_KEY_COLUMNS} FROM virtual_api_key WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_row(params![id], |row| Ok(Some(row_to_virtual_key(row)?)))
}

/// Load all active (non-revoked, non-expired) virtual keys from the database.
pub fn load_active_virtual_keys(conn: &Connection) -> rusqlite::Result<Vec<VirtualKeyRow>> {
    let now = now_iso8601();
    let sql = format!(
        "SELECT {VIRTUAL_KEY_COLUMNS} FROM virtual_api_key \
         WHERE revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?1)"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![now], row_to_virtual_key)?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::init_db(&conn).unwrap();
        conn
    }

    fn sample_key_params() -> InsertVirtualKeyParams<'static> {
        InsertVirtualKeyParams {
            key_hash: "hash-abc",
            key_prefix: "sk-vk-test",
            description: Some("test key"),
            expires_at: None,
            rpm_limit: Some(100),
            tpm_limit: None,
            spend_limit: None,
            role: "user",
            max_budget_usd: Some(10.0),
            budget_duration: Some("monthly"),
            allowed_models: None,
            allowed_routes: None,
        }
    }

    #[test]
    fn update_virtual_key_returns_updated_row() {
        let conn = in_memory_db();
        let id = insert_virtual_key(&conn, &sample_key_params()).unwrap();

        let params = UpdateVirtualKeyParams {
            description: Some("updated desc"),
            expires_at: None,
            rpm_limit: Some(200),
            tpm_limit: None,
            max_budget_usd: None,
            budget_duration: None,
            allowed_models: None,
            allowed_routes: None,
        };
        let row = update_virtual_key(&conn, id, &params).unwrap();
        assert!(row.is_some());
        let row = row.unwrap();
        assert_eq!(row.description.as_deref(), Some("updated desc"));
        assert_eq!(row.rpm_limit, Some(200));
    }

    #[test]
    fn update_virtual_key_on_revoked_returns_none() {
        let conn = in_memory_db();
        let id = insert_virtual_key(&conn, &sample_key_params()).unwrap();
        revoke_virtual_key(&conn, id).unwrap();

        let params = UpdateVirtualKeyParams {
            description: Some("should not apply"),
            expires_at: None,
            rpm_limit: None,
            tpm_limit: None,
            max_budget_usd: None,
            budget_duration: None,
            allowed_models: None,
            allowed_routes: None,
        };
        let row = update_virtual_key(&conn, id, &params).unwrap();
        assert!(row.is_none());
    }

    #[test]
    fn update_virtual_key_allowed_models_roundtrip() {
        let conn = in_memory_db();
        let id = insert_virtual_key(&conn, &sample_key_params()).unwrap();

        let models_json = serde_json::to_string(&["gpt-4o", "claude-*"]).unwrap();
        let params = UpdateVirtualKeyParams {
            description: None,
            expires_at: None,
            rpm_limit: None,
            tpm_limit: None,
            max_budget_usd: None,
            budget_duration: None,
            allowed_models: Some(models_json),
            allowed_routes: None,
        };
        let row = update_virtual_key(&conn, id, &params).unwrap().unwrap();
        assert_eq!(
            row.allowed_models,
            Some(vec!["gpt-4o".to_string(), "claude-*".to_string()])
        );
    }

    #[test]
    fn update_virtual_key_budget_duration_resets_period() {
        let conn = in_memory_db();
        let id = insert_virtual_key(&conn, &sample_key_params()).unwrap();
        conn.execute(
            "UPDATE virtual_api_key SET period_spend_usd = 5.0, period_start = '2020-01-01' WHERE id = ?1",
            params![id],
        )
        .unwrap();

        let params = UpdateVirtualKeyParams {
            description: None,
            expires_at: None,
            rpm_limit: None,
            tpm_limit: None,
            max_budget_usd: None,
            budget_duration: Some("daily"),
            allowed_models: None,
            allowed_routes: None,
        };
        update_virtual_key(&conn, id, &params).unwrap();

        let (spend, start): (f64, Option<String>) = conn
            .query_row(
                "SELECT period_spend_usd, period_start FROM virtual_api_key WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(spend, 0.0);
        assert!(start.is_none());
    }
}
