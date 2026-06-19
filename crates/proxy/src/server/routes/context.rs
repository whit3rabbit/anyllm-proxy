use crate::admin::state::RequestLogEntry;
use crate::backend::BackendClient;
use anyllm_translate::{anthropic, mapping, openai};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

/// Captures per-request context shared across success/error log paths.
pub struct RequestCtx {
    pub request_id: String,
    pub start: std::time::Instant,
    pub model_requested: String,
}

impl RequestCtx {
    /// Build a log entry, filling common fields from the context.
    pub fn log_entry(
        &self,
        backend_name: &str,
        model_mapped: Option<String>,
        status_code: u16,
        tokens: Option<(u64, u64)>,
        is_streaming: bool,
        error_message: Option<String>,
    ) -> RequestLogEntry {
        RequestLogEntry {
            request_id: self.request_id.clone(),
            timestamp: crate::admin::db::now_iso8601(),
            backend: backend_name.to_string(),
            model_requested: Some(self.model_requested.clone()),
            model_mapped,
            status_code,
            latency_ms: self.start.elapsed().as_millis() as u64,
            input_tokens: tokens.map(|(i, _)| i),
            output_tokens: tokens.map(|(_, o)| o),
            is_streaming,
            error_message,
            error_kind: None,
            key_id: None,
            cost_usd: None,
        }
    }

    /// Build a log entry with attribution (key_id from virtual key, cost from pricing).
    #[allow(clippy::too_many_arguments)]
    pub fn log_entry_with_attribution(
        &self,
        backend_name: &str,
        model_mapped: Option<String>,
        status_code: u16,
        tokens: Option<(u64, u64)>,
        is_streaming: bool,
        error_message: Option<String>,
        vk_ctx: &Option<super::super::middleware::VirtualKeyContext>,
        cost_usd: Option<f64>,
    ) -> RequestLogEntry {
        let mut entry = self.log_entry(
            backend_name,
            model_mapped,
            status_code,
            tokens,
            is_streaming,
            error_message,
        );
        entry.key_id = vk_ctx.as_ref().map(|ctx| ctx.key_id);
        // Only store non-zero costs.
        entry.cost_usd = cost_usd.filter(|&c| c > 0.0);
        entry
    }
}

/// When routing through the Gemini OpenAI-compatible endpoint, inject Anthropic's
/// thinking config into the `google` extension field that Gemini expects.
pub fn inject_gemini_thinking(
    body: &anthropic::MessageCreateRequest,
    backend: &BackendClient,
    req: &mut openai::ChatCompletionRequest,
) {
    if !matches!(
        backend,
        BackendClient::GeminiOpenAI(_) | BackendClient::Vertex(_)
    ) {
        return;
    }
    if let Some(anthropic::ThinkingConfig::Enabled { budget_tokens }) = &body.thinking {
        req.extra.insert(
            "google".to_string(),
            serde_json::json!({
                "thinking_config": { "thinking_budget": budget_tokens }
            }),
        );
    }
}

/// When routing to ZhipuAI (Z.AI / GLM models), inject Anthropic's thinking config
/// as GLM's `thinking` extension field. GLM has no token budget parameter, so
/// `budget_tokens` is not forwarded. `reasoning_effort` (injected by
/// `anthropic_to_openai_request`) is also removed since GLM doesn't use it.
pub fn inject_glm_thinking(
    body: &anthropic::MessageCreateRequest,
    backend: &BackendClient,
    req: &mut openai::ChatCompletionRequest,
) {
    let is_glm = matches!(
        backend,
        BackendClient::OpenAI(c) if c.provider_id() == Some("zai")
    );
    if !is_glm {
        return;
    }
    if matches!(
        &body.thinking,
        Some(anthropic::ThinkingConfig::Enabled { .. })
    ) {
        // reasoning_effort was injected by anthropic_to_openai_request; GLM doesn't use it.
        req.extra.remove("reasoning_effort");
        req.extra.insert(
            "thinking".to_string(),
            serde_json::json!({"type": "enabled", "clear_thinking": false}),
        );
    }
}

pub(super) fn route_scope_forbidden_response(
    error: crate::server::policy::RouteScopeError,
) -> Response {
    let err = mapping::errors_map::create_anthropic_error(
        anthropic::ErrorType::PermissionError,
        error.message().to_string(),
        None,
    );
    (StatusCode::FORBIDDEN, Json(err)).into_response()
}
