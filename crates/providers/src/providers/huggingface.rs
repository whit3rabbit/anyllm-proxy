use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Hugging Face Inference Providers: unified OpenAI-compatible router that fans out
/// to partner providers (Together, Fireworks, SambaNova, Novita, Groq, Cerebras,
/// Replicate, Hyperbolic, Fal, Nscale, Scaleway, HF Inference, etc.).
///
/// Chat completions endpoint: POST https://router.huggingface.co/v1/chat/completions
/// Models are addressed by HF model id (e.g. `meta-llama/Llama-3.3-70B-Instruct`),
/// optionally with a provider/policy suffix (`:fastest`, `:cheapest`, `:preferred`,
/// or `:<provider>`). Auth is a Bearer HF token (`HF_TOKEN`).
///
/// Note: the unified OpenAI-compat endpoint covers chat completions only. Other
/// tasks (embeddings, text-to-image, speech) require the HF Inference clients.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "huggingface",
    display_name: "HuggingFace",
    default_base_url: "https://router.huggingface.co",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["HF_TOKEN", "HUGGINGFACE_API_KEY"],
    litellm_prefix: "huggingface/",
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

// Representative popular public models routed via HF Inference Providers.
// Context/output windows reflect the upstream model; the actual limits served
// depend on which partner provider handles the request.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "meta-llama/Llama-3.3-70B-Instruct",
        provider_id: "huggingface",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Meta-Llama-3.1-8B-Instruct",
        provider_id: "huggingface",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Meta-Llama-3.1-405B-Instruct",
        provider_id: "huggingface",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-V3",
        provider_id: "huggingface",
        context_window: 64_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-R1",
        provider_id: "huggingface",
        context_window: 64_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen2.5-72B-Instruct",
        provider_id: "huggingface",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen2.5-Coder-32B-Instruct",
        provider_id: "huggingface",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/QwQ-32B-Preview",
        provider_id: "huggingface",
        context_window: 32_768,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistralai/Mistral-7B-Instruct-v0.3",
        provider_id: "huggingface",
        context_window: 32_768,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistralai/Mixtral-8x7B-Instruct-v0.1",
        provider_id: "huggingface",
        context_window: 32_768,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "openai/gpt-oss-120b",
        provider_id: "huggingface",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
