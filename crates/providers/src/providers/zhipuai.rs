use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Zhipu AI (Z.AI) — GLM model series via OpenAI-compatible endpoint.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "zhipuai",
    display_name: "Zhipu AI (Z.AI)",
    default_base_url: "https://open.bigmodel.cn/api/paas/v4",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["ZHIPUAI_API_KEY"],
    litellm_prefix: "zhipuai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
