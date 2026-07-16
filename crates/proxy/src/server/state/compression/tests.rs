use super::*;
use crate::admin::state::RuntimeConfig;
use crate::config::{BackendAuth, BackendKind, Config, ModelMapping, OpenAIApiFormat, TlsConfig};
use crate::metrics::Metrics;
use anyllm_optimize_core::Mode;
use anyllm_providers::ProviderCatalog;
use std::sync::{Arc, RwLock};
use tokio::sync::Semaphore;

/// Long enough that FFEC's min-length gate actually has something to compress.
fn long_text() -> String {
    "The quick brown fox jumps over the lazy dog again and again across the wide \
     green field toward the distant blue mountains far beyond the winding river."
        .repeat(4)
}

fn minimal_state(optimizer_mode: Mode) -> AppState {
    let config = Config {
        backend: BackendKind::OpenAI,
        openai_api_key: "test".into(),
        openai_base_url: "https://api.openai.com".into(),
        listen_port: 3000,
        model_mapping: ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("test".into()),
        log_bodies: false,
        redact_secrets: false,
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    };
    let backend = crate::backend::BackendClient::OpenAI(
        crate::backend::openai_client::OpenAIClient::new(&config),
    );
    let runtime_config = Arc::new(RwLock::new(RuntimeConfig {
        model_mappings: indexmap::IndexMap::new(),
        log_level: "info".to_string(),
        log_bodies: false,
        redact_secrets: false,
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
        pxpipe_models: String::new(),
        rtk_compress: false,
        rtk_models: String::new(),
        forward_client_auth: false,
        tool_guardrail_mode: "disabled".to_string(),
        optimizer_mode: optimizer_mode.as_str().to_string(),
        router: Default::default(),
    }));
    AppState {
        backend,
        metrics: Metrics::new(),
        runtime_config,
        shared: None,
        route_options: None,
        backend_name: "openai".to_string(),
        provider_id: None,
        concurrency: Arc::new(Semaphore::new(64)),
        omit_stream_options: false,
        stream_timeout_secs: 0,
        expose_degradation_warnings: false,
        cache: None,
        thinking_repair: None,
        pxpipe: None,
        rtk: None,
        optimizer: Some(Arc::new(crate::optimizer::OptimizerEngine::new(
            optimizer_mode,
        ))),
        model_router: None,
        provider_catalog: Arc::new(ProviderCatalog::bundled()),
        all_backends: None,
        tool_engine: None,
        batch_engine: None,
    }
}

fn long_openai_request() -> openai::ChatCompletionRequest {
    let long = long_text();
    let mut body = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "system", "content": "you are helpful"}],
    });
    let msgs = body["messages"].as_array_mut().unwrap();
    for _ in 0..16 {
        msgs.push(serde_json::json!({"role": "user", "content": long}));
        msgs.push(serde_json::json!({"role": "assistant", "content": long}));
    }
    msgs.push(serde_json::json!({"role": "user", "content": "what is the latest?"}));
    serde_json::from_value(body).expect("valid ChatCompletionRequest")
}

fn long_anthropic_request() -> anthropic::MessageCreateRequest {
    let long = long_text();
    let mut body = serde_json::json!({
        "model": "claude-sonnet-5",
        "max_tokens": 1024,
        "messages": [],
    });
    let msgs = body["messages"].as_array_mut().unwrap();
    for _ in 0..16 {
        msgs.push(serde_json::json!({"role": "user", "content": long}));
        msgs.push(serde_json::json!({"role": "assistant", "content": long}));
    }
    msgs.push(serde_json::json!({"role": "user", "content": "what is the latest?"}));
    serde_json::from_value(body).expect("valid MessageCreateRequest")
}

#[test]
fn shadow_mode_forwards_openai_body_unchanged() {
    let state = minimal_state(Mode::Shadow);
    let mut req = long_openai_request();
    let before = serde_json::to_value(&req).unwrap();
    state.apply_optimizer_to_openai(&mut req, "chat_completions");
    let after = serde_json::to_value(&req).unwrap();
    assert_eq!(before, after, "shadow mode must forward the original body");
    assert_eq!(
        state.metrics.snapshot().optimizer_compressed_total,
        0,
        "shadow mode must never record a metrics-visible compression"
    );
}

#[test]
fn shadow_mode_forwards_anthropic_body_unchanged() {
    let state = minimal_state(Mode::Shadow);
    let mut req = long_anthropic_request();
    let before = serde_json::to_value(&req).unwrap();
    state.apply_optimizer_to_anthropic(&mut req, "messages");
    let after = serde_json::to_value(&req).unwrap();
    assert_eq!(before, after, "shadow mode must forward the original body");
}

#[test]
fn live_mode_compresses_openai_history_and_preserves_latest() {
    let state = minimal_state(Mode::Live);
    let mut req = long_openai_request();
    let before = serde_json::to_value(&req).unwrap();
    state.apply_optimizer_to_openai(&mut req, "chat_completions");
    let after = serde_json::to_value(&req).unwrap();
    assert_eq!(
        before["messages"].as_array().unwrap().last(),
        after["messages"].as_array().unwrap().last(),
        "the latest turn must never be rewritten"
    );
    assert_eq!(
        state.metrics.snapshot().optimizer_compressed_total,
        1,
        "an applied Live compression must be recorded in metrics"
    );
}

#[test]
fn live_mode_compresses_anthropic_history_and_preserves_latest() {
    let state = minimal_state(Mode::Live);
    let mut req = long_anthropic_request();
    let before = serde_json::to_value(&req).unwrap();
    state.apply_optimizer_to_anthropic(&mut req, "messages");
    let after = serde_json::to_value(&req).unwrap();
    assert_eq!(
        before["messages"].as_array().unwrap().last(),
        after["messages"].as_array().unwrap().last(),
        "the latest turn must never be rewritten"
    );
}

