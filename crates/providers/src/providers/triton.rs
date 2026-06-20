use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// NVIDIA Triton Inference Server — self-hosted.
///
/// Triton's OpenAI-compatible frontend (stable as of 2025) binds to
/// `http://localhost:9000` by default when launched via Triton CLI
/// (`triton start --frontend openai`) or `openai_frontend/main.py`.
/// It exposes `/v1/chat/completions`, `/v1/completions`, `/v1/models`,
/// and (via the vLLM backend) embeddings. Tools and `tool_choice` are
/// supported on chat completions. Models are user-deployed so no static
/// catalog is shipped; operators override `default_base_url` per deployment.
///
/// Note: the native Triton KServe v2 HTTP frontend uses port 8000 and a
/// different wire format — this entry targets the OpenAI-compat frontend.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "triton",
    display_name: "NVIDIA Triton",
    default_base_url: "http://localhost:9000",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "triton/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
