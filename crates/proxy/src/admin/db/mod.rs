//! SQLite database persistence and schema migrations.
//!
//! Provides modular storage tables for request logs, configuration overrides,
//! virtual API keys, audit logs, provider favorites, and route definitions.

use rusqlite::Connection;

/// Audit log table queries.
pub mod audit;
/// Managed backend credentials queries.
pub mod backends;
/// Shared database utilities.
pub mod common;
/// Configuration override queries.
pub mod config;
/// Provider favorites queries.
pub mod favorites;
/// Backend health check history queries.
pub mod health;
/// Virtual API key and quota tracking queries.
pub mod keys;
/// Request log and metrics summary queries.
pub mod logs;
/// Route definition and provider mappings queries.
pub mod routes;

pub use audit::*;
pub use backends::*;
pub use common::*;
pub use config::*;
pub use favorites::*;
pub use health::*;
pub use keys::*;
pub use logs::*;
pub use routes::*;

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

    // provider_favorites: admin-starred providers, shown in a Favorites row in the UI.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS provider_favorites (
            provider_id TEXT PRIMARY KEY,
            created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
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

    // Route-level dispatch controls: on/off toggle + per-route option overrides.
    // NULL option columns mean "inherit the global RuntimeConfig value".
    let route_migrations = [
        "ALTER TABLE routes ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
        "ALTER TABLE routes ADD COLUMN guardrail_mode TEXT",
        "ALTER TABLE routes ADD COLUMN pxpipe_compress INTEGER",
        "ALTER TABLE routes ADD COLUMN pxpipe_models TEXT",
        "ALTER TABLE routes ADD COLUMN redact_secrets INTEGER",
        // Explicit cross-route ordering when a model matches multiple routes
        // (lower wins). Default 0 preserves the exact-over-wildcard, then name tiebreak.
        "ALTER TABLE routes ADD COLUMN position INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE managed_backends ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
    ];
    for stmt in &route_migrations {
        idempotent_add_column(conn, stmt)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_db_creates_tables() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

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
    fn init_db_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        init_db(&conn).unwrap();
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
}
