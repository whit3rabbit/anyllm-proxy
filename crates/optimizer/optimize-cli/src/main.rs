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

use std::collections::HashSet;

use anyhow::{Context, Result};
use anyllm_optimize_core::{
    net_cost_delta_usd, BudgetCounter, CacheModel, CacheStrategy, HeuristicBudgetCounter, Mode,
    OptimizationReport, Policy, TokenScorer, UniformScorer, Workspace,
};
use anyllm_optimize_passes::adapter::{anthropic, openai};
use anyllm_optimize_passes::{
    anthropic_pricing, openai_pricing, AnthropicStrategy, OpenAiStrategy,
};
use clap::{Parser, ValueEnum};
use serde_json::{json, Value};

#[cfg(feature = "tiktoken")]
mod budget_tiktoken;

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq)]
enum Api {
    Openai,
    Anthropic,
}

#[derive(Parser, Debug)]
#[command(
    name = "optimize-eval",
    about = "FFEC token-savings + response-quality harness"
)]
struct Args {
    /// Wire format of the target endpoint.
    #[arg(long, value_enum, default_value_t = Api::Openai)]
    api: Api,
    /// Base URL. Ollama: http://localhost:11434/v1 ; OpenRouter: https://openrouter.ai/api/v1
    #[arg(long, default_value = "http://localhost:11434/v1")]
    base_url: String,
    /// Target model id.
    #[arg(long)]
    model: String,
    /// API key. Falls back to $OPENROUTER_API_KEY / $ANYLLM_API_KEY; empty for local.
    #[arg(long)]
    api_key: Option<String>,
    /// JSONL file: each line a request body (`{"messages":[...]}`) or a bare prompt string.
    #[arg(long, conflicts_with = "prompt")]
    input: Option<std::path::PathBuf>,
    /// A single prompt to test instead of --input.
    #[arg(long)]
    prompt: Option<String>,
    /// Use streaming requests (adds include_usage for OpenAI).
    #[arg(long)]
    stream: bool,
    /// max_tokens for the response (Anthropic requires it; also sent to OpenAI).
    #[arg(long, default_value_t = 256)]
    max_tokens: u64,
    /// Expected remaining turns (cost-gate horizon).
    #[arg(long, default_value_t = 8)]
    horizon: u64,
    /// Only compute local estimates; do not call the network (no quality signal).
    #[arg(long)]
    offline: bool,
    /// Print both full responses per row (raw vs compressed) for eyeballing quality.
    #[arg(long)]
    show_responses: bool,
    /// Optional OpenAI-compatible judge model. If set, scores 1-5 how well the compressed
    /// response preserves the raw response's meaning/quality (adds one call per row).
    #[arg(long)]
    judge_model: Option<String>,
    /// Base URL for the judge (defaults to --base-url). Must be OpenAI-compatible.
    #[arg(long)]
    judge_base_url: Option<String>,
    /// Simulate the frozen-frontier cost gate over the input and print the signed net
    /// USD delta (compress vs skip) per row plus a class total, using the CacheModel and
    /// Pricing table implied by `--api` unless `--cost-model` overrides it (EH-0002 M0.3:
    /// offline, no network needed).
    #[arg(long)]
    net_cost: bool,
    /// Override which provider's CacheModel/Pricing the `--net-cost` dollar math uses,
    /// independent of `--api` (which must still match the input's actual wire shape — the
    /// adapter mis-segments tool/content blocks if it doesn't). Defaults to `--api`.
    #[arg(long, value_enum)]
    cost_model: Option<Api>,
    /// M3.6: directory containing `model.onnx` + `tokenizer.json` for the real
    /// LLMLingua2Pass ML scorer (ROADMAP D8), used in place of `UniformScorer` for every
    /// message the frontier already selects. Requires building with `--features onnx`;
    /// without it (or on a load failure) this falls back to `UniformScorer` with a
    /// warning, per the fail-open invariant — never a hard error.
    #[arg(long)]
    llmlingua2_model_dir: Option<std::path::PathBuf>,
}

