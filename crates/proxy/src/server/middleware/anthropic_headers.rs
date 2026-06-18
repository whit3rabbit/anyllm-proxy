use axum::{body::Body, http::Request, middleware::Next, response::Response};

/// Log Anthropic-specific headers without rejecting requests that lack them.
/// Claude Code CLI and other Anthropic SDK clients send these headers.
pub async fn log_anthropic_headers(request: Request<Body>, next: Next) -> Response {
    if let Some(v) = request
        .headers()
        .get("anthropic-version")
        .and_then(|v| v.to_str().ok())
    {
        tracing::debug!(anthropic_version = %v, "anthropic-version header present");
    }
    if let Some(b) = request
        .headers()
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
    {
        tracing::debug!(anthropic_beta = %b, "anthropic-beta header present");
    }
    // Claude Code v2.1.86+ sends this for proxy-side session routing/aggregation.
    if let Some(s) = request
        .headers()
        .get("x-claude-code-session-id")
        .and_then(|v| v.to_str().ok())
    {
        tracing::debug!(session_id = %s, "x-claude-code-session-id header present");
    }
    next.run(request).await
}
