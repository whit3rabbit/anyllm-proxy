use super::common::now_iso8601;
use rusqlite::{params, Connection};

/// A route row as stored in SQLite.
#[derive(Debug, Clone)]
pub struct RouteRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub strategy: String,
    pub rpm: Option<u32>,
    pub tpm: Option<u64>,
    pub budget_usd: Option<f64>,
    /// Route on/off toggle. Disabled routes do not dispatch and lose virtual-key scope.
    pub enabled: bool,
    /// Per-route option overrides. `None` means "inherit the global RuntimeConfig value".
    pub guardrail_mode: Option<String>,
    pub pxpipe_compress: Option<bool>,
    pub pxpipe_models: Option<String>,
    pub redact_secrets: Option<bool>,
    /// Explicit cross-route ordering (lower wins) when a model matches several routes.
    pub position: i32,
    pub created_at: String,
    pub updated_at: String,
}

/// A route↔provider assignment row.
#[derive(Debug, Clone)]
pub struct RouteProviderRow {
    pub id: String,
    pub route_id: String,
    pub backend_id: String,
    pub models: Vec<String>,
    pub priority: i32,
    pub enabled: bool,
    pub created_at: String,
}

pub fn insert_route(conn: &Connection, row: &RouteRow) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO routes (id, name, description, strategy, rpm, tpm, budget_usd,
             enabled, guardrail_mode, pxpipe_compress, pxpipe_models, redact_secrets,
             position, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            row.id,
            row.name,
            row.description,
            row.strategy,
            row.rpm.map(|v| v as i64),
            row.tpm.map(|v| v as i64),
            row.budget_usd,
            row.enabled as i32,
            row.guardrail_mode,
            row.pxpipe_compress.map(|v| v as i32),
            row.pxpipe_models,
            row.redact_secrets.map(|v| v as i32),
            row.position,
            row.created_at,
            row.updated_at,
        ],
    )?;
    Ok(())
}

