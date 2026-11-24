//! The line the log gets when a vision call ends: what it was for, how long it took, how it was answered and
//! the token counts the answer reports.

use super::*;

/// The token counts an answer reports, each where its provider says it: what it read, what it wrote, and
/// how much it spent thinking. A count the answer does not give is left out.
#[derive(Debug, Default, PartialEq, Eq)]
struct ReportedTokens {
    input: Option<u64>,
    output: Option<u64>,
    thinking: Option<u64>,
}

fn reported_tokens(answer: &Value) -> ReportedTokens {
    let count = |places: &[&str]| {
        places
            .iter()
            .find_map(|place| answer.pointer(place).and_then(Value::as_u64))
    };
    ReportedTokens {
        input: count(&[
            "/usage/input_tokens",
            "/usage/prompt_tokens",
            "/usageMetadata/promptTokenCount",
        ]),
        output: count(&[
            "/usage/output_tokens",
            "/usage/completion_tokens",
            "/usageMetadata/candidatesTokenCount",
        ]),
        thinking: count(&[
            "/usage/output_tokens_details/reasoning_tokens",
            "/usage/completion_tokens_details/reasoning_tokens",
            "/usageMetadata/thoughtsTokenCount",
        ]),
    }
}

/// One vision call, for the line the log gets when it ends: what it was for, which model was asked, how long
/// it took and how it was answered. Labels and numbers only: never the prompt, the picture, the words of the
/// answer, or anything the request carried.
pub(super) struct VisionCall<'a> {
    purpose: &'static str,
    provider: &'a str,
    model: &'a str,
    asked: std::time::Instant,
}

impl<'a> VisionCall<'a> {
    /// A call being sent now.
    pub(super) fn begun(purpose: &'static str, provider: &'a str, model: &'a str) -> Self {
        Self {
            purpose,
            provider,
            model,
            asked: std::time::Instant::now(),
        }
    }

    /// `status` is the answer's, when one came; `ok` when it was a success read whole; `answer` is its JSON.
    pub(super) fn ended(
        &self,
        status: Option<reqwest::StatusCode>,
        ok: bool,
        answer: Option<&Value>,
    ) {
        let tokens = answer.map(reported_tokens).unwrap_or_default();
        tracing::info!(
            purpose = self.purpose,
            provider = self.provider,
            model = self.model,
            elapsed_ms = self.asked.elapsed().as_millis() as u64,
            ok,
            status = status.map(|status| u64::from(status.as_u16())),
            input_tokens = tokens.input,
            output_tokens = tokens.output,
            thinking_tokens = tokens.thinking,
            "vision call ended"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_counts_are_read_where_each_provider_reports_them() {
        let counts = |input, output, thinking| ReportedTokens {
            input,
            output,
            thinking,
        };
        let chat = json!({ "usage": {
            "prompt_tokens": 1200,
            "completion_tokens": 90,
            "completion_tokens_details": { "reasoning_tokens": 64 }
        } });
        assert_eq!(
            reported_tokens(&chat),
            counts(Some(1200), Some(90), Some(64))
        );
        let responses = json!({ "usage": {
            "input_tokens": 1200,
            "output_tokens": 90,
            "output_tokens_details": { "reasoning_tokens": 0 }
        } });
        assert_eq!(
            reported_tokens(&responses),
            counts(Some(1200), Some(90), Some(0))
        );
        let gemini = json!({ "usageMetadata": {
            "promptTokenCount": 1300,
            "candidatesTokenCount": 40,
            "thoughtsTokenCount": 500
        } });
        assert_eq!(
            reported_tokens(&gemini),
            counts(Some(1300), Some(40), Some(500))
        );
        // Thinking is not told apart, and an answer may report no usage at all.
        let plain = json!({ "usage": { "input_tokens": 1500, "output_tokens": 70 } });
        assert_eq!(reported_tokens(&plain), counts(Some(1500), Some(70), None));
        let silent = json!({ "choices": [] });
        assert_eq!(reported_tokens(&silent), ReportedTokens::default());
    }
}
