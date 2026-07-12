use crate::backend::{BackendClient, BackendError};
use crate::cache::{self, CacheBackend, CacheNamespace};
use crate::openai_tool_policy::{
    backend_kind_for_policy, prepare_openai_tool_request, validate_anthropic_tool_request,
    OpenAiToolPolicyContext,
};
use anyllm_translate::{
    anthropic, mapping, openai, translate_anthropic_to_openai_response,
    translate_anthropic_to_openai_response_with_context, translate_openai_to_anthropic_request,
    translate_openai_to_anthropic_request_with_context, AnthropicTranslationContext,
    TranslationWarnings,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

use super::super::routes::{
    cache_auth_identity, inject_degradation_header, log_request, set_backend_error_kind, RequestCtx,
};
use super::super::state::{AppState, ConcurrencyPermit};

use super::extensions::{
    apply_anthropic_chat_extensions, merge_anthropic_beta_headers,
    serialize_anthropic_upstream_request, AnthropicChatExtensions,
};
use super::helpers::{
    backend_error_to_openai_response, cache_key_body_for_chat_completions, header_refs,
    is_anthropic_backend, mapped_model_for_backend, openai_error_response,
    safe_anthropic_extra_headers,
};
use super::stream::{chat_completions_stream, ChatCompletionsStreamMeta};

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

    // Resolve routing first (route selection depends only on the model name, not
    // body content) so per-route option overrides on `effective` (redaction,
    // guardrails) apply. Resolution advances the route's round-robin/failover
    // counter, so it must run exactly once, before redaction.
    let original_model = body.model.clone();
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

    let body =
        match super::super::secret_redaction::redact_json_value(effective.redact_secrets(), body)
            .await
        {
            Ok(body) => body,
            Err(err) => {
                return openai_error_response(err.safe_message(), "api_error", err.status_code());
            }
        };

    let is_streaming = body.stream == Some(true);
    let mut safe_headers = safe_anthropic_extra_headers(&headers);
    let caller_omitted_max_tokens =
        body.max_tokens.is_none() && body.max_completion_tokens.is_none();

    let mut translated_body = body.clone();
    if is_anthropic_backend(&effective) && caller_omitted_max_tokens {
        translated_body.max_tokens = Some(4096);
    }

    // Translate OpenAI request -> Anthropic request.
    let mut warnings = TranslationWarnings::default();
    let mut tool_context = AnthropicTranslationContext::default();
    let mut anthropic_req = if is_anthropic_backend(&effective) {
        match translate_openai_to_anthropic_request_with_context(&translated_body, &mut warnings) {
            Ok((req, context)) => {
                tool_context = context;
                req
            }
            Err(e) => {
                return openai_error_response(
                    &e.to_string(),
                    "invalid_request_error",
                    StatusCode::BAD_REQUEST,
                );
            }
        }
    } else {
        match translate_openai_to_anthropic_request(&translated_body, &mut warnings) {
            Ok(req) => req,
            Err(e) => {
                return openai_error_response(
                    &e.to_string(),
                    "invalid_request_error",
                    StatusCode::BAD_REQUEST,
                );
            }
        }
    };
    let mut anthropic_extensions = AnthropicChatExtensions::default();
    if is_anthropic_backend(&effective) {
        anthropic_extensions = match apply_anthropic_chat_extensions(
            &body,
            &mut anthropic_req,
            &mut warnings,
            caller_omitted_max_tokens,
        ) {
            Ok(extensions) => extensions,
            Err(message) => {
                return openai_error_response(
                    &message,
                    "invalid_request_error",
                    StatusCode::BAD_REQUEST,
                );
            }
        };
        merge_anthropic_beta_headers(&mut safe_headers, &anthropic_extensions.beta_headers);
    }

    if anthropic_req.messages.is_empty() {
        return openai_error_response(
            "messages array must not be empty",
            "invalid_request_error",
            StatusCode::BAD_REQUEST,
        );
    }

    if is_streaming {
        let deployment_accounting =
            super::super::streaming::StreamDeploymentAccounting::start(deployment);
        let stream_meta = ChatCompletionsStreamMeta {
            ctx,
            original_model,
            mapped_model,
            warnings,
            safe_headers,
            raw_anthropic_tools: anthropic_extensions.raw_tools,
            tool_context,
            concurrency_permit: permit,
            vk_ctx,
            deployment_accounting,
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
    let cache_control = match cache::parse_cache_control(&body_value) {
        Ok(control) => control,
        Err(msg) => {
            return openai_error_response(&msg, "invalid_request_error", StatusCode::BAD_REQUEST);
        }
    };

    let auth_identity = cache_auth_identity(&headers, &vk_ctx);
    let cache_key = if cache_control.lookup || cache_control.store {
        Some(cache::cache_key_for_request(
            &body_value,
            CacheNamespace::OpenAI,
            &cache::CacheScope {
                backend_name: &effective.backend_name,
                auth_identity: &auth_identity,
                namespace: cache_control.namespace.as_deref(),
            },
        ))
    } else {
        None
    };
    let store_cache_key = if cache_control.store {
        cache_key.clone()
    } else {
        None
    };

    // Check cache when lookup is enabled.
    if let (true, Some(ref key), Some(ref c)) = (cache_control.lookup, &cache_key, &state.cache) {
        if let Some(entry) = c.get(key).await {
            if cache::cache_entry_is_fresh(&entry, cache_control.max_age_secs) {
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
            tracing::debug!(cache_key = %key, "cache entry rejected by cache.s-maxage");
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
            super::super::routes::inject_gemini_thinking(
                &anthropic_req,
                &effective.backend,
                &mut openai_req,
            );
            super::super::routes::inject_glm_thinking(
                &anthropic_req,
                &effective.backend,
                &mut openai_req,
            );
            if effective.omit_stream_options {
                openai_req.stream_options = None;
            }
            openai_req.model = mapped_model.clone();
            if let Err(err) = prepare_openai_tool_request(
                &mut openai_req,
                OpenAiToolPolicyContext {
                    backend_kind: backend_kind_for_policy(&effective.backend),
                    provider_id: effective.provider_id.as_deref(),
                    model: &mapped_model,
                    provider_catalog: &effective.provider_catalog,
                },
                &mut warnings,
            ) {
                return openai_error_response(
                    err.message(),
                    "invalid_request_error",
                    StatusCode::BAD_REQUEST,
                );
            }
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
                    // Use `effective` (per-route option overrides) for guardrails
                    // + follow-up redaction, not the base `state`.
                    let anthropic_resp = if let Some(ref engine) = effective.tool_engine {
                        let client_for_tools = client.clone();
                        let model_for_tools = mapped_model.clone();
                        let orig_model_for_tools = original_model.clone();
                        let redact_follow_up = effective.redact_secrets();
                        let backend_kind_for_tools = backend_kind_for_policy(&effective.backend);
                        let provider_id_for_tools = effective.provider_id.clone();
                        let provider_catalog_for_tools = effective.provider_catalog.clone();
                        let server_advertised_tool_names = std::collections::HashSet::new();
                        let guardrails_for_tools = effective.effective_tool_guardrails(engine);
                        let (resp, _trace) = crate::tools::execution::maybe_execute_tools(
                            engine,
                            &anthropic_req,
                            &server_advertised_tool_names,
                            anthropic_resp,
                            &guardrails_for_tools,
                            |follow_up_req| {
                                let c = client_for_tools.clone();
                                let m = model_for_tools.clone();
                                let policy_model = m.clone();
                                let om = orig_model_for_tools.clone();
                                let backend_kind = backend_kind_for_tools.clone();
                                let provider_id = provider_id_for_tools.clone();
                                let provider_catalog = provider_catalog_for_tools.clone();
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
                                    let mut follow_up_warnings = TranslationWarnings::default();
                                    prepare_openai_tool_request(
                                        &mut oai_req,
                                        OpenAiToolPolicyContext {
                                            backend_kind,
                                            provider_id: provider_id.as_deref(),
                                            model: &policy_model,
                                            provider_catalog: &provider_catalog,
                                        },
                                        &mut follow_up_warnings,
                                    )
                                    .map_err(|err| err.to_string())?;
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
                    let cost = super::super::routes::record_virtual_key_usage(
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
                    super::super::routes::try_cache_response(
                        &store_cache_key,
                        &state.cache,
                        cache_control.ttl_secs,
                        &oai_response,
                        original_model.clone(),
                    )
                    .await;

                    let cache_hv = super::super::routes::cache_header_value(!cache_control.lookup);
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
                    let cost = super::super::routes::record_virtual_key_usage(
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

                    super::super::routes::try_cache_response(
                        &store_cache_key,
                        &state.cache,
                        cache_control.ttl_secs,
                        &oai_response,
                        original_model.clone(),
                    )
                    .await;

                    let cache_hv = super::super::routes::cache_header_value(!cache_control.lookup);
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
            if let Err(err) = validate_anthropic_tool_request(
                &upstream_req,
                OpenAiToolPolicyContext {
                    backend_kind: backend_kind_for_policy(&effective.backend),
                    provider_id: effective.provider_id.as_deref(),
                    model: &mapped_model,
                    provider_catalog: &effective.provider_catalog,
                },
            ) {
                return openai_error_response(
                    err.message(),
                    "invalid_request_error",
                    StatusCode::BAD_REQUEST,
                );
            }
            let body = match serialize_anthropic_upstream_request(
                &upstream_req,
                &anthropic_extensions.raw_tools,
            ) {
                Ok(body) => body,
                Err(e) => {
                    return openai_error_response(
                        &format!("failed to serialize Anthropic request: {e}"),
                        "server_error",
                        StatusCode::INTERNAL_SERVER_ERROR,
                    );
                }
            };
            let refs = header_refs(&safe_headers);

            match client.forward(body, &refs, None).await {
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
                    let oai_response = translate_anthropic_to_openai_response_with_context(
                        &anthropic_resp,
                        &original_model,
                        &tool_context,
                    );
                    let cost = super::super::routes::record_virtual_key_usage(
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

                    super::super::routes::try_cache_response(
                        &store_cache_key,
                        &state.cache,
                        cache_control.ttl_secs,
                        &oai_response,
                        original_model.clone(),
                    )
                    .await;

                    let cache_hv = super::super::routes::cache_header_value(!cache_control.lookup);
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