#[test]
fn off_mode_is_noop_and_engine_absent_is_noop() {
    // Off mode: engine present, mode Off -> never applied.
    let state = minimal_state(Mode::Off);
    let mut req = long_openai_request();
    let before = serde_json::to_value(&req).unwrap();
    state.apply_optimizer_to_openai(&mut req, "chat_completions");
    assert_eq!(before, serde_json::to_value(&req).unwrap());

    // No engine at all (e.g. non-Anthropic/Translate mode backend): no panic, no-op.
    let mut state_no_engine = minimal_state(Mode::Live);
    state_no_engine.optimizer = None;
    let mut req2 = long_openai_request();
    let before2 = serde_json::to_value(&req2).unwrap();
    state_no_engine.apply_optimizer_to_openai(&mut req2, "chat_completions");
    assert_eq!(before2, serde_json::to_value(&req2).unwrap());
}

#[test]
fn short_history_below_min_len_gate_is_a_noop_not_a_panic() {
    // A short request has nothing worth compressing (below FFEC's min-length
    // gate) -- the seam must still round-trip cleanly without panicking or
    // corrupting the body, i.e. it fails open when there's nothing to do.
    let state = minimal_state(Mode::Live);
    let mut req: openai::ChatCompletionRequest = serde_json::from_value(serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
    }))
    .unwrap();
    let before = serde_json::to_value(&req).unwrap();
    state.apply_optimizer_to_openai(&mut req, "chat_completions");
    assert_eq!(before, serde_json::to_value(&req).unwrap());
}

fn state_with_rtk(enabled: bool) -> AppState {
    let config = Config {
        backend: BackendKind::OpenAI,
        openai_api_key: "test".into(),
        openai_base_url: "https://api.openai.com".into(),
        listen_port: 3000,
        model_mapping: ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: TlsConfig::default(),
        backend_auth: BackendAuth::BearerToken("test".into()),
        log_bodies: false,
        redact_secrets: false,
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
        expose_degradation_warnings: false,
        openai_api_format: OpenAIApiFormat::Chat,
        provider_id: None,
    };
    let backend = crate::backend::BackendClient::OpenAI(
        crate::backend::openai_client::OpenAIClient::new(&config),
    );
    let runtime_config = Arc::new(RwLock::new(RuntimeConfig {
        model_mappings: indexmap::IndexMap::new(),
        log_level: "info".to_string(),
        log_bodies: false,
        redact_secrets: false,
        anthropic_thinking_repair: false,
        pxpipe_compress: false,
        pxpipe_models: String::new(),
        rtk_compress: enabled,
        rtk_models: String::new(),
        forward_client_auth: false,
        tool_guardrail_mode: "disabled".to_string(),
        optimizer_mode: "off".to_string(),
        router: Default::default(),
    }));
    AppState {
        backend,
        metrics: Metrics::new(),
        runtime_config,
        shared: None,
        route_options: None,
        backend_name: "openai".to_string(),
        provider_id: None,
        concurrency: Arc::new(Semaphore::new(64)),
        omit_stream_options: false,
        stream_timeout_secs: 0,
        expose_degradation_warnings: false,
        cache: None,
        thinking_repair: None,
        pxpipe: None,
        rtk: Some(Arc::new(crate::rtk::RtkEngine::new())),
        optimizer: None,
        model_router: None,
        provider_catalog: Arc::new(ProviderCatalog::bundled()),
        all_backends: None,
        tool_engine: None,
        batch_engine: None,
    }
}

fn request_with_noisy_tool_output() -> openai::ChatCompletionRequest {
    let mut noise = String::from("On branch main\nChanges not staged for commit:\n");
    for i in 0..200 {
        noise.push_str(&format!("  (use \"git add ...\" file {i})\n"));
    }
    serde_json::from_value(serde_json::json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "user", "content": "run git status"},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "t1", "type": "function",
                 "function": {"name": "bash", "arguments": "{\"cmd\":\"git status\"}"}}
            ]},
            {"role": "tool", "tool_call_id": "t1", "content": noise},
        ],
    }))
    .expect("valid ChatCompletionRequest")
}

#[test]
fn enabled_compresses_tool_output_and_records_metrics() {
    let state = state_with_rtk(true);
    let mut req = request_with_noisy_tool_output();
    let before = serde_json::to_value(&req).unwrap();
    state.apply_rtk_to_openai(&mut req, "gpt-4o");
    let after = serde_json::to_value(&req).unwrap();
    assert_ne!(before, after, "tool output should have been compressed");
    assert_eq!(state.metrics.snapshot().rtk_compressed_total, 1);
}

#[test]
fn disabled_is_a_noop() {
    let state = state_with_rtk(false);
    let mut req = request_with_noisy_tool_output();
    let before = serde_json::to_value(&req).unwrap();
    state.apply_rtk_to_openai(&mut req, "gpt-4o");
    assert_eq!(before, serde_json::to_value(&req).unwrap());
    assert_eq!(state.metrics.snapshot().rtk_compressed_total, 0);
}

#[test]
fn engine_absent_is_a_noop() {
    let mut state = state_with_rtk(true);
    state.rtk = None;
    let mut req = request_with_noisy_tool_output();
    let before = serde_json::to_value(&req).unwrap();
    state.apply_rtk_to_openai(&mut req, "gpt-4o");
    assert_eq!(before, serde_json::to_value(&req).unwrap());
    assert_eq!(state.metrics.snapshot().rtk_compressed_total, 0);
}
