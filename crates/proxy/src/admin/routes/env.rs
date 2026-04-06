//! Admin endpoints for importing and exporting `.anyllm.env` files.
//!
//! - `POST /admin/api/env/import` — multipart upload; parses, validates, writes to `env_import` table.
//!   Changes take effect on the next proxy restart (startup reads from `env_import`).
//! - `GET /admin/api/env/export` — downloads the current effective env as a `.anyllm.env` file (values unmasked).
//!
//! Neither endpoint modifies the live process environment; they only read/write SQLite.

use crate::admin::state::SharedState;
use crate::env_parser::{escape_for_env_file, parse_env_content, EnvWarning, KNOWN_KEYS};
use axum::{
    extract::{ConnectInfo, Multipart, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use std::net::SocketAddr;

/// Maximum accepted upload size for an env file. Env files are tiny;
/// anything larger is almost certainly the wrong file type.
/// The router already enforces a 1 MB body limit; this is a tighter application-level check.
const MAX_UPLOAD_BYTES: usize = 64 * 1024; // 64 KB

/// Successful import response body.
#[derive(Serialize)]
pub struct ImportResponse {
    /// Number of key-value pairs written to the database.
    pub applied: usize,
    /// Non-fatal issues (unknown keys, overwritten sensitive keys, etc.).
    pub warnings: Vec<EnvWarning>,
}

/// Failed import response body (returned with HTTP 422).
#[derive(Serialize)]
pub struct ImportErrorResponse {
    /// Reasons the import was rejected. Nothing was written to the database.
    pub hard_errors: Vec<String>,
    /// Non-fatal issues found before the hard error.
    pub warnings: Vec<EnvWarning>,
}

/// POST /admin/api/env/import
///
/// Accepts a multipart upload with a `file` field containing a `.anyllm.env`-format
/// file. Parses the content, applies strict validation, and on success writes the
/// key-value pairs to the `env_import` SQLite table. Changes take effect on the
/// next proxy restart (the startup bootstrap reads from this table).
///
/// Returns 200 + `{applied, warnings}` on success.
/// Returns 422 + `{hard_errors, warnings}` on validation failure (nothing written to DB).
/// Returns 413 if the upload exceeds 64 KB.
pub(super) async fn import_env(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    mut multipart: Multipart,
) -> axum::response::Response {
    // Extract the first field from the multipart body (any field name accepted).
    let raw_bytes = match multipart.next_field().await {
        Ok(Some(field)) => match field.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "hard_errors": [format!("failed to read upload: {e}")],
                        "warnings": []
                    })),
                )
                    .into_response();
            }
        },
        Ok(None) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({
                    "hard_errors": ["no file field found in upload"],
                    "warnings": []
                })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "hard_errors": [format!("multipart error: {e}")],
                    "warnings": []
                })),
            )
                .into_response();
        }
    };

    // Hard reject: file too large.
    if raw_bytes.len() > MAX_UPLOAD_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({
                "hard_errors": [format!(
                    "file is {} bytes; limit is {} KB",
                    raw_bytes.len(),
                    MAX_UPLOAD_BYTES / 1024
                )],
                "warnings": []
            })),
        )
            .into_response();
    }

    // Hard reject: must be valid UTF-8 text.
    let content = match std::str::from_utf8(&raw_bytes) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ImportErrorResponse {
                    hard_errors: vec!["file is not valid UTF-8 (binary content?)".to_string()],
                    warnings: vec![],
                }),
            )
                .into_response();
        }
    };

    // parse_env_content handles null-byte detection and all validation internally.
    let result = parse_env_content(content);

    if !result.hard_errors.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ImportErrorResponse {
                hard_errors: result.hard_errors,
                warnings: result.warnings,
            }),
        )
            .into_response();
    }

    let pairs: Vec<(String, String)> = result
        .pairs
        .iter()
        .map(|p| (p.key.clone(), p.value.clone()))
        .collect();
    let applied = pairs.len();

    // Write to SQLite env_import table.
    let db = shared.db.clone();
    let write_result = tokio::task::spawn_blocking(move || {
        let mut conn = db.lock().unwrap_or_else(|e| e.into_inner());
        crate::admin::db::upsert_env_import(&mut conn, &pairs)
    })
    .await;

    match write_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::error!("env import: db write failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "hard_errors": ["database write failed"],
                    "warnings": []
                })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("env import: spawn_blocking panicked: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    super::emit_audit(
        &shared,
        crate::admin::db::AuditEntry {
            id: None,
            timestamp: None,
            action: "env_import".to_string(),
            target_type: "env_import".to_string(),
            target_id: None,
            detail: Some(format!("{applied} vars imported")),
            source_ip: Some(addr.ip().to_string()),
        },
    );

    (
        StatusCode::OK,
        Json(ImportResponse {
            applied,
            warnings: result.warnings,
        }),
    )
        .into_response()
}

