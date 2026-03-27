// Backend fallback chain: when the primary backend returns a retryable error,
// iterate through fallback backends until one succeeds or all are exhausted.

pub mod config;

pub use config::{BackendSpec, FallbackConfig};

use crate::backend::BackendError;

/// Header set when all backends in the fallback chain have failed.
pub const FALLBACK_EXHAUSTED_HEADER: &str = "x-anyllm-fallback-exhausted";

/// A chain of backend specs to try in order on retryable failures.
#[derive(Debug, Clone)]
pub struct FallbackChain {
    pub backends: Vec<BackendSpec>,
}

/// Outcome of a fallback attempt: either a successful result or the last error
/// after exhausting all backends.
#[derive(Debug)]
pub struct FallbackOutcome<T> {
    /// The successful result, if any backend succeeded.
    pub result: Result<T, BackendError>,
    /// Index of the backend that produced the result (0 = primary, 1+ = fallback).
    pub backend_index: usize,
    /// Name of the backend that produced the result.
    pub backend_name: String,
    /// True if all backends were tried and failed.
    pub exhausted: bool,
}

impl FallbackChain {
    /// Create a new fallback chain from a list of backend specs.
    pub fn new(backends: Vec<BackendSpec>) -> Self {
        Self { backends }
    }

    /// Determine whether a failure should trigger fallback to the next backend.
    ///
    /// Retryable: 429 (rate limit), 500, 502, 503, connection errors, timeouts.
    /// Non-retryable: 400, 401, 403, 404, and other 4xx (client errors that won't
    /// resolve by switching backends).
    pub fn should_fallback(status: u16, is_connection_error: bool) -> bool {
        if is_connection_error {
            return true;
        }
        matches!(status, 429 | 500 | 502 | 503)
    }

    /// Execute a fallback chain. Calls `try_backend` for each backend in order,
    /// stopping on the first success or non-retryable error.
    ///
    /// `try_backend` receives the backend spec and index, returns the backend result.
    /// The caller is responsible for constructing the actual backend client from the spec.
    ///
    /// For streaming requests where SSE has already started, callers should NOT use
    /// this method. Mid-stream failures should terminate the SSE with an error event
    /// rather than retrying (the client has already started consuming events).
    pub async fn attempt_with_fallback<T, F, Fut>(&self, mut try_backend: F) -> FallbackOutcome<T>
    where
        F: FnMut(&BackendSpec, usize) -> Fut,
        Fut: std::future::Future<Output = Result<T, BackendError>>,
    {
        let mut last_error: Option<BackendError> = None;
        let mut last_index = 0;
        let mut last_name = String::new();

        for (i, spec) in self.backends.iter().enumerate() {
            last_index = i;
            last_name.clone_from(&spec.name);

            match try_backend(spec, i).await {
                Ok(result) => {
                    if i > 0 {
                        tracing::info!(
                            backend = %spec.name,
                            attempt = i + 1,
                            "fallback backend succeeded"
                        );
                    }
                    return FallbackOutcome {
                        result: Ok(result),
                        backend_index: i,
                        backend_name: spec.name.clone(),
                        exhausted: false,
                    };
                }
                Err(e) => {
                    let status = e.status_code();
                    let is_conn = is_connection_error(&e);

                    tracing::warn!(
                        backend = %spec.name,
                        attempt = i + 1,
                        status = status,
                        is_connection_error = is_conn,
                        error = %e,
                        "backend attempt failed"
                    );

                    // Non-retryable error: stop immediately, don't try other backends.
                    if !Self::should_fallback(status, is_conn) {
                        return FallbackOutcome {
                            result: Err(e),
                            backend_index: i,
                            backend_name: spec.name.clone(),
                            exhausted: false,
                        };
                    }

                    last_error = Some(e);
                }
            }
        }

        // All backends exhausted.
        tracing::error!(
            total_backends = self.backends.len(),
            "all fallback backends exhausted"
        );

        // All backends in the chain returned retryable errors.
        // `last_error` is always Some when backends is non-empty. If the chain
        // was empty (a misconfiguration), we fabricate a descriptive error.
        let final_err = match last_error {
            Some(e) => e,
            None => {
                // Empty chain: no backends to try. Fabricate an error.
                let err_resp = anyllm_translate::openai::errors::ErrorResponse {
                    error: anyllm_translate::openai::errors::ErrorDetail {
                        message: "fallback chain is empty: no backends configured".to_string(),
                        error_type: "configuration_error".to_string(),
                        param: None,
                        code: None,
                    },
                };
                BackendError::OpenAI(crate::backend::openai_client::OpenAIClientError::ApiError {
                    status: 500,
                    error: err_resp,
                })
            }
        };

        FallbackOutcome {
            result: Err(final_err),
            backend_index: last_index,
            backend_name: last_name,
            exhausted: true,
        }
    }
}

