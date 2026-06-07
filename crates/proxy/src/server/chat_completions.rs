// OpenAI Chat Completions input handler.
//
// Accepts POST /v1/chat/completions in OpenAI format, translates through
// the Anthropic pipeline, returns OpenAI-format responses.

use crate::backend::{find_double_newline, BackendClient, BackendError, MAX_SSE_BUFFER_SIZE};
use crate::cache::{self, CacheBackend, CacheNamespace};
use anyllm_translate::{
    anthropic, mapping, openai, translate_anthropic_to_openai_response,
    translate_openai_to_anthropic_request, ReverseStreamingTranslator, TranslationWarnings,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use bytes::BytesMut;
use futures::StreamExt;

use super::routes::{
    cache_auth_identity, inject_degradation_header, log_request, set_backend_error_kind, RequestCtx,
};
use super::state::{AppState, ConcurrencyPermit};
use super::streaming::{AnthropicStreamUsage, StreamOutcome};

struct ChatCompletionsStreamMeta {
    ctx: RequestCtx,
    original_model: String,
    mapped_model: String,
    warnings: TranslationWarnings,
    safe_headers: Vec<(String, String)>,
    concurrency_permit: Option<ConcurrencyPermit>,
    vk_ctx: Option<crate::server::middleware::VirtualKeyContext>,
}

/// OpenAI-shaped error response body.
fn openai_error_response(message: &str, error_type: &str, status: StatusCode) -> Response {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": error_type,
            "param": null,
            "code": null
        }
    });
    (status, Json(body)).into_response()
}

/// Convert a BackendError into an OpenAI-shaped error response.
fn backend_error_to_openai_response(error: BackendError) -> Response {
    if let Some((message, status)) = error.api_error_details() {
        let error_type = if status == 429 {
            "rate_limit_error"
        } else if status >= 500 {
            "server_error"
        } else {
            "invalid_request_error"
        };
        let http_status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return openai_error_response(&message, error_type, http_status);
    }
    tracing::error!("backend client error: {error}");
    openai_error_response(
        "An internal error occurred while communicating with the upstream service.",
        "server_error",
        StatusCode::INTERNAL_SERVER_ERROR,
    )
}

fn safe_anthropic_extra_headers(headers: &axum::http::HeaderMap) -> Vec<(String, String)> {
    ["x-claude-code-session-id", "anthropic-beta"]
        .iter()
        .filter_map(|&name| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|v| (name.to_string(), v.to_string()))
        })
        .collect()
}

fn cache_key_body_for_chat_completions(
    body: &openai::ChatCompletionRequest,
    safe_headers: &[(String, String)],
) -> serde_json::Value {
    let mut value = serde_json::to_value(body).unwrap_or_default();
    let Some(obj) = value.as_object_mut() else {
        return value;
    };

    if let Some(v) = body.extra.get("reasoning_effort") {
        obj.insert("reasoning_effort".to_string(), v.clone());
    }
    if let Some(v) = &body.response_format {
        if let Ok(v) = serde_json::to_value(v) {
            obj.insert("response_format".to_string(), v);
        }
    }
    for (name, val) in safe_headers {
        obj.insert(
            format!("_header_{}", name.to_ascii_lowercase()),
            val.clone().into(),
        );
    }
    value
}

