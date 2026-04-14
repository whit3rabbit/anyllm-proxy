// SQLite schema, migrations, queries, and write buffer for request logging.

use crate::admin::state::RequestLogEntry;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

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

/// Run an ALTER TABLE ADD COLUMN statement, ignoring "duplicate column" errors
/// so migrations are idempotent across restarts.
fn idempotent_add_column(conn: &Connection, stmt: &str) -> rusqlite::Result<()> {
    match conn.execute_batch(stmt) {
        Ok(()) => Ok(()),
        Err(e) if e.to_string().contains("duplicate column") => Ok(()),
        Err(e) => Err(e),
    }
}

/// Initialize the SQLite database: create tables and indexes.
pub fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    // WAL mode: better read concurrency (proxy reads while admin writes)
    // and crash recovery compared to the default rollback journal.
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    // Wait up to 5 seconds on a write lock before returning SQLITE_BUSY.
    // Without this, concurrent writers (log flush task, batch processor, webhook
    // dispatcher) fail immediately on write contention even with WAL mode.
    conn.execute_batch("PRAGMA busy_timeout = 5000;")?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS request_log (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id      TEXT NOT NULL,
            timestamp       TEXT NOT NULL,
            backend         TEXT NOT NULL,
            model_requested TEXT,
            model_mapped    TEXT,
            status_code     INTEGER NOT NULL,
            latency_ms      INTEGER NOT NULL,
            input_tokens    INTEGER,
            output_tokens   INTEGER,
            is_streaming    INTEGER NOT NULL DEFAULT 0,
            error_message   TEXT,
            error_kind      TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_request_log_timestamp ON request_log(timestamp);
        CREATE INDEX IF NOT EXISTS idx_request_log_backend ON request_log(backend);
        CREATE INDEX IF NOT EXISTS idx_request_log_ts_latency ON request_log(timestamp, latency_ms);

        CREATE TABLE IF NOT EXISTS config_override (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS virtual_api_key (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            key_hash        TEXT NOT NULL UNIQUE,
            key_prefix      TEXT NOT NULL,
            description     TEXT,
            created_at      TEXT NOT NULL,
            expires_at      TEXT,
            revoked_at      TEXT,
            spend_limit     REAL,
            rpm_limit       INTEGER,
            tpm_limit       INTEGER,
            total_spend     REAL NOT NULL DEFAULT 0,
            total_requests  INTEGER NOT NULL DEFAULT 0,
            total_tokens    INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_vak_hash ON virtual_api_key(key_hash);

        CREATE TABLE IF NOT EXISTS audit_log (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp   TEXT NOT NULL,
            action      TEXT NOT NULL,
            target_type TEXT NOT NULL,
            target_id   TEXT,
            detail      TEXT,
            source_ip   TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_audit_log_timestamp ON audit_log(timestamp);

        ",
    )?;

    // Schema migrations for virtual_api_key new columns (idempotent via IF NOT EXISTS).
    // SQLite 3.37+ supports ADD COLUMN IF NOT EXISTS.
    let migration_stmts = [
        "ALTER TABLE virtual_api_key ADD COLUMN role TEXT NOT NULL DEFAULT 'developer'",
        "ALTER TABLE virtual_api_key ADD COLUMN max_budget_usd REAL",
        "ALTER TABLE virtual_api_key ADD COLUMN budget_duration TEXT",
        "ALTER TABLE virtual_api_key ADD COLUMN period_start TEXT",
        "ALTER TABLE virtual_api_key ADD COLUMN period_spend_usd REAL NOT NULL DEFAULT 0.0",
        "ALTER TABLE virtual_api_key ADD COLUMN total_input_tokens INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE virtual_api_key ADD COLUMN total_output_tokens INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE virtual_api_key ADD COLUMN allowed_models TEXT",
    ];
    for stmt in &migration_stmts {
        idempotent_add_column(conn, stmt)?;
    }

    // request_log migrations: add key_id and cost_usd for request attribution.
    let request_log_migrations = [
        "ALTER TABLE request_log ADD COLUMN key_id INTEGER",
        "ALTER TABLE request_log ADD COLUMN cost_usd REAL",
        "ALTER TABLE request_log ADD COLUMN error_kind TEXT",
    ];
    for stmt in &request_log_migrations {
        idempotent_add_column(conn, stmt)?;
    }

    // Index on key_id for filtering requests by virtual key.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_request_log_key_id ON request_log(key_id);",
    )?;

    // health_checks table for backend uptime tracking.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS health_checks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            backend     TEXT    NOT NULL,
            checked_at  INTEGER NOT NULL,
            status      TEXT    NOT NULL,
            latency_ms  INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_health_checks_backend_time
            ON health_checks (backend, checked_at DESC);
        -- Separate checked_at index for the prune DELETE (no leading backend column).
        CREATE INDEX IF NOT EXISTS idx_health_checks_checked_at
            ON health_checks (checked_at);
        ",
    )?;

    // env_import: key-value pairs written by the admin UI import endpoint.
    // Read back at startup (before the async runtime) to apply as env vars.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS env_import (
            key         TEXT PRIMARY KEY,
            value       TEXT NOT NULL,
            imported_at TEXT NOT NULL
        );",
    )?;

    // model_deployment: models added via the admin API, persisted across restarts.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS model_deployment (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            model_name   TEXT NOT NULL,
            backend_name TEXT NOT NULL,
            actual_model TEXT NOT NULL,
            rpm_limit    INTEGER,
            tpm_limit    INTEGER,
            weight       INTEGER NOT NULL DEFAULT 1,
            created_at   TEXT NOT NULL,
            UNIQUE(model_name, backend_name, actual_model)
        );",
    )?;

    // managed_backends: admin-configured backend credentials, persisted across restarts.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS managed_backends (
            id                    TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
            name                  TEXT NOT NULL UNIQUE,
            provider_id           TEXT NOT NULL,
            api_key               TEXT,
            api_base              TEXT,
            deployment            TEXT,
            api_version           TEXT,
            project               TEXT,
            region                TEXT,
            aws_access_key_id     TEXT,
            aws_secret_access_key TEXT,
            aws_session_token     TEXT,
            rpm                   INTEGER,
            tpm                   INTEGER,
            created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            updated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
        );",
    )?;

    // provider_models_cache: live model lists fetched from provider APIs.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS provider_models_cache (
            provider_id   TEXT    NOT NULL,
            model_id      TEXT    NOT NULL,
            fetched_at    INTEGER NOT NULL,
            PRIMARY KEY (provider_id, model_id)
        );
        CREATE INDEX IF NOT EXISTS idx_provider_models_cache_provider
            ON provider_models_cache (provider_id);",
    )?;

    // routes: named endpoint groupings that map to one or more managed backends.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS routes (
            id          TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
            name        TEXT NOT NULL UNIQUE,
            description TEXT,
            strategy    TEXT NOT NULL DEFAULT 'failover',
            rpm         INTEGER,
            tpm         INTEGER,
            budget_usd  REAL,
            created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
        );

        CREATE TABLE IF NOT EXISTS route_providers (
            id          TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
            route_id    TEXT NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
            backend_id  TEXT NOT NULL REFERENCES managed_backends(id) ON DELETE CASCADE,
            models      TEXT NOT NULL DEFAULT '[\"*\"]',
            priority    INTEGER NOT NULL DEFAULT 0,
            enabled     INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            UNIQUE(route_id, backend_id)
        );
        CREATE INDEX IF NOT EXISTS idx_route_providers_route
            ON route_providers (route_id);
        ",
    )?;

    // Virtual key route scoping.
    idempotent_add_column(
        conn,
        "ALTER TABLE virtual_api_key ADD COLUMN allowed_routes TEXT",
    )?;

    Ok(())
}

