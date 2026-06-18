//! In-process Chat Completions runtime.
//!
//! This module exposes model routing and backend dispatch without taking
//! ownership of HTTP routing, auth, admin UI, caching, or tool execution.

use crate::backend::{BackendClient, BackendError, RateLimitHeaders};
use crate::config::{BackendKind, Config, ModelMapping, MultiConfig, OpenAIApiFormat};
use anyllm_translate::{
    mapping, openai, translate_anthropic_to_openai_response, translate_openai_to_anthropic_request,
    TranslationWarnings,
};
use futures::{
    future::{BoxFuture, FutureExt},
    Stream,
};
use std::collections::HashMap;
use std::fmt;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Stream returned by [`ChatCompletionRuntime::complete_stream`].
pub type ChatCompletionChunkStream =
    Pin<Box<dyn Stream<Item = Result<openai::ChatCompletionChunk, ChatCompletionError>> + Send>>;

/// Object-safe service API for one Chat Completions call at a time.
pub trait ChatCompletionService: Send + Sync {
    fn complete<'a>(
        &'a self,
        req: openai::ChatCompletionRequest,
    ) -> BoxFuture<'a, Result<ChatCompletionResult, ChatCompletionError>>;

    fn complete_stream<'a>(
        &'a self,
        req: openai::ChatCompletionRequest,
    ) -> BoxFuture<'a, Result<ChatCompletionStreamResult, ChatCompletionError>>;
}

/// Non-streaming runtime response.
#[derive(Debug)]
pub struct ChatCompletionResult {
    pub response: openai::ChatCompletionResponse,
    pub usage: Option<openai::ChatUsage>,
    pub rate_limits: RateLimitHeaders,
    pub metadata: ChatCompletionMetadata,
    pub warnings: TranslationWarnings,
}

/// Streaming runtime response.
pub struct ChatCompletionStreamResult {
    pub chunks: ChatCompletionChunkStream,
    pub rate_limits: RateLimitHeaders,
    pub metadata: ChatCompletionMetadata,
    pub warnings: TranslationWarnings,
}

impl fmt::Debug for ChatCompletionStreamResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatCompletionStreamResult")
            .field("chunks", &"<stream>")
            .field("rate_limits", &self.rate_limits)
            .field("metadata", &self.metadata)
            .field("warnings", &self.warnings)
            .finish()
    }
}

/// Backend selection metadata for observability and callers that need accounting.
#[derive(Debug, Clone)]
pub struct ChatCompletionMetadata {
    pub requested_model: String,
    pub selected_backend: String,
    pub mapped_model: String,
    pub backend_kind: BackendKind,
    pub provider_id: Option<String>,
    pub api_format: OpenAIApiFormat,
    pub used_responses_api: bool,
}

/// Runtime errors that do not expose axum response types.
#[derive(Debug)]
pub enum ChatCompletionError {
    InvalidRequest(String),
    Translation(anyllm_translate::TranslateError),
    Routing(String),
    UnsupportedBackend {
        backend_name: String,
        backend_kind: BackendKind,
    },
    Backend(BackendError),
    StreamRead(String),
    StreamParse(String),
    StreamBufferOverflow,
    StreamTimeout,
}

impl ChatCompletionError {
    pub fn status_code(&self) -> u16 {
        match self {
            Self::InvalidRequest(_) | Self::Translation(_) | Self::UnsupportedBackend { .. } => 400,
            Self::Routing(_) => 429,
            Self::Backend(e) => e.status_code(),
            Self::StreamRead(_)
            | Self::StreamParse(_)
            | Self::StreamBufferOverflow
            | Self::StreamTimeout => 502,
        }
    }
}

impl fmt::Display for ChatCompletionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(msg) => write!(f, "{msg}"),
            Self::Translation(e) => write!(f, "{e}"),
            Self::Routing(msg) => write!(f, "{msg}"),
            Self::UnsupportedBackend {
                backend_name,
                backend_kind,
            } => write!(
                f,
                "backend '{backend_name}' ({backend_kind:?}) does not support Chat Completions runtime"
            ),
            Self::Backend(e) => write!(f, "{e}"),
            Self::StreamRead(e) => write!(f, "stream read error: {e}"),
            Self::StreamParse(e) => write!(f, "stream parse error: {e}"),
            Self::StreamBufferOverflow => write!(f, "SSE buffer exceeded maximum size"),
            Self::StreamTimeout => write!(f, "stream exceeded wall-clock timeout"),
        }
    }
}

impl std::error::Error for ChatCompletionError {}

