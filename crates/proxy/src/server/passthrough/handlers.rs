use crate::backend::{BackendClient, MAX_SSE_BUFFER_SIZE};
use crate::openai_tool_policy::{
    backend_kind_for_policy, validate_anthropic_tool_request, OpenAiToolPolicyContext,
};
use crate::server::middleware::ClientAuthPath;
use crate::server::routes::{log_request, record_virtual_key_usage, RequestCtx};
use crate::server::state::{AppState, ConcurrencyPermit};
use crate::server::streaming::{observe_anthropic_sse_frames, AnthropicStreamUsage, StreamOutcome};
use anyllm_translate::{anthropic, mapping};
use axum::{
    body::Bytes,
    extract::{OriginalUri, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use bytes::BytesMut;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::auth::resolve_client_auth_override;

/// Forward an Anthropic-format request byte-for-byte to the upstream Anthropic API.
/// No translation is performed. Only active when `BACKEND=anthropic`.
pub(crate) async fn anthropic_passthrough(
    State(state): State<AppState>,
    permit: Option<axum::Extension<ConcurrencyPermit>>,
    vk_ctx: Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>,
    auth_path: Option<axum::Extension<ClientAuthPath>>,
    claims: Option<axum::Extension<crate::server::oidc::JwtClaims>>,
    headers: axum::http::HeaderMap,
    mut body: Bytes,
) -> Response {
    let permit = permit.map(|axum::Extension(p)| p);
    let vk_ctx = vk_ctx.map(|axum::Extension(c)| c);
    let auth_path = auth_path.map(|axum::Extension(p)| p);
    let claims = claims.map(|axum::Extension(c)| c);
    state.metrics.record_request();

    // Verbatim client-credential override for ANTHROPIC_FORWARD_CLIENT_AUTH:
    // computed once per request, reused across the streaming/non-streaming
    // branches below. Borrowed straight from `headers` (which outlives both
    // branches and is never mutated), not owned -- `forward`/`forward_stream`
    // consume it synchronously before the branch that does
    // `tokio::spawn`, so there's no need to outlive the spawned task.
    let auth_override_ref = resolve_client_auth_override(
        state.forward_client_auth_enabled(),
        auth_path,
        &vk_ctx,
        &claims,
        &headers,
    );

    // Scopes every thinking-repair store lookup/commit to this backend and
    // virtual key: `state.thinking_repair` is one store shared across every
    // Anthropic-mode backend (see server/routes.rs), so without this a
    // colliding message id / thinking signature / tool_use id from a
    // different backend or tenant could resolve to this request's repair.
    // NUL, not `:`, joins the two parts: `state.backend_name` is an
    // operator-configured string whose validated charset (`is_safe_model_name`)
    // allows `:` but not NUL, so a backend literally named e.g. "anthropic:5"
    // can no longer produce the same namespace as backend "anthropic" + key
    // id 5 -- `:` let those collide onto one shared cache-record namespace.
    let thinking_repair_namespace = match &vk_ctx {
        Some(ctx) => format!("{}\u{0}{}", state.backend_name, ctx.key_id),
        None => state.backend_name.clone(),
    };

    let client = match &state.backend {
        BackendClient::Anthropic(c) => c,
        _ => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "Backend is not configured as anthropic passthrough".to_string(),
                None,
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response();
        }
    };

    // Collect Anthropic-specific client headers to forward upstream.
    // anthropic-beta enables beta features; must reach upstream to take effect.
    // x-claude-code-session-id allows upstream and intermediary proxies to correlate sessions.
    let extra_headers: Vec<(&str, &str)> = ["x-claude-code-session-id", "anthropic-beta"]
        .iter()
        .filter_map(|&name| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|v| (name, v))
        })
        .collect();

    // Peek at just the `stream` and `model` fields instead of parsing the full body.
    // Full deserialization would be wasteful for image-heavy requests
    // (up to 32MB) when we only need one boolean to choose the handler.
    #[derive(serde::Deserialize)]
    struct BodyPeek {
        #[serde(default)]
        stream: bool,
        model: Option<String>,
    }
    let peek = serde_json::from_slice::<BodyPeek>(&body).unwrap_or(BodyPeek {
        stream: false,
        model: None,
    });
    let is_stream = peek.stream;
    let ctx = RequestCtx {
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string(),
        start: std::time::Instant::now(),
        model_requested: peek.model.clone().unwrap_or_else(|| "unknown".to_string()),
    };

    // Enforce model allowlist for virtual keys.
    if let Some(ref ctx) = vk_ctx {
        match &peek.model {
            Some(m) => {
                if !crate::server::policy::is_model_allowed(m, &ctx.allowed_models) {
                    let err = mapping::errors_map::create_anthropic_error(
                        anthropic::ErrorType::PermissionError,
                        format!("Model '{}' is not allowed for this API key.", m),
                        None,
                    );
                    return (StatusCode::FORBIDDEN, Json(err)).into_response();
                }
            }
            None => {
                // If a model allowlist is configured, we cannot verify the request
                // is permitted without knowing the model. Reject rather than bypass.
                if ctx.allowed_models.is_some() {
                    let err = mapping::errors_map::create_anthropic_error(
                        anthropic::ErrorType::InvalidRequestError,
                        "Request must include a 'model' field when a model allowlist is configured."
                            .to_string(),
                        None,
                    );
                    return (StatusCode::BAD_REQUEST, Json(err)).into_response();
                }
            }
        }
    }

    // pxpipe compression is decided here but APPLIED after secret redaction
    // below: imaging bakes the system/tool_result text into PNG pixels, which the
    // text-based redactor cannot see, so redaction must run on the raw text first.
    let mut pxpipe_apply: Option<(std::sync::Arc<crate::pxpipe::PxpipeEngine>, String)> = None;
    // RTK tool-output compression, likewise deferred to after redaction and run
    // BEFORE pxpipe so imaging sees the already-filtered text.
    let mut rtk_apply: Option<(std::sync::Arc<crate::rtk::RtkEngine>, String)> = None;
    // FFEC prompt compression (history + frontier cache_control breakpoint). Runs
    // on the raw Value bytes (never round-tripping MessageCreateRequest, which has
    // no cache_control field), so the breakpoint survives to the Anthropic API. The
    // `.filter` gates the (up to 32MB) parse: only capture when the resolved mode is
    // not Off. Does not need the parsed model, so it sits outside the parse block.
    let optimizer_apply = state
        .effective_optimizer()
        .filter(|e| e.mode() != anyllm_optimize_core::Mode::Off);
    if let Ok(mut parsed_req) = serde_json::from_slice::<anthropic::MessageCreateRequest>(&body) {
        if let Err(err) = validate_anthropic_tool_request(
            &parsed_req,
            OpenAiToolPolicyContext {
                backend_kind: backend_kind_for_policy(&state.backend),
                provider_id: state.provider_id.as_deref(),
                model: &parsed_req.model,
                provider_catalog: &state.provider_catalog,
            },
        ) {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::InvalidRequestError,
                err.to_string(),
                None,
            );
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }

        // Repair the last assistant message's thinking blocks against
        // recorded ground truth (opt-in, see crate::thinking_repair). Only
        // rewrites `body` when something actually changed; on a byte-exact
        // replay (the common case) this is a no-op past the store lookups.
        if let Some(store) = state.active_thinking_repair() {
            if let Some(what) = crate::thinking_repair::repair_request(
                &store,
                &thinking_repair_namespace,
                &mut parsed_req,
            )
            .await
            {
                // Patch only the repaired message's `content` into the
                // ORIGINAL raw JSON instead of re-serializing the whole
                // typed request: ContentBlock/Tool have no cache_control or
                // flatten catch-all, so a full-struct round-trip would
                // silently drop cache_control breakpoints and any block/tool
                // type this crate doesn't model yet, on every OTHER message
                // too.
                match crate::thinking_repair::patch_repaired_body(&body, &parsed_req) {
                    Ok(bytes) => {
                        tracing::info!(repair = %what, "anthropic thinking-block repair applied");
                        body = bytes;
                    }
                    Err(e) => {
                        // Fail open: forward the original (unrepaired) bytes
                        // rather than drop the request.
                        tracing::warn!(
                            error = %e,
                            "failed to patch repaired anthropic request; forwarding original body"
                        );
                    }
                }
            }
        }

        // Opt-in text-to-image context compression (pxpipe). Decided here (needs
        // the parsed model for scope/vision gating) but deferred to after
        // redaction — see the `pxpipe_apply` capture note above. The transform
        // works on the raw `body` bytes (Value-level) so it can RELOCATE the
        // caller's cache_control anchor onto the image; it fails open on any error.
        if let Some(engine) = state.pxpipe_engine_for(&parsed_req.model) {
            pxpipe_apply = Some((engine, parsed_req.model.clone()));
        }
        if let Some(engine) = state.rtk_engine_for(&parsed_req.model) {
            rtk_apply = Some((engine, parsed_req.model.clone()));
        }
    }

    let mut body = match crate::server::secret_redaction::redact_body_with_content_type(
        state.redact_secrets(),
        Some("application/json"),
        body,
    )
    .await
    {
        Ok(body) => body,
        Err(err) => return crate::server::secret_redaction::error_response(err),
    };

    // FFEC prompt compression runs FIRST: it compresses conversation history text
    // and places the frontier cache_control breakpoint. It MUST precede pxpipe,
    // which images the static message slab into a PNG — after imaging there is no
    // history text left to compress and no place for a breakpoint. Combining the
    // optimizer's breakpoint with pxpipe's cache_control-anchor relocation is
    // untested (both are opt-in); we don't try to reconcile them here. Fails open.
    if let Some(engine) = optimizer_apply {
        body = engine.optimize_anthropic_bytes(body, "messages", &state.metrics);
    }

    // Now that secrets are redacted, compress tool output (RTK) then image the
    // static slab / large live regions (pxpipe). RTK first so pxpipe images the
    // already-filtered text; both fail open.
    if let Some((engine, model)) = rtk_apply {
        body = engine.compress_anthropic(body, &model, &state.metrics);
    }
    if let Some((engine, model)) = pxpipe_apply {
        body = engine.compress_anthropic(body, &model, &state.metrics);
    }

    if is_stream {
        match client
            .forward_stream(body, &extra_headers, auth_override_ref)
            .await
        {
            Ok((response, rate_limits)) => {
                let (tx, rx) = mpsc::channel::<Result<Bytes, std::convert::Infallible>>(32);
                let metrics = state.metrics.clone();
                let log_shared = state.shared.clone();
                let log_backend_name = state.backend_name.clone();
                let cost_model = peek.model.clone().unwrap_or_else(|| "unknown".to_string());
                // Captured once, before the stream starts: a toggle mid-stream
                // must not half-record ground truth.
                let thinking_repair = state.active_thinking_repair();
                let thinking_repair_namespace = thinking_repair_namespace.clone();

                tokio::spawn(async move {
                    let _permit = permit;
                    metrics.record_stream_started();
                    let mut byte_stream = response.bytes_stream();
                    let mut buffer = BytesMut::new();
                    let mut search_from = 0;
                    let mut usage = AnthropicStreamUsage::default();
                    let mut outcome = StreamOutcome::Completed;
                    let mut recorder = thinking_repair
                        .is_some()
                        .then(crate::thinking_repair::ThinkingRecorder::new);
                    let mut ready_to_commit: Vec<(String, Vec<anthropic::ContentBlock>)> =
                        Vec::new();

                    while let Some(chunk_result) = byte_stream.next().await {
                        let bytes = match chunk_result {
                            Ok(b) => b,
                            Err(e) => {
                                tracing::error!("Anthropic passthrough stream read error: {e}");
                                metrics.record_error();
                                outcome = StreamOutcome::UpstreamError;
                                break;
                            }
                        };

                        if tx.send(Ok(bytes.clone())).await.is_err() {
                            outcome = StreamOutcome::ClientDisconnected;
                            break;
                        }

                        if buffer.len() + bytes.len() > MAX_SSE_BUFFER_SIZE {
                            tracing::error!(
                                buffer_len = buffer.len(),
                                "Anthropic passthrough SSE buffer exceeded maximum size"
                            );
                            metrics.record_error();
                            outcome = StreamOutcome::UpstreamError;
                            break;
                        }
                        buffer.extend_from_slice(&bytes);
                        observe_anthropic_sse_frames(
                            &mut buffer,
                            &mut search_from,
                            &mut usage,
                            recorder.as_mut(),
                            &mut ready_to_commit,
                        );
                    }

                    if let Some(store) = &thinking_repair {
                        for (msg_id, blocks) in ready_to_commit {
                            store
                                .commit(&thinking_repair_namespace, &msg_id, blocks)
                                .await;
                        }
                    }

                    let tokens = usage.tokens();
                    let cost = tokens.map(|(input_t, output_t)| {
                        record_virtual_key_usage(
                            &log_shared,
                            &vk_ctx,
                            &cost_model,
                            input_t,
                            output_t,
                        )
                    });
                    let (status, err) = outcome.record(&metrics);
                    log_request(
                        &log_shared,
                        ctx.log_entry_with_attribution(
                            &log_backend_name,
                            Some(cost_model),
                            status,
                            tokens,
                            true,
                            err,
                            &vk_ctx,
                            cost,
                        ),
                    );
                });

                let stream = ReceiverStream::new(rx);
                let mut resp = axum::body::Body::from_stream(stream).into_response();
                resp.headers_mut()
                    .insert("content-type", "text/event-stream".parse().unwrap());
                resp.headers_mut()
                    .insert("cache-control", "no-cache".parse().unwrap());
                rate_limits.inject_anthropic_response_headers(resp.headers_mut());
                resp
            }
            Err(e) => {
                state.metrics.record_error();
                let status = passthrough_error_status(&e);
                log_request(
                    &state.shared,
                    ctx.log_entry_with_attribution(
                        &state.backend_name,
                        peek.model.clone(),
                        status,
                        None,
                        true,
                        Some(e.to_string()),
                        &vk_ctx,
                        None,
                    ),
                );
                passthrough_error_to_response(e)
            }
        }
    } else {
        match client
            .forward(body, &extra_headers, auth_override_ref)
            .await
        {
            Ok((resp_body, rate_limits)) => {
                // Parsed once and shared between thinking-repair recording
                // and virtual-key accounting below (previously each parsed
                // the same bytes independently, and recording additionally
                // cloned the whole content Vec instead of moving it out).
                let mut parsed_resp =
                    serde_json::from_slice::<anthropic::MessageResponse>(&resp_body);

                if let Some(store) = state.active_thinking_repair() {
                    if let Ok(resp) = parsed_resp.as_mut() {
                        let content = std::mem::take(&mut resp.content);
                        let msg_id = resp.id.clone();
                        crate::thinking_repair::record_response(
                            &store,
                            &thinking_repair_namespace,
                            &msg_id,
                            content,
                        )
                        .await;
                    }
                }
                if vk_ctx.is_some() {
                    let anthropic_resp = match parsed_resp {
                        Ok(resp) => resp,
                        Err(e) => {
                            state.metrics.record_error();
                            log_request(
                                &state.shared,
                                ctx.log_entry_with_attribution(
                                    &state.backend_name,
                                    peek.model.clone(),
                                    StatusCode::BAD_GATEWAY.as_u16(),
                                    None,
                                    false,
                                    Some(format!(
                                        "failed to parse upstream usage for virtual key accounting: {e}"
                                    )),
                                    &vk_ctx,
                                    None,
                                ),
                            );
                            return virtual_key_accounting_parse_error();
                        }
                    };
                    state.metrics.record_success();
                    let tokens = (
                        anthropic_resp.usage.input_tokens as u64,
                        anthropic_resp.usage.output_tokens as u64,
                    );
                    let cost = record_virtual_key_usage(
                        &state.shared,
                        &vk_ctx,
                        &anthropic_resp.model,
                        tokens.0,
                        tokens.1,
                    );
                    log_request(
                        &state.shared,
                        ctx.log_entry_with_attribution(
                            &state.backend_name,
                            Some(anthropic_resp.model),
                            200,
                            Some(tokens),
                            false,
                            None,
                            &vk_ctx,
                            Some(cost),
                        ),
                    );
                } else {
                    state.metrics.record_success();
                }
                let mut resp = (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    resp_body,
                )
                    .into_response();
                rate_limits.inject_anthropic_response_headers(resp.headers_mut());
                resp
            }
            Err(e) => {
                state.metrics.record_error();
                let status = passthrough_error_status(&e);
                log_request(
                    &state.shared,
                    ctx.log_entry_with_attribution(
                        &state.backend_name,
                        peek.model.clone(),
                        status,
                        None,
                        false,
                        Some(e.to_string()),
                        &vk_ctx,
                        None,
                    ),
                );
                passthrough_error_to_response(e)
            }
        }
    }
}

