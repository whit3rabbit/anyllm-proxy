use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "openrouter",
    display_name: "OpenRouter",
    default_base_url: "https://openrouter.ai/api/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["OPENROUTER_API_KEY"],
    litellm_prefix: "openrouter/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

// OpenRouter aggregates hundreds of models from many upstream providers.
// Model ids use the form `<provider>/<model>` and are passed through as-is.
// Use GET /api/v1/models at runtime to enumerate available slugs.
pub const MODELS: &[ModelDef] = &[];
