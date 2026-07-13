use crate::backend::openai_client::OpenAIClient;
use crate::server::routes::{
    inject_degradation_header, log_request, record_virtual_key_usage, set_backend_error_kind,
    try_cache_response, RequestCtx,
};
use crate::server::state::AppState;
use anyllm_translate::{
    anthropic, mapping, translate_anthropic_to_openai_response, TranslationWarnings,
};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_responses_backend(
    client: &OpenAIClient,
    state: &AppState,
    anthropic_req: &anthropic::MessageCreateRequest,
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
    let mut responses_req =
        mapping::responses_message_map::anthropic_to_responses_request(anthropic_req);
    responses_req.model = mapped_model.to_string();
    let mapped_model = responses_req.model.clone();

    match client.responses(&responses_req).await {
        Ok((resp, _status, rate_limits)) => {
            if let Some(ref d) = deployment {
                d.record_finish(backend_start.elapsed().as_millis() as u64);
            }
            state.metrics.record_success();
            let anthropic_resp = mapping::responses_message_map::responses_to_anthropic_response(
                &resp,
                original_model,
            );
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
