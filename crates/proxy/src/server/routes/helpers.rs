use crate::admin::state::{AdminEvent, RequestLogEntry, SharedState};
use crate::backend::BackendError;
use crate::cache::{CacheBackend, CacheEntry};
use anyllm_translate::{anthropic, mapping};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Convert a BackendError into an Anthropic error Response.
pub fn backend_error_to_response(error: BackendError) -> Response {
    if let Some((message, status)) = error.api_error_details() {
        let anthropic_err = mapping::errors_map::status_to_anthropic_error(status, &message, None);
        let http_status = StatusCode::from_u16(
            mapping::errors_map::anthropic_error_type_to_status(&anthropic_err.error.error_type),
        )
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return (http_status, Json(anthropic_err)).into_response();
    }

    // Transport or deserialization error -- log details server-side only,
    // return a generic message to avoid leaking infrastructure details.
    tracing::error!("backend client error: {error}");
    let err = mapping::errors_map::create_anthropic_error(
        anthropic::ErrorType::ApiError,
        "An internal error occurred while communicating with the upstream service.".to_string(),
        None,
    );
    (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
}

/// Return the appropriate `x-anyllm-cache` header value.
pub fn cache_header_value(bypass: bool) -> axum::http::HeaderValue {
    if bypass {
        axum::http::HeaderValue::from_static("bypass")
    } else {
        axum::http::HeaderValue::from_static("miss")
    }
}

pub fn cache_auth_identity(
    headers: &axum::http::HeaderMap,
    vk_ctx: &Option<super::super::middleware::VirtualKeyContext>,
) -> String {
    if let Some(ctx) = vk_ctx {
        return format!("virtual-key:{}", ctx.key_id);
    }

    let credential = headers
        .get("x-api-key")
        .or_else(|| headers.get("x-goog-api-key"))
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        });

    match credential {
        Some(value) => {
            let digest = Sha256::digest(value.as_bytes());
            format!("credential:{}", hex::encode(digest))
        }
        None => "anonymous".to_string(),
    }
}

/// Store a serializable response in the cache if caching is enabled.
pub async fn try_cache_response<T: serde::Serialize>(
    cache_key: &Option<String>,
    cache: &Option<Arc<crate::cache::memory::MemoryCache>>,
    cache_ttl: Option<u64>,
    response: &T,
    model: String,
) {
    if let (Some(ref key), Some(ref c)) = (cache_key, cache) {
        if let Ok(resp_body) = serde_json::to_vec(response).map(bytes::Bytes::from) {
            let ttl = cache_ttl.unwrap_or(c.default_ttl_secs);
            c.put(
                key,
                CacheEntry {
                    response_body: resp_body,
                    model,
                    created_at: std::time::Instant::now(),
                    ttl_secs: cache_ttl,
                },
                ttl,
            )
            .await;
        }
    }
}

/// Inject degradation warnings as `x-anyllm-degradation` header if any features were dropped.
pub fn inject_degradation_header(
    headers: &mut axum::http::HeaderMap,
    warnings: &anyllm_translate::TranslationWarnings,
) {
    if let Some(val) = warnings.as_header_value() {
        if let Ok(hv) = axum::http::HeaderValue::from_str(&val) {
            headers.insert("x-anyllm-degradation", hv);
        }
    }
}

pub fn enforce_model_allowlist_from_json_body(
    vk_ctx: Option<&axum::Extension<super::super::middleware::VirtualKeyContext>>,
    body: &[u8],
) -> Option<Response> {
    let ctx = vk_ctx.map(|axum::Extension(c)| c)?;
    let model = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.as_object()
                .and_then(|obj| obj.get("model"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        });

    match model {
        Some(model) if !super::super::policy::is_model_allowed(&model, &ctx.allowed_models) => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::PermissionError,
                format!("Model '{}' is not allowed for this API key.", model),
                None,
            );
            return Some((StatusCode::FORBIDDEN, Json(err)).into_response());
        }
        Some(_) => {}
        None if ctx.allowed_models.is_some() => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::InvalidRequestError,
                "Request must include a 'model' field when a model allowlist is configured."
                    .to_string(),
                None,
            );
            return Some((StatusCode::BAD_REQUEST, Json(err)).into_response());
        }
        None => {}
    }

    None
}

/// Record output tokens against the virtual key's TPM sliding window.
/// Called after the backend response is received and token count is known.
pub fn record_vk_tpm(
    vk_ctx: &Option<super::super::middleware::VirtualKeyContext>,
    output_tokens: u32,
) {
    if let Some(ctx) = vk_ctx {
        let now_ms = crate::admin::keys::now_ms();
        ctx.rate_state.record_tpm(now_ms, output_tokens);

        #[cfg(feature = "redis")]
        if let Some(redis_limiter) = crate::ratelimit::get_redis_rate_limiter() {
            let key_hash_hex = ctx.key_hash_hex.clone();
            tokio::spawn(async move {
                redis_limiter
                    .record_tpm(&key_hash_hex, now_ms, output_tokens)
                    .await;
            });
        }
    }
}

/// Record all post-response virtual-key usage controls for known token usage.
///
/// The protected invariant is that output tokens from successful generation
/// requests contribute to the virtual key TPM window before the request is
/// logged or returned as fully accounted.
pub fn record_virtual_key_usage(
    shared: &Option<SharedState>,
    vk_ctx: &Option<super::super::middleware::VirtualKeyContext>,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> f64 {
    let capped_output = output_tokens.min(u32::MAX as u64) as u32;
    record_vk_tpm(vk_ctx, capped_output);
    crate::cost::record_cost(shared, vk_ctx, model, input_tokens, output_tokens)
}

/// Global webhook callback config, set once at startup.
static CALLBACKS: std::sync::OnceLock<Arc<crate::callbacks::CallbackConfig>> =
    std::sync::OnceLock::new();

/// Set the global webhook callback config (called once at startup).
pub fn set_callbacks(config: Arc<crate::callbacks::CallbackConfig>) {
    let _ = CALLBACKS.set(config);
}

/// Get a reference to the global webhook callback config, if set.
pub fn get_callbacks() -> Option<&'static Arc<crate::callbacks::CallbackConfig>> {
    CALLBACKS.get()
}

/// Log a completed request to the admin write buffer, broadcast to WebSocket clients,
/// and fire webhook callbacks if configured.
pub fn log_request(shared: &Option<SharedState>, entry: RequestLogEntry) {
    if let Some(cb) = CALLBACKS.get() {
        cb.notify(&entry);
    }
    if let Some(ref shared) = shared {
        let _ = shared
            .events_tx
            .send(AdminEvent::RequestCompleted(entry.clone()));
        let _ = shared.log_tx.try_send(entry);
    }
}

/// Populate `entry.error_kind` from a `BackendError` for operator-visible failure classification.
pub fn set_backend_error_kind(entry: &mut RequestLogEntry, error: &BackendError) {
    entry.error_kind = Some(error.error_kind().to_string());
}
