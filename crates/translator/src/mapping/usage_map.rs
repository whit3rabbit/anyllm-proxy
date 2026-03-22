// Usage field mapping between Anthropic and OpenAI
// PLAN.md lines 786-792

use crate::anthropic;
use crate::openai;

/// Convert OpenAI usage to Anthropic usage.
///
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/object>
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
pub fn openai_to_anthropic_usage(usage: &openai::ChatUsage) -> anthropic::Usage {
    anthropic::Usage {
        input_tokens: usage.prompt_tokens,
        output_tokens: usage.completion_tokens,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    }
}

/// Convert Anthropic usage to OpenAI usage.
///
/// Anthropic: <https://docs.anthropic.com/en/api/messages>
/// OpenAI: <https://platform.openai.com/docs/api-reference/chat/create>
pub fn anthropic_to_openai_usage(usage: &anthropic::Usage) -> openai::ChatUsage {
    openai::ChatUsage {
        prompt_tokens: usage.input_tokens,
        completion_tokens: usage.output_tokens,
        total_tokens: usage.input_tokens + usage.output_tokens,
        // Compat spec response: "Always empty". No Anthropic equivalent.
        // See: https://docs.anthropic.com/en/api/openai-sdk#response-fields
        completion_tokens_details: None,
        prompt_tokens_details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_to_anthropic_basic() {
        let oai = openai::ChatUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        };
        let anth = openai_to_anthropic_usage(&oai);
        assert_eq!(anth.input_tokens, 100);
        assert_eq!(anth.output_tokens, 50);
        assert!(anth.cache_creation_input_tokens.is_none());
        assert!(anth.cache_read_input_tokens.is_none());
    }

    #[test]
    fn anthropic_to_openai_basic() {
        let anth = anthropic::Usage {
            input_tokens: 200,
            output_tokens: 80,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(5),
        };
        let oai = anthropic_to_openai_usage(&anth);
        assert_eq!(oai.prompt_tokens, 200);
        assert_eq!(oai.completion_tokens, 80);
        assert_eq!(oai.total_tokens, 280);
    }

    #[test]
    fn zero_values() {
        let oai = openai::ChatUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            completion_tokens_details: None,
            prompt_tokens_details: None,
        };
        let anth = openai_to_anthropic_usage(&oai);
        assert_eq!(anth.input_tokens, 0);
        assert_eq!(anth.output_tokens, 0);

        let back = anthropic_to_openai_usage(&anth);
        assert_eq!(back.prompt_tokens, 0);
        assert_eq!(back.completion_tokens, 0);
        assert_eq!(back.total_tokens, 0);
    }

    #[test]
    fn total_tokens_computed_from_parts() {
        // OpenAI total_tokens is ignored when converting to Anthropic and back;
        // the round-trip recomputes it from input + output.
        let anth = anthropic::Usage {
            input_tokens: 30,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let oai = anthropic_to_openai_usage(&anth);
        assert_eq!(oai.total_tokens, 50);
    }

    #[test]
    fn cache_fields_dropped_on_conversion() {
        // Cache fields exist in Anthropic but not OpenAI; verify they survive round-trip
        // only as None on the way back.
        let anth = anthropic::Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: Some(3),
            cache_read_input_tokens: Some(7),
        };
        let oai = anthropic_to_openai_usage(&anth);
        let back = openai_to_anthropic_usage(&oai);
        assert_eq!(back.input_tokens, 10);
        assert_eq!(back.output_tokens, 5);
        assert!(back.cache_creation_input_tokens.is_none());
        assert!(back.cache_read_input_tokens.is_none());
    }
}
