use crate::backend::openai_client::OpenAIClient;
use crate::openai_tool_policy::{
    backend_kind_for_policy, prepare_openai_tool_request, OpenAiToolPolicyContext,
};
use crate::server::routes::{
    inject_degradation_header, inject_gemini_thinking, inject_glm_thinking, log_request,
    record_virtual_key_usage, set_backend_error_kind, try_cache_response, RequestCtx,
};
use crate::server::state::AppState;
use anyllm_translate::{
    anthropic, mapping, openai, translate_anthropic_to_openai_response, TranslationWarnings,
};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_openai_backend(
    client: &OpenAIClient,
    effective: &AppState,
    state: &AppState,
    anthropic_req: &anthropic::MessageCreateRequest,
    _body: &openai::ChatCompletionRequest,
    original_model: &str,
    mapped_model: &str,
    warnings: &mut TranslationWarnings,
    deployment: &Option<std::sync::Arc<crate::config::model_router::Deployment>>,
    backend_start: std::time::Instant,
    vk_ctx: &Option<crate::server::middleware::VirtualKeyContext>,
    ctx: &RequestCtx,
    cache_control: &crate::cache::CacheControl,
    store_cache_key: &Option<String>,
) -> Response {
    let mut openai_req = mapping::message_map::anthropic_to_openai_request(anthropic_req);
    inject_gemini_thinking(anthropic_req, &effective.backend, &mut openai_req);
    inject_glm_thinking(anthropic_req, &effective.backend, &mut openai_req);
    if effective.omit_stream_options {
        openai_req.stream_options = None;
    }
    openai_req.model = mapped_model.to_string();

    // Opt-in RTK tool-output compression (OpenAI-in translate path).
    effective.apply_rtk_to_openai(&mut openai_req, mapped_model);

    if let Err(err) = prepare_openai_tool_request(
        &mut openai_req,
        OpenAiToolPolicyContext {
            backend_kind: backend_kind_for_policy(&effective.backend),
            provider_id: effective.provider_id.as_deref(),
            model: mapped_model,
            provider_catalog: &effective.provider_catalog,
        },
        warnings,
    ) {
        return super::helpers::openai_error_response(
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
            let anthropic_resp =
                mapping::message_map::openai_to_anthropic_response(&openai_resp, original_model);

            // Tool execution: bounded loop with termination guards.
            let anthropic_resp = if let Some(ref engine) = effective.tool_engine {
                let client_for_tools = client.clone();
                let model_for_tools = mapped_model.clone();
                let orig_model_for_tools = original_model.to_string();
                let redact_follow_up = effective.redact_secrets();
                let backend_kind_for_tools = backend_kind_for_policy(&effective.backend);
                let provider_id_for_tools = effective.provider_id.clone();
                let provider_catalog_for_tools = effective.provider_catalog.clone();
                let server_advertised_tool_names = std::collections::HashSet::new();
                let guardrails_for_tools = effective.effective_tool_guardrails(engine);
                let (resp, _trace) = crate::tools::execution::maybe_execute_tools(
                    engine,
                    anthropic_req,
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
                                match crate::server::secret_redaction::redact_json_value(
                                    redact_follow_up,
                                    follow_up_req,
                                )
                                .await
                                {
                                    Ok(req) => req,
                                    Err(err) => return Err(err.safe_message().to_string()),
                                };
                            let mut oai_req =
                                mapping::message_map::anthropic_to_openai_request(&follow_up_req);
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
                                Ok((resp, _, _)) => Ok(
                                    mapping::message_map::openai_to_anthropic_response(&resp, &om),
                                ),
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
                translate_anthropic_to_openai_response(&anthropic_resp, original_model);
            let cost = record_virtual_key_usage(
                &state.shared,
                vk_ctx,
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
                    vk_ctx,
                    Some(cost),
                ),
            );
            try_cache_response(
                store_cache_key,
                &state.cache,
                cache_control.ttl_secs,
                &oai_response,
                original_model.to_string(),
            )
            .await;

            let cache_hv = crate::server::routes::cache_header_value(!cache_control.lookup);
            let mut response = (StatusCode::OK, Json(oai_response)).into_response();
            rate_limits.inject_anthropic_response_headers(response.headers_mut());
            if state.expose_degradation_warnings {
                inject_degradation_header(response.headers_mut(), warnings);
            }
            response.headers_mut().insert("x-anyllm-cache", cache_hv);
            response
        }
        Err(e) => {
            if let Some(ref d) = deployment {
                d.record_finish(backend_start.elapsed().as_millis() as u64);
            }
            state.metrics.record_error();
            let backend_error = crate::backend::BackendError::from(e);
            let mut entry = ctx.log_entry_with_attribution(
                &state.backend_name,
                Some(mapped_model),
                backend_error.status_code(),
                None,
                false,
                Some(backend_error.to_string()),
                vk_ctx,
                None,
            );
            set_backend_error_kind(&mut entry, &backend_error);
            log_request(&state.shared, entry);
            super::helpers::backend_error_to_openai_response(backend_error)
        }
    }
}
