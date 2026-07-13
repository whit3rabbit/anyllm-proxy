//! `optimize-eval` — the "are there real savings, and is quality preserved?" harness.
//!
//! For each input conversation it runs FFEC in Live mode, then sends BOTH the raw and the
//! compressed body to any OpenAI-compatible or Anthropic-compatible endpoint and compares:
//!   - local BudgetCounter estimate (validates our bytes/3.6 math),
//!   - provider-reported prompt tokens (ground truth savings),
//!   - RESPONSE QUALITY: word-Jaccard similarity between the two responses + length delta,
//!     and (optional `--judge-model`) an LLM equivalence score 1-5.
//!
//! Savings mean nothing if the compressed prompt changes the answer, so the quality
//! columns are the go/no-go signal, not the token columns.
//!
//! Streaming: with `--stream`, OpenAI requests set `stream_options.include_usage`; usage
//! and text are read from the chunks. Anthropic reads `message_start` usage +
//! `content_block_delta` text.
//!
//! Fail-open: a network/API error on one row is reported and skipped; the process exits
//! non-zero if any row failed, but never panics.

use anyhow::{Context, Result};
use anyllm_optimize_core::{
    Mode, OptimizationReport, Policy, TokenScorer, UniformScorer, Workspace,
};
use anyllm_optimize_passes::adapter::{anthropic, openai};
use anyllm_optimize_passes::{
    anthropic_pricing, openai_pricing, AnthropicStrategy, OpenAiStrategy,
};
use clap::{Parser, ValueEnum};
use serde_json::{json, Value};

#[cfg(feature = "tiktoken")]
pub(crate) mod budget_tiktoken;

mod client;
mod cost;
#[cfg(test)]
mod tests;
mod token_count;
mod utils;

use client::{run_judge, send_request};
use cost::{opt_u64, pct, run_net_cost};
use token_count::{build_counter, count_local};
use utils::{jaccard, wrap_prompt};

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq)]
pub(crate) enum Api {
    Openai,
    Anthropic,
}