/// Upsert a batch of env-file key-value pairs into the `env_import` table.
/// Existing keys are overwritten; `imported_at` is set to the current UTC timestamp.
/// All rows are written in a single transaction for atomicity and performance.
pub fn upsert_env_import(
    conn: &mut Connection,
    pairs: &[(String, String)],
) -> rusqlite::Result<()> {
    let now = chrono_now();
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO env_import (key, value, imported_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, imported_at = excluded.imported_at",
        )?;
        for (key, value) in pairs {
            stmt.execute(rusqlite::params![key, value, now])?;
        }
    }
    tx.commit()
}

/// Return all rows from `env_import` as (key, value) pairs.
/// Used during startup to apply persisted imports before the async runtime starts.
pub fn list_env_import(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT key, value FROM env_import ORDER BY key")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    rows.collect()
}

// ── Model deployment persistence ─────────────────────────────────────────────

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

// ── Managed backend persistence ──────────────────────────────────────────────

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

// ── Routes ───────────────────────────────────────────────────────────────────

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
        "INSERT INTO routes (id, name, description, strategy, rpm, tpm, budget_usd, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            row.id,
            row.name,
            row.description,
            row.strategy,
            row.rpm.map(|v| v as i64),
            row.tpm.map(|v| v as i64),
            row.budget_usd,
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
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub fn list_routes(conn: &Connection) -> rusqlite::Result<Vec<RouteRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, description, strategy, rpm, tpm, budget_usd, created_at, updated_at
         FROM routes ORDER BY name",
    )?;
    let rows = stmt.query_map([], row_to_route)?;
    rows.collect()
}

pub fn get_route(conn: &Connection, id: &str) -> rusqlite::Result<Option<RouteRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, description, strategy, rpm, tpm, budget_usd, created_at, updated_at
         FROM routes WHERE id = ?1",
    )?;
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

    if set_clauses.is_empty() {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM routes WHERE id = ?1", [id], |r| r.get(0))?;
        return Ok(count > 0);
    }

    set_clauses.push("updated_at = ?".into());
    param_values.push(Box::new(now_iso8601()));
    param_values.push(Box::new(id.to_string()));

    let sql = format!("UPDATE routes SET {} WHERE id = ?", set_clauses.join(", "));
    let params: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let updated = conn.execute(&sql, params.as_slice())?;
    Ok(updated > 0)
}

pub fn delete_route(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let deleted = conn.execute("DELETE FROM routes WHERE id = ?1", [id])?;
    Ok(deleted > 0)
}

// ── Route Providers ──────────────────────────────────────────────────────────

pub fn list_route_providers(conn: &Connection, route_id: &str) -> rusqlite::Result<Vec<RouteProviderRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, route_id, backend_id, models, priority, enabled, created_at
         FROM route_providers WHERE route_id = ?1 ORDER BY priority ASC",
    )?;
    let rows = stmt.query_map([route_id], |row| {
        let models_json: String = row.get(3)?;
        let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_else(|_| vec!["*".into()]);
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

pub fn count_route_providers(conn: &Connection, route_id: &str) -> rusqlite::Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM route_providers WHERE route_id = ?1",
        [route_id],
        |row| row.get(0),
    )?;
    Ok(n.max(0) as usize)
}

pub fn managed_backend_exists(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT 1 FROM managed_backends WHERE id = ?1",
        [id],
        |row| row.get(0),
    ).or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(0),
        other => Err(other),
    })?;
    Ok(n == 1)
}

pub fn add_route_provider(conn: &Connection, route_id: &str, backend_id: &str, models: &[String], priority: i32, enabled: bool) -> rusqlite::Result<()> {
    let models_json = serde_json::to_string(models).unwrap_or_else(|_| "[\"*\"]".into());
    conn.execute(
        "INSERT INTO route_providers (id, route_id, backend_id, models, priority, enabled)
         VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5)",
        params![route_id, backend_id, models_json, priority, enabled as i32],
    )?;
    Ok(())
}

