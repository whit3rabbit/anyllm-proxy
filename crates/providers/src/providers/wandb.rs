use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Weights & Biases Inference — OpenAI-compatible endpoint.
/// Docs: https://docs.wandb.ai/inference/api-reference
/// Base URL: https://api.inference.wandb.ai/v1
/// Auth: `Authorization: Bearer <WANDB_API_KEY>` (create at https://wandb.ai/authorize).
/// Chat Completions are supported; function/tool calling and vision vary per model and are
/// not documented uniformly, so provider-level capability flags are kept conservative.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "wandb",
    display_name: "Weights & Biases Inference",
    default_base_url: "https://api.inference.wandb.ai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["WANDB_API_KEY"],
    litellm_prefix: "wandb/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Model catalog intentionally empty: W&B Inference's hosted model list changes frequently
// and the public docs do not publish stable per-model context window / max output token
// values. Routing works via the `wandb/<model-id>` LiteLLM prefix at runtime.
pub const MODELS: &[ModelDef] = &[];
