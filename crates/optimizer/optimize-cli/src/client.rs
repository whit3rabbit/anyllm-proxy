use super::{Api, Args};
use anyhow::{Context, Result};
use serde_json::{json, Value};

/// One provider response: token usage + the assistant text.
pub struct Reply {
    pub prompt_tokens: u64,
    pub text: String,
}

/// Send one request and return usage + assistant text.
pub fn send_request(
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

pub fn resolve_key(args: &Args) -> String {
    args.api_key
        .clone()
        .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
        .or_else(|| std::env::var("ANYLLM_API_KEY").ok())
        .unwrap_or_default()
}

/// Pull prompt tokens from a JSON body or an SSE stream.
pub fn extract_prompt_tokens(text: &str, api: Api, stream: bool) -> Result<u64> {
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
pub fn extract_response_text(text: &str, api: Api, stream: bool) -> String {
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
pub fn sse_payloads(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().filter_map(|line| {
        let payload = line.trim().strip_prefix("data:")?.trim();
        (payload != "[DONE]").then(|| payload.to_string())
    })
}

/// Ask an OpenAI-compatible judge model to score 1-5 how well `comp` preserves `raw`.
pub fn run_judge(
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