pub fn update_route_provider(conn: &Connection, id: &str, models: Option<&[String]>, priority: Option<i32>, enabled: Option<bool>) -> rusqlite::Result<bool> {
    let mut set_clauses: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(m) = models {
        set_clauses.push("models = ?".into());
        param_values.push(Box::new(serde_json::to_string(m).unwrap_or_else(|_| "[\"*\"]".into())));
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
    let sql = format!("UPDATE route_providers SET {} WHERE id = ?", set_clauses.join(", "));
    let params: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
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
    // (see request-log flush); it relies on the surrounding Mutex to serialize
    // connection access rather than Rust's &mut borrow.
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

// ── Provider models cache ────────────────────────────────────────────────────

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

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

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

/// Ensure an HMAC secret exists in the settings table. Creates one if missing.
/// Returns the 32-byte secret used for HMAC-SHA256 key hashing.
/// The secret is generated from two UUID v4s (uuid is already a dep) to avoid
/// adding a CSPRNG dependency; the entropy is sufficient for HMAC keying.
pub fn ensure_hmac_secret(conn: &Connection) -> Vec<u8> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value BLOB NOT NULL);",
    )
    .expect("create settings table");

    let existing: Option<Vec<u8>> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'hmac_secret'",
            [],
            |row| row.get(0),
        )
        .ok();

    if let Some(secret) = existing {
        return secret;
    }

    // Generate 256-bit CSPRNG secret directly.
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).expect("CSPRNG failed");

    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('hmac_secret', ?1)",
        [&buf[..]],
    )
    .expect("insert hmac_secret");

    buf.to_vec()
}

/// Insert a single request log entry.
pub fn insert_request_log(conn: &Connection, entry: &RequestLogEntry) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO request_log (
            request_id, timestamp, backend, model_requested, model_mapped,
            status_code, latency_ms, input_tokens, output_tokens, is_streaming, error_message,
            error_kind, key_id, cost_usd
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            entry.request_id,
            entry.timestamp,
            entry.backend,
            entry.model_requested,
            entry.model_mapped,
            entry.status_code,
            entry.latency_ms,
            entry.input_tokens.map(|v| v as i64),
            entry.output_tokens.map(|v| v as i64),
            entry.is_streaming as i32,
            entry.error_message,
            entry.error_kind,
            entry.key_id,
            entry.cost_usd,
        ],
    )?;
    Ok(())
}

/// Query request log with optional filters and pagination.
/// Typed status code filter -- prevents SQL injection by construction.
/// Only valid patterns are representable; invalid input is rejected at parse time.
enum StatusFilter {
    Exact(u16),
    Class2xx,
    Class4xx,
    Class5xx,
}

impl StatusFilter {
    fn parse(s: &str) -> Option<Self> {
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

/// Map a SQLite row to a RequestLogEntry. Column order must match the SELECT
/// used in query_request_log and get_request_by_id.
fn row_to_request_log(row: &rusqlite::Row) -> rusqlite::Result<RequestLogEntry> {
    Ok(RequestLogEntry {
        request_id: row.get(0)?,
        timestamp: row.get(1)?,
        backend: row.get(2)?,
        model_requested: row.get(3)?,
        model_mapped: row.get(4)?,
        status_code: row.get::<_, i32>(5)? as u16,
        latency_ms: row.get::<_, i64>(6)? as u64,
        input_tokens: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        output_tokens: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
        is_streaming: row.get::<_, i32>(9)? != 0,
        error_message: row.get(10)?,
        error_kind: row.get(11)?,
        key_id: row.get(12)?,
        cost_usd: row.get(13)?,
    })
}

/// Get a single request log entry by request_id.
pub fn get_request_by_id(
    conn: &Connection,
    request_id: &str,
) -> rusqlite::Result<Option<RequestLogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT request_id, timestamp, backend, model_requested, model_mapped,
                status_code, latency_ms, input_tokens, output_tokens, is_streaming, error_message,
                error_kind, key_id, cost_usd
         FROM request_log WHERE request_id = ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![request_id], row_to_request_log)?;
    rows.next().transpose()
}

// -- Config overrides --

/// Get all config overrides from SQLite.
pub fn get_config_overrides(conn: &Connection) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut stmt =
        conn.prepare("SELECT key, value, updated_at FROM config_override ORDER BY key")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect()
}

/// Set a config override (upsert).
pub fn set_config_override(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    let now = chrono_now();
    conn.execute(
        "INSERT INTO config_override (key, value, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
        params![key, value, now],
    )?;
    Ok(())
}

/// Delete a config override.
pub fn delete_config_override(conn: &Connection, key: &str) -> rusqlite::Result<bool> {
    let changed = conn.execute("DELETE FROM config_override WHERE key = ?1", params![key])?;
    Ok(changed > 0)
}

/// Delete request log entries older than the given number of days.
pub fn purge_old_logs(conn: &Connection, retention_days: u32) -> rusqlite::Result<usize> {
    // SQLite datetime comparison: delete rows where timestamp < cutoff
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(retention_days as u64 * 86400);
    let cutoff_iso = epoch_to_iso8601(cutoff);
    let changed = conn.execute(
        "DELETE FROM request_log WHERE timestamp < ?1",
        params![cutoff_iso],
    )?;
    Ok(changed)
}

/// Count request log entries with a timestamp >= `since_epoch` (Unix seconds).
/// Used to compute requests-per-second for the metrics dashboard.
pub fn count_requests_since(conn: &Connection, since_epoch: u64) -> rusqlite::Result<u64> {
    let since_iso = epoch_to_iso8601(since_epoch);
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM request_log WHERE timestamp >= ?1",
        rusqlite::params![since_iso],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as u64)
}

/// One time-bucket in the request timeseries (1-minute granularity).
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct ObservabilityBucket {
    pub bucket_start: String,
    #[serde(rename = "requests")]
    pub requests_total: u64,
    #[serde(rename = "errors")]
    pub requests_error: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// A single request entry in the waterfall timeline view.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct ObservabilityTimelineItem {
    pub request_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub backend: String,
    pub model: Option<String>,
    pub status_code: u16,
    pub latency_ms: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub is_streaming: bool,
    pub key_id: Option<i64>,
    pub cost_usd: Option<f64>,
    pub error_message: Option<String>,
    pub error_kind: Option<String>,
}

