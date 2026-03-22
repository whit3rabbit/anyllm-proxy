#[test]
fn malformed_openai_response_fails_deserialization() {
    let json = include_str!("../../../fixtures/openai/chat_completion_malformed.json");
    let result = serde_json::from_str::<anthropic_openai_translate::openai::ChatCompletionResponse>(json);
    assert!(result.is_err(), "malformed response should fail deserialization");
}
