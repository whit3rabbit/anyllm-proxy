use crate::backend::anthropic_client::AnthropicClient;
use crate::openai_tool_policy::{
    backend_kind_for_policy, validate_anthropic_tool_request, OpenAiToolPolicyContext,
};
use crate::server::routes::{
    inject_degradation_header, log_request, record_virtual_key_usage, set_backend_error_kind,
    try_cache_response, RequestCtx,
};
use crate::server::state::AppState;
use anyllm_translate::{
    anthropic, translate_anthropic_to_openai_response_with_context, AnthropicTranslationContext,
    TranslationWarnings,
};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_anthropic_backend(
    client: &AnthropicClient,
    effective: &AppState,
    state: &AppState,
    anthropic_req: &anthropic::MessageCreateRequest,
    raw_tools: &[serde_json::Value],
    safe_headers: &[(String, String)],
    tool_context: &AnthropicTranslationContext,
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
    let mut upstream_req = anthropic_req.clone();
    upstream_req.model = mapped_model.to_string();
    upstream_req.stream = Some(false);
    if let Err(err) = validate_anthropic_tool_request(
        &upstream_req,
        OpenAiToolPolicyContext {
            backend_kind: backend_kind_for_policy(&effective.backend),
            provider_id: effective.provider_id.as_deref(),
            model: mapped_model,
            provider_catalog: &effective.provider_catalog,
        },
    ) {
        return super::helpers::openai_error_response(
            err.message(),
            "invalid_request_error",
            StatusCode::BAD_REQUEST,
        );
    }
    let body =
        match super::extensions::serialize_anthropic_upstream_request(&upstream_req, raw_tools) {
            Ok(body) => body,
            Err(e) => {
                return super::helpers::openai_error_response(
                    &format!("failed to serialize Anthropic request: {e}"),
                    "server_error",
                    StatusCode::INTERNAL_SERVER_ERROR,
                );
            }
        };
    let refs = super::helpers::header_refs(safe_headers);

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
                                Some(mapped_model.to_string()),
                                StatusCode::BAD_GATEWAY.as_u16(),
                                None,
                                false,
                                Some(format!("failed to parse Anthropic upstream response: {e}")),
                                vk_ctx,
                                None,
                            ),
                        );
                        return super::helpers::openai_error_response(
                            "Upstream Anthropic response could not be parsed.",
                            "server_error",
                            StatusCode::BAD_GATEWAY,
                        );
                    }
                };
            state.metrics.record_success();
            let oai_response = translate_anthropic_to_openai_response_with_context(
                &anthropic_resp,
                original_model,
                tool_context,
            );
            let cost = record_virtual_key_usage(
                &state.shared,
                vk_ctx,
                mapped_model,
                anthropic_resp.usage.input_tokens as u64,
                anthropic_resp.usage.output_tokens as u64,
            );
            log_request(
                &state.shared,
                ctx.log_entry_with_attribution(
                    &state.backend_name,
                    Some(mapped_model.to_string()),
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
            let mut response = (StatusCode::OK, axum::Json(oai_response)).into_response();
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
                Some(mapped_model.to_string()),
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