/// An aggregated failure group for the failure-breakdown panel.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct ObservabilityFailureItem {
    pub error_kind: Option<String>,
    pub backend: String,
    pub model: Option<String>,
    pub status_code: u16,
    pub count: u64,
    pub latest_seen: String,
    pub avg_latency_ms: u64,
    pub summary: String,
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

    // Fetch at most 2000 rows before Rust-side aggregation. Aggregation in SQL would require
    // a stable normalized-key column; doing it here avoids a schema change. Groups whose rows
    // span the cutoff may have incomplete counts, which is acceptable for the dashboard view.
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
        // Compute once; both the display summary and the group key derive from the same line.
        let first_line = first_failure_line(error_message.as_deref());
        let summary = truncate_for_display(&first_line, 120);
        let normalized = normalize_failure_group_key_from_line(&first_line);
        // U+001F (Unit Separator) is used as a field delimiter because it cannot appear
        // in backend names, model names, or normalized error tokens.
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
            avg_latency_ms: if aggregate.count == 0 {
                0
            } else {
                aggregate.total_latency_ms / aggregate.count
            },
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

fn first_failure_line(message: Option<&str>) -> String {
    collapse_whitespace(
        message
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("Unknown failure"),
    )
}

fn collapse_whitespace(input: &str) -> String {
    let mut collapsed = String::with_capacity(input.len());
    let mut previous_was_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                collapsed.push(' ');
                previous_was_space = true;
            }
        } else {
            collapsed.push(ch);
            previous_was_space = false;
        }
    }
    collapsed.trim().to_string()
}

fn truncate_for_display(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let truncated: String = value.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{truncated}...")
}

fn normalize_failure_group_key_from_line(first_line: &str) -> String {
    let lowercase = first_line.to_ascii_lowercase();
    let tokens = lowercase
        .split_whitespace()
        .filter_map(|token| {
            let trimmed = token
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_');
            if trimmed.is_empty() {
                None
            } else {
                Some(normalize_failure_token(trimmed))
            }
        })
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        "<empty>".to_string()
    } else {
        tokens.join(" ")
    }
}

fn normalize_failure_token(token: &str) -> String {
    if looks_like_id(token) {
        "<id>".to_string()
    } else if looks_like_numberish(token) {
        "<num>".to_string()
    } else {
        token.to_string()
    }
}

fn looks_like_numberish(token: &str) -> bool {
    fn is_numericish(input: &str) -> bool {
        !input.is_empty()
            && input
                .chars()
                .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | ':' | '/' | '%'))
    }

    if is_numericish(token) {
        return true;
    }

    for suffix in ["ms", "s", "sec", "secs"] {
        if let Some(prefix) = token.strip_suffix(suffix) {
            return is_numericish(prefix);
        }
    }

    false
}

fn looks_like_id(token: &str) -> bool {
    let lowercase = token.to_ascii_lowercase();
    if [
        "req_",
        "msg_",
        "run_",
        "resp_",
        "call_",
        "toolu_",
        "chatcmpl-",
        "cmpl-",
    ]
    .iter()
    .any(|prefix| lowercase.starts_with(prefix))
    {
        return true;
    }

    let compact = lowercase.replace('-', "");
    if compact.len() >= 24 && compact.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return true;
    }

    // Single pass: check all three conditions simultaneously.
    lowercase.len() >= 16 && {
        let mut has_alpha = false;
        let mut has_digit = false;
        let all_valid = lowercase.chars().all(|ch| {
            if ch.is_ascii_alphabetic() {
                has_alpha = true;
            } else if ch.is_ascii_digit() {
                has_digit = true;
            }
            ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'
        });
        all_valid && has_alpha && has_digit
    }
}

/// Spawn the write buffer background task. Returns the sender for proxy handlers.
/// Flushes every 100ms or 100 rows, whichever comes first.
pub fn spawn_write_buffer(db: Arc<Mutex<Connection>>) -> mpsc::Sender<RequestLogEntry> {
    let (tx, mut rx) = mpsc::channel::<RequestLogEntry>(1024);

    tokio::spawn(async move {
        let mut buf: Vec<RequestLogEntry> = Vec::with_capacity(128);
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));

        loop {
            tokio::select! {
                maybe_entry = rx.recv() => {
                    match maybe_entry {
                        Some(entry) => {
                            buf.push(entry);
                            if buf.len() >= 100 {
                                flush_buffer(&db, &mut buf).await;
                            }
                        }
                        None => {
                            // Channel closed, flush remaining and exit.
                            if !buf.is_empty() {
                                flush_buffer(&db, &mut buf).await;
                            }
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    if !buf.is_empty() {
                        flush_buffer(&db, &mut buf).await;
                    }
                }
            }
        }
    });

    tx
}

async fn flush_buffer(db: &Arc<Mutex<Connection>>, buf: &mut Vec<RequestLogEntry>) {
    let entries = std::mem::take(buf);
    let db = db.clone();
    // Run SQLite IO on the blocking threadpool to avoid stalling the tokio executor.
    // On failure, return the entries so they can be re-queued for retry.
    let result = tokio::task::spawn_blocking(move || {
        // Mutex poisoning recovery: if a prior request panicked while holding the lock,
        // we recover the inner value rather than permanently locking the database.
        // This is safe because SQLite transactions provide ACID guarantees -- a panic
        // mid-transaction means the transaction was rolled back by SQLite.
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = (|| -> rusqlite::Result<()> {
            let tx = conn.unchecked_transaction()?;
            for entry in &entries {
                insert_request_log(&tx, entry)?;
            }
            tx.commit()?;
            Ok(())
        })() {
            tracing::error!(error = %e, count = entries.len(), "failed to flush request log buffer");
            Some(entries)
        } else {
            None
        }
    })
    .await;

    // On failure, re-queue entries so they can be retried on the next flush.
    if let Ok(Some(mut entries)) = result {
        buf.append(&mut entries);
        // Cap retry buffer to prevent unbounded growth on persistent DB failure.
        const MAX_RETRY_BUFFER: usize = 1000;
        if buf.len() > MAX_RETRY_BUFFER {
            let dropped = buf.len() - MAX_RETRY_BUFFER;
            buf.drain(..dropped);
            tracing::warn!(dropped, "dropped oldest log entries to cap retry buffer");
        }
    }
}

/// ISO 8601 UTC timestamp for "now".
fn chrono_now() -> String {
    // Use std only, no chrono dependency. Format: 2026-03-22T10:15:30Z
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    epoch_to_iso8601(dur.as_secs())
}

