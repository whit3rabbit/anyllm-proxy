use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "hosted_vllm",
    display_name: "vLLM (self-hosted)",
    // vLLM's OpenAI-compatible server defaults to http://localhost:8000.
    // The `/v1` suffix matches the OpenAI SDK convention documented by vLLM
    // (see https://docs.vllm.ai/en/latest/getting_started/quickstart).
    // Override per deployment via config / env.
    default_base_url: "http://localhost:8000/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    // Self-hosted: no auth by default. vLLM examples use api_key="empty"/"dummy".
    // An API key can be enforced with `--api-key` at server start, but there is
    // no canonical env var convention, so users supply it via managed-backend config.
    env_vars: &[],
    litellm_prefix: "hosted_vllm/",
    capabilities: ProviderCapabilities {
        // Endpoint-level capabilities. Model-level support (tool_use, vision,
        // embeddings) varies — vLLM exposes the endpoints; whether a loaded
        // model honors them depends on the model and server flags
        // (e.g. --enable-auto-tool-choice, --tool-call-parser).
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
