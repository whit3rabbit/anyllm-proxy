use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "azure",
    display_name: "Azure OpenAI",
    // No fixed base URL — each Azure resource has its own endpoint.
    default_base_url: "",
    protocol: ProviderProtocol::AzureOpenAI,
    auth: AuthKind::AzureApiKey,
    status: ProviderStatus::Wired,
    env_vars: &["AZURE_OPENAI_API_KEY"],
    litellm_prefix: "azure/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// Azure deploys user-chosen models under user-chosen deployment names.
// No static model list is maintained here.
pub const MODELS: &[ModelDef] = &[];
