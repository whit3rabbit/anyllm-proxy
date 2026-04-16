use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "hyperbolic",
    display_name: "Hyperbolic",
    default_base_url: "https://api.hyperbolic.xyz/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["HYPERBOLIC_API_KEY"],
    litellm_prefix: "hyperbolic/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

// Model IDs follow Hyperbolic's HuggingFace-style format as documented in
// quickstart examples (e.g. "meta-llama/Meta-Llama-3.1-70B-Instruct").
// Context windows reflect the underlying model cards. Tool use on Hyperbolic
// is not documented as first-class for most models, so kept conservative.
pub const MODELS: &[ModelDef] = &[
    // DeepSeek
    ModelDef {
        id: "deepseek-ai/DeepSeek-V3",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-V3-0324",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-R1",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-R1-0528",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Meta Llama 3.x
    ModelDef {
        id: "meta-llama/Meta-Llama-3.1-405B-Instruct",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Meta-Llama-3.1-70B-Instruct",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Meta-Llama-3.1-8B-Instruct",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Llama-3.2-3B-Instruct",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Llama-3.3-70B-Instruct",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Meta-Llama-3-70B-Instruct",
        provider_id: "hyperbolic",
        context_window: 8_192,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Qwen
    ModelDef {
        id: "Qwen/Qwen2.5-72B-Instruct",
        provider_id: "hyperbolic",
        context_window: 32_768,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen2.5-Coder-32B-Instruct",
        provider_id: "hyperbolic",
        context_window: 32_768,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/QwQ-32B",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Nous Hermes
    ModelDef {
        id: "NousResearch/Hermes-3-Llama-3.1-70B",
        provider_id: "hyperbolic",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