/// Convert unix epoch seconds to ISO 8601 string (UTC, second precision).
pub(crate) fn epoch_to_iso8601(epoch: u64) -> String {
    // Manual conversion without chrono.
    let secs = epoch;
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since 1970-01-01 to year/month/day.
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert unix epoch milliseconds to ISO 8601 string with millisecond precision.
/// Format: "2026-03-27T10:15:30.500Z"
pub(crate) fn epoch_to_iso8601_ms(epoch_ms: u64) -> String {
    let secs = epoch_ms / 1000;
    let ms = epoch_ms % 1000;
    let base = epoch_to_iso8601(secs);
    // epoch_to_iso8601 returns "YYYY-MM-DDTHH:MM:SSZ"; strip the Z, append .mmmZ
    let without_z = base.trim_end_matches('Z');
    format!("{}.{:03}Z", without_z, ms)
}

/// Convert days since 1970-01-01 to (year, month, day).
pub(crate) fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Get the current time as ISO 8601 UTC string.
pub fn now_iso8601() -> String {
    chrono_now()
}

// --- Virtual API Key CRUD ---

use super::keys::VirtualKeyRow;

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

// --- Audit Log ---

/// A single audit log entry recording an admin mutation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub action: String,
    pub target_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ip: Option<String>,
}

/// Insert an audit log entry with current UTC timestamp.
pub fn insert_audit_entry(conn: &Connection, entry: &AuditEntry) -> rusqlite::Result<()> {
    let ts = chrono_now();
    conn.execute(
        "INSERT INTO audit_log (timestamp, action, target_type, target_id, detail, source_ip)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            ts,
            entry.action,
            entry.target_type,
            entry.target_id,
            entry.detail,
            entry.source_ip,
        ],
    )?;
    Ok(())
}

