use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// SiliconFlow — inference platform with an OpenAI-compatible API.
///
/// Official base URL: <https://api.siliconflow.cn/v1>
/// Docs: <https://docs.siliconflow.cn/en/api-reference/chat-completions/chat-completions>
///
/// SiliconFlow hosts chat, embeddings, image generation (FLUX, Kolors, Qwen image),
/// reranking, and audio/TTS (CosyVoice, MOSS-TTSD) models behind the same base URL.
///
/// Model list intentionally left empty: SiliconFlow's model catalog rotates
/// frequently (Qwen3.x, GLM-4.x/5, DeepSeek-V3.x, Pro/ vs. free tiers) and the
/// public docs do not publish an authoritative context_window / max_output_tokens
/// table per model. Values must be looked up per-model at
/// <https://cloud.siliconflow.cn/models>. Guessing would violate the provider
/// contract (see `ModelDef` docs).
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "siliconflow",
    display_name: "SiliconFlow",
    default_base_url: "https://api.siliconflow.cn/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["SILICONFLOW_API_KEY"],
    litellm_prefix: "siliconflow/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
