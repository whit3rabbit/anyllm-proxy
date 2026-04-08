use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Snowflake Cortex AI — per-account URL; requires SNOWFLAKE_ACCOUNT_ID.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "snowflake",
    display_name: "Snowflake Cortex",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["SNOWFLAKE_JWT", "SNOWFLAKE_ACCOUNT_ID"],
    litellm_prefix: "snowflake/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
