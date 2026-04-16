use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Exa — neural web search API for AI applications. Not a chat provider.
/// Endpoints: POST /search, POST /contents, POST /findSimilar, POST /answer.
/// /answer is a one-shot Q&A endpoint (not conversational), so chat_completions stays false.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "exa",
    display_name: "Exa",
    // Endpoints live under the root: /search, /contents, /findSimilar, /answer.
    default_base_url: "https://api.exa.ai",
    protocol: ProviderProtocol::OpenAICompat,
    // NOTE: Exa uses `x-api-key: <key>`, not `Authorization: Bearer`.
    // AuthKind has no dedicated variant for a custom header; leaving as Bearer per scope.
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["EXA_API_KEY"],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        streaming: false,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Search API — no model selection. Endpoint is chosen by path, not by model id.
pub const MODELS: &[ModelDef] = &[];
