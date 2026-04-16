use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "friendliai",
    display_name: "FriendliAI",
    default_base_url: "https://api.friendli.ai/serverless/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    // Official token env var per Friendli docs is FRIENDLI_TOKEN; keep the
    // FRIENDLIAI_* alias for LiteLLM-style compatibility.
    env_vars: &["FRIENDLI_TOKEN", "FRIENDLIAI_TOKEN", "FRIENDLIAI_API_KEY"],
    litellm_prefix: "friendliai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// GA serverless models listed on Friendli's pricing page. IDs match the
// native Friendli format (vendor/Model-Name). OpenAI-compat callers may
// also use the short dash form (e.g. meta-llama-3.3-70b-instruct).
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "meta-llama/Llama-3.3-70B-Instruct",
        provider_id: "friendliai",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Llama-3.1-8B-Instruct",
        provider_id: "friendliai",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen3-235B-A22B-Instruct-2507",
        provider_id: "friendliai",
        context_window: 262_144,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen3-30B-A3B",
        provider_id: "friendliai",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-V3.1",
        provider_id: "friendliai",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-V3.2",
        provider_id: "friendliai",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