impl From<anyllm_translate::TranslateError> for ChatCompletionError {
    fn from(e: anyllm_translate::TranslateError) -> Self {
        Self::Translation(e)
    }
}

impl From<BackendError> for ChatCompletionError {
    fn from(e: BackendError) -> Self {
        Self::Backend(e)
    }
}

/// Chat Completions runtime built from proxy backend configuration.
#[derive(Clone)]
pub struct ChatCompletionRuntime {
    default_backend: String,
    backends: Arc<HashMap<String, RuntimeBackend>>,
    model_router: Option<Arc<RwLock<crate::config::model_router::ModelRouter>>>,
}

#[derive(Clone)]
struct RuntimeBackend {
    backend: BackendClient,
    backend_name: String,
    model_mapping: ModelMapping,
    backend_kind: BackendKind,
    api_format: OpenAIApiFormat,
    omit_stream_options: bool,
    stream_timeout_secs: u64,
    provider_id: Option<String>,
}

struct ResolvedBackend {
    state: RuntimeBackend,
    mapped_model: String,
    deployment: Option<Arc<crate::config::model_router::Deployment>>,
}

impl ChatCompletionRuntime {
    /// Build a runtime from a legacy single-backend config.
    pub fn from_config(config: Config) -> Self {
        let multi = MultiConfig::from_single_config(&config);
        let default_backend = multi.default_backend.clone();
        let (_, bc) = multi
            .backends
            .get_key_value(&default_backend)
            .expect("wrapped config must contain default backend");

        let backend = if config.backend == BackendKind::Bedrock {
            BackendClient::from_backend_config(bc)
        } else {
            BackendClient::new(&config)
        };

        let mut backends = HashMap::new();
        backends.insert(
            default_backend.clone(),
            RuntimeBackend {
                backend,
                backend_name: default_backend.clone(),
                model_mapping: bc.model_mapping.clone(),
                backend_kind: bc.kind.clone(),
                api_format: bc.api_format.clone(),
                omit_stream_options: bc.omit_stream_options,
                stream_timeout_secs: bc.stream_timeout_secs,
                provider_id: config.provider_id.map(str::to_string),
            },
        );

        Self {
            default_backend,
            backends: Arc::new(backends),
            model_router: None,
        }
    }

    /// Build a runtime from multi-backend config without model-list routing.
    pub fn from_multi_config(config: MultiConfig) -> Self {
        Self::from_multi_config_with_model_router(config, None)
    }

    /// Build a runtime from multi-backend config and an optional model router.
    pub fn from_multi_config_with_model_router(
        config: MultiConfig,
        model_router: Option<Arc<RwLock<crate::config::model_router::ModelRouter>>>,
    ) -> Self {
        let mut backends = HashMap::new();
        for (name, bc) in &config.backends {
            backends.insert(
                name.clone(),
                RuntimeBackend {
                    backend: BackendClient::from_backend_config(bc),
                    backend_name: name.clone(),
                    model_mapping: bc.model_mapping.clone(),
                    backend_kind: bc.kind.clone(),
                    api_format: bc.api_format.clone(),
                    omit_stream_options: bc.omit_stream_options,
                    stream_timeout_secs: bc.stream_timeout_secs,
                    provider_id: None,
                },
            );
        }

        Self {
            default_backend: config.default_backend,
            backends: Arc::new(backends),
            model_router,
        }
    }

    fn resolve(&self, model: &str) -> Result<ResolvedBackend, ChatCompletionError> {
        if let Some(ref router_lock) = self.model_router {
            let router = router_lock.read().unwrap_or_else(|e| e.into_inner());
            if let Some(routed) = router.route(model) {
                let state = self
                    .backends
                    .get(routed.backend_name)
                    .cloned()
                    .ok_or_else(|| {
                        ChatCompletionError::Routing(format!(
                            "model '{model}' routed to unknown backend '{}'",
                            routed.backend_name
                        ))
                    })?;
                return Ok(ResolvedBackend {
                    state,
                    mapped_model: routed.actual_model.to_string(),
                    deployment: Some(routed.deployment.clone()),
                });
            }
            if router.has_model(model) {
                return Err(ChatCompletionError::Routing(
                    "all deployments for this model are at their RPM limit".to_string(),
                ));
            }
            return Err(ChatCompletionError::InvalidRequest(format!(
                "model '{model}' is not configured in model_list"
            )));
        }

        let state = self
            .backends
            .get(&self.default_backend)
            .cloned()
            .ok_or_else(|| {
                ChatCompletionError::Routing(format!(
                    "default backend '{}' is not configured",
                    self.default_backend
                ))
            })?;
        Ok(ResolvedBackend {
            mapped_model: state.model_mapping.map_model(model),
            state,
            deployment: None,
        })
    }
}

