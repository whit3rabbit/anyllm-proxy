use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// Predibase: fine-tuning + inference platform.
// Serverless ("always-on shared") endpoints are tenant-scoped. Full URL shape:
//   https://serving.app.predibase.com/{tenant_short_code}/deployments/v2/llms/{deployment_name}/v1/chat/completions
// `default_base_url` only covers the host; the tenant + deployment path must be
// supplied via configuration (managed backend `api_base`) at runtime.
// Auth: `Authorization: Bearer $PREDIBASE_API_KEY`.
// The endpoint is OpenAI chat-completions compatible.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "predibase",
    display_name: "Predibase",
    default_base_url: "https://serving.app.predibase.com",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["PREDIBASE_API_KEY"],
    litellm_prefix: "predibase/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        tool_choice: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Only GA "Always On Shared Endpoint" serverless base models per
// https://docs.predibase.com/inference/models/language-models (supported models table).
// Dedicated / private deployments can run many more base models, but those require
// per-tenant deployment IDs and are not part of the shared catalog.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "llama-3-1-8b-instruct",
        provider_id: "predibase",
        context_window: 64_000,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "qwen3-8b",
        provider_id: "predibase",
        context_window: 64_000,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "qwen3-32b",
        provider_id: "predibase",
        context_window: 16_000,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
