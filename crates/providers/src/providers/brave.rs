use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Brave Search API — web/news/image/video search, summarizer, and LLM context.
/// Not a chat API; no streaming, tools, embeddings, or vision.
/// Auth header: `X-Subscription-Token: <key>` (not a bearer token — see note below).
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "brave",
    display_name: "Brave Search",
    // Endpoints live under /res/v1/{web,news,images,videos,summarizer,suggest,spellcheck}/search
    default_base_url: "https://api.search.brave.com/res/v1",
    protocol: ProviderProtocol::OpenAICompat,
    // NOTE: Brave uses `X-Subscription-Token`, not `Authorization: Bearer`.
    // AuthKind has no dedicated variant for this header; leaving as Bearer per scope.
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["BRAVE_API_KEY"],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        streaming: false,
        tool_use: false,
        tool_choice: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Search API — no model selection. Endpoint is chosen by path, not by model id.
pub const MODELS: &[ModelDef] = &[];