/// Check if a `BackendError` represents a connection-level failure (not an HTTP status).
pub fn is_connection_error(err: &BackendError) -> bool {
    match err {
        BackendError::OpenAI(crate::backend::openai_client::OpenAIClientError::Request(e)) => {
            e.is_connect() || e.is_timeout()
        }
        BackendError::Anthropic(
            crate::backend::anthropic_client::AnthropicClientError::Transport(_),
        ) => true,
        BackendError::Bedrock(crate::backend::bedrock_client::BedrockClientError::Transport(_)) => {
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_fallback_retryable_statuses() {
        assert!(FallbackChain::should_fallback(500, false));
        assert!(FallbackChain::should_fallback(502, false));
        assert!(FallbackChain::should_fallback(503, false));
        assert!(FallbackChain::should_fallback(429, false));
    }

    #[test]
    fn should_fallback_connection_error() {
        // Connection errors should always fallback, regardless of status.
        assert!(FallbackChain::should_fallback(0, true));
        assert!(FallbackChain::should_fallback(200, true));
    }

    #[test]
    fn should_not_fallback_client_errors() {
        assert!(!FallbackChain::should_fallback(400, false));
        assert!(!FallbackChain::should_fallback(401, false));
        assert!(!FallbackChain::should_fallback(403, false));
        assert!(!FallbackChain::should_fallback(404, false));
        assert!(!FallbackChain::should_fallback(422, false));
    }

    #[test]
    fn should_not_fallback_success() {
        assert!(!FallbackChain::should_fallback(200, false));
        assert!(!FallbackChain::should_fallback(201, false));
    }

    #[tokio::test]
    async fn fallback_succeeds_on_second_backend() {
        let chain = FallbackChain::new(vec![
            BackendSpec {
                name: "primary".to_string(),
                env_prefix: "PRIMARY_".to_string(),
            },
            BackendSpec {
                name: "secondary".to_string(),
                env_prefix: "SECONDARY_".to_string(),
            },
        ]);

        let outcome = chain
            .attempt_with_fallback(|spec, _idx| {
                let name = spec.name.clone();
                async move {
                    if name == "primary" {
                        Err(make_api_error(503))
                    } else {
                        Ok("success from secondary")
                    }
                }
            })
            .await;

        assert!(outcome.result.is_ok());
        assert_eq!(outcome.backend_index, 1);
        assert_eq!(outcome.backend_name, "secondary");
        assert!(!outcome.exhausted);
    }

    #[tokio::test]
    async fn fallback_stops_on_non_retryable() {
        let chain = FallbackChain::new(vec![
            BackendSpec {
                name: "primary".to_string(),
                env_prefix: "PRIMARY_".to_string(),
            },
            BackendSpec {
                name: "secondary".to_string(),
                env_prefix: "SECONDARY_".to_string(),
            },
        ]);

        let outcome: FallbackOutcome<&str> = chain
            .attempt_with_fallback(|spec, _idx| {
                let name = spec.name.clone();
                async move {
                    if name == "primary" {
                        Err(make_api_error(400))
                    } else {
                        Ok("should not reach here")
                    }
                }
            })
            .await;

        assert!(outcome.result.is_err());
        assert_eq!(outcome.backend_index, 0);
        assert_eq!(outcome.backend_name, "primary");
        assert!(!outcome.exhausted);
    }

    #[tokio::test]
    async fn fallback_all_exhausted() {
        let chain = FallbackChain::new(vec![
            BackendSpec {
                name: "primary".to_string(),
                env_prefix: "PRIMARY_".to_string(),
            },
            BackendSpec {
                name: "secondary".to_string(),
                env_prefix: "SECONDARY_".to_string(),
            },
        ]);

        let outcome: FallbackOutcome<&str> = chain
            .attempt_with_fallback(|_spec, _idx| async move {
                // Both backends return 503.
                Err(make_api_error(503))
            })
            .await;

        assert!(outcome.result.is_err());
        assert_eq!(outcome.backend_index, 1);
        assert!(outcome.exhausted);
    }

    #[tokio::test]
    async fn fallback_first_succeeds() {
        let chain = FallbackChain::new(vec![
            BackendSpec {
                name: "primary".to_string(),
                env_prefix: "PRIMARY_".to_string(),
            },
            BackendSpec {
                name: "secondary".to_string(),
                env_prefix: "SECONDARY_".to_string(),
            },
        ]);

        let outcome = chain
            .attempt_with_fallback(|_spec, _idx| async move { Ok("immediate success") })
            .await;

        assert!(outcome.result.is_ok());
        assert_eq!(outcome.backend_index, 0);
        assert_eq!(outcome.backend_name, "primary");
        assert!(!outcome.exhausted);
    }

    /// Helper: create a `BackendError::OpenAI(ApiError { .. })` with a given status code.
    fn make_api_error(status: u16) -> BackendError {
        use anyllm_translate::openai::errors::{ErrorDetail, ErrorResponse};
        BackendError::OpenAI(crate::backend::openai_client::OpenAIClientError::ApiError {
            status,
            error: ErrorResponse {
                error: ErrorDetail {
                    message: format!("mock error {status}"),
                    error_type: "test".to_string(),
                    param: None,
                    code: None,
                },
            },
        })
    }
}
