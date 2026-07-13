//! End-to-end pipeline through the provider adapters: from_value -> optimize -> apply.
//! Asserts I6 (JSON validity / structure preserved) and that protected messages are
//! byte-identical while old history shrinks.

use anyllm_optimize_core::{HeuristicBudgetCounter, Mode, Policy, UniformScorer, Workspace};
use anyllm_optimize_passes::adapter::openai;
use anyllm_optimize_passes::OpenAiStrategy;
use serde_json::json;

fn long_text() -> String {
    "The quick brown fox jumps over the lazy dog again and again across the wide green \
     field toward the distant blue mountains far beyond the winding river and the trees."
        .repeat(2)
}

#[test]
fn openai_pipeline_preserves_structure_and_compresses_history() {
    let long = long_text();
    let mut messages = vec![json!({"role":"system","content":"you are helpful"})];
    for _ in 0..16 {
        messages.push(json!({"role":"user","content": long}));
        messages.push(json!({"role":"assistant","content": long}));
    }
    messages.push(json!({"role":"user","content":"what is the latest?"}));
    let body = json!({"model":"gpt-4o","messages": messages});

    let conv = openai::from_value(&body);
    let policy = Policy {
        mode: Mode::Live,
        ..Default::default()
    };
    let mut ws = Workspace::new();
    let out = anyllm_optimize_core::optimize(
        &conv,
        &policy,
        &OpenAiStrategy::default(),
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut ws,
    );

    let rendered = out
        .rendered
        .expect("live mode should render on a long convo");
    let mut applied = body.clone();
    openai::apply_rendered(&mut applied, &rendered);

    // structure preserved: same message count, still valid, system + latest identical.
    let orig_msgs = body["messages"].as_array().unwrap();
    let new_msgs = applied["messages"].as_array().unwrap();
    assert_eq!(orig_msgs.len(), new_msgs.len());
    assert_eq!(orig_msgs[0], new_msgs[0], "system untouched");
    assert_eq!(
        orig_msgs.last().unwrap(),
        new_msgs.last().unwrap(),
        "latest user message untouched"
    );

    // some early history message got shorter.
    let shrunk = orig_msgs.iter().zip(new_msgs.iter()).any(|(o, n)| {
        n["content"]
            .as_str()
            .is_some_and(|s| s.len() < o["content"].as_str().unwrap_or("").len())
    });
    assert!(shrunk, "at least one history message should be compressed");
}

#[test]
fn shadow_mode_never_mutates_body() {
    let long = long_text();
    let mut messages = vec![];
    for _ in 0..16 {
        messages.push(json!({"role":"user","content": long}));
        messages.push(json!({"role":"assistant","content": long}));
    }
    let body = json!({"model":"gpt-4o","messages": messages});

    let conv = openai::from_value(&body);
    let policy = Policy::default(); // Shadow by default
    let mut ws = Workspace::new();
    let out = anyllm_optimize_core::optimize(
        &conv,
        &policy,
        &OpenAiStrategy::default(),
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut ws,
    );
    assert!(out.rendered.is_none(), "shadow never renders");
    assert!(
        out.report.removed_tokens_est > 0,
        "but it still reports would-be savings"
    );
}
