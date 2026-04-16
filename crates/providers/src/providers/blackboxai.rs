use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Blackbox AI — aggregator offering an OpenAI-compatible `/chat/completions`
/// endpoint fronting Anthropic, OpenAI, Google, Meta, and other frontier models.
/// Docs: https://docs.blackbox.ai/api-reference/introduction
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "blackboxai",
    display_name: "Blackbox AI",
    // Per docs: `https://api.blackbox.ai/chat/completions`. The OpenAI-compat
    // client appends `/chat/completions`, so the base is the host root.
    default_base_url: "https://api.blackbox.ai",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["BLACKBOXAI_API_KEY"],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

// Curated GA subset from Blackbox's chat-models catalog. Only models whose
// upstream context/output limits are publicly documented are included.
// Context windows / max output reflect the upstream provider's spec.
pub const MODELS: &[ModelDef] = &[
    // --- OpenAI via Blackbox ---
    ModelDef {
        id: "gpt-4o",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gpt-4o-mini",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gpt-4-turbo",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gpt-4",
        provider_id: "blackboxai",
        context_window: 8_192,
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
        id: "gpt-3.5-turbo",
        provider_id: "blackboxai",
        context_window: 16_385,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "o1",
        provider_id: "blackboxai",
        context_window: 200_000,
        max_output_tokens: 100_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "o3",
        provider_id: "blackboxai",
        context_window: 200_000,
        max_output_tokens: 100_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "o3-mini",
        provider_id: "blackboxai",
        context_window: 200_000,
        max_output_tokens: 100_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // --- Anthropic via Blackbox ---
    ModelDef {
        id: "claude-opus-4.1",
        provider_id: "blackboxai",
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-opus-4",
        provider_id: "blackboxai",
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-sonnet-4",
        provider_id: "blackboxai",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-3.7-sonnet",
        provider_id: "blackboxai",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-3.5-haiku",
        provider_id: "blackboxai",
        context_window: 200_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-3-haiku",
        provider_id: "blackboxai",
        context_window: 200_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- Google via Blackbox ---
    ModelDef {
        id: "gemini-2.5-pro",
        provider_id: "blackboxai",
        context_window: 1_048_576,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gemini-2.5-flash",
        provider_id: "blackboxai",
        context_window: 1_048_576,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // --- Meta Llama via Blackbox ---
    ModelDef {
        id: "llama-3.3-70b-instruct",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "llama-3.1-405b-instruct",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "llama-3.1-70b-instruct",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "llama-3.1-8b-instruct",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "llama-3.2-11b-vision-instruct",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
