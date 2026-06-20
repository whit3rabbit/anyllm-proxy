use crate::backend::openai_client::OpenAIClient;
use crate::backend::SseFrameBuffer;
use crate::server::middleware::VirtualKeyContext;
use crate::server::state::ToolEngineState;
use anyllm_translate::{anthropic, mapping, openai, ReverseStreamingTranslator};
use futures::StreamExt;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_tool_loop_for_stream(
    mut accumulated_tool_calls: Vec<(String, String, String)>,
    engine: &ToolEngineState,
    anthropic_req_for_tools: &anthropic::MessageCreateRequest,
    client_for_tools: &OpenAIClient,
    omit_stream_options_for_tools: bool,
    cost_model: &str,
    model_for_translator: &str,
    tx: &tokio::sync::mpsc::Sender<Result<String, std::convert::Infallible>>,
    _vk_ctx: &Option<VirtualKeyContext>,
    _log_shared: &Option<crate::admin::state::SharedState>,
) {
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
                input: serde_json::from_str(args).unwrap_or(serde_json::Value::Null),
            })
            .collect();

        let (auto_exec, _pass_through, denied) = crate::tools::execution::partition_tool_calls(
            &tool_calls,
            &engine.registry,
            &engine.policy,
            &server_advertised_tool_names,
        );
        let denied_results = crate::tools::execution::denied_tool_results(&denied);

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
        let assistant_content: Vec<anyllm_translate::anthropic::ContentBlock> = tool_calls
            .iter()
            .map(|tc| anyllm_translate::anthropic::ContentBlock::ToolUse {
                id: tc.id.clone(),
                name: tc.name.clone(),
                input: tc.input.clone(),
            })
            .collect();

        current_messages.push(anyllm_translate::anthropic::InputMessage {
            role: anyllm_translate::anthropic::Role::Assistant,
            content: anyllm_translate::anthropic::Content::Blocks(assistant_content),
        });
        current_messages.push(crate::tools::execution::tool_results_to_user_message(
            &results,
        ));

        let mut follow_up_req = anthropic_req_for_tools.clone();
        follow_up_req.messages = current_messages.clone();

        let mut follow_up_openai =
            mapping::message_map::anthropic_to_openai_request(&follow_up_req);
        follow_up_openai.model = cost_model.to_string();
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
            model_for_translator.to_string(),
        );
        let mut follow_stream_translator =
            mapping::streaming_map::StreamingTranslator::new(model_for_translator.to_string());

        match client_for_tools
            .chat_completion_stream(&follow_up_openai)
            .await
        {
            Ok((follow_resp, _follow_rate_limits)) => {
                let mut follow_byte_stream = follow_resp.bytes_stream();
                let mut follow_buffer = SseFrameBuffer::new();

                while let Some(chunk_result) = follow_byte_stream.next().await {
                    let bytes = match chunk_result {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!("follow-up stream read error: {e}");
                            break;
                        }
                    };
                    let frames = match follow_buffer.push(&bytes) {
                        Ok(frames) => frames,
                        Err(e) => {
                            tracing::error!(error = %e, "follow-up SSE buffer exceeded maximum size");
                            break;
                        }
                    };

                    for frame in frames {
                        if let Ok(frame_str) = std::str::from_utf8(&frame) {
                            for line in frame_str.lines() {
                                let line = line.trim();
                                if let Some(json_str) = line.strip_prefix("data: ") {
                                    if json_str == "[DONE]" {
                                        continue;
                                    }
                                    if let Ok(chunk) =
                                        serde_json::from_str::<openai::ChatCompletionChunk>(
                                            json_str,
                                        )
                                    {
                                        // Accumulate tool calls for the next iteration.
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

                                        let events = follow_stream_translator.process_chunk(&chunk);
                                        for event in &events {
                                            let oai_chunks = follow_translator.process_event(event);
                                            for oai_chunk in &oai_chunks {
                                                if let Ok(json) = serde_json::to_string(oai_chunk) {
                                                    if tx
                                                        .send(Ok(format!("data: {}\n\n", json)))
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
                    }
                }

                // Emit finish events for the follow-up stream.
                let follow_finish = follow_stream_translator.finish();
                for event in &follow_finish {
                    let oai_chunks = follow_translator.process_event(event);
                    for oai_chunk in &oai_chunks {
                        if let Ok(json) = serde_json::to_string(oai_chunk) {
                            let _ = tx.send(Ok(format!("data: {}\n\n", json))).await;
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
