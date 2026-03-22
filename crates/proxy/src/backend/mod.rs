pub mod openai_client;

use crate::config::{BackendKind, Config};
use anthropic_openai_translate::openai;
use openai_client::{OpenAIClient, OpenAIClientError};

/// Backend-agnostic client for dispatching chat completion requests.
/// Both variants use OpenAIClient today; native Gemini would add a third variant.
#[derive(Clone)]
pub enum BackendClient {
    OpenAI(OpenAIClient),
    Vertex(OpenAIClient),
}

/// Re-export for cleaner API at call sites.
pub type BackendError = OpenAIClientError;

impl BackendClient {
    pub fn new(config: &Config) -> Self {
        let client = OpenAIClient::new(config);
        match config.backend {
            BackendKind::OpenAI => Self::OpenAI(client),
            BackendKind::Vertex => Self::Vertex(client),
        }
    }

    fn inner(&self) -> &OpenAIClient {
        match self {
            Self::OpenAI(c) | Self::Vertex(c) => c,
        }
    }

    pub async fn chat_completion(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<(openai::ChatCompletionResponse, u16), BackendError> {
        self.inner().chat_completion(req).await
    }

    pub async fn chat_completion_stream(
        &self,
        req: &openai::ChatCompletionRequest,
    ) -> Result<reqwest::Response, BackendError> {
        self.inner().chat_completion_stream(req).await
    }
}