fn row_to_route(row: &rusqlite::Row) -> rusqlite::Result<RouteRow> {
    Ok(RouteRow {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        strategy: row.get(3)?,
        rpm: row.get::<_, Option<i64>>(4)?.map(|v| v as u32),
        tpm: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
        budget_usd: row.get(6)?,
        enabled: row.get::<_, i32>(7)? != 0,
        guardrail_mode: row.get(8)?,
        pxpipe_compress: row.get::<_, Option<i32>>(9)?.map(|v| v != 0),
        pxpipe_models: row.get(10)?,
        redact_secrets: row.get::<_, Option<i32>>(11)?.map(|v| v != 0),
        position: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

/// Column list for route SELECTs, matching `row_to_route`'s index order.
const ROUTE_COLUMNS: &str = "id, name, description, strategy, rpm, tpm, budget_usd, \
     enabled, guardrail_mode, pxpipe_compress, pxpipe_models, redact_secrets, \
     position, created_at, updated_at";

pub fn list_routes(conn: &Connection) -> rusqlite::Result<Vec<RouteRow>> {
    let mut stmt = conn.prepare(&format!("SELECT {ROUTE_COLUMNS} FROM routes ORDER BY name"))?;
    let rows = stmt.query_map([], row_to_route)?;
    rows.collect()
}

pub fn get_route(conn: &Connection, id: &str) -> rusqlite::Result<Option<RouteRow>> {
    let mut stmt = conn.prepare(&format!("SELECT {ROUTE_COLUMNS} FROM routes WHERE id = ?1"))?;
    let mut rows = stmt.query_map([id], row_to_route)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Patch struct for updating a route. All fields optional; None means unchanged.
#[derive(serde::Deserialize)]
pub struct RoutePatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub strategy: Option<String>,
    pub rpm: Option<Option<u32>>,
    pub tpm: Option<Option<u64>>,
    pub budget_usd: Option<Option<f64>>,
    pub enabled: Option<bool>,
    // Nullable option overrides: `Some(Some(v))` sets, `Some(None)` clears to
    // NULL (= inherit global), `None` leaves unchanged.
    pub guardrail_mode: Option<Option<String>>,
    pub pxpipe_compress: Option<Option<bool>>,
    pub pxpipe_models: Option<Option<String>>,
    pub redact_secrets: Option<Option<bool>>,
    pub position: Option<i32>,
}

pub fn update_route(conn: &Connection, id: &str, patch: &RoutePatch) -> rusqlite::Result<bool> {
    let mut set_clauses: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref v) = patch.name {
        set_clauses.push("name = ?".into());
        param_values.push(Box::new(v.clone()));
    }
    if let Some(ref v) = patch.description {
        set_clauses.push("description = ?".into());
        param_values.push(Box::new(v.clone()));
    }
    if let Some(ref v) = patch.strategy {
        set_clauses.push("strategy = ?".into());
        param_values.push(Box::new(v.clone()));
    }
    if let Some(ref v) = patch.rpm {
        match v {
            Some(val) => {
                set_clauses.push("rpm = ?".into());
                param_values.push(Box::new(*val as i64));
            }
            None => {
                set_clauses.push("rpm = NULL".into());
            }
        }
    }
    if let Some(ref v) = patch.tpm {
        match v {
            Some(val) => {
                set_clauses.push("tpm = ?".into());
                param_values.push(Box::new(*val as i64));
            }
            None => {
                set_clauses.push("tpm = NULL".into());
            }
        }
    }
    if let Some(ref v) = patch.budget_usd {
        match v {
            Some(val) => {
                set_clauses.push("budget_usd = ?".into());
                param_values.push(Box::new(*val));
            }
            None => {
                set_clauses.push("budget_usd = NULL".into());
            }
        }
    }
    if let Some(v) = patch.enabled {
        set_clauses.push("enabled = ?".into());
        param_values.push(Box::new(v as i32));
    }
    if let Some(v) = patch.position {
        set_clauses.push("position = ?".into());
        param_values.push(Box::new(v));
    }
    // Nullable option overrides: Some(Some) sets, Some(None) clears to NULL.
    if let Some(ref v) = patch.guardrail_mode {
        match v {
            Some(val) => {
                set_clauses.push("guardrail_mode = ?".into());
                param_values.push(Box::new(val.clone()));
            }
            None => set_clauses.push("guardrail_mode = NULL".into()),
        }
    }
    if let Some(ref v) = patch.pxpipe_compress {
        match v {
            Some(val) => {
                set_clauses.push("pxpipe_compress = ?".into());
                param_values.push(Box::new(*val as i32));
            }
            None => set_clauses.push("pxpipe_compress = NULL".into()),
        }
    }
    if let Some(ref v) = patch.pxpipe_models {
        match v {
            Some(val) => {
                set_clauses.push("pxpipe_models = ?".into());
                param_values.push(Box::new(val.clone()));
            }
            None => set_clauses.push("pxpipe_models = NULL".into()),
        }
    }
    if let Some(ref v) = patch.redact_secrets {
        match v {
            Some(val) => {
                set_clauses.push("redact_secrets = ?".into());
                param_values.push(Box::new(*val as i32));
            }
            None => set_clauses.push("redact_secrets = NULL".into()),
        }
    }

    if set_clauses.is_empty() {
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM routes WHERE id = ?1", [id], |r| {
                r.get(0)
            })?;
        return Ok(count > 0);
    }

    set_clauses.push("updated_at = ?".into());
    param_values.push(Box::new(now_iso8601()));
    param_values.push(Box::new(id.to_string()));

    let sql = format!("UPDATE routes SET {} WHERE id = ?", set_clauses.join(", "));
    let params: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let updated = conn.execute(&sql, params.as_slice())?;
    Ok(updated > 0)
}

pub fn delete_route(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let deleted = conn.execute("DELETE FROM routes WHERE id = ?1", [id])?;
    Ok(deleted > 0)
}

