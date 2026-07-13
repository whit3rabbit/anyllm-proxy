use super::*;
use serde_json::json;

fn long_text() -> String {
    "The quick brown fox jumps over the lazy dog again and again across the wide \
     green field toward the distant blue mountains far beyond the winding river."
        .repeat(2)
}

fn long_openai_body() -> Value {
    let long = long_text();
    let mut messages = vec![json!({"role":"system","content":"you are helpful"})];
    for _ in 0..16 {
        messages.push(json!({"role":"user","content": long}));
        messages.push(json!({"role":"assistant","content": long}));
    }
    messages.push(json!({"role":"user","content":"what is the latest?"}));
    json!({"model":"gpt-4o","messages": messages})
}

#[test]
fn resolve_default_mode_unset_is_off() {
    // Deliberately does not touch process env: relies on OPTIMIZER_MODE being
    // absent by default in the test environment.
    if std::env::var("OPTIMIZER_MODE").is_err() {
        assert_eq!(resolve_default_mode(), Mode::Off);
    }
}

#[test]
fn off_mode_never_mutates_and_reports_off() {
    let mut body = long_openai_body();
    let before = body.clone();
    let engine = OptimizerEngine::new(Mode::Off);
    let report = engine.optimize_openai(&mut body, "chat_completions");
    assert_eq!(body, before, "off mode must not mutate the body");
    assert_eq!(report.mode, Mode::Off);
    assert!(!report.applied);
}

#[test]
fn shadow_mode_never_mutates_but_reports_savings() {
    let mut body = long_openai_body();
    let before = body.clone();
    let engine = OptimizerEngine::new(Mode::Shadow);
    let report = engine.optimize_openai(&mut body, "chat_completions");
    assert_eq!(body, before, "shadow mode must not mutate the body");
    assert!(
        report.removed_tokens_est > 0,
        "shadow should still report would-be savings on a long convo"
    );
}

#[test]
fn live_mode_compresses_openai_history_and_preserves_latest_and_system() {
    let mut body = long_openai_body();
    let orig_msgs = body["messages"].as_array().unwrap().clone();
    let engine = OptimizerEngine::new(Mode::Live);
    let report = engine.optimize_openai(&mut body, "chat_completions");
    assert!(report.applied || report.rewrite_suffix_tokens == 0);

    let new_msgs = body["messages"].as_array().unwrap();
    assert_eq!(orig_msgs.len(), new_msgs.len());
    assert_eq!(orig_msgs[0], new_msgs[0], "system untouched");
    assert_eq!(
        orig_msgs.last().unwrap(),
        new_msgs.last().unwrap(),
        "latest message untouched"
    );
}

#[test]
fn live_mode_compresses_anthropic_history_and_preserves_latest() {
    let long = long_text();
    let mut messages = vec![];
    for _ in 0..16 {
        messages.push(json!({"role":"user","content": long}));
        messages.push(json!({"role":"assistant","content": long}));
    }
    messages.push(json!({"role":"user","content":"what is the latest?"}));
    let mut body = json!({
        "model":"claude-sonnet-5",
        "system":"you are helpful",
        "messages": messages,
    });
    let orig_msgs = body["messages"].as_array().unwrap().clone();
    let orig_system = body["system"].clone();

    let engine = OptimizerEngine::new(Mode::Live);
    let report = engine.optimize_anthropic(&mut body, "messages");

    assert_eq!(body["system"], orig_system, "system field untouched");
    let new_msgs = body["messages"].as_array().unwrap();
    assert_eq!(orig_msgs.len(), new_msgs.len());
    assert_eq!(
        orig_msgs.last().unwrap(),
        new_msgs.last().unwrap(),
        "latest message untouched"
    );
    // Live mode must place the deepest cache breakpoint at the frontier, not just
    // compress text (crates/optimizer/CLAUDE.md checklist item 5).
    assert!(
        report.frontier > 0,
        "long history must have a nonzero frontier"
    );
    let bp_idx = report.frontier - 1;
    let bp_msg = &new_msgs[bp_idx];
    let has_marker = bp_msg["content"]
        .as_array()
        .is_some_and(|arr| arr.iter().any(|b| b.get("cache_control").is_some()));
    assert!(
        has_marker,
        "expected a cache_control breakpoint on message {bp_idx}, got {bp_msg}"
    );
}

fn long_anthropic_body() -> Value {
    let long = long_text();
    let mut messages = vec![];
    for _ in 0..16 {
        messages.push(json!({"role":"user","content": long}));
        messages.push(json!({"role":"assistant","content": long}));
    }
    messages.push(json!({"role":"user","content":"what is the latest?"}));
    json!({
        "model":"claude-sonnet-5",
        "system":"you are helpful",
        "messages": messages,
    })
}

