use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// Meta's official hosted Llama API. OpenAI-compatible surface is served at
// /compat/v1 (native REST is at /v1). The compat endpoint supports chat
// completions, tools/function calling, and json_schema response_format.
// Docs: https://llama.developer.meta.com/docs
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "meta_llama",
    display_name: "Meta Llama API",
    default_base_url: "https://api.llama.com/compat/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["LLAMA_API_KEY"],
    litellm_prefix: "meta_llama/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

// GA catalog on the hosted Llama API. Max output tokens is 4028 across the
// current catalog per LiteLLM's price table; context windows match Meta's
// published limits (Scout 10M, Maverick 1M, Llama 3.3 128k). Llama 4 models
// are natively multimodal; Llama 3.3 variants are text-only.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "Llama-4-Maverick-17B-128E-Instruct-FP8",
        provider_id: "meta_llama",
        context_window: 1_000_000,
        max_output_tokens: 4_028,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Llama-4-Scout-17B-16E-Instruct-FP8",
        provider_id: "meta_llama",
        context_window: 10_000_000,
        max_output_tokens: 4_028,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Llama-3.3-70B-Instruct",
        provider_id: "meta_llama",
        context_window: 128_000,
        max_output_tokens: 4_028,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Llama-3.3-8B-Instruct",
        provider_id: "meta_llama",
        context_window: 128_000,
        max_output_tokens: 4_028,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
