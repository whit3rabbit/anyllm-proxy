use crate::backend::BackendClient;
use crate::server::routes::RequestCtx;
use crate::server::state::{AppState, ConcurrencyPermit};
use crate::server::streaming::StreamDeploymentAccounting;
use anyllm_translate::{
    anthropic as translate_anthropic, AnthropicTranslationContext, TranslationWarnings,
};
use axum::response::Response;

pub(super) mod anthropic;
pub(super) mod generic;

pub(super) struct ChatCompletionsStreamMeta {
    pub(super) ctx: RequestCtx,
    pub(super) original_model: String,
    pub(super) mapped_model: String,
    pub(super) warnings: TranslationWarnings,
    pub(super) safe_headers: Vec<(String, String)>,
    pub(super) raw_anthropic_tools: Vec<serde_json::Value>,
    pub(super) tool_context: AnthropicTranslationContext,
    pub(super) concurrency_permit: Option<ConcurrencyPermit>,
    pub(super) vk_ctx: Option<crate::server::middleware::VirtualKeyContext>,
    pub(super) deployment_accounting: StreamDeploymentAccounting,
}

pub(super) async fn chat_completions_stream(
    state: AppState,
    anthropic_req: translate_anthropic::MessageCreateRequest,
    meta: ChatCompletionsStreamMeta,
) -> Response {
    if let BackendClient::Anthropic(client) = &state.backend {
        return anthropic::anthropic_chat_completions_stream(
            state.clone(),
            client.clone(),
            anthropic_req,
            meta,
        )
        .await;
    }

    generic::generic_chat_completions_stream(state, anthropic_req, meta).await
}