/// One provider response: token usage + the assistant text.
struct Reply {
    prompt_tokens: u64,
    text: String,
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
fn compress(raw: &Value, args: &Args, scorer: &dyn TokenScorer) -> (Value, OptimizationReport) {
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
                &HeuristicBudgetCounter::default(),
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
                &HeuristicBudgetCounter::default(),
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
fn build_scorer(args: &Args) -> Box<dyn TokenScorer> {
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

/// EH-0002 bite 2: simulate the frozen-frontier policy over `--input` and print the
/// signed net USD delta (compress vs skip) the cost gate computes per row, plus a class
/// total. Fully offline (no network) — the ΔT/S the gate uses come straight out of the
/// same `optimize()` call the harness already runs, so this is not a re-derivation, it is
/// the actual decision the frontier + cost gate would make.
fn run_net_cost(args: &Args) -> Result<()> {
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

fn ensure_model(body: &mut Value, args: &Args) {
    body["model"] = json!(args.model);
    if args.api == Api::Anthropic && body.get("max_tokens").is_none() {
        body["max_tokens"] = json!(args.max_tokens);
    }
}

/// M3.6: selects the counter `count_local` uses to report est_raw/est_comp. Defaults to
/// `HeuristicBudgetCounter` (bytes/3.6, provider-agnostic). When this binary is built with
/// `--features tiktoken` AND the target is OpenAI-shaped, uses the exact `o200k_base`
/// tokenizer instead — Anthropic's tokenizer is unpublished, so the heuristic remains the
/// only option there (ROADMAP risk 3). This only changes what the harness *reports*; the
/// planning counter `compress()` passes into `optimize()` is unchanged.
#[cfg_attr(not(feature = "tiktoken"), allow(unused_variables))]
fn build_counter(args: &Args) -> Box<dyn BudgetCounter> {
    #[cfg(feature = "tiktoken")]
    if args.api == Api::Openai {
        return Box::new(budget_tiktoken::TiktokenBudgetCounter);
    }
    Box::new(HeuristicBudgetCounter::default())
}

/// Local token estimate: sum the budget counter over all message text content.
fn count_local(body: &Value, api: Api, counter: &dyn BudgetCounter) -> u64 {
    let mut total = 0u64;
    if api == Api::Anthropic {
        if let Some(sys) = body.get("system") {
            total += count_content(sys, counter);
        }
    }
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for m in msgs {
            if let Some(c) = m.get("content") {
                total += count_content(c, counter);
            }
        }
    }
    total
}

fn count_content(c: &Value, counter: &dyn BudgetCounter) -> u64 {
    match c {
        Value::String(s) => counter.count(s),
        Value::Array(parts) => parts
            .iter()
            .map(|p| match p.get("text").and_then(|t| t.as_str()) {
                Some(t) => counter.count(t),
                None => counter.count(&p.to_string()),
            })
            .sum(),
        _ => 0,
    }
}

/// Send one request and return usage + assistant text.
fn send_request(
    client: &reqwest::blocking::Client,
    args: &Args,
    body: &Value,
    base_url: &str,
    api: Api,
) -> Result<Reply> {
    let mut body = body.clone();
    let url = match api {
        Api::Openai => format!("{}/chat/completions", base_url.trim_end_matches('/')),
        Api::Anthropic => format!("{}/messages", base_url.trim_end_matches('/')),
    };
    if args.stream {
        body["stream"] = json!(true);
        if api == Api::Openai {
            body["stream_options"] = json!({ "include_usage": true });
        }
    }

    let key = resolve_key(args);
    let mut req = client.post(&url).json(&body);
    req = match api {
        Api::Openai => {
            if key.is_empty() {
                req
            } else {
                req.bearer_auth(key)
            }
        }
        Api::Anthropic => req
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
    };

    let resp = req.send().context("request send failed")?;
    let status = resp.status();
    let text = resp.text().context("reading response body")?;
    if !status.is_success() {
        anyhow::bail!(
            "HTTP {status}: {}",
            text.chars().take(300).collect::<String>()
        );
    }
    Ok(Reply {
        prompt_tokens: extract_prompt_tokens(&text, api, args.stream)
            .context("could not find prompt/input token usage")?,
        text: extract_response_text(&text, api, args.stream),
    })
}

fn resolve_key(args: &Args) -> String {
    args.api_key
        .clone()
        .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
        .or_else(|| std::env::var("ANYLLM_API_KEY").ok())
        .unwrap_or_default()
}

/// Pull prompt tokens from a JSON body or an SSE stream.
fn extract_prompt_tokens(text: &str, api: Api, stream: bool) -> Result<u64> {
    let want = |v: &Value| -> Option<u64> {
        match api {
            Api::Openai => v.get("usage")?.get("prompt_tokens")?.as_u64(),
            Api::Anthropic => v
                .get("usage")
                .or_else(|| v.get("message").and_then(|m| m.get("usage")))?
                .get("input_tokens")?
                .as_u64(),
        }
    };
    if !stream {
        let v: Value = serde_json::from_str(text)?;
        return want(&v).context("usage field missing");
    }
    for payload in sse_payloads(text) {
        if let Ok(v) = serde_json::from_str::<Value>(&payload) {
            if let Some(n) = want(&v) {
                return Ok(n);
            }
        }
    }
    anyhow::bail!("no usage in stream")
}

/// Pull assistant text from a JSON body or an SSE stream. Best-effort: returns "" if the
/// shape is unrecognized (quality just reads as low, never panics).
fn extract_response_text(text: &str, api: Api, stream: bool) -> String {
    if !stream {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return String::new();
        };
        return match api {
            Api::Openai => v
                .pointer("/choices/0/message/content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
            Api::Anthropic => v
                .get("content")
                .and_then(|c| c.as_array())
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default(),
        };
    }
    // Streaming: concatenate incremental text deltas.
    let mut out = String::new();
    for payload in sse_payloads(text) {
        let Ok(v) = serde_json::from_str::<Value>(&payload) else {
            continue;
        };
        match api {
            Api::Openai => {
                if let Some(s) = v
                    .pointer("/choices/0/delta/content")
                    .and_then(|c| c.as_str())
                {
                    out.push_str(s);
                }
            }
            Api::Anthropic => {
                if let Some(s) = v.pointer("/delta/text").and_then(|c| c.as_str()) {
                    out.push_str(s);
                }
            }
        }
    }
    out
}

/// Yield the JSON payload of each `data:` SSE line, skipping `[DONE]`.
fn sse_payloads(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().filter_map(|line| {
        let payload = line.trim().strip_prefix("data:")?.trim();
        (payload != "[DONE]").then(|| payload.to_string())
    })
}

/// Word-level Jaccard similarity in [0,1]. 1.0 if both empty.
fn jaccard(a: &str, b: &str) -> f64 {
    let sa: HashSet<&str> = a.split_whitespace().collect();
    let sb: HashSet<&str> = b.split_whitespace().collect();
    if sa.is_empty() && sb.is_empty() {
        return 1.0;
    }
    let inter = sa.intersection(&sb).count() as f64;
    let uni = sa.union(&sb).count() as f64;
    if uni == 0.0 {
        1.0
    } else {
        inter / uni
    }
}

/// Ask an OpenAI-compatible judge model to score 1-5 how well `comp` preserves `raw`.
fn run_judge(
    client: &reqwest::blocking::Client,
    args: &Args,
    raw: &str,
    comp: &str,
) -> Result<u32> {
    let model = args.judge_model.as_ref().context("no judge model")?;
    let base = args.judge_base_url.as_deref().unwrap_or(&args.base_url);
    let prompt = format!(
        "You compare two AI assistant responses to the same user request. Response A is \
         from the full prompt; Response B is from a compressed prompt. Score 1-5 how well B \
         preserves A's meaning and quality (5 = equivalent, 1 = badly degraded). Reply with \
         ONLY the integer.\n\n[A]\n{raw}\n\n[B]\n{comp}"
    );
    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "max_tokens": 4,
        "temperature": 0,
    });
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    let key = resolve_key(args);
    let mut req = client.post(&url).json(&body);
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let resp = req.send().context("judge send failed")?;
    let text = resp.text().context("reading judge response")?;
    let v: Value = serde_json::from_str(&text).context("judge response not JSON")?;
    let content = v
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .context("judge response has no content")?;
    let score = content
        .chars()
        .find(|c| ('1'..='5').contains(c))
        .and_then(|c| c.to_digit(10))
        .context("no 1-5 score in judge reply")?;
    Ok(score)
}

fn opt_u64(v: Option<u64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
}

fn pct(raw: u64, comp: u64) -> f64 {
    if raw == 0 {
        0.0
    } else {
        (raw.saturating_sub(comp)) as f64 / raw as f64 * 100.0
    }
}

/// Load input bodies. Each JSONL line is either a full request body (`{"messages":...}`)
/// or a bare JSON string (treated as one user message).
fn load_inputs(args: &Args) -> Result<Vec<Value>> {
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

fn wrap_prompt(p: &str) -> Value {
    json!({ "messages": [ { "role": "user", "content": p } ] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_bounds() {
        assert_eq!(jaccard("", ""), 1.0);
        assert_eq!(jaccard("a b c", "a b c"), 1.0);
        assert!((jaccard("a b", "b c") - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(jaccard("a", "z"), 0.0);
    }

    #[test]
    fn extract_openai_nonstream() {
        let body =
            r#"{"choices":[{"message":{"content":"hello there"}}],"usage":{"prompt_tokens":42}}"#;
        assert_eq!(extract_prompt_tokens(body, Api::Openai, false).unwrap(), 42);
        assert_eq!(
            extract_response_text(body, Api::Openai, false),
            "hello there"
        );
    }

    #[test]
    fn extract_anthropic_nonstream() {
        let body = r#"{"content":[{"type":"text","text":"hi"},{"type":"text","text":" world"}],"usage":{"input_tokens":7}}"#;
        assert_eq!(
            extract_prompt_tokens(body, Api::Anthropic, false).unwrap(),
            7
        );
        assert_eq!(
            extract_response_text(body, Api::Anthropic, false),
            "hi world"
        );
    }

    #[test]
    fn extract_openai_stream_text_and_usage() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"foo\"}}]}\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\"bar\"}}]}\n\
                   data: {\"choices\":[],\"usage\":{\"prompt_tokens\":11}}\n\
                   data: [DONE]\n";
        assert_eq!(extract_prompt_tokens(sse, Api::Openai, true).unwrap(), 11);
        assert_eq!(extract_response_text(sse, Api::Openai, true), "foobar");
    }
}
