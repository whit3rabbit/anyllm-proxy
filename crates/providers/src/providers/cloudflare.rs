use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Cloudflare Workers AI — URL includes account ID; set via CLOUDFLARE_ACCOUNT_ID.
/// Base URL pattern: https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "cloudflare",
    display_name: "Cloudflare Workers AI",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["CLOUDFLARE_API_KEY", "CLOUDFLARE_ACCOUNT_ID"],
    litellm_prefix: "cloudflare/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
