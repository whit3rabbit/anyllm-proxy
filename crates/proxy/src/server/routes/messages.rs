use super::super::streaming::messages_stream;
use super::helpers::{
    backend_error_to_response, cache_auth_identity, cache_header_value, inject_degradation_header,
    log_request, record_virtual_key_usage, set_backend_error_kind, try_cache_response,
};
use crate::backend::{BackendClient, BackendError};
use crate::cache::{self, CacheBackend, CacheNamespace};
use crate::server::state::{AnthropicJson, AppState, ConcurrencyPermit};
use anyllm_translate::{anthropic, compute_request_warnings, mapping};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

use super::context::{
    inject_gemini_thinking, inject_glm_thinking, route_scope_forbidden_response, RequestCtx,
};

pub(crate) async fn messages(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    permit: Option<axum::Extension<ConcurrencyPermit>>,
    vk_ctx: Option<axum::Extension<super::super::middleware::VirtualKeyContext>>,
    AnthropicJson(body): AnthropicJson<anthropic::MessageCreateRequest>,
) -> Response {
    // Hold concurrency permit for streaming: passed to the spawned task so
    // the permit lives until the stream completes, not just until headers are sent.
    let permit = permit.map(|axum::Extension(p)| p);
    let vk_ctx = vk_ctx.map(|axum::Extension(c)| c);
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
        if !super::super::policy::is_model_allowed(&body.model, &ctx.allowed_models) {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::PermissionError,
                format!("Model '{}' is not allowed for this API key.", body.model),
                None,
            );
            return (StatusCode::FORBIDDEN, Json(err)).into_response();
        }
    }

    let body = match super::super::secret_redaction::redact_json_value(state.redact_secrets(), body)
        .await
    {
        Ok(body) => body,
        Err(err) => return super::super::secret_redaction::error_response(err),
    };

    if state.log_bodies() {
        tracing::debug!(
            model = %body.model,
            stream = ?body.stream,
            message_count = body.messages.len(),
            body = %serde_json::to_string(&body).unwrap_or_else(|_| "[serialization failed]".into()),
            "request body"
        );
    }

    let warnings = compute_request_warnings(&body);

    let is_streaming = body.stream == Some(true);

    if is_streaming {
        if state.log_bodies() {
            tracing::debug!(model = %body.model, "streaming request initiated");
        }
        let (mapped_model, effective, deployment) = match state.resolve_model_and_state(&body.model)
        {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        if let Some(ref ctx) = vk_ctx {
            if let Err(error) = super::super::policy::enforce_route_scope(
                &effective.backend_name,
                &effective.shared,
                &ctx.allowed_routes,
            )
            .await
            {
                return route_scope_forbidden_response(error);
            }
        }
        // Logging deferred until stream completes (inside messages_stream tasks).
        let deployment_accounting =
            super::super::streaming::StreamDeploymentAccounting::start(deployment);
        match messages_stream(
            effective,
            body,
            super::super::routes::RequestCtx {
                request_id: ctx.request_id.clone(),
                start: ctx.start,
                model_requested: ctx.model_requested.clone(),
            },
            mapped_model,
            permit,
            vk_ctx.clone(),
            deployment_accounting,
        )
        .await
        {
            Ok((rate_limits, sse)) => {
                let mut response = sse.into_response();
                rate_limits.inject_anthropic_response_headers(response.headers_mut());
                if state.expose_degradation_warnings {
                    inject_degradation_header(response.headers_mut(), &warnings);
                }
                response.headers_mut().insert(
                    "x-anyllm-cache",
                    axum::http::HeaderValue::from_static("bypass"),
                );
                return response;
            }
            Err(e) => {
                // Pre-stream backend error: return proper HTTP status instead of 200 OK
                return backend_error_to_response(e);
            }
        }
    }

    // Non-streaming: check cache before calling backend.
    let body_value = serde_json::to_value(&body).unwrap_or_default();
    let cache_ttl = match cache::parse_cache_ttl(&body_value) {
        Ok(ttl) => ttl,
        Err(msg) => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::InvalidRequestError,
                msg,
                None,
            );
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }
    };
    let bypass_cache = cache_ttl == Some(0);

    // Resolve model routing (may switch to a different backend) before reading
    // cache so route-scoped keys cannot receive cached disallowed-backend data.
    let (mapped_model, effective, deployment) = match state.resolve_model_and_state(&body.model) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if let Some(ref ctx) = vk_ctx {
        if let Err(error) = super::super::policy::enforce_route_scope(
            &effective.backend_name,
            &effective.shared,
            &ctx.allowed_routes,
        )
        .await
        {
            return route_scope_forbidden_response(error);
        }
    }
    let auth_identity = cache_auth_identity(&headers, &vk_ctx);
    let cache_key = if !bypass_cache {
        Some(cache::cache_key_for_request(
            &body_value,
            CacheNamespace::Anthropic,
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
            tracing::debug!(cache_key = %key, "cache hit for /v1/messages");
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

    match &effective.backend {
        BackendClient::OpenAI(client)
        | BackendClient::AzureOpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
            inject_gemini_thinking(&body, &effective.backend, &mut openai_req);
            inject_glm_thinking(&body, &effective.backend, &mut openai_req);
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
            let original_model = body.model.clone();

            match client.chat_completion(&openai_req).await {
                Ok((openai_resp, _status, rate_limits)) => {
                    if let Some(ref d) = deployment {
                        d.record_finish(backend_start.elapsed().as_millis() as u64);
                    }
                    state.metrics.record_success();
                    let anthropic_resp = mapping::message_map::openai_to_anthropic_response(
                        &openai_resp,
                        &original_model,
                    );

                    // Tool execution: bounded loop with termination guards.
                    let anthropic_resp = if let Some(ref engine) = state.tool_engine {
                        let client_for_tools = client.clone();
                        let model_for_tools = mapped_model.clone();
                        let orig_model_for_tools = original_model.clone();
                        let redact_follow_up = state.redact_secrets();
                        let server_advertised_tool_names = std::collections::HashSet::new();
                        let (resp, trace) = crate::tools::execution::maybe_execute_tools(
                            engine,
                            &body,
                            &server_advertised_tool_names,
                            anthropic_resp,
                            |follow_up_req| {
                                let c = client_for_tools.clone();
                                let m = model_for_tools.clone();
                                let om = orig_model_for_tools.clone();
                                async move {
                                    let follow_up_req =
                                        match super::super::secret_redaction::redact_json_value(
                                            redact_follow_up,
                                            follow_up_req,
                                        )
                                        .await
                                        {
                                            Ok(req) => req,
                                            Err(err) => return Err(err.safe_message().to_string()),
                                        };
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
                        tracing::debug!(
                            termination_reason = ?trace.termination_reason,
                            iterations = trace.iterations.len(),
                            tool_calls = trace.total_tool_calls(),
                            total_ms = trace.total_duration.as_millis(),
                            "tool execution loop complete"
                        );
                        resp
                    } else {
                        anthropic_resp
                    };

                    if state.log_bodies() {
                        tracing::debug!(
                            body = %serde_json::to_string(&anthropic_resp).unwrap_or_else(|_| "[serialization failed]".into()),
                            "response body"
                        );
                    }
                    let cost = record_virtual_key_usage(
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

                    try_cache_response(
                        &cache_key,
                        &state.cache,
                        cache_ttl,
                        &anthropic_resp,
                        original_model,
                    )
                    .await;

                    let cache_hv = cache_header_value(bypass_cache);
                    let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
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
                    backend_error_to_response(backend_error)
                }
            }
        }
        BackendClient::OpenAIResponses(client) => {
            let mut responses_req =
                mapping::responses_message_map::anthropic_to_responses_request(&body);
            responses_req.model = mapped_model.clone();
            let mapped_model = responses_req.model.clone();
            let original_model = body.model.clone();

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
                    if state.log_bodies() {
                        tracing::debug!(
                            body = %serde_json::to_string(&anthropic_resp).unwrap_or_else(|_| "[serialization failed]".into()),
                            "response body"
                        );
                    }
                    let cost = record_virtual_key_usage(
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
                    try_cache_response(
                        &cache_key,
                        &state.cache,
                        cache_ttl,
                        &anthropic_resp,
                        original_model,
                    )
                    .await;

                    let cache_hv = cache_header_value(bypass_cache);
                    let mut response = (StatusCode::OK, Json(anthropic_resp)).into_response();
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
                    backend_error_to_response(backend_error)
                }
            }
        }
        BackendClient::Anthropic(_)
        | BackendClient::Bedrock(_)
        | BackendClient::GeminiNative(_) => {
            // These backends are handled by separate handlers (passthrough / Bedrock / Gemini native).
            // If we reach here, something is misconfigured.
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "This backend does not use the translation handler".to_string(),
                None,
            );
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response()
        }
    }
}
