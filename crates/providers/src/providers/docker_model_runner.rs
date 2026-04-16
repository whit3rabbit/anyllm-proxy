use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Docker Model Runner — Docker Desktop's local OpenAI-compatible model server (no auth).
///
/// Exposes an OpenAI-compatible API on `http://localhost:12434/engines/v1` by default.
/// Authentication is not required: DMR ignores the `Authorization` header.
/// Models are user-pulled from Docker Hub / OCI registries / Hugging Face, so no
/// fixed catalog ships here.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "docker_model_runner",
    display_name: "Docker Model Runner",
    default_base_url: "http://localhost:12434/engines/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "docker_model_runner/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        // Tool use supported with llama.cpp backend for compatible models.
        tool_use: true,
        // `/engines/v1/embeddings` is a first-class endpoint.
        embeddings: true,
        // Vision supported for multi-modal models (e.g. LLaVA).
        vision: true,
        batch: false,
    },
};

// Models are pulled locally by the user (OCI / Hugging Face); no fixed catalog.
pub const MODELS: &[ModelDef] = &[];
