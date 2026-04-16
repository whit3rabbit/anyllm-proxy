use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// Clarifai exposes an OpenAI-compatible chat completions endpoint at
// https://api.clarifai.com/v2/ext/openai/v1. Auth is a Personal Access Token
// (PAT) passed as `Authorization: Bearer <PAT>`. Model IDs follow the
// "<user>.<app>.<model>" form used by Clarifai's community catalog.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "clarifai",
    display_name: "Clarifai",
    default_base_url: "https://api.clarifai.com/v2/ext/openai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["CLARIFAI_PAT", "CLARIFAI_API_KEY"],
    litellm_prefix: "clarifai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// Curated subset of publicly GA models from Clarifai's community catalog.
// Context windows reflect upstream model specs; Clarifai may apply lower
// per-deployment limits. Tool use availability depends on the hosted runtime.
pub const MODELS: &[ModelDef] = &[
    // OpenAI open-weight (GPT-OSS) hosted on Clarifai compute.
    ModelDef {
        id: "openai.chat-completion.gpt-oss-120b",
        provider_id: "clarifai",
        context_window: 131_072,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "openai.chat-completion.gpt-oss-20b",
        provider_id: "clarifai",
        context_window: 131_072,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Anthropic via Clarifai (proxied commercial models).
    ModelDef {
        id: "anthropic.completion.claude-sonnet-4",
        provider_id: "clarifai",
        context_window: 200_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic.completion.claude-opus-4",
        provider_id: "clarifai",
        context_window: 200_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic.completion.claude-3_7-sonnet",
        provider_id: "clarifai",
        context_window: 200_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic.completion.claude-3_5-haiku",
        provider_id: "clarifai",
        context_window: 200_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Meta Llama 3.x hosted on Clarifai.
    ModelDef {
        id: "meta.Llama-3.Llama-3_2-3B-Instruct",
        provider_id: "clarifai",
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
    // DeepSeek open-weight reasoning distill hosted on Clarifai.
    ModelDef {
        id: "deepseek-ai.deepseek-chat.DeepSeek-R1-0528-Qwen3-8B",
        provider_id: "clarifai",
        context_window: 65_536,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Qwen3 open-weight hosted on Clarifai.
    ModelDef {
        id: "qwen.qwenLM.Qwen3-30B-A3B-Instruct-2507",
        provider_id: "clarifai",
        context_window: 262_144,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Google Gemini via Clarifai (proxied commercial model).
    ModelDef {
        id: "gcp.generate.gemini-2_5-pro",
        provider_id: "clarifai",
        context_window: 1_048_576,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // xAI Grok via Clarifai (proxied commercial model).
    ModelDef {
        id: "xai.chat-completion.grok-3",
        provider_id: "clarifai",
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
];