/// Section definitions for .anyllm.env export output.
/// Each entry is (comment, list of keys to emit in that section).
static EXPORT_SECTIONS: &[(&str, &[&str])] = &[
    (
        "# Core proxy",
        &[
            "BACKEND",
            "LISTEN_PORT",
            "BIG_MODEL",
            "SMALL_MODEL",
            "RUST_LOG",
            "LOG_BODIES",
            "PROXY_CONFIG",
            "REQUEST_TIMEOUT_SECS",
            "MODEL_PRICING_FILE",
            "ANYLLM_DEGRADATION_WARNINGS",
        ],
    ),
    ("# Authentication / relay", &["PROXY_API_KEYS", "PROXY_OPEN_RELAY"]),
    ("# OpenAI / compatible", &["OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_API_FORMAT"]),
    (
        "# Azure OpenAI",
        &[
            "AZURE_OPENAI_ENDPOINT",
            "AZURE_OPENAI_DEPLOYMENT",
            "AZURE_OPENAI_API_KEY",
            "AZURE_OPENAI_API_VERSION",
        ],
    ),
    ("# Google Vertex AI", &["VERTEX_PROJECT", "VERTEX_REGION", "VERTEX_API_KEY", "GOOGLE_ACCESS_TOKEN"]),
    ("# Google Gemini", &["GEMINI_API_KEY", "GEMINI_BASE_URL"]),
    (
        "# AWS Bedrock",
        &["AWS_REGION", "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN"],
    ),
    ("# TLS", &["TLS_CLIENT_CERT_P12", "TLS_CLIENT_CERT_PASSWORD", "TLS_CA_CERT"]),
    (
        "# Network / security",
        &["IP_ALLOWLIST", "TRUST_PROXY_HEADERS", "WEBHOOK_URLS", "RATE_LIMIT_FAIL_POLICY"],
    ),
    (
        "# Admin server",
        &[
            "ADMIN_PORT",
            "ADMIN_BIND",
            "ADMIN_DB_PATH",
            "ADMIN_TOKEN_PATH",
            "ADMIN_TOKEN",
            "DISABLE_ADMIN",
            "WEBUI",
            "ADMIN_LOG_RETENTION_DAYS",
        ],
    ),
    ("# OIDC / JWT", &["OIDC_ISSUER_URL", "OIDC_AUDIENCE"]),
    ("# Redis (optional rate-limit backend)", &["REDIS_URL"]),
    ("# Qdrant (optional semantic cache)", &["QDRANT_URL", "QDRANT_COLLECTION"]),
    (
        "# OpenTelemetry",
        &["OTEL_EXPORTER_OTLP_ENDPOINT", "OTEL_SERVICE_NAME", "OTEL_TRACES_SAMPLER"],
    ),
    ("# Langfuse tracing", &["LANGFUSE_PUBLIC_KEY", "LANGFUSE_SECRET_KEY", "LANGFUSE_HOST"]),
];

/// GET /admin/api/env/export
///
/// Returns the current effective environment as a downloadable `.anyllm.env` file.
/// Values are unmasked and properly double-quoted with escape sequences applied.
/// Only keys with non-empty values are included.
/// Gated by admin Bearer auth + CSRF + localhost-only origin (same as all protected routes).
pub(super) async fn export_env() -> impl IntoResponse {
    // Snapshot all known env vars in one pass to avoid repeated std::env::var calls.
    let env_cache: std::collections::HashMap<&str, String> = KNOWN_KEYS
        .iter()
        .filter_map(|&k| std::env::var(k).ok().filter(|v| !v.is_empty()).map(|v| (k, v)))
        .collect();

    let mut out = String::with_capacity(2048);
    out.push_str("# .anyllm.env — exported by anyllm-proxy admin UI\n");

    let mut emitted = std::collections::HashSet::new();

    for (comment, keys) in EXPORT_SECTIONS {
        if !keys.iter().any(|k| env_cache.contains_key(k)) {
            continue;
        }
        out.push('\n');
        out.push_str(comment);
        out.push('\n');
        for &key in *keys {
            emitted.insert(key);
            if let Some(val) = env_cache.get(key) {
                out.push_str(&format!("{key}=\"{}\"\n", escape_for_env_file(val)));
            }
        }
    }

    // Emit any KNOWN_KEY with a value that wasn't covered by a named section.
    let mut extras: Vec<(&str, &str)> = env_cache
        .iter()
        .filter(|(k, _)| !emitted.contains(**k))
        .map(|(&k, v)| (k, v.as_str()))
        .collect();
    if !extras.is_empty() {
        extras.sort_by_key(|(k, _)| *k);
        out.push_str("\n# Other\n");
        for (key, val) in extras {
            out.push_str(&format!("{key}=\"{}\"\n", escape_for_env_file(val)));
        }
    }

    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\".anyllm.env\"",
        )
        .body(axum::body::Body::from(out))
        .unwrap()
        .into_response()
}