/// Generic catch-all passthrough for any /v1/* path in Anthropic mode.
/// Forwards batch, file CRUD, count_tokens, and other Anthropic-native endpoints
/// directly to the upstream Anthropic API. Registered after /v1/messages so that
/// route retains its dedicated streaming/model-peek logic.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn anthropic_generic_passthrough(
    State(state): State<AppState>,
    vk_ctx: Option<axum::Extension<crate::server::middleware::VirtualKeyContext>>,
    auth_path: Option<axum::Extension<ClientAuthPath>>,
    claims: Option<axum::Extension<crate::server::oidc::JwtClaims>>,
    OriginalUri(uri): OriginalUri,
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    state.metrics.record_request();

    // Virtual keys must use policy-aware handlers only. The generic passthrough
    // can reach Anthropic-native endpoints that lack per-key authorization,
    // resource ownership checks, and usage accounting.
    if vk_ctx.is_some() {
        let err = mapping::errors_map::create_anthropic_error(
            anthropic::ErrorType::PermissionError,
            "This endpoint is not available for virtual API keys.".to_string(),
            None,
        );
        return (StatusCode::FORBIDDEN, Json(err)).into_response();
    }
    let vk_ctx = vk_ctx.map(|axum::Extension(c)| c);
    let auth_path = auth_path.map(|axum::Extension(p)| p);
    let claims = claims.map(|axum::Extension(c)| c);
    // vk_ctx is already rejected above (so resolve_client_auth_override's
    // vk_ctx.is_none() check is always true here), but an OIDC-authenticated
    // non-virtual-key request can still reach here, so this must also be
    // gated on claims/auth_path (never forward a JWT upstream as if it were
    // an Anthropic credential).
    let auth_override_ref = resolve_client_auth_override(
        state.forward_client_auth_enabled(),
        auth_path,
        &vk_ctx,
        &claims,
        &headers,
    );

    let client = match &state.backend {
        BackendClient::Anthropic(c) => c,
        _ => {
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "Backend is not configured as anthropic passthrough".to_string(),
                None,
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(err)).into_response();
        }
    };

    // Build full path with query string preserved.
    let mut full_path = uri.path().to_string();
    if let Some(q) = uri.query() {
        full_path.push('?');
        full_path.push_str(q);
    }

    // Collect owned Strings before building the &str slice (lifetime requirement).
    let session_id = headers
        .get("x-claude-code-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let beta = headers
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut extra: Vec<(&str, &str)> = Vec::new();
    if let Some(ref v) = session_id {
        extra.push(("x-claude-code-session-id", v));
    }
    if let Some(ref v) = beta {
        extra.push(("anthropic-beta", v));
    }

    let body =
        match crate::server::secret_redaction::redact_body(state.redact_secrets(), &headers, body)
            .await
        {
            Ok(body) => body,
            Err(err) => return crate::server::secret_redaction::error_response(err),
        };

    match client
        .forward_generic(method, &full_path, body, &extra, auth_override_ref)
        .await
    {
        Ok(response) => {
            let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::OK);
            if status.is_success() {
                state.metrics.record_success();
            } else {
                state.metrics.record_error();
            }
            // Preserve upstream content-type (batches return application/x-jsonl, etc.)
            let upstream_ct = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_string();
            let stream = response.bytes_stream();
            let axum_body = axum::body::Body::from_stream(stream);
            let mut resp = (status, axum_body).into_response();
            if let Ok(hv) = axum::http::HeaderValue::from_str(&upstream_ct) {
                resp.headers_mut().insert("content-type", hv);
            }
            resp
        }
        Err(e) => {
            state.metrics.record_error();
            passthrough_error_to_response(e)
        }
    }
}