fn header_refs(headers: &[(String, String)]) -> Vec<(&str, &str)> {
    headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

fn is_anthropic_backend(state: &AppState) -> bool {
    matches!(state.backend, BackendClient::Anthropic(_))
}

fn mapped_model_for_backend(
    original_model: &str,
    mapped_model: String,
    effective: &AppState,
) -> String {
    if is_anthropic_backend(effective) && mapped_model.is_empty() {
        original_model.to_string()
    } else {
        mapped_model
    }
}

fn apply_anthropic_chat_extensions(
    openai_req: &openai::ChatCompletionRequest,
    anthropic_req: &mut anthropic::MessageCreateRequest,
    warnings: &mut TranslationWarnings,
) {
    let mut output_config = anthropic_req
        .extra
        .remove("output_config")
        .unwrap_or_else(|| serde_json::json!({}));
    if !output_config.is_object() {
        output_config = serde_json::json!({});
    }

    if let Some(effort) = openai_req
        .extra
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
    {
        output_config["effort"] = serde_json::Value::String(effort.to_string());
    }

    if let Some(response_format) = &openai_req.response_format {
        if response_format.format_type == "json_schema" {
            if let Some(json_schema) = &response_format.json_schema {
                let schema = json_schema
                    .get("schema")
                    .cloned()
                    .unwrap_or_else(|| json_schema.clone());
                output_config["format"] = serde_json::json!({
                    "type": "json_schema",
                    "schema": schema,
                });
                warnings.remove("response_format");
            }
        }
    }

    if output_config
        .as_object()
        .map(|o| !o.is_empty())
        .unwrap_or(false)
    {
        anthropic_req
            .extra
            .insert("output_config".to_string(), output_config);
    }
}

/// Handler for POST /v1/chat/completions (non-streaming and streaming).
pub(crate) async fn chat_completions(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    permit: Option<axum::Extension<ConcurrencyPermit>>,
    vk_ctx: Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>,
    body: Result<Json<openai::ChatCompletionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let vk_ctx = vk_ctx.map(|axum::Extension(c)| c);
    let body = match body {
        Ok(Json(b)) => b,
        Err(e) => {
            return openai_error_response(
                &e.body_text(),
                "invalid_request_error",
                StatusCode::BAD_REQUEST,
            );
        }
    };

    let permit = permit.map(|axum::Extension(p)| p);
    let ctx = RequestCtx {
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string(),
        start: std::time::Instant::now(),
        model_requested: body.model.clone(),
    };
    state.metrics.record_request();

    // Enforce model allowlist policy for virtual keys.
    if let Some(ref ctx) = vk_ctx {
        if !crate::server::policy::is_model_allowed(&body.model, &ctx.allowed_models) {
            return openai_error_response(
                &format!("Model '{}' is not allowed for this API key.", body.model),
                "permission_error",
                axum::http::StatusCode::FORBIDDEN,
            );
        }
    }

    let is_streaming = body.stream == Some(true);
    let original_model = body.model.clone();
    let safe_headers = safe_anthropic_extra_headers(&headers);

    // Resolve model routing before translation so Anthropic-targeted requests
    // can apply LiteLLM's Anthropic defaults without changing other backends.
    let (mapped_model, effective, deployment) = match state.resolve_model_and_state(&original_model)
    {
        Ok((mapped, effective, deployment)) => (
            mapped_model_for_backend(&original_model, mapped, &effective),
            effective,
            deployment,
        ),
        Err(resp) => return resp,
    };
    if let Some(ref ctx) = vk_ctx {
        if let Err(error) = crate::server::policy::enforce_route_scope(
            &effective.backend_name,
            &effective.shared,
            &ctx.allowed_routes,
        )
        .await
        {
            return openai_error_response(
                error.message(),
                "permission_error",
                StatusCode::FORBIDDEN,
            );
        }
    }

    let mut translated_body = body.clone();
    if is_anthropic_backend(&effective)
        && translated_body.max_tokens.is_none()
        && translated_body.max_completion_tokens.is_none()
    {
        translated_body.max_tokens = Some(4096);
    }

    // Translate OpenAI request -> Anthropic request.
    let mut warnings = TranslationWarnings::default();
    let mut anthropic_req =
        match translate_openai_to_anthropic_request(&translated_body, &mut warnings) {
            Ok(req) => req,
            Err(e) => {
                return openai_error_response(
                    &e.to_string(),
                    "invalid_request_error",
                    StatusCode::BAD_REQUEST,
                );
            }
        };
    if is_anthropic_backend(&effective) {
        apply_anthropic_chat_extensions(&body, &mut anthropic_req, &mut warnings);
    }

    if anthropic_req.messages.is_empty() {
        return openai_error_response(
            "messages array must not be empty",
            "invalid_request_error",
            StatusCode::BAD_REQUEST,
        );
    }

    if is_streaming {
        let stream_meta = ChatCompletionsStreamMeta {
            ctx,
            original_model,
            mapped_model,
            warnings,
            safe_headers,
            concurrency_permit: permit,
            vk_ctx,
        };
        let mut response = chat_completions_stream(effective, anthropic_req, stream_meta).await;
        response.headers_mut().insert(
            "x-anyllm-cache",
            axum::http::HeaderValue::from_static("bypass"),
        );
        return response;
    }

    // Non-streaming path: check cache before calling backend.
    let body_value = cache_key_body_for_chat_completions(&body, &safe_headers);
    let cache_ttl = match cache::parse_cache_ttl(&body_value) {
        Ok(ttl) => ttl,
        Err(msg) => {
            return openai_error_response(&msg, "invalid_request_error", StatusCode::BAD_REQUEST);
        }
    };
    let bypass_cache = cache_ttl == Some(0);

    let auth_identity = cache_auth_identity(&headers, &vk_ctx);
    let cache_key = if !bypass_cache {
        Some(cache::cache_key_for_request(
            &body_value,
            CacheNamespace::OpenAI,
            &cache::CacheScope {
                backend_name: &effective.backend_name,
                auth_identity: &auth_identity,
            },
        ))
    } else {
        None
    };

    // Check cache on non-bypass requests
    if let (Some(ref key), Some(ref c)) = (&cache_key, &state.cache) {
        if let Some(entry) = c.get(key).await {
            tracing::debug!(cache_key = %key, "cache hit for /v1/chat/completions");
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("x-anyllm-cache", "hit")
                .body(axum::body::Body::from(entry.response_body))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            if state.expose_degradation_warnings {
                inject_degradation_header(response.headers_mut(), &warnings);
            }
            return response;
        }
    }

    if let Some(ref d) = deployment {
        d.record_start();
    }
    let backend_start = std::time::Instant::now();

    // Non-streaming path
    match &effective.backend {
        BackendClient::OpenAI(client)
        | BackendClient::AzureOpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&anthropic_req);
            super::routes::inject_gemini_thinking(
                &anthropic_req,
                &effective.backend,
                &mut openai_req,
            );
            super::routes::inject_glm_thinking(&anthropic_req, &effective.backend, &mut openai_req);
            // Gemini/Vertex rejects standard JSON Schema keywords; sanitize tool schemas.
            if matches!(
                effective.backend,
                BackendClient::GeminiOpenAI(_) | BackendClient::Vertex(_)
            ) {
                if let Some(tools) = openai_req.tools.take() {
                    openai_req.tools = Some(
                        tools
                            .into_iter()
                            .map(|mut t| {
                                if let Some(params) = t.function.parameters.take() {
                                    t.function.parameters = Some(
                                        mapping::tools_map::sanitize_schema_for_gemini(params),
                                    );
                                }
                                t
                            })
                            .collect(),
                    );
                }
            }
            if effective.omit_stream_options {
                openai_req.stream_options = None;
            }
            openai_req.model = mapped_model.clone();
            let mapped_model = openai_req.model.clone();

            match client.chat_completion(&openai_req).await {
                Ok((openai_resp, _status, rate_limits)) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_success();
                    // Translate Anthropic response back to OpenAI format
                    let anthropic_resp = mapping::message_map::openai_to_anthropic_response(
                        &openai_resp,
                        &original_model,
                    );

                    // Tool execution: bounded loop with termination guards.
                    let anthropic_resp = if let Some(ref engine) = state.tool_engine {
                        let client_for_tools = client.clone();
                        let model_for_tools = mapped_model.clone();
                        let orig_model_for_tools = original_model.clone();
                        let server_advertised_tool_names = std::collections::HashSet::new();
                        let (resp, _trace) = crate::tools::execution::maybe_execute_tools(
                            engine,
                            &anthropic_req,
                            &server_advertised_tool_names,
                            anthropic_resp,
                            |follow_up_req| {
                                let c = client_for_tools.clone();
                                let m = model_for_tools.clone();
                                let om = orig_model_for_tools.clone();
                                async move {
                                    let mut oai_req =
                                        mapping::message_map::anthropic_to_openai_request(
                                            &follow_up_req,
                                        );
                                    oai_req.model = m;
                                    match c.chat_completion(&oai_req).await {
                                        Ok((resp, _, _)) => {
                                            Ok(mapping::message_map::openai_to_anthropic_response(
                                                &resp, &om,
                                            ))
                                        }
                                        Err(e) => Err(format!("{e}")),
                                    }
                                }
                            },
                        )
                        .await;
                        resp
                    } else {
                        anthropic_resp
                    };

                    let oai_response =
                        translate_anthropic_to_openai_response(&anthropic_resp, &original_model);
                    let cost = super::routes::record_virtual_key_usage(
                        &state.shared,
                        &vk_ctx,
                        &mapped_model,
                        anthropic_resp.usage.input_tokens as u64,
                        anthropic_resp.usage.output_tokens as u64,
                    );
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                            &vk_ctx,
                            Some(cost),
                        ),
                    );
                    super::routes::try_cache_response(
                        &cache_key,
                        &state.cache,
                        cache_ttl,
                        &oai_response,
                        original_model.clone(),
                    )
                    .await;

                    let cache_hv = super::routes::cache_header_value(bypass_cache);
                    let mut response = (StatusCode::OK, Json(oai_response)).into_response();
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
                    if state.expose_degradation_warnings {
                        inject_degradation_header(response.headers_mut(), &warnings);
                    }
                    response.headers_mut().insert("x-anyllm-cache", cache_hv);
                    response
                }
                Err(e) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_error();
                    let backend_error = BackendError::from(e);
                    let mut entry = ctx.log_entry_with_attribution(
                        &state.backend_name,
                        Some(mapped_model),
                        backend_error.status_code(),
                        None,
                        false,
                        Some(backend_error.to_string()),
                        &vk_ctx,
                        None,
                    );
                    set_backend_error_kind(&mut entry, &backend_error);
                    log_request(&state.shared, entry);
                    backend_error_to_openai_response(backend_error)
                }
            }
        }
        BackendClient::OpenAIResponses(client) => {
            let mut responses_req =
                mapping::responses_message_map::anthropic_to_responses_request(&anthropic_req);
            responses_req.model = mapped_model.clone();
            let mapped_model = responses_req.model.clone();

            match client.responses(&responses_req).await {
                Ok((resp, _status, rate_limits)) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_success();
                    let anthropic_resp =
                        mapping::responses_message_map::responses_to_anthropic_response(
                            &resp,
                            &original_model,
                        );
                    let oai_response =
                        translate_anthropic_to_openai_response(&anthropic_resp, &original_model);
                    let cost = super::routes::record_virtual_key_usage(
                        &state.shared,
                        &vk_ctx,
                        &mapped_model,
                        anthropic_resp.usage.input_tokens as u64,
                        anthropic_resp.usage.output_tokens as u64,
                    );
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                            &vk_ctx,
                            Some(cost),
                        ),
                    );

                    super::routes::try_cache_response(
                        &cache_key,
                        &state.cache,
                        cache_ttl,
                        &oai_response,
                        original_model.clone(),
                    )
                    .await;

                    let cache_hv = super::routes::cache_header_value(bypass_cache);
                    let mut response = (StatusCode::OK, Json(oai_response)).into_response();
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
                    if state.expose_degradation_warnings {
                        inject_degradation_header(response.headers_mut(), &warnings);
                    }
                    response.headers_mut().insert("x-anyllm-cache", cache_hv);
                    response
                }
                Err(e) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_error();
                    let backend_error = BackendError::from(e);
                    let mut entry = ctx.log_entry_with_attribution(
                        &state.backend_name,
                        Some(mapped_model),
                        backend_error.status_code(),
                        None,
                        false,
                        Some(backend_error.to_string()),
                        &vk_ctx,
                        None,
                    );
                    set_backend_error_kind(&mut entry, &backend_error);
                    log_request(&state.shared, entry);
                    backend_error_to_openai_response(backend_error)
                }
            }
        }
        BackendClient::Anthropic(client) => {
            let mut upstream_req = anthropic_req.clone();
            upstream_req.model = mapped_model.clone();
            upstream_req.stream = Some(false);
            let body = match serde_json::to_vec(&upstream_req) {
                Ok(body) => bytes::Bytes::from(body),
                Err(e) => {
                    return openai_error_response(
                        &format!("failed to serialize Anthropic request: {e}"),
                        "server_error",
                        StatusCode::INTERNAL_SERVER_ERROR,
                    );
                }
            };
            let refs = header_refs(&safe_headers);

            match client.forward(body, &refs).await {
                Ok((resp_body, rate_limits)) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    let anthropic_resp =
                        match serde_json::from_slice::<anthropic::MessageResponse>(&resp_body) {
                            Ok(resp) => resp,
                            Err(e) => {
                                state.metrics.record_error();
                                log_request(
                                    &state.shared,
                                    ctx.log_entry_with_attribution(
                                        &state.backend_name,
                                        Some(mapped_model.clone()),
                                        StatusCode::BAD_GATEWAY.as_u16(),
                                        None,
                                        false,
                                        Some(format!(
                                            "failed to parse Anthropic upstream response: {e}"
                                        )),
                                        &vk_ctx,
                                        None,
                                    ),
                                );
                                return openai_error_response(
                                    "Upstream Anthropic response could not be parsed.",
                                    "server_error",
                                    StatusCode::BAD_GATEWAY,
                                );
                            }
                        };
                    state.metrics.record_success();
                    let oai_response =
                        translate_anthropic_to_openai_response(&anthropic_resp, &original_model);
                    let cost = super::routes::record_virtual_key_usage(
                        &state.shared,
                        &vk_ctx,
                        &mapped_model,
                        anthropic_resp.usage.input_tokens as u64,
                        anthropic_resp.usage.output_tokens as u64,
                    );
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(mapped_model.clone()),
                            200,
                            Some((
                                anthropic_resp.usage.input_tokens as u64,
                                anthropic_resp.usage.output_tokens as u64,
                            )),
                            false,
                            None,
                            &vk_ctx,
                            Some(cost),
                        ),
                    );

                    super::routes::try_cache_response(
                        &cache_key,
                        &state.cache,
                        cache_ttl,
                        &oai_response,
                        original_model.clone(),
                    )
                    .await;

                    let cache_hv = super::routes::cache_header_value(bypass_cache);
                    let mut response = (StatusCode::OK, Json(oai_response)).into_response();
                    rate_limits.inject_anthropic_response_headers(response.headers_mut());
                    if state.expose_degradation_warnings {
                        inject_degradation_header(response.headers_mut(), &warnings);
                    }
                    response.headers_mut().insert("x-anyllm-cache", cache_hv);
                    response
                }
                Err(e) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_error();
                    let backend_error = BackendError::from(e);
                    let mut entry = ctx.log_entry_with_attribution(
                        &state.backend_name,
                        Some(mapped_model),
                        backend_error.status_code(),
                        None,
                        false,
                        Some(backend_error.to_string()),
                        &vk_ctx,
                        None,
                    );
                    set_backend_error_kind(&mut entry, &backend_error);
                    log_request(&state.shared, entry);
                    backend_error_to_openai_response(backend_error)
                }
            }
        }
        BackendClient::Bedrock(_) | BackendClient::GeminiNative(_) => openai_error_response(
            "This backend does not support /v1/chat/completions. Use /v1/messages instead.",
            "invalid_request_error",
            StatusCode::BAD_REQUEST,
        ),
    }
}

