use super::common::chrono_now;
use rusqlite::{params, Connection};

/// A fully-hydrated backend row as stored in SQLite.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManagedBackendRow {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub project: Option<String>,
    pub region: Option<String>,
    pub aws_access_key_id: Option<String>,
    pub aws_secret_access_key: Option<String>,
    pub aws_session_token: Option<String>,
    pub rpm: Option<u32>,
    pub tpm: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
}

/// Patch struct for partial updates — all fields optional.
/// Note: Option<T> fields mean "update this field if Some, leave unchanged if None".
/// There is no way to clear a field back to NULL via this patch type.
/// UI should keep original value in form fields when editing, not attempt to clear.
#[derive(Debug, Default, serde::Deserialize)]
pub struct ManagedBackendPatch {
    pub provider_id: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub project: Option<String>,
    pub region: Option<String>,
    pub aws_access_key_id: Option<String>,
    pub aws_secret_access_key: Option<String>,
    pub aws_session_token: Option<String>,
    pub rpm: Option<u32>,
    pub tpm: Option<u64>,
}

/// Row returned by `list_model_deployments`.
pub struct ModelDeploymentRow {
    pub model_name: String,
    pub backend_name: String,
    pub actual_model: String,
    pub rpm_limit: Option<u32>,
    pub tpm_limit: Option<u64>,
    pub weight: u32,
}

/// Insert or ignore a model deployment (unique constraint prevents duplicates).
pub fn insert_model_deployment(
    conn: &Connection,
    model_name: &str,
    backend_name: &str,
    actual_model: &str,
    rpm: Option<u32>,
    tpm: Option<u64>,
    weight: u32,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO model_deployment
         (model_name, backend_name, actual_model, rpm_limit, tpm_limit, weight, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            model_name,
            backend_name,
            actual_model,
            rpm,
            tpm,
            weight,
            chrono_now()
        ],
    )?;
    Ok(())
}

/// Delete all deployments for a given model name. Returns the number of rows deleted.
pub fn delete_model_deployments(conn: &Connection, model_name: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM model_deployment WHERE model_name = ?1",
        [model_name],
    )
}

/// Return all persisted model deployments, ordered by model name.
pub fn list_model_deployments(conn: &Connection) -> rusqlite::Result<Vec<ModelDeploymentRow>> {
    let mut stmt = conn.prepare(
        "SELECT model_name, backend_name, actual_model, rpm_limit, tpm_limit, weight
         FROM model_deployment ORDER BY model_name, backend_name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ModelDeploymentRow {
            model_name: r.get(0)?,
            backend_name: r.get(1)?,
            actual_model: r.get(2)?,
            rpm_limit: r.get(3)?,
            tpm_limit: r.get(4)?,
            weight: r.get(5)?,
        })
    })?;
    rows.collect()
}

/// Insert a new managed backend. Returns an error if `name` already exists (UNIQUE constraint).
pub fn insert_managed_backend(conn: &Connection, row: &ManagedBackendRow) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO managed_backends
             (id, name, provider_id, api_key, api_base, deployment, api_version,
              project, region, aws_access_key_id, aws_secret_access_key, aws_session_token,
              rpm, tpm, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            row.id,
            row.name,
            row.provider_id,
            row.api_key,
            row.api_base,
            row.deployment,
            row.api_version,
            row.project,
            row.region,
            row.aws_access_key_id,
            row.aws_secret_access_key,
            row.aws_session_token,
            row.rpm,
            row.tpm,
            row.created_at,
            row.updated_at,
        ],
    )?;
    Ok(())
}

/// Return all managed backends ordered by name.
pub fn list_managed_backends(conn: &Connection) -> rusqlite::Result<Vec<ManagedBackendRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, provider_id, api_key, api_base, deployment, api_version,
                project, region, aws_access_key_id, aws_secret_access_key, aws_session_token,
                rpm, tpm, created_at, updated_at
         FROM managed_backends ORDER BY name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ManagedBackendRow {
            id: r.get(0)?,
            name: r.get(1)?,
            provider_id: r.get(2)?,
            api_key: r.get(3)?,
            api_base: r.get(4)?,
            deployment: r.get(5)?,
            api_version: r.get(6)?,
            project: r.get(7)?,
            region: r.get(8)?,
            aws_access_key_id: r.get(9)?,
            aws_secret_access_key: r.get(10)?,
            aws_session_token: r.get(11)?,
            rpm: r.get(12)?,
            tpm: r.get(13)?,
            created_at: r.get(14)?,
            updated_at: r.get(15)?,
        })
    })?;
    rows.collect()
}

