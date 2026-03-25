//! Axum middleware for adding Anthropic Messages API compatibility to existing services.
//!
//! Requires the `middleware` feature: `anyllm_translate = { features = ["middleware"] }`
//!
//! # Usage
//!
//! ```rust,no_run
//! use anyllm_translate::TranslationConfig;
//! use anyllm_translate::middleware::{
//!     AnthropicCompatConfig, AnthropicTranslationLayer, anthropic_compat_router,
//! };
//! use axum::Router;
//!
//! let config = AnthropicCompatConfig::builder()
//!     .backend_url("https://api.openai.com")
//!     .api_key("sk-...")
//!     .translation(
//!         TranslationConfig::builder()
//!             .model_map("haiku", "gpt-4o-mini")
//!             .model_map("sonnet", "gpt-4o")
//!             .build()
//!     )
//!     .build();
//!
//! // Option A: Router factory -- adds POST /v1/messages
//! let app: Router = Router::new()
//!     .merge(anthropic_compat_router(config.clone()));
//!
//! // Option B: Tower Layer -- intercepts POST /v1/messages, passes other requests through
//! let app: Router = Router::new()
//!     .layer(AnthropicTranslationLayer::new(config));
//! ```

mod client;
mod handler;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::Json;
use axum::http::{Method, Request};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use tower::{Layer, Service};

pub use client::ForwardingError;
pub use handler::stream_event_to_sse;

use crate::config::TranslationConfig;

// --- Configuration ---

/// Configuration for the Anthropic compatibility middleware.
#[derive(Clone, Debug)]
pub struct AnthropicCompatConfig {
    /// Base URL of the OpenAI-compatible backend (e.g., `https://api.openai.com`).
    pub backend_url: String,
    /// API key for the backend (sent as Bearer token).
    pub api_key: String,
    /// Translation settings (model mapping, lossy behavior).
    pub translation: TranslationConfig,
}

impl AnthropicCompatConfig {
    /// Create a builder for configuring the middleware.
    pub fn builder() -> AnthropicCompatConfigBuilder {
        AnthropicCompatConfigBuilder {
            backend_url: String::new(),
            api_key: String::new(),
            translation: TranslationConfig::default(),
        }
    }
}

/// Builder for [`AnthropicCompatConfig`].
pub struct AnthropicCompatConfigBuilder {
    backend_url: String,
    api_key: String,
    translation: TranslationConfig,
}

impl AnthropicCompatConfigBuilder {
    /// Set the base URL of the OpenAI-compatible backend (e.g., `https://api.openai.com`).
    pub fn backend_url(mut self, url: impl Into<String>) -> Self {
        self.backend_url = url.into();
        self
    }

    /// Set the API key sent as a Bearer token to the backend.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = key.into();
        self
    }

    /// Set translation settings (model mapping, lossy behavior).
    pub fn translation(mut self, config: TranslationConfig) -> Self {
        self.translation = config;
        self
    }

    /// Build the configuration. Does not validate; invalid URLs will fail at request time.
    pub fn build(self) -> AnthropicCompatConfig {
        AnthropicCompatConfig {
            backend_url: self.backend_url,
            api_key: self.api_key,
            translation: self.translation,
        }
    }
}

// --- Shared state ---

/// Internal shared state for the middleware handler.
pub(crate) struct MiddlewareState {
    pub(crate) config: AnthropicCompatConfig,
    pub(crate) client: client::ForwardingClient,
}

fn make_state(config: AnthropicCompatConfig) -> Arc<MiddlewareState> {
    let client = client::ForwardingClient::new(&config.backend_url, &config.api_key);
    Arc::new(MiddlewareState { config, client })
}

// --- Router factory ---

/// Create an axum [`Router`] that handles `POST /v1/messages`.
///
/// Merge this into your existing router to add Anthropic Messages API compatibility.
/// Incoming Anthropic requests are translated to OpenAI Chat Completions format,
/// forwarded to the configured backend URL, and the response is translated back.
pub fn anthropic_compat_router(config: AnthropicCompatConfig) -> Router {
    let state = make_state(config);

    Router::new().route(
        "/v1/messages",
        post(
            move |Json(body): Json<crate::anthropic::MessageCreateRequest>| {
                let state = Arc::clone(&state);
                async move { handler::handle_messages(state, body).await }
            },
        ),
    )
}

// --- Tower Layer ---

/// Tower [`Layer`] that intercepts `POST /v1/messages` requests and translates them.
///
/// Other requests pass through to the inner service unchanged.
#[derive(Clone)]
pub struct AnthropicTranslationLayer {
    state: Arc<MiddlewareState>,
}

impl AnthropicTranslationLayer {
    /// Create a new layer that will intercept `POST /v1/messages` and translate.
    pub fn new(config: AnthropicCompatConfig) -> Self {
        Self {
            state: make_state(config),
        }
    }
}

impl<S> Layer<S> for AnthropicTranslationLayer {
    type Service = AnthropicTranslationService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AnthropicTranslationService {
            inner,
            state: Arc::clone(&self.state),
        }
    }
}

/// Tower [`Service`] created by [`AnthropicTranslationLayer`].
///
/// Intercepts `POST /v1/messages` and handles translation.
/// All other requests are forwarded to the inner service.
#[derive(Clone)]
pub struct AnthropicTranslationService<S> {
    inner: S,
    state: Arc<MiddlewareState>,
}

impl<S> Service<Request<Body>> for AnthropicTranslationService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        // Only intercept POST /v1/messages
        if req.method() == Method::POST && req.uri().path() == "/v1/messages" {
            let state = Arc::clone(&self.state);
            Box::pin(async move {
                // Read the full body
                let body_bytes =
                    match axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024).await {
                        Ok(b) => b,
                        Err(_) => {
                            let err = crate::mapping::errors_map::create_anthropic_error(
                                crate::anthropic::ErrorType::InvalidRequestError,
                                "Request body too large".to_string(),
                                None,
                            );
                            return Ok((axum::http::StatusCode::PAYLOAD_TOO_LARGE, Json(err))
                                .into_response());
                        }
                    };

                let anthropic_req: crate::anthropic::MessageCreateRequest =
                    match serde_json::from_slice(&body_bytes) {
                        Ok(r) => r,
                        Err(e) => {
                            let err = crate::mapping::errors_map::create_anthropic_error(
                                crate::anthropic::ErrorType::InvalidRequestError,
                                format!("Invalid JSON: {e}"),
                                None,
                            );
                            return Ok(
                                (axum::http::StatusCode::BAD_REQUEST, Json(err)).into_response()
                            );
                        }
                    };

                Ok(handler::handle_messages(state, anthropic_req).await)
            })
        } else {
            // Pass through to inner service
            let fut = self.inner.call(req);
            Box::pin(fut)
        }
    }
}