/// Streaming handler for POST /v1/chat/completions with stream: true.
///
/// Translates the Anthropic request to OpenAI, streams the backend response,
/// then uses ReverseStreamingTranslator to convert Anthropic SSE events back
/// to OpenAI ChatCompletionChunk SSE format.
async fn anthropic_chat_completions_stream(
    state: AppState,
    client: crate::backend::anthropic_client::AnthropicClient,
    mut anthropic_req: anthropic::MessageCreateRequest,
    meta: ChatCompletionsStreamMeta,
) -> Response {
    let ChatCompletionsStreamMeta {
        ctx,
        original_model,
        mapped_model,
        warnings,
        safe_headers,
        concurrency_permit,
        vk_ctx,
    } = meta;

    anthropic_req.model = mapped_model.clone();
    anthropic_req.stream = Some(true);
    let body = match serde_json::to_vec(&anthropic_req) {
        Ok(body) => bytes::Bytes::from(body),
        Err(e) => {
            return openai_error_response(
                &format!("failed to serialize Anthropic request: {e}"),
                "server_error",
                StatusCode::INTERNAL_SERVER_ERROR,
            );
        }
    };
    let refs = header_refs(&safe_headers);

    let (response, rate_limits) = match client.forward_stream(body, &refs).await {
        Ok(result) => result,
        Err(e) => {
            state.metrics.record_error();
            let backend_error = BackendError::from(e);
            let mut entry = ctx.log_entry_with_attribution(
                &state.backend_name,
                Some(mapped_model),
                backend_error.status_code(),
                None,
                true,
                Some(backend_error.to_string()),
                &vk_ctx,
                None,
            );
            set_backend_error_kind(&mut entry, &backend_error);
            log_request(&state.shared, entry);
            return backend_error_to_openai_response(backend_error);
        }
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::convert::Infallible>>(32);
    let metrics = state.metrics.clone();
    let log_shared = state.shared.clone();
    let log_backend_name = state.backend_name.clone();
    let stream_timeout_secs = state.stream_timeout_secs;
    let permit = concurrency_permit;

    tokio::spawn(async move {
        let _permit = permit;
        metrics.record_stream_started();
        let mut translator = ReverseStreamingTranslator::new(
            format!("chatcmpl-{}", uuid::Uuid::new_v4().as_simple()),
            original_model,
        );
        let mut usage = AnthropicStreamUsage::default();
        let mut byte_stream = response.bytes_stream();
        let mut buffer = BytesMut::new();
        let mut search_from: usize = 0;
        let mut emitted_done = false;

        let stream_loop = async {
            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = match chunk_result {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::error!("Anthropic chat-completions stream read error: {e}");
                        metrics.record_error();
                        return StreamOutcome::UpstreamError;
                    }
                };

                if buffer.len() + bytes.len() > MAX_SSE_BUFFER_SIZE {
                    tracing::error!(
                        buffer_len = buffer.len(),
                        "Anthropic chat-completions SSE buffer exceeded maximum size"
                    );
                    metrics.record_error();
                    return StreamOutcome::UpstreamError;
                }
                buffer.extend_from_slice(&bytes);

                while let Some((pos, delim_len)) = find_double_newline(&buffer, search_from) {
                    if let Ok(frame_str) = std::str::from_utf8(&buffer[..pos]) {
                        for line in frame_str.lines() {
                            let line = line.trim();
                            let Some(json_str) = line.strip_prefix("data: ") else {
                                continue;
                            };
                            usage.observe_data(json_str);
                            let event =
                                match serde_json::from_str::<anthropic::StreamEvent>(json_str) {
                                    Ok(event) => event,
                                    Err(e) => {
                                        tracing::debug!(
                                            "failed to parse Anthropic streaming event: {e}"
                                        );
                                        continue;
                                    }
                                };
                            let chunks = translator.process_event(&event);
                            for chunk in chunks {
                                let Ok(json) = serde_json::to_string(&chunk) else {
                                    continue;
                                };
                                if tx.send(Ok(format!("data: {json}\n\n"))).await.is_err() {
                                    return StreamOutcome::ClientDisconnected;
                                }
                            }
                            if translator.is_done() && !emitted_done {
                                emitted_done = true;
                                if tx.send(Ok("data: [DONE]\n\n".to_string())).await.is_err() {
                                    return StreamOutcome::ClientDisconnected;
                                }
                            }
                        }
                    }
                    let _ = buffer.split_to(pos + delim_len);
                    search_from = 0;
                }
                search_from = buffer.len().saturating_sub(3);
            }
            StreamOutcome::Completed
        };

        let outcome = if stream_timeout_secs > 0 {
            match tokio::time::timeout(
                std::time::Duration::from_secs(stream_timeout_secs),
                stream_loop,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(_) => {
                    tracing::warn!(
                        timeout_secs = stream_timeout_secs,
                        "Anthropic chat-completions stream exceeded wall-clock timeout"
                    );
                    metrics.record_error();
                    StreamOutcome::UpstreamError
                }
            }
        } else {
            stream_loop.await
        };

        if matches!(outcome, StreamOutcome::Completed) && !emitted_done {
            let _ = tx.send(Ok("data: [DONE]\n\n".to_string())).await;
        }

        let tokens = usage.tokens();
        let cost = tokens.map(|(input_t, output_t)| {
            super::routes::record_virtual_key_usage(
                &log_shared,
                &vk_ctx,
                &mapped_model,
                input_t,
                output_t,
            )
        });
        let (status, err) = outcome.record(&metrics);
        log_request(
            &log_shared,
            ctx.log_entry_with_attribution(
                &log_backend_name,
                Some(mapped_model),
                status,
                tokens,
                true,
                err,
                &vk_ctx,
                cost,
            ),
        );
    });

    let body_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let body = axum::body::Body::from_stream(body_stream);
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    rate_limits.inject_anthropic_response_headers(response.headers_mut());
    if state.expose_degradation_warnings {
        inject_degradation_header(response.headers_mut(), &warnings);
    }
    response
}