#[test]
fn optimize_anthropic_bytes_live_shrinks_and_keeps_cache_control() {
    let body = Bytes::from(serde_json::to_vec(&long_anthropic_body()).unwrap());
    let metrics = crate::metrics::Metrics::new();
    let engine = OptimizerEngine::new(Mode::Live);
    let out = engine.optimize_anthropic_bytes(body.clone(), "messages", &metrics);
    assert!(out.len() < body.len(), "live output must be smaller");
    // The frontier cache_control breakpoint must survive to the wire bytes --
    // the whole point of doing this on Bytes instead of the typed round-trip.
    let root: Value = serde_json::from_slice(&out).unwrap();
    let has_marker = root["messages"].as_array().unwrap().iter().any(|m| {
        m["content"]
            .as_array()
            .is_some_and(|arr| arr.iter().any(|b| b.get("cache_control").is_some()))
    });
    assert!(
        has_marker,
        "cache_control breakpoint dropped from wire bytes"
    );
    assert_eq!(metrics.snapshot().optimizer_compressed_total, 1);
}

#[test]
fn optimize_anthropic_bytes_off_is_noop() {
    let body = Bytes::from(serde_json::to_vec(&long_anthropic_body()).unwrap());
    let metrics = crate::metrics::Metrics::new();
    let engine = OptimizerEngine::new(Mode::Off);
    let out = engine.optimize_anthropic_bytes(body.clone(), "messages", &metrics);
    assert_eq!(out, body, "off mode must return the body unchanged");
    assert_eq!(metrics.snapshot().optimizer_compressed_total, 0);
}

#[test]
fn optimize_anthropic_bytes_fails_open_on_garbage() {
    let body = Bytes::from_static(b"not json");
    let metrics = crate::metrics::Metrics::new();
    let engine = OptimizerEngine::new(Mode::Live);
    let out = engine.optimize_anthropic_bytes(body.clone(), "messages", &metrics);
    assert_eq!(out, body, "garbage body returned unchanged");
    assert_eq!(metrics.snapshot().optimizer_compressed_total, 0);
}

#[test]
fn with_mode_override_prefers_override() {
    let engine = OptimizerEngine::new(Mode::Off);
    let overridden = engine.with_mode_override("live");
    assert_eq!(overridden.policy.mode, Mode::Live);
    // The original engine is untouched.
    assert_eq!(engine.policy.mode, Mode::Off);
}

#[test]
fn with_mode_override_falls_back_on_unparseable_value() {
    let engine = OptimizerEngine::new(Mode::Shadow);
    let overridden = engine.with_mode_override("not-a-real-mode");
    assert_eq!(overridden.policy.mode, Mode::Shadow);
}

#[test]
fn resolve_runtime_optimizer_prefers_runtime_override() {
    let engine = OptimizerEngine::new(Mode::Off);
    let resolved = resolve_runtime_optimizer(&engine, "live");
    assert_eq!(resolved.policy.mode, Mode::Live);
}

#[test]
fn resolve_runtime_optimizer_keeps_static_when_modes_match() {
    let engine = OptimizerEngine::new(Mode::Shadow);
    let resolved = resolve_runtime_optimizer(&engine, "shadow");
    assert_eq!(resolved.policy.mode, Mode::Shadow);
}

#[test]
fn resolve_runtime_optimizer_falls_back_on_unparseable_value() {
    let engine = OptimizerEngine::new(Mode::Shadow);
    let resolved = resolve_runtime_optimizer(&engine, "not-a-real-mode");
    assert_eq!(resolved.policy.mode, Mode::Shadow);
}

#[test]
fn route_precedence_wins_over_runtime_and_static() {
    // Full three-tier precedence, mirroring `effective_optimizer`'s own logic:
    // static (env-seeded) engine mode is Off, runtime tier says Shadow, but a
    // route-level override of Live must win over both.
    let static_engine = OptimizerEngine::new(Mode::Off);
    let runtime_tier = resolve_runtime_optimizer(&static_engine, "shadow");
    assert_eq!(
        runtime_tier.policy.mode,
        Mode::Shadow,
        "runtime beats static"
    );

    // Route override is applied directly against the static engine (as
    // `effective_optimizer` does), bypassing the runtime tier entirely.
    let route_tier = static_engine.with_mode_override("live");
    assert_eq!(
        route_tier.policy.mode,
        Mode::Live,
        "route beats runtime and static"
    );
}

#[test]
fn fails_open_on_malformed_messages_field() {
    // "messages" is a string, not an array — adapters degrade to an empty
    // Conversation rather than panicking, so this must not mutate or panic.
    let mut body = json!({"model":"gpt-4o","messages":"not an array"});
    let before = body.clone();
    let engine = OptimizerEngine::new(Mode::Live);
    let report = engine.optimize_openai(&mut body, "chat_completions");
    assert_eq!(body, before);
    assert!(!report.applied);
}
