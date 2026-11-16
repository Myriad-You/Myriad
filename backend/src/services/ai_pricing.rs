//! What one model answer cost, in dollars, from what the answer itself
//! says: the source's own figure when it gives one (OpenRouter's
//! `usage.cost`), else the token counts it reported (`usage`) at the list
//! prices below. An answer that reports no usage has no cost here: the
//! ledger then falls back to its length/4 estimate and says so.
//!
//! Usage in the two shapes the site's sources answer in:
//! - Anthropic (and OpenAI Responses): `usage.input_tokens`,
//!   `usage.output_tokens`, with `cache_creation_input_tokens` and
//!   `cache_read_input_tokens` beside them when a prompt was cached;
//! - OpenAI-compatible chat: `usage.prompt_tokens`,
//!   `usage.completion_tokens`.

use serde_json::Value;

/// Dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    /// Past this many prompt tokens (input, cache writes and reads
    /// together), the whole call is billed at `long`.
    pub long_after: Option<u64>,
    pub long: Option<(f64, f64)>,
}

/// Anthropic's list prices, as kept on 2026-10-06 (checked against the
/// published table by the coordinating session, 2026-11-11). A model not
/// here has no price: a capped run refuses to start on it, and the ledger
/// keeps its cost empty rather than guess.
pub const PRICES: &[(&str, Price)] = &[
    (
        "claude-haiku-5-5",
        Price {
            input: 0.10,
            output: 0.50,
            long_after: Some(100_000),
            long: Some((0.50, 2.50)),
        },
    ),
    (
        "claude-sonnet-5-5",
        Price {
            input: 2.0,
            output: 10.0,
            long_after: None,
            long: None,
        },
    ),
    (
        "claude-opus-5-5",
        Price {
            input: 4.0,
            output: 20.0,
            long_after: None,
            long: None,
        },
    ),
];

/// What a cache write and a cache read cost against the input price:
/// Anthropic's standing rule (a write 1.25 times, a read a tenth).
const CACHE_WRITE: f64 = 1.25;
const CACHE_READ: f64 = 0.1;

/// The token counts an answer reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

impl Usage {
    /// Every prompt token, cached or not.
    pub fn prompt(&self) -> u64 {
        self.input + self.cache_write + self.cache_read
    }

    pub fn add(&mut self, other: Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_write += other.cache_write;
        self.cache_read += other.cache_read;
    }

    /// Field by field the larger: a stream says its counts more than once,
    /// each time as they stand so far.
    pub fn widen(&mut self, other: Usage) {
        self.input = self.input.max(other.input);
        self.output = self.output.max(other.output);
        self.cache_write = self.cache_write.max(other.cache_write);
        self.cache_read = self.cache_read.max(other.cache_read);
    }
}

/// The usage an answer (or one event of a stream) reports, if any.
pub fn usage_of(answer: &Value) -> Option<Usage> {
    let usage = answer.get("usage")?;
    let count = |name: &str| usage.get(name).and_then(Value::as_u64);
    if let Some(input) = count("input_tokens") {
        return Some(Usage {
            input,
            output: count("output_tokens").unwrap_or(0),
            cache_write: count("cache_creation_input_tokens").unwrap_or(0),
            cache_read: count("cache_read_input_tokens").unwrap_or(0),
        });
    }
    if let Some(output) = count("output_tokens") {
        // A stream's later event: only how far the output has got.
        return Some(Usage {
            output,
            ..Usage::default()
        });
    }
    let input = count("prompt_tokens")?;
    Some(Usage {
        input,
        output: count("completion_tokens").unwrap_or(0),
        ..Usage::default()
    })
}

/// The price of `model`: its own name, or its name with a date after it
/// (`claude-haiku-5-5-20260901`).
pub fn price_of(model: &str) -> Option<Price> {
    let model = model.trim();
    PRICES.iter().find_map(|(name, price)| {
        (model == *name
            || model
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('-')))
        .then_some(*price)
    })
}

/// What `usage` of `model` costs at the list price, in dollars.
pub fn cost_usd(model: &str, usage: &Usage) -> Option<f64> {
    let price = price_of(model)?;
    let (input, output) = match (price.long_after, price.long) {
        (Some(after), Some(long)) if usage.prompt() > after => long,
        _ => (price.input, price.output),
    };
    let prompt = usage.input as f64
        + usage.cache_write as f64 * CACHE_WRITE
        + usage.cache_read as f64 * CACHE_READ;
    Some((prompt * input + usage.output as f64 * output) / 1_000_000.0)
}

/// What one answer says about its cost.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Answered {
    pub usage: Option<Usage>,
    /// Dollars: the source's figure, else priced from `usage`.
    pub cost_usd: Option<f64>,
    /// The model it names, or the one asked for.
    pub model: String,
}

/// An answer's usage and cost. `asked` is the model the call named, for an
/// answer that does not say which model answered.
pub fn answered(answer: &Value, asked: Option<&str>) -> Answered {
    let model = answer
        .get("model")
        .and_then(Value::as_str)
        .or(asked)
        .unwrap_or_default()
        .to_string();
    let usage = usage_of(answer);
    let said = answer
        .get("usage")
        .and_then(|usage| usage.get("cost"))
        .and_then(Value::as_f64);
    let cost_usd = said.or_else(|| usage.as_ref().and_then(|usage| cost_usd(&model, usage)));
    Answered {
        usage,
        cost_usd,
        model,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_anthropic_answer_is_priced_from_its_own_usage() {
        let answer = json!({
            "model": "claude-haiku-5-5",
            "usage": {"input_tokens": 2000, "output_tokens": 100,
                "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0},
        });
        let got = answered(&answer, None);
        assert_eq!(
            got.usage,
            Some(Usage {
                input: 2000,
                output: 100,
                ..Usage::default()
            })
        );
        // 2000 × $0.10 + 100 × $0.50, per million.
        let cost = got.cost_usd.unwrap();
        assert!((cost - 0.00025).abs() < 1e-12, "{cost}");
    }

    #[test]
    fn a_long_prompt_and_a_cache_are_priced_as_billed() {
        let long = Usage {
            input: 100_001,
            output: 1000,
            ..Usage::default()
        };
        let cost = cost_usd("claude-haiku-5-5-20260901", &long).unwrap();
        assert!((cost - (100_001.0 * 0.5 + 1000.0 * 2.5) / 1e6).abs() < 1e-12);
        let cached = Usage {
            input: 100,
            output: 0,
            cache_write: 1000,
            cache_read: 10_000,
        };
        let cost = cost_usd("claude-sonnet-5-5", &cached).unwrap();
        assert!((cost - (100.0 + 1250.0 + 1000.0) * 2.0 / 1e6).abs() < 1e-12);
    }

    #[test]
    fn the_sources_own_figure_wins_and_an_unknown_model_has_no_price() {
        let routed = json!({"model": "anthropic/claude-haiku-5.5",
            "usage": {"prompt_tokens": 10, "completion_tokens": 2, "cost": 0.0003}});
        assert_eq!(answered(&routed, None).cost_usd, Some(0.0003));
        let unknown = json!({"usage": {"prompt_tokens": 10, "completion_tokens": 2}});
        let got = answered(&unknown, Some("someone/else"));
        assert_eq!(got.model, "someone/else");
        assert!(got.usage.is_some() && got.cost_usd.is_none());
        assert_eq!(price_of("claude-haiku-5-50"), None);
        assert_eq!(answered(&json!({"choices": []}), None).usage, None);
    }
}