async fn chat_completions_stream(
    state: AppState,
    anthropic_req: anthropic::MessageCreateRequest,
    meta: ChatCompletionsStreamMeta,
) -> Response {
    if let BackendClient::Anthropic(client) = &state.backend {
        return anthropic_chat_completions_stream(
            state.clone(),
            client.clone(),
            anthropic_req,
            meta,
        )
        .await;
    }

    let ChatCompletionsStreamMeta {
        ctx,
        original_model,
        mapped_model: mapped_model_resolved,
        warnings,
        safe_headers: _,
        concurrency_permit,
        vk_ctx,
    } = meta;

    // Translate to OpenAI format for the backend
    let mut openai_req = mapping::message_map::anthropic_to_openai_request(&anthropic_req);
    super::routes::inject_gemini_thinking(&anthropic_req, &state.backend, &mut openai_req);
    super::routes::inject_glm_thinking(&anthropic_req, &state.backend, &mut openai_req);
    // Gemini/Vertex rejects standard JSON Schema keywords; sanitize tool schemas.
    if matches!(
        state.backend,
        BackendClient::GeminiOpenAI(_) | BackendClient::Vertex(_)
    ) {
        if let Some(tools) = openai_req.tools.take() {
            openai_req.tools = Some(
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
    openai_req.model = mapped_model_resolved;
    openai_req.stream = Some(true);
    if !state.omit_stream_options {
        openai_req.stream_options = Some(openai::StreamOptions {
            include_usage: true,
        });
    }

    let client = match &state.backend {
        BackendClient::OpenAI(c)
        | BackendClient::AzureOpenAI(c)
        | BackendClient::Vertex(c)
        | BackendClient::GeminiOpenAI(c)
        | BackendClient::OpenAIResponses(c) => c.clone(),
        BackendClient::Anthropic(_)
        | BackendClient::Bedrock(_)
        | BackendClient::GeminiNative(_) => {
            return openai_error_response(
                "This backend does not support /v1/chat/completions. Use /v1/messages instead.",
                "invalid_request_error",
                StatusCode::BAD_REQUEST,
            );
        }
    };

    let mapped_model = openai_req.model.clone();

    // Start the backend request
    let response = match client.chat_completion_stream(&openai_req).await {
        Ok((resp, rate_limits)) => {
            // Build the SSE response with OpenAI chunk format
            let (tx, rx) =
                tokio::sync::mpsc::channel::<Result<String, std::convert::Infallible>>(32);
            let metrics = state.metrics.clone();
            let log_shared = state.shared.clone();
            let log_backend_name = state.backend_name.clone();
            let model_for_translator = original_model.clone();
            let cost_model = mapped_model.clone();
            let stream_timeout_secs = state.stream_timeout_secs;
            let tool_engine = state.tool_engine.clone();
            let anthropic_req_for_tools = anthropic_req.clone();
            let client_for_tools = client.clone();
            let omit_stream_options_for_tools = state.omit_stream_options;
            let permit = concurrency_permit;

            tokio::spawn(async move {
                let _permit = permit;
                metrics.record_stream_started();
                let mut translator = ReverseStreamingTranslator::new(
                    format!("chatcmpl-{}", uuid::Uuid::new_v4().as_simple()),
                    model_for_translator.clone(),
                );
                let mut stream_translator =
                    mapping::streaming_map::StreamingTranslator::new(model_for_translator.clone());

                let mut byte_stream = resp.bytes_stream();
                let mut buffer = BytesMut::new();
                let mut search_from: usize = 0;
                let mut timed_out = false;
                // Accumulate tool call fragments for collect-then-execute.
                // Each entry: (id, function_name, arguments_json).
                let mut accumulated_tool_calls: Vec<(String, String, String)> = Vec::new();

                let stream_loop = async {
                    while let Some(chunk_result) = byte_stream.next().await {
                        let bytes = match chunk_result {
                            Ok(b) => b,
                            Err(e) => {
                                tracing::error!("stream read error: {e}");
                                metrics.record_error();
                                metrics.record_stream_failed();
                                return;
                            }
                        };
                        buffer.extend_from_slice(&bytes);

                        if buffer.len() > MAX_SSE_BUFFER_SIZE {
                            tracing::error!("SSE buffer exceeded maximum size");
                            metrics.record_error();
                            metrics.record_stream_failed();
                            return;
                        }

                        while let Some((pos, delim_len)) = find_double_newline(&buffer, search_from)
                        {
                            if let Ok(frame_str) = std::str::from_utf8(&buffer[..pos]) {
                                for line in frame_str.lines() {
                                    let line = line.trim();
                                    if let Some(json_str) = line.strip_prefix("data: ") {
                                        if json_str == "[DONE]" {
                                            // Defer [DONE] until after potential tool execution.
                                            continue;
                                        }
                                        // Parse OpenAI chunk, translate to Anthropic events,
                                        // then reverse-translate to OpenAI chunks
                                        if let Ok(chunk) =
                                            serde_json::from_str::<openai::ChatCompletionChunk>(
                                                json_str,
                                            )
                                        {
                                            // Accumulate tool call fragments from delta.
                                            if let Some(choice) = chunk.choices.first() {
                                                if let Some(ref tc_list) = choice.delta.tool_calls {
                                                    for tc in tc_list {
                                                        let idx = tc.index as usize;
                                                        while accumulated_tool_calls.len() <= idx {
                                                            accumulated_tool_calls.push((
                                                                String::new(),
                                                                String::new(),
                                                                String::new(),
                                                            ));
                                                        }
                                                        if let Some(ref id) = tc.id {
                                                            if !id.is_empty() {
                                                                accumulated_tool_calls[idx].0 =
                                                                    id.clone();
                                                            }
                                                        }
                                                        if let Some(ref func) = tc.function {
                                                            if let Some(ref name) = func.name {
                                                                if !name.is_empty() {
                                                                    accumulated_tool_calls[idx].1 =
                                                                        name.clone();
                                                                }
                                                            }
                                                            if let Some(ref args) = func.arguments {
                                                                accumulated_tool_calls[idx]
                                                                    .2
                                                                    .push_str(args);
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            let anthropic_events =
                                                stream_translator.process_chunk(&chunk);
                                            for event in &anthropic_events {
                                                let oai_chunks = translator.process_event(event);
                                                for oai_chunk in &oai_chunks {
                                                    if let Ok(json) =
                                                        serde_json::to_string(oai_chunk)
                                                    {
                                                        let sse_line =
                                                            format!("data: {}\n\n", json);
                                                        if tx.send(Ok(sse_line)).await.is_err() {
                                                            metrics
                                                                .record_stream_client_disconnected(
                                                                );
                                                            return; // Client disconnected
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            let _ = buffer.split_to(pos + delim_len);
                            search_from = 0;
                        }
                        search_from = buffer.len().saturating_sub(3);
                    }
                };

                if stream_timeout_secs > 0 {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(stream_timeout_secs),
                        stream_loop,
                    )
                    .await
                    {
                        Ok(()) => {}
                        Err(_) => {
                            tracing::warn!(
                                timeout_secs = stream_timeout_secs,
                                "chat_completions streaming response exceeded wall-clock timeout"
                            );
                            metrics.record_error();
                            metrics.record_stream_failed();
                            timed_out = true;
                        }
                    }
                } else {
                    stream_loop.await;
                }

                if timed_out {
                    // Log and exit without emitting finish events on timeout.
                    log_request(
                        &log_shared,
                        ctx.log_entry_with_attribution(
                            &log_backend_name,
                            Some(mapped_model),
                            504,
                            None,
                            true,
                            Some("stream timeout".into()),
                            &vk_ctx,
                            None,
                        ),
                    );
                    return;
                }

                // Emit any remaining finish events
                let finish_events = stream_translator.finish();
                for event in &finish_events {
                    let oai_chunks = translator.process_event(event);
                    for oai_chunk in &oai_chunks {
                        if let Ok(json) = serde_json::to_string(oai_chunk) {
                            let _ = tx.send(Ok(format!("data: {}\n\n", json))).await;
                        }
                    }
                }

                // Collect-then-execute loop: bounded by engine.loop_config.max_iterations.
                // Mirrors the non-streaming `maybe_execute_tools` loop so follow-up tool
                // calls are not silently dropped.
                if !accumulated_tool_calls.is_empty() {
                    if let Some(ref engine) = tool_engine {
                        let loop_start = std::time::Instant::now();
                        let mut current_messages = anthropic_req_for_tools.messages.clone();
                        let server_advertised_tool_names = std::collections::HashSet::new();

                        'tool_loop: for _iteration in 0..engine.loop_config.max_iterations {
                            if loop_start.elapsed() > engine.loop_config.total_timeout {
                                tracing::warn!("streaming tool loop: total timeout reached");
                                break 'tool_loop;
                            }

                            let tool_calls: Vec<crate::tools::ToolCall> = accumulated_tool_calls
                                .iter()
                                .filter(|(_, name, _)| !name.is_empty())
                                .map(|(id, name, args)| crate::tools::ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: serde_json::from_str(args)
                                        .unwrap_or(serde_json::Value::Null),
                                })
                                .collect();

                            let (auto_exec, _pass_through, denied) =
                                crate::tools::execution::partition_tool_calls(
                                    &tool_calls,
                                    &engine.registry,
                                    &engine.policy,
                                    &server_advertised_tool_names,
                                );
                            let denied_results =
                                crate::tools::execution::denied_tool_results(&denied);

                            if auto_exec.is_empty() && denied_results.is_empty() {
                                break 'tool_loop;
                            }

                            let mut results = crate::tools::execution::execute_tool_calls(
                                &auto_exec,
                                engine.registry.clone(),
                                &engine.policy,
                                &engine.loop_config,
                            )
                            .await;

                            // Include denied-tool errors in the follow-up so the LLM sees them.
                            results.extend(denied_results);

                            // Build the assistant message from accumulated tool calls.
                            let assistant_content: Vec<anyllm_translate::anthropic::ContentBlock> =
                                tool_calls
                                    .iter()
                                    .map(|tc| anyllm_translate::anthropic::ContentBlock::ToolUse {
                                        id: tc.id.clone(),
                                        name: tc.name.clone(),
                                        input: tc.input.clone(),
                                    })
                                    .collect();

                            current_messages.push(anyllm_translate::anthropic::InputMessage {
                                role: anyllm_translate::anthropic::Role::Assistant,
                                content: anyllm_translate::anthropic::Content::Blocks(
                                    assistant_content,
                                ),
                            });
                            current_messages.push(
                                crate::tools::execution::tool_results_to_user_message(&results),
                            );

                            let mut follow_up_req = anthropic_req_for_tools.clone();
                            follow_up_req.messages = current_messages.clone();

                            let mut follow_up_openai =
                                mapping::message_map::anthropic_to_openai_request(&follow_up_req);
                            follow_up_openai.model = cost_model.clone();
                            follow_up_openai.stream = Some(true);
                            if !omit_stream_options_for_tools {
                                follow_up_openai.stream_options = Some(openai::StreamOptions {
                                    include_usage: true,
                                });
                            }

                            tracing::info!(
                                tools_executed = results.len(),
                                iteration = _iteration,
                                "streaming tool execution: starting follow-up backend call"
                            );

                            // Reset for this follow-up pass so we can detect new tool calls.
                            accumulated_tool_calls = Vec::new();

                            // Create a fresh translator pair for the follow-up stream.
                            let mut follow_translator = ReverseStreamingTranslator::new(
                                format!("chatcmpl-{}", uuid::Uuid::new_v4().as_simple()),
                                model_for_translator.clone(),
                            );
                            let mut follow_stream_translator =
                                mapping::streaming_map::StreamingTranslator::new(
                                    model_for_translator.clone(),
                                );

                            match client_for_tools
                                .chat_completion_stream(&follow_up_openai)
                                .await
                            {
                                Ok((follow_resp, _follow_rate_limits)) => {
                                    let mut follow_byte_stream = follow_resp.bytes_stream();
                                    let mut follow_buffer = BytesMut::new();
                                    let mut follow_search_from: usize = 0;

                                    while let Some(chunk_result) = follow_byte_stream.next().await {
                                        let bytes = match chunk_result {
                                            Ok(b) => b,
                                            Err(e) => {
                                                tracing::error!("follow-up stream read error: {e}");
                                                break;
                                            }
                                        };
                                        follow_buffer.extend_from_slice(&bytes);

                                        while let Some((pos, delim_len)) =
                                            find_double_newline(&follow_buffer, follow_search_from)
                                        {
                                            if let Ok(frame_str) =
                                                std::str::from_utf8(&follow_buffer[..pos])
                                            {
                                                for line in frame_str.lines() {
                                                    let line = line.trim();
                                                    if let Some(json_str) =
                                                        line.strip_prefix("data: ")
                                                    {
                                                        if json_str == "[DONE]" {
                                                            continue;
                                                        }
                                                        if let Ok(chunk) = serde_json::from_str::<
                                                            openai::ChatCompletionChunk,
                                                        >(
                                                            json_str
                                                        ) {
                                                            // Accumulate tool calls for the
                                                            // next iteration.
                                                            if let Some(choice) =
                                                                chunk.choices.first()
                                                            {
                                                                if let Some(ref tc_list) =
                                                                    choice.delta.tool_calls
                                                                {
                                                                    for tc in tc_list {
                                                                        let idx = tc.index as usize;
                                                                        while accumulated_tool_calls
                                                                            .len()
                                                                            <= idx
                                                                        {
                                                                            accumulated_tool_calls
                                                                                .push((
                                                                                    String::new(),
                                                                                    String::new(),
                                                                                    String::new(),
                                                                                ));
                                                                        }
                                                                        if let Some(ref id) = tc.id
                                                                        {
                                                                            if !id.is_empty() {
                                                                                accumulated_tool_calls
                                                                                    [idx]
                                                                                    .0 = id.clone();
                                                                            }
                                                                        }
                                                                        if let Some(ref func) =
                                                                            tc.function
                                                                        {
                                                                            if let Some(ref name) =
                                                                                func.name
                                                                            {
                                                                                if !name.is_empty()
                                                                                {
                                                                                    accumulated_tool_calls[idx].1 = name.clone();
                                                                                }
                                                                            }
                                                                            if let Some(ref args) =
                                                                                func.arguments
                                                                            {
                                                                                accumulated_tool_calls[idx].2.push_str(args);
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }

                                                            let events = follow_stream_translator
                                                                .process_chunk(&chunk);
                                                            for event in &events {
                                                                let oai_chunks = follow_translator
                                                                    .process_event(event);
                                                                for oai_chunk in &oai_chunks {
                                                                    if let Ok(json) =
                                                                        serde_json::to_string(
                                                                            oai_chunk,
                                                                        )
                                                                    {
                                                                        if tx
                                                                            .send(Ok(format!(
                                                                                "data: {}\n\n",
                                                                                json
                                                                            )))
                                                                            .await
                                                                            .is_err()
                                                                        {
                                                                            return;
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            let _ = follow_buffer.split_to(pos + delim_len);
                                            follow_search_from = 0;
                                        }
                                        follow_search_from = follow_buffer.len().saturating_sub(3);
                                    }

                                    // Emit finish events for the follow-up stream.
                                    let follow_finish = follow_stream_translator.finish();
                                    for event in &follow_finish {
                                        let oai_chunks = follow_translator.process_event(event);
                                        for oai_chunk in &oai_chunks {
                                            if let Ok(json) = serde_json::to_string(oai_chunk) {
                                                let _ = tx
                                                    .send(Ok(format!("data: {}\n\n", json)))
                                                    .await;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        error = %e,
                                        "follow-up streaming backend call failed"
                                    );
                                    break 'tool_loop;
                                }
                            }
                            // If accumulated_tool_calls is still empty after the follow-up
                            // stream, the next iteration's early-exit check will break out.
                        } // end 'tool_loop
                    }
                }

                // Send final [DONE] after initial stream and any tool execution follow-up.
                let _ = tx.send(Ok("data: [DONE]\n\n".to_string())).await;

                // Extract token counts from the stream translator for cost tracking.
                let usage = stream_translator.usage();
                let tokens = usage.map(|u| (u.input_tokens as u64, u.output_tokens as u64));
                let cost = if let Some((input_t, output_t)) = tokens {
                    Some(super::routes::record_virtual_key_usage(
                        &log_shared,
                        &vk_ctx,
                        &cost_model,
                        input_t,
                        output_t,
                    ))
                } else {
                    None
                };

                metrics.record_success();
                metrics.record_stream_completed();
                log_request(
                    &log_shared,
                    ctx.log_entry_with_attribution(
                        &log_backend_name,
                        Some(mapped_model),
                        200,
                        tokens,
                        true,
                        None,
                        &vk_ctx,
                        cost,
                    ),
                );
            });

            // Build the SSE response using raw text/event-stream
            let body_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
            let body = axum::body::Body::from_stream(body_stream);
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .header("connection", "keep-alive")
                .body(body)
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            rate_limits.inject_anthropic_response_headers(response.headers_mut());
            if state.expose_degradation_warnings {
                inject_degradation_header(response.headers_mut(), &warnings);
            }
            response
        }
        Err(e) => {
            state.metrics.record_error();
            let backend_error = BackendError::from(e);
            let mut entry = ctx.log_entry_with_attribution(
                &state.backend_name,
                Some(mapped_model),
                backend_error.status_code(),
                None,
                true,
                Some(backend_error.to_string()),
                &vk_ctx,
                None,
            );
            set_backend_error_kind(&mut entry, &backend_error);
            log_request(&state.shared, entry);
            backend_error_to_openai_response(backend_error)
        }
    };

    response
}