/// Convert an AnthropicClientError into a Response.
/// For API errors, return the upstream error body directly (it's already Anthropic format).
fn passthrough_error_to_response(
    error: crate::backend::anthropic_client::AnthropicClientError,
) -> Response {
    use crate::backend::anthropic_client::AnthropicClientError;
    match error {
        AnthropicClientError::ApiError { status, body } => {
            let http_status =
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (http_status, [("content-type", "application/json")], body).into_response()
        }
        AnthropicClientError::Transport(msg) => {
            tracing::error!("Anthropic passthrough transport error: {msg}");
            let err = mapping::errors_map::create_anthropic_error(
                anthropic::ErrorType::ApiError,
                "An internal error occurred while communicating with the upstream service."
                    .to_string(),
                None,
            );
            (StatusCode::BAD_GATEWAY, Json(err)).into_response()
        }
    }
}

fn passthrough_error_status(error: &crate::backend::anthropic_client::AnthropicClientError) -> u16 {
    match error {
        crate::backend::anthropic_client::AnthropicClientError::ApiError { status, .. } => *status,
        crate::backend::anthropic_client::AnthropicClientError::Transport(_) => {
            StatusCode::BAD_GATEWAY.as_u16()
        }
    }
}

fn virtual_key_accounting_parse_error() -> Response {
    let err = mapping::errors_map::create_anthropic_error(
        anthropic::ErrorType::ApiError,
        "Upstream response could not be accounted for this virtual API key.".to_string(),
        None,
    );
    (StatusCode::BAD_GATEWAY, Json(err)).into_response()
}