pub fn list_route_providers(
    conn: &Connection,
    route_id: &str,
) -> rusqlite::Result<Vec<RouteProviderRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, route_id, backend_id, models, priority, enabled, created_at
         FROM route_providers WHERE route_id = ?1 ORDER BY priority ASC",
    )?;
    let rows = stmt.query_map([route_id], |row| {
        let models_json: String = row.get(3)?;
        let models: Vec<String> =
            serde_json::from_str(&models_json).unwrap_or_else(|_| vec!["*".into()]);
        Ok(RouteProviderRow {
            id: row.get(0)?,
            route_id: row.get(1)?,
            backend_id: row.get(2)?,
            models,
            priority: row.get(4)?,
            enabled: row.get::<_, i32>(5)? != 0,
            created_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

pub fn enabled_route_ids_for_backend_name(
    conn: &Connection,
    backend_name: &str,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT rp.route_id
         FROM route_providers rp
         JOIN managed_backends mb ON mb.id = rp.backend_id
         JOIN routes r ON r.id = rp.route_id
         WHERE mb.name = ?1 AND rp.enabled = 1 AND r.enabled = 1
         ORDER BY rp.route_id",
    )?;
    let rows = stmt.query_map([backend_name], |row| row.get::<_, String>(0))?;
    rows.collect()
}

pub fn count_route_providers(conn: &Connection, route_id: &str) -> rusqlite::Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM route_providers WHERE route_id = ?1",
        [route_id],
        |row| row.get(0),
    )?;
    Ok(n.max(0) as usize)
}

pub fn managed_backend_exists(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let n: i64 = conn
        .query_row(
            "SELECT 1 FROM managed_backends WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(0),
            other => Err(other),
        })?;
    Ok(n == 1)
}

pub fn add_route_provider(
    conn: &Connection,
    route_id: &str,
    backend_id: &str,
    models: &[String],
    priority: i32,
    enabled: bool,
) -> rusqlite::Result<()> {
    let models_json = serde_json::to_string(models).unwrap_or_else(|_| "[\"*\"]".into());
    conn.execute(
        "INSERT INTO route_providers (id, route_id, backend_id, models, priority, enabled)
         VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5)",
        params![route_id, backend_id, models_json, priority, enabled as i32],
    )?;
    Ok(())
}

pub fn update_route_provider(
    conn: &Connection,
    id: &str,
    models: Option<&[String]>,
    priority: Option<i32>,
    enabled: Option<bool>,
) -> rusqlite::Result<bool> {
    let mut set_clauses: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(m) = models {
        set_clauses.push("models = ?".into());
        param_values.push(Box::new(
            serde_json::to_string(m).unwrap_or_else(|_| "[\"*\"]".into()),
        ));
    }
    if let Some(p) = priority {
        set_clauses.push("priority = ?".into());
        param_values.push(Box::new(p));
    }
    if let Some(e) = enabled {
        set_clauses.push("enabled = ?".into());
        param_values.push(Box::new(e as i32));
    }

    if set_clauses.is_empty() {
        return Ok(true);
    }

    param_values.push(Box::new(id.to_string()));
    let sql = format!(
        "UPDATE route_providers SET {} WHERE id = ?",
        set_clauses.join(", ")
    );
    let params: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let updated = conn.execute(&sql, params.as_slice())?;
    Ok(updated > 0)
}

pub fn remove_route_provider(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let deleted = conn.execute("DELETE FROM route_providers WHERE id = ?1", [id])?;
    Ok(deleted > 0)
}

/// Result of `reorder_route_providers`.
///
/// `Ok` carries the reordered rows (priority ascending).
/// `Mismatch` means the caller's `ordered_ids` does not exactly match the
/// route's current provider set (wrong route, duplicate ids, missing ids, or extra ids).
/// On `Mismatch` the transaction is rolled back, so priorities are unchanged.
pub enum ReorderOutcome {
    Ok(Vec<RouteProviderRow>),
    Mismatch,
}

