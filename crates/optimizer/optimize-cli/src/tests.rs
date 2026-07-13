use super::{client::*, utils::*, Api};

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
