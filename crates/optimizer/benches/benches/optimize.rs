//! Criterion bench. Phase-1 exit target: p99 no-ML optimizer overhead < 1ms.
//! Covers all ROADMAP §7 corpus classes: prose (short chat + 100-turn convo), RAG,
//! tool-heavy, JSON, markdown, code. Perf envelope documented in benches/README.md.

use anyllm_optimize_benches::{
    code_conversation, json_conversation, markdown_conversation, prose_conversation,
    rag_conversation, tool_conversation,
};
use anyllm_optimize_core::{
    optimize, CacheModel, CacheStrategy, Conversation, HeuristicBudgetCounter, Mode, Policy,
    Pricing, UniformScorer, Workspace,
};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

struct Strat;
impl CacheStrategy for Strat {
    fn pricing(&self) -> Pricing {
        Pricing {
            input: 3.0,
            cached_read: 0.3,
            cache_write_mult: 1.25,
        }
    }
    fn model(&self) -> CacheModel {
        CacheModel::ExplicitBreakpoints
    }
    fn breakpoint_at(&self, f: usize) -> Option<usize> {
        Some(f)
    }
}

fn bench_optimize(c: &mut Criterion) {
    let policy = Policy {
        mode: Mode::Live,
        ..Default::default()
    };
    let budget = HeuristicBudgetCounter::default();

    let mut group = c.benchmark_group("optimize_prose_uniform");
    for turns in [4usize, 20, 100] {
        let conv = prose_conversation(turns);
        group.bench_with_input(BenchmarkId::from_parameter(turns), &conv, |b, conv| {
            let mut ws = Workspace::new();
            b.iter(|| {
                let out = optimize(conv, &policy, &Strat, &UniformScorer, &budget, &mut ws);
                std::hint::black_box(out.report.decisions_hash);
            });
        });
    }
    group.finish();
}

/// One representative instance per ROADMAP §7 corpus class, benched under the same policy.
fn bench_corpus_classes(c: &mut Criterion) {
    let policy = Policy {
        mode: Mode::Live,
        ..Default::default()
    };
    let budget = HeuristicBudgetCounter::default();

    let classes: Vec<(&str, Conversation)> = vec![
        ("rag_1mb", rag_conversation(1024)),
        ("tool_heavy", tool_conversation(16)),
        ("json", json_conversation(16)),
        ("markdown", markdown_conversation(20)),
        ("code", code_conversation(20)),
    ];

    let mut group = c.benchmark_group("optimize_corpus_classes");
    for (name, conv) in &classes {
        group.bench_with_input(BenchmarkId::from_parameter(name), conv, |b, conv| {
            let mut ws = Workspace::new();
            b.iter(|| {
                let out = optimize(conv, &policy, &Strat, &UniformScorer, &budget, &mut ws);
                std::hint::black_box(out.report.decisions_hash);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_optimize, bench_corpus_classes);
criterion_main!(benches);