/// Query the audit log, returning entries in reverse chronological order.
pub fn query_audit_log(
    conn: &Connection,
    limit: u32,
    offset: u32,
    action: Option<&str>,
    target_type: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
) -> rusqlite::Result<Vec<AuditEntry>> {
    let mut sql = String::from(
        "SELECT id, timestamp, action, target_type, target_id, detail, source_ip
         FROM audit_log WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(a) = action {
        sql.push_str(" AND action = ?");
        param_values.push(Box::new(a.to_string()));
    }
    if let Some(t) = target_type {
        sql.push_str(" AND target_type = ?");
        param_values.push(Box::new(t.to_string()));
    }
    if let Some(s) = since {
        sql.push_str(" AND timestamp >= ?");
        param_values.push(Box::new(s.to_string()));
    }
    if let Some(u) = until {
        sql.push_str(" AND timestamp <= ?");
        param_values.push(Box::new(u.to_string()));
    }
    sql.push_str(" ORDER BY id DESC LIMIT ? OFFSET ?");
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(AuditEntry {
            id: Some(row.get(0)?),
            timestamp: Some(row.get(1)?),
            action: row.get(2)?,
            target_type: row.get(3)?,
            target_id: row.get(4)?,
            detail: row.get(5)?,
            source_ip: row.get(6)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn sample_entry() -> RequestLogEntry {
        RequestLogEntry {
            request_id: "test-123".into(),
            timestamp: "2099-01-01T00:00:00Z".into(),
            backend: "openai".into(),
            model_requested: Some("claude-sonnet-4-6".into()),
            model_mapped: Some("gpt-4o".into()),
            status_code: 200,
            latency_ms: 342,
            input_tokens: Some(150),
            output_tokens: Some(87),
            is_streaming: false,
            error_message: None,
            error_kind: None,
            key_id: None,
            cost_usd: None,
        }
    }

    #[test]
    fn init_db_creates_tables() {
        let conn = in_memory_db();
        // Verify tables exist by querying them.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM request_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM config_override", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn insert_and_query_request_log() {
        let conn = in_memory_db();
        let entry = sample_entry();
        insert_request_log(&conn, &entry).unwrap();

        let results = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].request_id, "test-123");
        assert_eq!(results[0].status_code, 200);
        assert_eq!(results[0].latency_ms, 342);
        assert_eq!(results[0].input_tokens, Some(150));
    }

    #[test]
    fn query_with_backend_filter() {
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap();

        let mut entry2 = sample_entry();
        entry2.request_id = "test-456".into();
        entry2.backend = "gemini".into();
        insert_request_log(&conn, &entry2).unwrap();

        let results =
            query_request_log(&conn, 10, 0, Some("gemini"), None, None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].backend, "gemini");
    }

    #[test]
    fn query_with_status_filter() {
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap();

        let mut err_entry = sample_entry();
        err_entry.request_id = "test-err".into();
        err_entry.status_code = 500;
        insert_request_log(&conn, &err_entry).unwrap();

        let results = query_request_log(&conn, 10, 0, None, None, None, Some("5xx"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status_code, 500);

        let results = query_request_log(&conn, 10, 0, None, None, None, Some("2xx"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status_code, 200);
    }

    #[test]
    fn query_pagination() {
        let conn = in_memory_db();
        for i in 0..5 {
            let mut entry = sample_entry();
            entry.request_id = format!("test-{i}");
            insert_request_log(&conn, &entry).unwrap();
        }

        let page1 = query_request_log(&conn, 2, 0, None, None, None, None, None).unwrap();
        assert_eq!(page1.len(), 2);

        let page2 = query_request_log(&conn, 2, 2, None, None, None, None, None).unwrap();
        assert_eq!(page2.len(), 2);

        let page3 = query_request_log(&conn, 2, 4, None, None, None, None, None).unwrap();
        assert_eq!(page3.len(), 1);
    }

    #[test]
    fn get_request_by_id_found() {
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap();

        let result = get_request_by_id(&conn, "test-123").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().request_id, "test-123");
    }

    #[test]
    fn get_request_by_id_not_found() {
        let conn = in_memory_db();
        let result = get_request_by_id(&conn, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn config_override_crud() {
        let conn = in_memory_db();

        // Set
        set_config_override(&conn, "log_level", "debug").unwrap();
        let overrides = get_config_overrides(&conn).unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].0, "log_level");
        assert_eq!(overrides[0].1, "debug");

        // Update (upsert)
        set_config_override(&conn, "log_level", "trace").unwrap();
        let overrides = get_config_overrides(&conn).unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].1, "trace");

        // Delete
        let deleted = delete_config_override(&conn, "log_level").unwrap();
        assert!(deleted);
        let overrides = get_config_overrides(&conn).unwrap();
        assert!(overrides.is_empty());

        // Delete non-existent
        let deleted = delete_config_override(&conn, "nonexistent").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn purge_old_logs_removes_old_entries() {
        let conn = in_memory_db();

        // Insert an old entry (timestamp in 2020).
        let mut old = sample_entry();
        old.timestamp = "2020-01-01T00:00:00Z".into();
        insert_request_log(&conn, &old).unwrap();

        // Insert a recent entry.
        insert_request_log(&conn, &sample_entry()).unwrap();

        let purged = purge_old_logs(&conn, 1).unwrap();
        assert_eq!(purged, 1);

        let remaining = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].request_id, "test-123");
    }

    #[test]
    fn epoch_to_iso8601_known_value() {
        // 2026-03-22T00:00:00Z = 1774070400 (approximate)
        let result = epoch_to_iso8601(0);
        assert_eq!(result, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn epoch_to_iso8601_ms_formats_fractional_seconds() {
        assert_eq!(epoch_to_iso8601_ms(500), "1970-01-01T00:00:00.500Z");
        assert_eq!(epoch_to_iso8601_ms(1000), "1970-01-01T00:00:01.000Z");
        assert_eq!(epoch_to_iso8601_ms(1001), "1970-01-01T00:00:01.001Z");
        let result = epoch_to_iso8601_ms(1774070400000);
        assert!(result.ends_with(".000Z"), "got: {result}");
    }

    #[test]
    fn init_db_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        // Running again should not error.
        init_db(&conn).unwrap();
    }

    #[test]
    fn insert_and_query_with_key_id_and_cost() {
        let conn = in_memory_db();
        let mut entry = sample_entry();
        entry.key_id = Some(42);
        entry.cost_usd = Some(0.0075);
        insert_request_log(&conn, &entry).unwrap();

        // Query without key_id filter returns all.
        let results = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key_id, Some(42));
        assert!((results[0].cost_usd.unwrap() - 0.0075).abs() < 1e-12);

        // Query with matching key_id filter.
        let results = query_request_log(&conn, 10, 0, None, None, None, None, Some(42)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].request_id, "test-123");

        // Query with non-matching key_id filter.
        let results = query_request_log(&conn, 10, 0, None, None, None, None, Some(99)).unwrap();
        assert!(results.is_empty());

        // get_request_by_id also returns the new fields.
        let found = get_request_by_id(&conn, "test-123").unwrap().unwrap();
        assert_eq!(found.key_id, Some(42));
        assert!((found.cost_usd.unwrap() - 0.0075).abs() < 1e-12);
    }

    #[test]
    fn insert_without_attribution_fields() {
        // Entries without key_id/cost_usd should still work (NULL columns).
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap();

        let results = query_request_log(&conn, 10, 0, None, None, None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key_id, None);
        assert_eq!(results[0].cost_usd, None);
    }

    #[test]
    fn audit_log_insert_and_query() {
        let conn = in_memory_db();
        let entry1 = AuditEntry {
            id: None,
            timestamp: None,
            action: "key_created".into(),
            target_type: "virtual_key".into(),
            target_id: Some("42".into()),
            detail: Some("description=test key, prefix=sk-vk-abc".into()),
            source_ip: Some("127.0.0.1".into()),
        };
        let entry2 = AuditEntry {
            id: None,
            timestamp: None,
            action: "key_revoked".into(),
            target_type: "virtual_key".into(),
            target_id: Some("42".into()),
            detail: None,
            source_ip: None,
        };
        insert_audit_entry(&conn, &entry1).unwrap();
        insert_audit_entry(&conn, &entry2).unwrap();

        let results = query_audit_log(&conn, 50, 0, None, None, None, None).unwrap();
        assert_eq!(results.len(), 2);
        // Reverse chronological: most recent first.
        assert_eq!(results[0].action, "key_revoked");
        assert_eq!(results[1].action, "key_created");
        assert!(results[0].id.unwrap() > results[1].id.unwrap());
        // Timestamps are filled in by the insert function.
        assert!(results[0].timestamp.is_some());
        assert_eq!(results[1].target_id.as_deref(), Some("42"));
        assert_eq!(
            results[1].detail.as_deref(),
            Some("description=test key, prefix=sk-vk-abc")
        );
        assert_eq!(results[1].source_ip.as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn audit_log_empty_returns_empty_vec() {
        let conn = in_memory_db();
        let results = query_audit_log(&conn, 50, 0, None, None, None, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn audit_log_pagination() {
        let conn = in_memory_db();
        for i in 0..5 {
            insert_audit_entry(
                &conn,
                &AuditEntry {
                    id: None,
                    timestamp: None,
                    action: format!("action_{i}"),
                    target_type: "test".into(),
                    target_id: None,
                    detail: None,
                    source_ip: None,
                },
            )
            .unwrap();
        }
        let page1 = query_audit_log(&conn, 2, 0, None, None, None, None).unwrap();
        assert_eq!(page1.len(), 2);
        let page2 = query_audit_log(&conn, 2, 2, None, None, None, None).unwrap();
        assert_eq!(page2.len(), 2);
        let page3 = query_audit_log(&conn, 2, 4, None, None, None, None).unwrap();
        assert_eq!(page3.len(), 1);
    }

    #[test]
    fn status_filter_parses_valid_inputs() {
        assert!(StatusFilter::parse("200").is_some());
        assert!(StatusFilter::parse("2xx").is_some());
        assert!(StatusFilter::parse("4xx").is_some());
        assert!(StatusFilter::parse("5xx").is_some());
        assert!(StatusFilter::parse("404").is_some());
    }

    #[test]
    fn status_filter_rejects_invalid_inputs() {
        assert!(StatusFilter::parse("abc").is_none());
        assert!(StatusFilter::parse("2xx; DROP TABLE").is_none());
        assert!(StatusFilter::parse("").is_none());
        assert!(StatusFilter::parse("99999").is_none()); // overflows u16
        assert!(StatusFilter::parse("-1").is_none());
    }

    #[test]
    fn status_filter_exact_code_query() {
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap(); // status 200

        let mut err_entry = sample_entry();
        err_entry.request_id = "test-404".into();
        err_entry.status_code = 404;
        insert_request_log(&conn, &err_entry).unwrap();

        // Exact code filter should match only the 404 entry.
        let results = query_request_log(&conn, 10, 0, None, None, None, Some("404"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status_code, 404);
    }

    #[test]
    fn status_filter_invalid_ignored() {
        let conn = in_memory_db();
        insert_request_log(&conn, &sample_entry()).unwrap();

        // Invalid filter should be silently ignored, returning all rows.
        let results =
            query_request_log(&conn, 10, 0, None, None, None, Some("garbage"), None).unwrap();
        assert_eq!(results.len(), 1);
    }

    // --- update_virtual_key tests ---

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
        // row.allowed_models is parsed from JSON into Vec<String>.
        assert_eq!(
            row.allowed_models,
            Some(vec!["gpt-4o".to_string(), "claude-*".to_string()])
        );
    }

    #[test]
    fn update_virtual_key_budget_duration_resets_period() {
        let conn = in_memory_db();
        // Insert with a non-null period_spend_usd to verify reset.
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

        // period_spend_usd should be reset to 0, period_start to NULL.
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

    // --- query_audit_log filter tests ---

    fn insert_audit(conn: &Connection, action: &str, target_type: &str, ts: &str) {
        conn.execute(
            "INSERT INTO audit_log (timestamp, action, source_ip, target_type, target_id, detail) \
             VALUES (?1, ?2, '127.0.0.1', ?3, NULL, NULL)",
            params![ts, action, target_type],
        )
        .unwrap();
    }

    #[test]
    fn audit_filter_by_action() {
        let conn = in_memory_db();
        insert_audit(&conn, "key_created", "virtual_key", "2099-01-01T00:00:00Z");
        insert_audit(&conn, "key_revoked", "virtual_key", "2099-01-02T00:00:00Z");

        let results = query_audit_log(&conn, 10, 0, Some("key_created"), None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "key_created");
    }

    #[test]
    fn audit_filter_by_target_type() {
        let conn = in_memory_db();
        insert_audit(&conn, "key_created", "virtual_key", "2099-01-01T00:00:00Z");
        insert_audit(&conn, "config_changed", "config", "2099-01-02T00:00:00Z");

        let results = query_audit_log(&conn, 10, 0, None, Some("config"), None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].target_type, "config");
    }

    #[test]
    fn audit_filter_since_until() {
        let conn = in_memory_db();
        insert_audit(&conn, "key_created", "virtual_key", "2099-01-01T00:00:00Z");
        insert_audit(&conn, "key_revoked", "virtual_key", "2099-01-03T00:00:00Z");
        insert_audit(&conn, "key_updated", "virtual_key", "2099-01-05T00:00:00Z");

        // since + until window should return only the middle entry.
        let results = query_audit_log(
            &conn,
            10,
            0,
            None,
            None,
            Some("2099-01-02T00:00:00Z"),
            Some("2099-01-04T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "key_revoked");
    }

    #[test]
    fn count_requests_since_returns_zero_on_empty_log() {
        let conn = in_memory_db();
        let count = count_requests_since(&conn, 0).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn count_requests_since_counts_recent_entries() {
        let conn = in_memory_db();

        // Insert a recent entry (sample_entry uses current time).
        let recent = sample_entry();
        insert_request_log(&conn, &recent).unwrap();

        // Insert an old entry.
        let mut old = sample_entry();
        old.request_id = "old-req".to_string();
        old.timestamp = "2020-01-01T00:00:00Z".to_string();
        insert_request_log(&conn, &old).unwrap();

        // Count since 2025-01-01 should include only the recent entry.
        let since_2025: u64 = 1735689600; // 2025-01-01T00:00:00Z
        let count = count_requests_since(&conn, since_2025).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn observability_timeseries_groups_requests_into_buckets() {
        let conn = in_memory_db();

        let mut first = sample_entry();
        first.timestamp = "2099-01-01T00:00:05Z".into();
        first.input_tokens = Some(100);
        first.output_tokens = Some(25);
        first.cost_usd = Some(0.12);
        insert_request_log(&conn, &first).unwrap();

        let mut second = sample_entry();
        second.request_id = "test-456".into();
        second.timestamp = "2099-01-01T00:00:40Z".into();
        second.status_code = 503;
        second.input_tokens = Some(20);
        second.output_tokens = Some(4);
        second.cost_usd = Some(0.03);
        second.error_message = Some("upstream timeout".into());
        insert_request_log(&conn, &second).unwrap();

        let mut third = sample_entry();
        third.request_id = "test-789".into();
        third.timestamp = "2099-01-01T00:01:10Z".into();
        third.input_tokens = Some(7);
        third.output_tokens = Some(9);
        third.cost_usd = Some(0.01);
        insert_request_log(&conn, &third).unwrap();

        let buckets = query_request_timeseries(
            &conn,
            "2099-01-01T00:00:00Z",
            Some("2099-01-01T00:10:00Z"),
            None,
            None,
        )
        .unwrap();

        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].bucket_start, "2099-01-01T00:00:00Z");
        assert_eq!(buckets[0].requests_total, 2);
        assert_eq!(buckets[0].requests_error, 1);
        assert_eq!(buckets[0].input_tokens, 120);
        assert_eq!(buckets[0].output_tokens, 29);
        assert!((buckets[0].cost_usd - 0.15).abs() < 0.000001);
        assert_eq!(buckets[1].bucket_start, "2099-01-01T00:01:00Z");
        assert_eq!(buckets[1].requests_total, 1);
    }

    #[test]
    fn observability_timeline_derives_request_start_time() {
        let conn = in_memory_db();

        let mut entry = sample_entry();
        entry.timestamp = "2099-01-01T00:00:10Z".into();
        entry.latency_ms = 1_500;
        insert_request_log(&conn, &entry).unwrap();

        let items = query_request_timeline(
            &conn,
            "2099-01-01T00:00:00Z",
            Some("2099-01-01T00:05:00Z"),
            None,
            None,
            10,
        )
        .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].finished_at, "2099-01-01T00:00:10.000Z");
        assert_eq!(items[0].started_at, "2099-01-01T00:00:08.500Z");
    }

    #[test]
    fn observability_failure_breakdown_groups_similar_failures() {
        let conn = in_memory_db();

        let mut first = sample_entry();
        first.request_id = "test-fail-1".into();
        first.timestamp = "2099-01-01T00:00:10Z".into();
        first.status_code = 429;
        first.latency_ms = 500;
        first.error_message = Some("Upstream request req_abc123 throttled after 30s".into());
        first.error_kind = Some("rate_limit".into());
        insert_request_log(&conn, &first).unwrap();

        let mut second = sample_entry();
        second.request_id = "test-fail-2".into();
        second.timestamp = "2099-01-01T00:00:20Z".into();
        second.status_code = 429;
        second.latency_ms = 700;
        second.error_message = Some("Upstream request req_xyz789 throttled after 45s".into());
        second.error_kind = Some("rate_limit".into());
        insert_request_log(&conn, &second).unwrap();

        let mut third = sample_entry();
        third.request_id = "test-fail-3".into();
        third.timestamp = "2099-01-01T00:00:30Z".into();
        third.status_code = 500;
        third.error_message = Some("Backend crashed".into());
        third.error_kind = Some("upstream".into());
        insert_request_log(&conn, &third).unwrap();

        let mut fourth = sample_entry();
        fourth.request_id = "test-fail-4".into();
        fourth.timestamp = "2099-01-01T00:00:40Z".into();
        fourth.status_code = 429;
        fourth.error_message = Some("Upstream request req_qwe999 throttled after 60s".into());
        fourth.error_kind = Some("timeout".into());
        insert_request_log(&conn, &fourth).unwrap();

        let failures = query_failure_breakdown(
            &conn,
            "2099-01-01T00:00:00Z",
            Some("2099-01-01T01:00:00Z"),
            None,
            None,
            10,
        )
        .unwrap();

        assert_eq!(failures.len(), 3);
        assert_eq!(failures[0].error_kind.as_deref(), Some("rate_limit"));
        assert_eq!(failures[0].status_code, 429);
        assert_eq!(failures[0].count, 2);
        assert_eq!(failures[0].avg_latency_ms, 600);
        assert!(failures[0].summary.starts_with("Upstream request"));
    }

    #[test]
    fn init_db_sets_busy_timeout() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, 5000, "busy_timeout must be 5000ms");
    }

    // ── managed_backends CRUD tests ───────────────────────────────────────────

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
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

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
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        let row = test_row("dup");
        insert_managed_backend(&conn, &row).unwrap();
        // Second insert with same name must fail (UNIQUE constraint on name).
        assert!(insert_managed_backend(&conn, &row).is_err());
    }

    #[test]
    fn managed_backend_update_returns_true_on_match() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        insert_managed_backend(&conn, &test_row("upd-backend")).unwrap();

        let patch = crate::admin::routes::managed_backends::ManagedBackendPatch {
            provider_id: Some("anthropic".to_string()),
            rpm: Some(50),
            ..Default::default()
        };
        let updated = update_managed_backend(&conn, "upd-backend", &patch).unwrap();
        assert!(updated);

        let rows = list_managed_backends(&conn).unwrap();
        assert_eq!(rows[0].provider_id, "anthropic");
        assert_eq!(rows[0].rpm, Some(50));
        // Unchanged field preserved.
        assert_eq!(rows[0].api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn managed_backend_update_returns_false_when_not_found() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        let patch = crate::admin::routes::managed_backends::ManagedBackendPatch {
            provider_id: Some("gemini".to_string()),
            ..Default::default()
        };
        let updated = update_managed_backend(&conn, "nonexistent", &patch).unwrap();
        assert!(!updated);
    }

    #[test]
    fn managed_backend_update_empty_patch_is_noop() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        insert_managed_backend(&conn, &test_row("noop-backend")).unwrap();

        // Empty patch: all None fields, should not modify the row.
        let patch = crate::admin::routes::managed_backends::ManagedBackendPatch::default();
        let updated = update_managed_backend(&conn, "noop-backend", &patch).unwrap();
        // Row exists so returns true even though nothing changed.
        assert!(updated);

        let rows = list_managed_backends(&conn).unwrap();
        assert_eq!(rows[0].provider_id, "openai");
    }

    #[test]
    fn managed_backend_delete_returns_true_on_match() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        insert_managed_backend(&conn, &test_row("del-backend")).unwrap();
        assert_eq!(list_managed_backends(&conn).unwrap().len(), 1);

        let deleted = delete_managed_backend(&conn, "del-backend").unwrap();
        assert!(deleted);
        assert!(list_managed_backends(&conn).unwrap().is_empty());
    }

    #[test]
    fn managed_backend_delete_returns_false_when_not_found() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        let deleted = delete_managed_backend(&conn, "ghost").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn managed_backend_list_ordered_by_name() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        // Insert out of order.
        insert_managed_backend(&conn, &test_row("zebra")).unwrap();
        insert_managed_backend(&conn, &test_row("alpha")).unwrap();
        insert_managed_backend(&conn, &test_row("middle")).unwrap();

        let rows = list_managed_backends(&conn).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["alpha", "middle", "zebra"]);
    }

    // ── reorder_route_providers tests ────────────────────────────────────────

    fn seed_route_with_providers(conn: &Connection, count: usize) -> (String, Vec<String>) {
        let route = RouteRow {
            id: uuid::Uuid::new_v4().to_string(),
            name: "r".into(),
            description: None,
            strategy: "failover".into(),
            rpm: None,
            tpm: None,
            budget_usd: None,
            created_at: now_iso8601(),
            updated_at: now_iso8601(),
        };
        insert_route(conn, &route).unwrap();

        for i in 0..count {
            let mut b = test_row(&format!("b{i}"));
            b.id = format!("backend-{i}");
            insert_managed_backend(conn, &b).unwrap();
            add_route_provider(
                conn,
                &route.id,
                &b.id,
                &["*".to_string()],
                i as i32,
                true,
            )
            .unwrap();
        }

        let ids: Vec<String> = list_route_providers(conn, &route.id)
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        (route.id, ids)
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
            assert_eq!(row.priority, i as i32, "priorities must be unchanged after Mismatch");
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
