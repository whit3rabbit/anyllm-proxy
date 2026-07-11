use crate::admin::state::SharedState;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct ProxyStatus {
    pub configured: bool,
}

/// GET /admin/api/status -- returns whether the proxy has a backend configured.
/// "Configured" means the user set at least one backend-relevant env var
/// (API key, base URL, provider choice), pointed to a config file, OR created a
/// managed backend via the admin UI (stored in SQLite). Without the managed-backend
/// check, Settings would warn "no backend configured" while the Backends page shows
/// a UI-added backend up.
pub async fn get_status(State(shared): State<SharedState>) -> Json<ProxyStatus> {
    let has_managed_backend = crate::admin::state::with_db(&shared.db, |conn| {
        crate::admin::db::list_managed_backends(conn).map(|rows| !rows.is_empty())
    })
    .await
    .and_then(Result::ok)
    .unwrap_or(false);

    Json(ProxyStatus {
        configured: is_backend_configured() || has_managed_backend,
    })
}

/// Returns true when the user has provided enough information for the proxy to
/// know where to forward requests: an API key, a custom base URL, an explicit
/// backend choice, or a config file.
pub fn is_backend_configured() -> bool {
    const BACKEND_SIGNALS: &[&str] = &[
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "BACKEND",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "GEMINI_API_KEY",
        "VERTEX_API_KEY",
        "GOOGLE_ACCESS_TOKEN",
        "AWS_ACCESS_KEY_ID",
        "AZURE_OPENAI_API_KEY",
        "AZURE_OPENAI_ENDPOINT",
    ];
    let has_signal = BACKEND_SIGNALS
        .iter()
        .any(|k| std::env::var(k).map(|v| !v.is_empty()).unwrap_or(false));
    let has_proxy_config = std::env::var("PROXY_CONFIG")
        .map(|p| std::path::Path::new(&p).exists())
        .unwrap_or(false);
    has_signal || has_proxy_config
}
