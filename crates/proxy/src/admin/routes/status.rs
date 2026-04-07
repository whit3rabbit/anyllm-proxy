use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct ProxyStatus {
    pub configured: bool,
}

/// GET /admin/api/status -- returns whether the proxy has a backend configured.
/// "Configured" means the user set at least one backend-relevant env var
/// (API key, base URL, provider choice) or pointed to a config file.
pub async fn get_status() -> Json<ProxyStatus> {
    Json(ProxyStatus {
        configured: is_backend_configured(),
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
