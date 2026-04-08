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
        embeddings: false,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "anthropic.claude-sonnet-4-20250514-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic.claude-haiku-4-5-20251001-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 8_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic.claude-3-5-sonnet-20241022-v2:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 8_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic.claude-3-haiku-20240307-v1:0",
        provider_id: "bedrock",
        context_window: 200_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
