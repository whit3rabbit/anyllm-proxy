use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Serper — Google Search API for AI applications. Chat completions not supported.
/// Base URL: https://google.serper.dev (POST /search, /images, /news, /places,
/// /videos, /maps, /shopping, /scholar, /patents, /autocomplete).
/// NOTE: Serper authenticates via `X-API-KEY` header, not `Authorization: Bearer`.
/// `AuthKind::Bearer` below is a placeholder until `AuthKind::XApiKey` is added.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "serper",
    display_name: "Serper",
    default_base_url: "https://google.serper.dev",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["SERPER_API_KEY"],
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

// No model selection — single endpoint API
pub const MODELS: &[ModelDef] = &[];
