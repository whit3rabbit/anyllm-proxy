use super::{build_scorer, compress, ensure_model, load_inputs, Api, Args};
use anyhow::Result;
use anyllm_optimize_core::{net_cost_delta_usd, CacheModel, CacheStrategy};
use anyllm_optimize_passes::{AnthropicStrategy, OpenAiStrategy};

/// EH-0002 bite 2: simulate the frozen-frontier policy over `--input` and print the
/// signed net USD delta (compress vs skip) the cost gate computes per row, plus a class
/// total. Fully offline (no network) — the ΔT/S the gate uses come straight out of the
/// same `optimize()` call the harness already runs, so this is not a re-derivation, it is
/// the actual decision the frontier + cost gate would make.
pub fn run_net_cost(args: &Args) -> Result<()> {
    let bodies = load_inputs(args)?;
    let scorer = build_scorer(args);
    let (pricing, model) = match args.cost_model.unwrap_or(args.api) {
        Api::Openai => (
            OpenAiStrategy::default().pricing(),
            OpenAiStrategy::default().model(),
        ),
        Api::Anthropic => (
            AnthropicStrategy::default().pricing(),
            AnthropicStrategy::default().model(),
        ),
    };
    println!(
        "{:<4} {:>9} {:>7} {:>7} {:>7} {:>14}",
        "#", "frontier", "dt", "s", "apply", "net_usd"
    );
    let mut total_dt = 0u64;
    let mut total_net = 0.0f64;
    for (i, mut raw) in bodies.into_iter().enumerate() {
        ensure_model(&mut raw, args);
        // dt/s come from `--api`'s adapter (must match the input's wire shape, or tool/JSON
        // blocks get mis-segmented — see cost_model doc comment); the apply decision is
        // then re-derived under `--cost-model`'s pricing/CacheModel, since `report.applied`
        // reflects `--api`'s own strategy and the two can differ.
        let (_compressed, report) = compress(&raw, args, scorer.as_ref());
        let dt = report.removed_tokens_est;
        let s = report.rewrite_suffix_tokens;
        let applies = anyllm_optimize_core::should_apply(dt, s, args.horizon, &pricing, &model);
        // Only realize the delta when the gate actually applies: `applies == false` means
        // the original is forwarded untouched, so the realized cost change is $0, not the
        // negative "forced a no-op rewrite" value the raw formula would give for dt=0
        // (which `should_apply` exists precisely to avoid ever choosing).
        let net = if applies {
            net_cost_delta_usd(dt, s, args.horizon, &pricing, &model)
        } else {
            0.0
        };
        total_dt += dt;
        total_net += net;
        println!(
            "{:<4} {:>9} {:>7} {:>7} {:>7} {:>14.6}",
            i, report.frontier, dt, s, applies, net
        );
    }
    let model_name = match model {
        CacheModel::ImplicitPrefix => "implicit-prefix",
        CacheModel::ExplicitBreakpoints => "explicit-breakpoints",
    };
    println!(
        "\nNET COST (horizon={}, {model_name}, input=${:.2}/Mtok cached_read=${:.2}/Mtok \
         write_mult={:.2}x): total ΔT={total_dt} tokens, net_delta=${total_net:.6}",
        args.horizon, pricing.input, pricing.cached_read, pricing.cache_write_mult,
    );
    Ok(())
}

pub fn opt_u64(v: Option<u64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
}

pub fn pct(raw: u64, comp: u64) -> f64 {
    if raw == 0 {
        0.0
    } else {
        (raw.saturating_sub(comp)) as f64 / raw as f64 * 100.0
    }
}