/// Apply a partial update to a managed backend identified by `name`.
/// Returns `true` if a row was updated, `false` if no row matched.
/// Only non-None fields in `patch` are written; None fields are left as-is.
pub fn update_managed_backend(
    conn: &Connection,
    name: &str,
    patch: &ManagedBackendPatch,
) -> rusqlite::Result<bool> {
    let mut set_clauses: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    // Build SET clause dynamically from non-None patch fields.
    macro_rules! push_field {
        ($field:expr, $col:literal) => {
            if let Some(ref v) = $field {
                set_clauses.push(format!("{} = ?", $col));
                param_values.push(Box::new(v.clone()));
            }
        };
    }

    push_field!(patch.provider_id, "provider_id");
    push_field!(patch.api_key, "api_key");
    push_field!(patch.api_base, "api_base");
    push_field!(patch.deployment, "deployment");
    push_field!(patch.api_version, "api_version");
    push_field!(patch.project, "project");
    push_field!(patch.region, "region");
    push_field!(patch.aws_access_key_id, "aws_access_key_id");
    push_field!(patch.aws_secret_access_key, "aws_secret_access_key");
    push_field!(patch.aws_session_token, "aws_session_token");

    // Numeric fields need separate handling since they're Copy, not String.
    if let Some(v) = patch.rpm {
        set_clauses.push("rpm = ?".to_string());
        param_values.push(Box::new(v));
    }
    if let Some(v) = patch.tpm {
        set_clauses.push("tpm = ?".to_string());
        param_values.push(Box::new(v as i64));
    }

    if set_clauses.is_empty() {
        // Nothing to update; check existence and return accordingly.
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM managed_backends WHERE name = ?1",
            [name],
            |r| r.get(0),
        )?;
        return Ok(count > 0);
    }

    // Always bump updated_at.
    set_clauses.push("updated_at = ?".to_string());
    param_values.push(Box::new(chrono_now()));

    // Name is the final bound parameter for the WHERE clause.
    param_values.push(Box::new(name.to_string()));

    let sql = format!(
        "UPDATE managed_backends SET {} WHERE name = ?",
        set_clauses.join(", ")
    );

    let changed = conn.execute(&sql, rusqlite::params_from_iter(param_values.iter()))?;
    Ok(changed > 0)
}

/// Delete a managed backend by name.
/// Returns `true` if a row was deleted, `false` if no row matched.
pub fn delete_managed_backend(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    let deleted = conn.execute("DELETE FROM managed_backends WHERE name = ?1", [name])?;
    Ok(deleted > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn managed_backend_insert_and_list() {
        let conn = in_memory_db();

        let row = test_row("my-backend");
        insert_managed_backend(&conn, &row).unwrap();

        let rows = list_managed_backends(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "my-backend");
        assert_eq!(rows[0].provider_id, "openai");
        assert_eq!(rows[0].api_key.as_deref(), Some("sk-test"));
        assert_eq!(rows[0].rpm, Some(100));
        assert_eq!(rows[0].tpm, Some(10_000));
    }

    #[test]
    fn managed_backend_insert_duplicate_name_fails() {
        let conn = in_memory_db();

        let row = test_row("dup");
        insert_managed_backend(&conn, &row).unwrap();
        assert!(insert_managed_backend(&conn, &row).is_err());
    }

    #[test]
    fn managed_backend_update_returns_true_on_match() {
        let conn = in_memory_db();

        insert_managed_backend(&conn, &test_row("upd-backend")).unwrap();

        let patch = ManagedBackendPatch {
            provider_id: Some("anthropic".to_string()),
            rpm: Some(50),
            ..Default::default()
        };
        let updated = update_managed_backend(&conn, "upd-backend", &patch).unwrap();
        assert!(updated);

        let rows = list_managed_backends(&conn).unwrap();
        assert_eq!(rows[0].provider_id, "anthropic");
        assert_eq!(rows[0].rpm, Some(50));
        assert_eq!(rows[0].api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn managed_backend_update_returns_false_when_not_found() {
        let conn = in_memory_db();

        let patch = ManagedBackendPatch {
            provider_id: Some("gemini".to_string()),
            ..Default::default()
        };
        let updated = update_managed_backend(&conn, "nonexistent", &patch).unwrap();
        assert!(!updated);
    }

    #[test]
    fn managed_backend_update_empty_patch_is_noop() {
        let conn = in_memory_db();

        insert_managed_backend(&conn, &test_row("noop-backend")).unwrap();

        let patch = ManagedBackendPatch::default();
        let updated = update_managed_backend(&conn, "noop-backend", &patch).unwrap();
        assert!(updated);

        let rows = list_managed_backends(&conn).unwrap();
        assert_eq!(rows[0].provider_id, "openai");
    }

    #[test]
    fn managed_backend_delete_returns_true_on_match() {
        let conn = in_memory_db();

        insert_managed_backend(&conn, &test_row("del-backend")).unwrap();
        assert_eq!(list_managed_backends(&conn).unwrap().len(), 1);

        let deleted = delete_managed_backend(&conn, "del-backend").unwrap();
        assert!(deleted);
        assert!(list_managed_backends(&conn).unwrap().is_empty());
    }

    #[test]
    fn managed_backend_delete_returns_false_when_not_found() {
        let conn = in_memory_db();

        let deleted = delete_managed_backend(&conn, "ghost").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn managed_backend_list_ordered_by_name() {
        let conn = in_memory_db();

        insert_managed_backend(&conn, &test_row("zebra")).unwrap();
        insert_managed_backend(&conn, &test_row("alpha")).unwrap();
        insert_managed_backend(&conn, &test_row("middle")).unwrap();

        let rows = list_managed_backends(&conn).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["alpha", "middle", "zebra"]);
    }
}