#[derive(Parser, Debug)]
#[command(
    name = "optimize-eval",
    about = "FFEC token-savings + response-quality harness"
)]
pub(crate) struct Args {
    /// Wire format of the target endpoint.
    #[arg(long, value_enum, default_value_t = Api::Openai)]
    pub(crate) api: Api,
    /// Base URL. Ollama: http://localhost:11434/v1 ; OpenRouter: https://openrouter.ai/api/v1
    #[arg(long, default_value = "http://localhost:11434/v1")]
    pub(crate) base_url: String,
    /// Target model id.
    #[arg(long)]
    pub(crate) model: String,
    /// API key. Falls back to $OPENROUTER_API_KEY / $ANYLLM_API_KEY; empty for local.
    #[arg(long)]
    pub(crate) api_key: Option<String>,
    /// JSONL file: each line a request body (`{"messages":[...]}`) or a bare prompt string.
    #[arg(long, conflicts_with = "prompt")]
    pub(crate) input: Option<std::path::PathBuf>,
    /// A single prompt to test instead of --input.
    #[arg(long)]
    pub(crate) prompt: Option<String>,
    /// Use streaming requests (adds include_usage for OpenAI).
    #[arg(long)]
    pub(crate) stream: bool,
    /// max_tokens for the response (Anthropic requires it; also sent to OpenAI).
    #[arg(long, default_value_t = 256)]
    pub(crate) max_tokens: u64,
    /// Expected remaining turns (cost-gate horizon).
    #[arg(long, default_value_t = 8)]
    pub(crate) horizon: u64,
    /// Only compute local estimates; do not call the network (no quality signal).
    #[arg(long)]
    pub(crate) offline: bool,
    /// Print both full responses per row (raw vs compressed) for eyeballing quality.
    #[arg(long)]
    pub(crate) show_responses: bool,
    /// Optional OpenAI-compatible judge model. If set, scores 1-5 how well the compressed
    /// response preserves the raw response's meaning/quality (adds one call per row).
    #[arg(long)]
    pub(crate) judge_model: Option<String>,
    /// Base URL for the judge (defaults to --base-url). Must be OpenAI-compatible.
    #[arg(long)]
    pub(crate) judge_base_url: Option<String>,
    /// Simulate the frozen-frontier cost gate over the input and print the signed net
    /// USD delta (compress vs skip) per row plus a class total, using the CacheModel and
    /// Pricing table implied by `--api` unless `--cost-model` overrides it (EH-0002 M0.3:
    /// offline, no network needed).
    #[arg(long)]
    pub(crate) net_cost: bool,
    /// Override which provider's CacheModel/Pricing the `--net-cost` dollar math uses,
    /// independent of `--api` (which must still match the input's actual wire shape — the
    /// adapter mis-segments tool/content blocks if it doesn't). Defaults to `--api`.
    #[arg(long, value_enum)]
    pub(crate) cost_model: Option<Api>,
    /// M3.6: directory containing `model.onnx` + `tokenizer.json` for the real
    /// LLMLingua2Pass ML scorer (ROADMAP D8), used in place of `UniformScorer` for every
    /// message the frontier already selects. Requires building with `--features onnx`;
    /// without it (or on a load failure) this falls back to `UniformScorer` with a
    /// warning, per the fail-open invariant — never a hard error.
    #[arg(long)]
    pub(crate) llmlingua2_model_dir: Option<std::path::PathBuf>,
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(0) => std::process::ExitCode::SUCCESS,
        Ok(failed) => {
            eprintln!("\n{failed} row(s) failed (see errors above).");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("fatal: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<u32> {
    if args.net_cost {
        run_net_cost(args)?;
        return Ok(0);
    }
    let bodies = load_inputs(args)?;
    let counter = build_counter(args);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let scorer = build_scorer(args);

    println!(
        "{:<4} {:>9} {:>9} {:>7} {:>9} {:>9} {:>7} {:>8} {:>6}",
        "#", "est_raw", "est_cmp", "est_%", "prov_raw", "prov_cmp", "prov_%", "resp_sim", "judge"
    );
    let mut failed = 0u32;
    let mut tot_est_raw = 0u64;
    let mut tot_est_comp = 0u64;
    let mut sims: Vec<f64> = Vec::new();
    let mut judges: Vec<u32> = Vec::new();

    for (i, mut raw) in bodies.into_iter().enumerate() {
        ensure_model(&mut raw, args);
        let (compressed, _report) = compress(&raw, args, scorer.as_ref());

        let est_raw = count_local(&raw, args.api, counter.as_ref());
        let est_comp = count_local(&compressed, args.api, counter.as_ref());
        tot_est_raw += est_raw;
        tot_est_comp += est_comp;

        let mut prov_raw = None;
        let mut prov_comp = None;
        let mut sim: Option<f64> = None;
        let mut judge: Option<u32> = None;

        if !args.offline {
            let r = send_request(&client, args, &raw, &args.base_url, args.api);
            let c = send_request(&client, args, &compressed, &args.base_url, args.api);
            match (&r, &c) {
                (Ok(rr), Ok(cc)) => {
                    prov_raw = Some(rr.prompt_tokens);
                    prov_comp = Some(cc.prompt_tokens);
                    let s = jaccard(&rr.text, &cc.text);
                    sim = Some(s);
                    sims.push(s);
                    if args.show_responses {
                        println!("--- row {i} RAW response ---\n{}", rr.text.trim());
                        println!("--- row {i} COMPRESSED response ---\n{}", cc.text.trim());
                    }
                    if args.judge_model.is_some() {
                        match run_judge(&client, args, &rr.text, &cc.text) {
                            Ok(score) => {
                                judge = Some(score);
                                judges.push(score);
                            }
                            Err(e) => eprintln!("row {i}: judge failed: {e:#}"),
                        }
                    }
                }
                _ => {
                    if let Err(e) = &r {
                        eprintln!("row {i}: raw request failed: {e:#}");
                        failed += 1;
                    }
                    if let Err(e) = &c {
                        eprintln!("row {i}: compressed request failed: {e:#}");
                        failed += 1;
                    }
                }
            }
        }

        println!(
            "{:<4} {:>9} {:>9} {:>6.1}% {:>9} {:>9} {:>6} {:>8} {:>6}",
            i,
            est_raw,
            est_comp,
            pct(est_raw, est_comp),
            opt_u64(prov_raw),
            opt_u64(prov_comp),
            match (prov_raw, prov_comp) {
                (Some(r), Some(c)) => format!("{:.1}%", pct(r, c)),
                _ => "-".into(),
            },
            sim.map(|s| format!("{:.0}%", s * 100.0))
                .unwrap_or_else(|| "-".into()),
            judge
                .map(|j| format!("{j}/5"))
                .unwrap_or_else(|| "-".into()),
        );
    }

    let price = match args.api {
        Api::Openai => openai_pricing().input,
        Api::Anthropic => anthropic_pricing().input,
    };
    let saved_tokens = tot_est_raw.saturating_sub(tot_est_comp);
    let saved_usd = saved_tokens as f64 / 1_000_000.0 * price;
    println!(
        "\nTOTAL est raw={tot_est_raw} comp={tot_est_comp} saved={saved_tokens} tokens \
         (~${saved_usd:.4} input @ ${price}/Mtok)"
    );
    if !sims.is_empty() {
        let avg = sims.iter().sum::<f64>() / sims.len() as f64;
        println!(
            "QUALITY mean response similarity = {:.0}% over {} rows",
            avg * 100.0,
            sims.len()
        );
    }
    if !judges.is_empty() {
        let avg = judges.iter().sum::<u32>() as f64 / judges.len() as f64;
        println!(
            "JUDGE   mean equivalence score = {avg:.2}/5 over {} rows",
            judges.len()
        );
    }
    Ok(failed)
}

/// Run FFEC in Live mode; return the compressed body (or a clone of raw if nothing won)
/// plus the `OptimizationReport` (frontier, ΔT, S, applied) the frozen-frontier cost gate
/// computed along the way, for `--net-cost` to turn into a dollar figure. `scorer` is
/// `UniformScorer` by default, or the real `LLMLingua2Pass` when `--llmlingua2-model-dir`
/// resolves (see `build_scorer`) — either way it is only ever invoked by `optimize()` for
/// frontier-eligible D8 targets (tool results, old RAG blocks, old assistant messages).
pub(crate) fn compress(
    raw: &Value,
    args: &Args,
    scorer: &dyn TokenScorer,
) -> (Value, OptimizationReport) {
    let policy = Policy {
        mode: Mode::Live,
        horizon: args.horizon,
        ..Default::default()
    };
    let mut ws = Workspace::new();
    let mut out = raw.clone();

    match args.api {
        Api::Openai => {
            let conv = openai::from_value(raw);
            let res = anyllm_optimize_core::optimize(
                &conv,
                &policy,
                &OpenAiStrategy::default(),
                scorer,
                &anyllm_optimize_core::HeuristicBudgetCounter::default(),
                &mut ws,
            );
            if let Some(r) = &res.rendered {
                openai::apply_rendered(&mut out, r);
            }
            (out, res.report)
        }
        Api::Anthropic => {
            let conv = anthropic::from_value(raw);
            let res = anyllm_optimize_core::optimize(
                &conv,
                &policy,
                &AnthropicStrategy::default(),
                scorer,
                &anyllm_optimize_core::HeuristicBudgetCounter::default(),
                &mut ws,
            );
            if let Some(r) = &res.rendered {
                anthropic::apply_rendered(&mut out, r);
            }
            (out, res.report)
        }
    }
}

/// Selects the scorer `compress()` passes into `optimize()`. Defaults to `UniformScorer`
/// (fail-open, no artifact required). If `--llmlingua2-model-dir <dir>` is given and this
/// binary was built with `--features onnx`, loads `LLMLingua2Pass` from
/// `<dir>/model.onnx` + `<dir>/tokenizer.json` and uses it instead — this is the M3.6
/// wiring: the real ML scorer only ever runs behind the frontier, on D8 targets, because
/// that gating already lives in `optimize()`/`compress_message` (see `LLMLingua2Pass`'s
/// doc comment). Any failure to build/load falls back to `UniformScorer` with a warning
/// rather than aborting the run.
pub(crate) fn build_scorer(args: &Args) -> Box<dyn TokenScorer> {
    let Some(dir) = &args.llmlingua2_model_dir else {
        return Box::new(UniformScorer);
    };
    #[cfg(feature = "onnx")]
    {
        let onnx_path = dir.join("model.onnx");
        let tokenizer_path = dir.join("tokenizer.json");
        match std::fs::read(&onnx_path) {
            Ok(bytes) => {
                // Not a cryptographic hash: good enough to fold into PolicyVersion for
                // this offline eval harness so a model swap still forces a cache
                // re-write in its own bookkeeping. A real proxy integration must use a
                // verified sha256 per `LlmLingua2Scorer`'s module docs, not this.
                let artifact_hash = fnv1a64(&bytes);
                match anyllm_optimize_scorer::LLMLingua2Pass::from_files(
                    &onnx_path,
                    &tokenizer_path,
                    artifact_hash,
                ) {
                    Ok(pass) => {
                        eprintln!("using LLMLingua2Pass scorer from {}", dir.display());
                        return Box::new(pass);
                    }
                    Err(e) => eprintln!(
                        "warning: failed to load LLMLingua2Pass from {}: {e} — falling back to UniformScorer",
                        dir.display()
                    ),
                }
            }
            Err(e) => eprintln!(
                "warning: could not read {}: {e} — falling back to UniformScorer",
                onnx_path.display()
            ),
        }
    }
    #[cfg(not(feature = "onnx"))]
    {
        eprintln!(
            "warning: --llmlingua2-model-dir {} given but this binary was built without \
             --features onnx; falling back to UniformScorer",
            dir.display()
        );
    }
    Box::new(UniformScorer)
}

/// FNV-1a 64-bit — fast, deterministic, non-cryptographic. Only used to fold a local
/// artifact's bytes into a `PolicyVersion`-friendly hash for `build_scorer`'s own
/// bookkeeping (see its doc comment); not a security or integrity check.
#[cfg(feature = "onnx")]
fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    bytes
        .iter()
        .fold(OFFSET, |h, &b| (h ^ b as u64).wrapping_mul(PRIME))
}

pub(crate) fn ensure_model(body: &mut Value, args: &Args) {
    body["model"] = json!(args.model);
    if args.api == Api::Anthropic && body.get("max_tokens").is_none() {
        body["max_tokens"] = json!(args.max_tokens);
    }
}

/// Load input bodies. Each JSONL line is either a full request body (`{"messages":...}`)
/// or a bare JSON string (treated as one user message).
pub(crate) fn load_inputs(args: &Args) -> Result<Vec<Value>> {
    if let Some(p) = &args.prompt {
        return Ok(vec![wrap_prompt(p)]);
    }
    let path = args
        .input
        .as_ref()
        .context("provide --input <file.jsonl> or --prompt <text>")?;
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut out = Vec::new();
    for (ln, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line)
            .with_context(|| format!("{}: line {} is not JSON", path.display(), ln + 1))?;
        out.push(match v {
            Value::String(s) => wrap_prompt(&s),
            other => other,
        });
    }
    Ok(out)
}
