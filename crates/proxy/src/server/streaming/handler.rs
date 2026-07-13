use crate::backend::{BackendClient, RateLimitHeaders};
use crate::openai_tool_policy::{
    backend_kind_for_policy, parse_openai_chat_completion_chunk, prepare_openai_tool_request,
    tool_policy_error_to_backend_error, OpenAiToolPolicyContext,
};
use crate::server::routes::{log_request, set_backend_error_kind, RequestCtx};
use crate::server::state::AppState;
use anyllm_translate::{anthropic, mapping, TranslationWarnings};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::helpers::{read_sse_frames, send_events, StreamDeploymentAccounting, StreamOutcome};

pub(crate) async fn messages_stream(
    state: AppState,
    body: anthropic::MessageCreateRequest,
    ctx: RequestCtx,
    mapped_model: String,
    concurrency_permit: Option<crate::server::state::ConcurrencyPermit>,
    vk_ctx: Option<crate::server::middleware::VirtualKeyContext>,
    deployment_accounting: StreamDeploymentAccounting,
) -> Result<
    (
        RateLimitHeaders,
        Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>,
    ),
    crate::backend::BackendError,
> {
    let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(32);
    let (rl_tx, rl_rx) =
        tokio::sync::oneshot::channel::<Result<RateLimitHeaders, crate::backend::BackendError>>();

    let metrics = state.metrics.clone();
    let log_shared = state.shared.clone();
    let log_backend_name = state.backend_name.clone();
    let stream_timeout_secs = state.stream_timeout_secs;

    match &state.backend {
        BackendClient::OpenAI(client)
        | BackendClient::AzureOpenAI(client)
        | BackendClient::Vertex(client)
        | BackendClient::GeminiOpenAI(client) => {
            let client = client.clone();
            let mut openai_req = mapping::message_map::anthropic_to_openai_request(&body);
            crate::server::routes::inject_gemini_thinking(&body, &state.backend, &mut openai_req);
            crate::server::routes::inject_glm_thinking(&body, &state.backend, &mut openai_req);
            if state.omit_stream_options {
                openai_req.stream_options = None;
            }
            openai_req.model = mapped_model.clone();
            let policy_model = openai_req.model.clone();
            let mut policy_warnings = TranslationWarnings::default();
            if let Err(err) = prepare_openai_tool_request(
                &mut openai_req,
                OpenAiToolPolicyContext {
                    backend_kind: backend_kind_for_policy(&state.backend),
                    provider_id: state.provider_id.as_deref(),
                    model: &policy_model,
                    provider_catalog: &state.provider_catalog,
                },
                &mut policy_warnings,
            ) {
                return Err(tool_policy_error_to_backend_error(err));
            }

            state.apply_rtk_to_openai(&mut openai_req, &mapped_model);

            let model = body.model.clone();
            let permit = concurrency_permit.clone();
            let mut deployment_accounting = deployment_accounting;

            tokio::spawn(async move {
                let _permit = permit;
                metrics.record_stream_started();
                match client.chat_completion_stream(&openai_req).await {
                    Ok((response, rate_limits)) => {
                        rl_tx.send(Ok(rate_limits)).ok();
                        let mut translator =
                            mapping::streaming_map::StreamingTranslator::new(model);
                        let mut done = false;

                        let stream_future = read_sse_frames(response, &tx, &metrics, |json_str| {
                            if json_str == "[DONE]" {
                                done = true;
                                let events = translator.finish();
                                return Some(events);
                            }
                            match parse_openai_chat_completion_chunk(json_str) {
                                Ok(chunk) => Some(translator.process_chunk(&chunk)),
                                Err(e) => {
                                    tracing::debug!("failed to parse OpenAI streaming chunk: {e}");
                                    None
                                }
                            }
                        });
                        let outcome = if stream_timeout_secs > 0 {
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(stream_timeout_secs),
                                stream_future,
                            )
                            .await
                            {
                                Ok(o) => o,
                                Err(_) => {
                                    tracing::warn!(
                                        timeout_secs = stream_timeout_secs,
                                        "streaming response exceeded wall-clock timeout"
                                    );
                                    StreamOutcome::UpstreamError
                                }
                            }
                        } else {
                            stream_future.await
                        };

                        if matches!(outcome, StreamOutcome::Completed) && !done {
                            let events = translator.finish();
                            send_events(&tx, &events).await;
                        }
                        let usage = translator.usage();
                        let tokens = usage.map(|u| (u.input_tokens as u64, u.output_tokens as u64));
                        let cost = tokens.map(|(input_t, output_t)| {
                            crate::server::routes::record_virtual_key_usage(
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
                        deployment_accounting.finish();
                    }
                    Err(e) => {
                        let backend_error = crate::backend::BackendError::from(e);
                        metrics.record_error();
                        let mut entry = ctx.log_entry_with_attribution(
                            &log_backend_name,
                            Some(mapped_model),
                            backend_error.status_code(),
                            None,
                            true,
                            Some(backend_error.to_string()),
                            &vk_ctx,
                            None,
                        );
                        set_backend_error_kind(&mut entry, &backend_error);
                        log_request(&log_shared, entry);
                        let _ = rl_tx.send(Err(backend_error));
                        deployment_accounting.finish();
                    }
                }
            });
        }
        BackendClient::OpenAIResponses(client) => {
            let client = client.clone();
            let mut responses_req =
                mapping::responses_message_map::anthropic_to_responses_request(&body);
            responses_req.model = mapped_model.clone();
            responses_req.stream = Some(true);
            let model = body.model.clone();
            let permit = concurrency_permit;
            let mut deployment_accounting = deployment_accounting;

            tokio::spawn(async move {
                let _permit = permit;
                metrics.record_stream_started();
                match client.responses_stream(&responses_req).await {
                    Ok((response, rate_limits)) => {
                        rl_tx.send(Ok(rate_limits)).ok();
                        let mut translator =
                            mapping::responses_streaming_map::ResponsesStreamingTranslator::new(
                                model,
                            );

                        let stream_future = read_sse_frames(response, &tx, &metrics, |json_str| {
                            match serde_json::from_str::<
                                mapping::responses_streaming_map::ResponsesStreamEvent,
                            >(json_str)
                            {
                                Ok(event) => Some(translator.process_event(&event)),
                                Err(e) => {
                                    tracing::debug!(
                                        "failed to parse Responses API streaming event: {e}"
                                    );
                                    None
                                }
                            }
                        });
                        let outcome = if stream_timeout_secs > 0 {
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(stream_timeout_secs),
                                stream_future,
                            )
                            .await
                            {
                                Ok(o) => o,
                                Err(_) => {
                                    tracing::warn!(
                                        timeout_secs = stream_timeout_secs,
                                        "streaming response exceeded wall-clock timeout"
                                    );
                                    StreamOutcome::UpstreamError
                                }
                            }
                        } else {
                            stream_future.await
                        };

                        if matches!(outcome, StreamOutcome::Completed) {
                            let events = translator.finish();
                            send_events(&tx, &events).await;
                        }
                        let usage = translator.usage();
                        let tokens = usage.map(|u| (u.input_tokens as u64, u.output_tokens as u64));
                        let cost = tokens.map(|(input_t, output_t)| {
                            crate::server::routes::record_virtual_key_usage(
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
                        deployment_accounting.finish();
                    }
                    Err(e) => {
                        let backend_error = crate::backend::BackendError::from(e);
                        metrics.record_error();
                        let mut entry = ctx.log_entry_with_attribution(
                            &log_backend_name,
                            Some(mapped_model),
                            backend_error.status_code(),
                            None,
                            true,
                            Some(backend_error.to_string()),
                            &vk_ctx,
                            None,
                        );
                        set_backend_error_kind(&mut entry, &backend_error);
                        log_request(&log_shared, entry);
                        let _ = rl_tx.send(Err(backend_error));
                        deployment_accounting.finish();
                    }
                }
            });
        }
        BackendClient::Anthropic(_)
        | BackendClient::Bedrock(_)
        | BackendClient::GeminiNative(_) => {
            drop(rl_tx);
            let _ = tx
                .send(Ok(Event::default().data(
                    r#"{"error":"this backend does not use the translation streaming handler"}"#,
                )))
                .await;
        }
    }

    match rl_rx.await {
        Ok(Ok(rate_limits)) => Ok((
            rate_limits,
            Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()),
        )),
        Ok(Err(backend_err)) => Err(backend_err),
        Err(_) => Ok((
            RateLimitHeaders::default(),
            Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()),
        )),
    }
}
