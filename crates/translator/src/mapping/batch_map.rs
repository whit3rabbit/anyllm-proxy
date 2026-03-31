// crates/translator/src/mapping/batch_map.rs
// Pure translation between Anthropic batch JSONL format and OpenAI batch JSONL format.
// No I/O. All functions are deterministic.

use crate::anthropic::batch::{BatchRequestItem, BatchResultItem, BatchResultVariant};
use crate::anthropic::errors::{ErrorDetail, ErrorType};
use crate::mapping::message_map::{anthropic_to_openai_request, openai_to_anthropic_response};
use crate::openai;

/// Translate one Anthropic batch request item into an OpenAI JSONL batch line.
///
/// OpenAI format: `{"custom_id":"…","method":"POST","url":"/v1/chat/completions","body":{…}}`
pub fn batch_request_item_to_openai_jsonl_line(item: &BatchRequestItem) -> String {
    let openai_req = anthropic_to_openai_request(&item.params);
    let line = serde_json::json!({
        "custom_id": item.custom_id,
        "method": "POST",
        "url": "/v1/chat/completions",
        "body": openai_req,
    });
    serde_json::to_string(&line).expect("infallible")
}

/// Translate a complete Anthropic batch request into OpenAI JSONL (newline-separated lines).
pub fn translate_batch_to_openai_jsonl(items: &[BatchRequestItem]) -> String {
    items
        .iter()
        .map(batch_request_item_to_openai_jsonl_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Translate one OpenAI batch output JSONL line into an Anthropic result JSONL line.
///
/// OpenAI output format:
/// `{"id":"br_…","custom_id":"…","response":{"status_code":200,"body":{ChatCompletion}},"error":null}`
///
/// `model` is the Anthropic model name to embed in the resulting MessageResponse.
pub fn translate_openai_result_line(line: &str, model: &str) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("JSON parse error: {e}"))?;

    let custom_id = v["custom_id"]
        .as_str()
        .ok_or("missing custom_id field")?
        .to_string();

    let variant = if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        let msg = err["message"]
            .as_str()
            .unwrap_or("unknown batch error")
            .to_string();
        BatchResultVariant::Errored {
            error: ErrorDetail {
                error_type: ErrorType::ApiError,
                message: msg,
            },
        }
    } else if let Some(response) = v.get("response").filter(|r| !r.is_null()) {
        let status = response["status_code"].as_u64().unwrap_or(0);
        if status == 200 {
            let body = &response["body"];
            let completion: openai::ChatCompletionResponse =
                serde_json::from_value(body.clone())
                    .map_err(|e| format!("failed to parse ChatCompletionResponse: {e}"))?;
            let message = openai_to_anthropic_response(&completion, model);
            BatchResultVariant::Succeeded { message }
        } else {
            BatchResultVariant::Errored {
                error: ErrorDetail {
                    error_type: ErrorType::ApiError,
                    message: format!("backend status {status}"),
                },
            }
        }
    } else {
        BatchResultVariant::Expired
    };

    let item = BatchResultItem {
        custom_id,
        result: variant,
    };
    serde_json::to_string(&item).map_err(|e| format!("serialize error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request_item() -> crate::anthropic::batch::BatchRequestItem {
        crate::anthropic::batch::BatchRequestItem {
            custom_id: "req-1".to_string(),
            params: serde_json::from_value(serde_json::json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 100,
                "messages": [{"role": "user", "content": "Hello"}]
            }))
            .unwrap(),
        }
    }

    #[test]
    fn request_item_serializes_to_openai_jsonl_line() {
        let item = make_request_item();
        let line = batch_request_item_to_openai_jsonl_line(&item);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["custom_id"], "req-1");
        assert_eq!(v["method"], "POST");
        assert_eq!(v["url"], "/v1/chat/completions");
        assert!(v["body"]["messages"].is_array());
    }

    #[test]
    fn translate_openai_success_result_to_anthropic() {
        let openai_line = serde_json::json!({
            "id": "br_abc",
            "custom_id": "req-1",
            "response": {
                "status_code": 200,
                "body": {
                    "id": "chatcmpl-xyz",
                    "object": "chat.completion",
                    "created": 1_700_000_000u64,
                    "model": "gpt-4o",
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "Hi!"},
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 10,
                        "completion_tokens": 3,
                        "total_tokens": 13
                    }
                }
            },
            "error": null
        });
        let line = serde_json::to_string(&openai_line).unwrap();
        let result = translate_openai_result_line(&line, "claude-3-5-sonnet-20241022").unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["custom_id"], "req-1");
        assert_eq!(v["result"]["type"], "succeeded");
        assert_eq!(v["result"]["message"]["role"], "assistant");
    }

    #[test]
    fn translate_openai_error_result_to_anthropic() {
        let openai_line = serde_json::json!({
            "id": "br_abc",
            "custom_id": "req-2",
            "response": null,
            "error": {"code": "rate_limit_exceeded", "message": "quota exceeded"}
        });
        let line = serde_json::to_string(&openai_line).unwrap();
        let result = translate_openai_result_line(&line, "claude-3-5-sonnet-20241022").unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["custom_id"], "req-2");
        assert_eq!(v["result"]["type"], "errored");
        assert!(v["result"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("quota"));
    }

    #[test]
    fn translate_batch_items_to_openai_jsonl() {
        let items = vec![make_request_item(), {
            let mut i = make_request_item();
            i.custom_id = "req-2".to_string();
            i
        }];
        let jsonl = translate_batch_to_openai_jsonl(&items);
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);
        let v: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(v["custom_id"], "req-2");
    }
}