/// Atomically rewrite every `priority` on `route_id`'s providers so that the row
/// whose id is `ordered_ids[i]` has priority `i`.
///
/// Validates that the submitted id set is exactly the current provider set for
/// the route before writing. On any validation failure the transaction is rolled
/// back by returning `Mismatch` without committing.
pub fn reorder_route_providers(
    conn: &Connection,
    route_id: &str,
    ordered_ids: &[String],
) -> rusqlite::Result<ReorderOutcome> {
    // `unchecked_transaction` matches the pattern used elsewhere in this file
    // relies on the surrounding Mutex to serialize connection access rather than Rust's &mut borrow.
    let tx = conn.unchecked_transaction()?;

    // Fetch current providers for the route.
    let mut existing_ids: Vec<String> = {
        let mut stmt = tx.prepare("SELECT id FROM route_providers WHERE route_id = ?1")?;
        let rows = stmt.query_map([route_id], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    existing_ids.sort();

    let mut submitted_ids: Vec<String> = ordered_ids.to_vec();
    submitted_ids.sort();

    // Set equality: same length, same sorted contents, no duplicates.
    if submitted_ids.len() != existing_ids.len() || submitted_ids != existing_ids {
        // Dropping `tx` without commit rolls back.
        return Ok(ReorderOutcome::Mismatch);
    }

    // Write new priorities.
    for (idx, id) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE route_providers SET priority = ?1 WHERE id = ?2 AND route_id = ?3",
            params![idx as i32, id, route_id],
        )?;
    }

    tx.commit()?;

    // Return fresh state in the new order.
    list_route_providers(conn, route_id).map(ReorderOutcome::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::db::backends::ManagedBackendRow;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::init_db(&conn).unwrap();
        conn
    }

    fn test_row(name: &str) -> ManagedBackendRow {
        ManagedBackendRow {
            id: format!("id-{name}"),
            name: name.to_string(),
            provider_id: "openai".to_string(),
            api_key: Some("sk-test".to_string()),
            api_base: None,
            deployment: None,
            api_version: None,
            project: None,
            region: None,
            aws_access_key_id: None,
            aws_secret_access_key: None,
            aws_session_token: None,
            rpm: Some(100),
            tpm: Some(10_000),
            enabled: true,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn seed_route_with_providers(conn: &Connection, count: usize) -> (String, Vec<String>) {
        let route = RouteRow {
            id: uuid::Uuid::new_v4().to_string(),
            name: "r".into(),
            description: None,
            strategy: "failover".into(),
            rpm: None,
            tpm: None,
            budget_usd: None,
            enabled: true,
            guardrail_mode: None,
            pxpipe_compress: None,
            pxpipe_models: None,
            redact_secrets: None,
            position: 0,
            created_at: now_iso8601(),
            updated_at: now_iso8601(),
        };
        insert_route(conn, &route).unwrap();

        for i in 0..count {
            let mut b = test_row(&format!("b{i}"));
            b.id = format!("backend-{i}");
            crate::admin::db::backends::insert_managed_backend(conn, &b).unwrap();
            add_route_provider(conn, &route.id, &b.id, &["*".to_string()], i as i32, true).unwrap();
        }

        let ids: Vec<String> = list_route_providers(conn, &route.id)
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        (route.id, ids)
    }

    #[test]
    fn update_route_sets_and_clears_option_override() {
        let conn = in_memory_db();
        let (route_id, _) = seed_route_with_providers(&conn, 1);

        // Set an override.
        let set = RoutePatch {
            name: None,
            description: None,
            strategy: None,
            rpm: None,
            tpm: None,
            budget_usd: None,
            enabled: Some(false),
            guardrail_mode: Some(Some("standard".into())),
            pxpipe_compress: Some(Some(true)),
            pxpipe_models: None,
            redact_secrets: None,
            position: None,
        };
        assert!(update_route(&conn, &route_id, &set).unwrap());
        let r = get_route(&conn, &route_id).unwrap().unwrap();
        assert!(!r.enabled);
        assert_eq!(r.guardrail_mode.as_deref(), Some("standard"));
        assert_eq!(r.pxpipe_compress, Some(true));

        // Clear the override back to NULL (inherit).
        let clear = RoutePatch {
            name: None,
            description: None,
            strategy: None,
            rpm: None,
            tpm: None,
            budget_usd: None,
            enabled: None,
            guardrail_mode: Some(None),
            pxpipe_compress: Some(None),
            pxpipe_models: None,
            redact_secrets: None,
            position: None,
        };
        assert!(update_route(&conn, &route_id, &clear).unwrap());
        let r = get_route(&conn, &route_id).unwrap().unwrap();
        assert_eq!(r.guardrail_mode, None);
        assert_eq!(r.pxpipe_compress, None);
        // enabled was left unchanged (None) — still false.
        assert!(!r.enabled);
    }

    #[test]
    fn disabled_route_excluded_from_enabled_route_ids() {
        let conn = in_memory_db();
        let (route_id, _) = seed_route_with_providers(&conn, 1);
        // The seeded backend is "b0"; while enabled the route id is returned.
        assert_eq!(
            enabled_route_ids_for_backend_name(&conn, "b0").unwrap(),
            vec![route_id.clone()]
        );

        // Disable the route -> it drops out of the virtual-key scope query.
        let patch = RoutePatch {
            name: None,
            description: None,
            strategy: None,
            rpm: None,
            tpm: None,
            budget_usd: None,
            enabled: Some(false),
            guardrail_mode: None,
            pxpipe_compress: None,
            pxpipe_models: None,
            redact_secrets: None,
            position: None,
        };
        assert!(update_route(&conn, &route_id, &patch).unwrap());
        assert!(enabled_route_ids_for_backend_name(&conn, "b0")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn reorder_route_providers_rewrites_priorities() {
        let conn = in_memory_db();
        let (route_id, ids) = seed_route_with_providers(&conn, 3);

        // Reverse the order.
        let reversed: Vec<String> = ids.iter().rev().cloned().collect();
        let outcome = reorder_route_providers(&conn, &route_id, &reversed).unwrap();

        match outcome {
            ReorderOutcome::Ok(rows) => {
                let new_order: Vec<String> = rows.iter().map(|p| p.id.clone()).collect();
                assert_eq!(new_order, reversed);
                for (i, row) in rows.iter().enumerate() {
                    assert_eq!(row.priority, i as i32);
                }
            }
            ReorderOutcome::Mismatch => panic!("expected Ok"),
        }
    }

    #[test]
    fn reorder_route_providers_mismatch_rolls_back() {
        let conn = in_memory_db();
        let (route_id, ids) = seed_route_with_providers(&conn, 3);

        // Submit a subset — should be rejected as Mismatch, priorities unchanged.
        let partial: Vec<String> = ids.iter().take(2).cloned().collect();
        let outcome = reorder_route_providers(&conn, &route_id, &partial).unwrap();
        assert!(matches!(outcome, ReorderOutcome::Mismatch));

        // Priorities must be untouched.
        let rows = list_route_providers(&conn, &route_id).unwrap();
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(
                row.priority, i as i32,
                "priorities must be unchanged after Mismatch"
            );
        }
    }

    #[test]
    fn reorder_route_providers_rejects_duplicates_and_extras() {
        let conn = in_memory_db();
        let (route_id, ids) = seed_route_with_providers(&conn, 3);

        // Duplicate id.
        let dup = vec![ids[0].clone(), ids[0].clone(), ids[1].clone()];
        assert!(matches!(
            reorder_route_providers(&conn, &route_id, &dup).unwrap(),
            ReorderOutcome::Mismatch
        ));

        // Extra id that isn't a provider on this route.
        let mut extra = ids.clone();
        extra.push("bogus-id".into());
        assert!(matches!(
            reorder_route_providers(&conn, &route_id, &extra).unwrap(),
            ReorderOutcome::Mismatch
        ));
    }
}
