use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "bedrock",
    display_name: "AWS Bedrock",
    // URL is constructed per-region: https://bedrock-runtime.{region}.amazonaws.com
    default_base_url: "",
    protocol: ProviderProtocol::BedrockNative,
    auth: AuthKind::AwsSigV4,
    status: ProviderStatus::Wired,
    env_vars: &["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_REGION"],
    litellm_prefix: "bedrock/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

// Model IDs sourced from Anthropic's official model overview and AWS Bedrock
// docs. On-demand IDs only (not cross-region inference profile IDs like
// `us.anthropic.*`). Context window / max output reflect Anthropic's current
// published limits.
pub const MODELS: &[ModelDef] = &[
    // --- Current generation (Claude 4.6 family) ---
    ModelDef {
        id: "anthropic.claude-opus-4-6-v1",
        provider_id: "bedrock",
        context_window: 1_000_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        // Anthropic docs list this without the -v1:0 suffix on Bedrock.
        id: "anthropic.claude-sonnet-4-6",
        provider_id: "bedrock",
        context_window: 1_000_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic.claude-haiku-4-5-20251001-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // --- Claude 4.5 family ---
    ModelDef {
        id: "anthropic.claude-opus-4-5-20251101-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic.claude-sonnet-4-5-20250929-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // --- Claude 4.1 / 4.0 family (legacy but still available) ---
    ModelDef {
        id: "anthropic.claude-opus-4-1-20250805-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        // Deprecated per Anthropic: retires 2026-06-15. Migrate to Opus 4.6.
        id: "anthropic.claude-opus-4-20250514-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Deprecated,
    },
    ModelDef {
        // Deprecated per Anthropic: retires 2026-06-15. Migrate to Sonnet 4.6.
        id: "anthropic.claude-sonnet-4-20250514-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Deprecated,
    },
    // --- Claude 3.x (legacy; widely used, still GA on Bedrock) ---
    ModelDef {
        id: "anthropic.claude-3-5-sonnet-20241022-v2:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic.claude-3-5-haiku-20241022-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        // Deprecated per Anthropic: retires 2026-04-19. Migrate to Haiku 4.5.
        id: "anthropic.claude-3-haiku-20240307-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Deprecated,
    },
];