impl ChatCompletionService for ChatCompletionRuntime {
    fn complete<'a>(
        &'a self,
        req: openai::ChatCompletionRequest,
    ) -> BoxFuture<'a, Result<ChatCompletionResult, ChatCompletionError>> {
        async move { self.complete_inner(req).await }.boxed()
    }

    fn complete_stream<'a>(
        &'a self,
        req: openai::ChatCompletionRequest,
    ) -> BoxFuture<'a, Result<ChatCompletionStreamResult, ChatCompletionError>> {
        async move { self.complete_stream_inner(req).await }.boxed()
    }
}

impl ChatCompletionRuntime {
    async fn complete_inner(
        &self,
        req: openai::ChatCompletionRequest,
    ) -> Result<ChatCompletionResult, ChatCompletionError> {
        if req.stream == Some(true) {
            return Err(ChatCompletionError::InvalidRequest(
                "complete does not accept stream=true; use complete_stream".to_string(),
            ));
        }

        let requested_model = req.model.clone();
        let resolved = self.resolve(&requested_model)?;
        let metadata = metadata(&requested_model, &resolved);
        let mut warnings = TranslationWarnings::default();

        match &resolved.state.backend {
            BackendClient::OpenAI(client)
            | BackendClient::AzureOpenAI(client)
            | BackendClient::Vertex(client)
            | BackendClient::GeminiOpenAI(client) => {
                let mut openai_req = req;
                prepare_openai_request(&mut openai_req, &resolved, false, &mut warnings);

                let start = record_start(&resolved.deployment);
                match client.chat_completion(&openai_req).await {
                    Ok((response, _status, rate_limits)) => {
                        record_finish(&resolved.deployment, start);
                        if let Some(ref deployment) = resolved.deployment {
                            if let Some(ref usage) = response.usage {
                                deployment.record_tokens(usage.total_tokens as u64);
                            }
                        }
                        Ok(ChatCompletionResult {
                            usage: response.usage.clone(),
                            response,
                            rate_limits,
                            metadata,
                            warnings,
                        })
                    }
                    Err(e) => {
                        record_finish(&resolved.deployment, start);
                        Err(ChatCompletionError::Backend(BackendError::from(e)))
                    }
                }
            }
            BackendClient::OpenAIResponses(client) => {
                let anthropic_req = translate_openai_to_anthropic_request(&req, &mut warnings)?;
                let mut responses_req =
                    mapping::responses_message_map::anthropic_to_responses_request(&anthropic_req);
                responses_req.model = resolved.mapped_model.clone();

                let start = record_start(&resolved.deployment);
                match client.responses(&responses_req).await {
                    Ok((resp, _status, rate_limits)) => {
                        record_finish(&resolved.deployment, start);
                        let anthropic_resp =
                            mapping::responses_message_map::responses_to_anthropic_response(
                                &resp,
                                &requested_model,
                            );
                        if let Some(ref deployment) = resolved.deployment {
                            deployment.record_tokens(
                                anthropic_resp.usage.input_tokens as u64
                                    + anthropic_resp.usage.output_tokens as u64,
                            );
                        }
                        let response = translate_anthropic_to_openai_response(
                            &anthropic_resp,
                            &requested_model,
                        );
                        Ok(ChatCompletionResult {
                            usage: response.usage.clone(),
                            response,
                            rate_limits,
                            metadata,
                            warnings,
                        })
                    }
                    Err(e) => {
                        record_finish(&resolved.deployment, start);
                        Err(ChatCompletionError::Backend(BackendError::from(e)))
                    }
                }
            }
            BackendClient::Anthropic(_)
            | BackendClient::Bedrock(_)
            | BackendClient::GeminiNative(_) => Err(ChatCompletionError::UnsupportedBackend {
                backend_name: resolved.state.backend_name,
                backend_kind: resolved.state.backend_kind,
            }),
        }
    }

    async fn complete_stream_inner(
        &self,
        req: openai::ChatCompletionRequest,
    ) -> Result<ChatCompletionStreamResult, ChatCompletionError> {
        let requested_model = req.model.clone();
        let resolved = self.resolve(&requested_model)?;
        let metadata = metadata(&requested_model, &resolved);
        let mut warnings = TranslationWarnings::default();

        match &resolved.state.backend {
            BackendClient::OpenAI(client)
            | BackendClient::AzureOpenAI(client)
            | BackendClient::Vertex(client)
            | BackendClient::GeminiOpenAI(client) => {
                let mut openai_req = req;
                prepare_openai_request(&mut openai_req, &resolved, true, &mut warnings);

                let start = record_start(&resolved.deployment);
                match client.chat_completion_stream(&openai_req).await {
                    Ok((response, rate_limits)) => Ok(ChatCompletionStreamResult {
                        chunks: openai_chunk_stream(
                            response,
                            resolved.state.stream_timeout_secs,
                            DeploymentLatencyGuard::from_started(
                                resolved.deployment.clone(),
                                start,
                            ),
                        ),
                        rate_limits,
                        metadata,
                        warnings,
                    }),
                    Err(e) => {
                        record_finish(&resolved.deployment, start);
                        Err(ChatCompletionError::Backend(BackendError::from(e)))
                    }
                }
            }
            BackendClient::OpenAIResponses(client) => {
                let anthropic_req = translate_openai_to_anthropic_request(&req, &mut warnings)?;
                let mut responses_req =
                    mapping::responses_message_map::anthropic_to_responses_request(&anthropic_req);
                responses_req.model = resolved.mapped_model.clone();
                responses_req.stream = Some(true);

                let start = record_start(&resolved.deployment);
                match client.responses_stream(&responses_req).await {
                    Ok((response, rate_limits)) => Ok(ChatCompletionStreamResult {
                        chunks: responses_chunk_stream(
                            response,
                            requested_model,
                            resolved.state.stream_timeout_secs,
                            DeploymentLatencyGuard::from_started(
                                resolved.deployment.clone(),
                                start,
                            ),
                        ),
                        rate_limits,
                        metadata,
                        warnings,
                    }),
                    Err(e) => {
                        record_finish(&resolved.deployment, start);
                        Err(ChatCompletionError::Backend(BackendError::from(e)))
                    }
                }
            }
            BackendClient::Anthropic(_)
            | BackendClient::Bedrock(_)
            | BackendClient::GeminiNative(_) => Err(ChatCompletionError::UnsupportedBackend {
                backend_name: resolved.state.backend_name,
                backend_kind: resolved.state.backend_kind,
            }),
        }
    }
}

fn metadata(requested_model: &str, resolved: &ResolvedBackend) -> ChatCompletionMetadata {
    ChatCompletionMetadata {
        requested_model: requested_model.to_string(),
        selected_backend: resolved.state.backend_name.clone(),
        mapped_model: resolved.mapped_model.clone(),
        backend_kind: resolved.state.backend_kind.clone(),
        provider_id: resolved.state.provider_id.clone(),
        api_format: resolved.state.api_format.clone(),
        used_responses_api: matches!(resolved.state.backend, BackendClient::OpenAIResponses(_)),
    }
}

fn record_start(
    deployment: &Option<Arc<crate::config::model_router::Deployment>>,
) -> Option<Instant> {
    if let Some(d) = deployment {
        d.record_start();
        Some(Instant::now())
    } else {
        None
    }
}

fn record_finish(
    deployment: &Option<Arc<crate::config::model_router::Deployment>>,
    start: Option<Instant>,
) {
    if let (Some(d), Some(start)) = (deployment, start) {
        d.record_finish(start.elapsed().as_millis() as u64);
    }
}

pub mod stream;
use stream::{openai_chunk_stream, responses_chunk_stream, DeploymentLatencyGuard};

fn prepare_openai_request(
    req: &mut openai::ChatCompletionRequest,
    resolved: &ResolvedBackend,
    streaming: bool,
    warnings: &mut TranslationWarnings,
) {
    req.model = resolved.mapped_model.clone();
    req.stream = Some(streaming);

    if streaming {
        if resolved.state.omit_stream_options {
            if req.stream_options.is_some() {
                warnings.add("stream_options");
            }
            req.stream_options = None;
        } else {
            req.stream_options = Some(openai::StreamOptions {
                include_usage: true,
            });
        }
    } else if resolved.state.omit_stream_options {
        if req.stream_options.is_some() {
            warnings.add("stream_options");
        }
        req.stream_options = None;
    }

    if matches!(
        resolved.state.backend,
        BackendClient::GeminiOpenAI(_) | BackendClient::Vertex(_)
    ) {
        sanitize_tools_for_gemini(req);
    }
}

fn sanitize_tools_for_gemini(req: &mut openai::ChatCompletionRequest) {
    if let Some(tools) = req.tools.take() {
        req.tools = Some(
            tools
                .into_iter()
                .map(|mut t| {
                    if let Some(params) = t.function.parameters.take() {
                        t.function.parameters =
                            Some(mapping::tools_map::sanitize_schema_for_gemini(params));
                    }
                    t
                })
                .collect(),
        );
    }
}
