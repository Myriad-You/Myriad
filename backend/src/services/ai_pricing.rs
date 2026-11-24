//! What one model answer cost, in dollars, from what the answer itself
//! says: the source's own figure when it gives one (OpenRouter's
//! `usage.cost`), else the token counts it reported (`usage`) at the list
//! prices below. An answer that reports no usage has no cost here: the
//! ledger then falls back to its length/4 estimate and says so.
//!
//! Usage in the two shapes the site's sources answer in:
//! - Anthropic (and OpenAI Responses): `usage.input_tokens`,
//!   `usage.output_tokens`, with `cache_creation_input_tokens` and
//!   `cache_read_input_tokens` beside them when a prompt was cached, and in
//!   `cache_creation` how long what was written is kept;
//! - OpenAI-compatible chat: `usage.prompt_tokens`,
//!   `usage.completion_tokens`.

use serde_json::Value;

/// Dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    /// What a token read from the cache costs against `input`.
    pub cache_read: f64,
    /// Past this many prompt tokens (input, cache writes and reads
    /// together), the whole call is billed at `long`.
    pub long_after: Option<u64>,
    pub long: Option<(f64, f64)>,
}

/// Anthropic's list prices, as kept on 2026-10-06 (checked against the
/// published table by the coordinating session, 2026-11-11; what a cache
/// read costs, on 2026-11-24). A model not
/// here has no price: a capped run refuses to start on it, and the ledger
/// keeps its cost empty rather than guess.
pub const PRICES: &[(&str, Price)] = &[
    (
        "claude-haiku-5-5",
        Price {
            input: 0.10,
            output: 0.50,
            cache_read: 0.1,
            long_after: Some(100_000),
            long: Some((0.50, 2.50)),
        },
    ),
    (
        "claude-sonnet-5-5",
        Price {
            input: 2.0,
            output: 10.0,
            cache_read: 0.05,
            long_after: None,
            long: None,
        },
    ),
    (
        "claude-opus-5-5",
        Price {
            input: 4.0,
            output: 20.0,
            cache_read: 0.05,
            long_after: None,
            long: None,
        },
    ),
];

/// What a cache write costs against the input price, kept five minutes and
/// kept an hour: the same for every model (Anthropic's prompt caching page,
/// 2026-11-24; what a read costs is each model's own).
const CACHE_WRITE: f64 = 1.25;
const CACHE_WRITE_HOUR: f64 = 2.0;

/// The token counts an answer reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    /// Of `cache_write`, what is kept for an hour.
    pub cache_write_hour: u64,
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
        self.cache_write_hour += other.cache_write_hour;
        self.cache_read += other.cache_read;
    }

    /// The larger: a stream says its counts more than once, each time as
    /// they stand so far. The prompt's are one count in parts, taken
    /// together (the same prompt said once whole and once in parts is not
    /// both): the larger prompt's, and of the same prompt, the one saying
    /// more of it was cached.
    pub fn widen(&mut self, other: Usage) {
        let prompt = |usage: &Usage| (usage.prompt(), usage.cache_write + usage.cache_read);
        if prompt(&other) > prompt(self) {
            self.input = other.input;
            self.cache_write = other.cache_write;
            self.cache_read = other.cache_read;
        }
        // An event that says the totals again may leave out how long what
        // was written is kept.
        self.cache_write_hour = self.cache_write_hour.max(other.cache_write_hour);
        self.output = self.output.max(other.output);
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
            cache_write_hour: usage
                .get("cache_creation")
                .and_then(|kept| kept.get("ephemeral_1h_input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
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
    // OpenAI-shaped (OpenRouter): what was read from a cache or written to
    // one is inside `prompt_tokens`, told apart in `prompt_tokens_details`.
    let prompt = count("prompt_tokens")?;
    let details = |name: &str| {
        usage
            .get("prompt_tokens_details")
            .and_then(|details| details.get(name))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let cache_read = details("cached_tokens").min(prompt);
    let cache_write = details("cache_write_tokens").min(prompt - cache_read);
    Some(Usage {
        input: prompt - cache_read - cache_write,
        output: count("completion_tokens").unwrap_or(0),
        cache_write,
        // OpenRouter tells the cost itself.
        cache_write_hour: 0,
        cache_read,
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
    let hour = usage.cache_write_hour.min(usage.cache_write);
    let prompt = usage.input as f64
        + (usage.cache_write - hour) as f64 * CACHE_WRITE
        + hour as f64 * CACHE_WRITE_HOUR
        + usage.cache_read as f64 * price.cache_read;
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
            ..Usage::default()
        };
        // A read costs a twentieth on Sonnet and Opus, a tenth on Haiku.
        let cost = cost_usd("claude-opus-5-5", &cached).unwrap();
        assert!((cost - (100.0 + 1250.0 + 500.0) * 4.0 / 1e6).abs() < 1e-12);
        let cost = cost_usd("claude-sonnet-5-5", &cached).unwrap();
        assert!((cost - (100.0 + 1250.0 + 500.0) * 2.0 / 1e6).abs() < 1e-12);
        let cost = cost_usd("claude-haiku-5-5", &cached).unwrap();
        assert!((cost - (100.0 + 1250.0 + 1000.0) * 0.1 / 1e6).abs() < 1e-12);
    }

    /// What is written to be kept an hour costs twice the input price, and
    /// is told apart from what is kept five minutes.
    #[test]
    fn what_is_kept_an_hour_is_priced_as_written_for_one() {
        let answer = json!({
            "model": "claude-sonnet-5-5",
            "usage": {"input_tokens": 100, "output_tokens": 0,
                "cache_creation_input_tokens": 3000, "cache_read_input_tokens": 0,
                "cache_creation": {"ephemeral_5m_input_tokens": 1000,
                    "ephemeral_1h_input_tokens": 2000}},
        });
        let got = answered(&answer, None);
        let cost = got.cost_usd.unwrap();
        assert!(
            (cost - (100.0 + 1250.0 + 4000.0) * 2.0 / 1e6).abs() < 1e-12,
            "{cost}"
        );
        assert_eq!(got.usage.unwrap().prompt(), 3100);
        // A stream that says the totals again, or larger ones, without how
        // long they are kept.
        let mut usage = Usage::default();
        usage.widen(got.usage.unwrap());
        for more in [0, 50] {
            let again = Usage {
                input: 100 + more,
                cache_write: 3000,
                ..Usage::default()
            };
            usage.widen(again);
            assert_eq!(usage.cache_write_hour, 2000, "{more}");
        }
        // Answers added up.
        usage.add(got.usage.unwrap());
        assert_eq!(usage.cache_write_hour, 4000);
        // Never more than was written.
        let odd = Usage {
            cache_write: 1000,
            cache_write_hour: 5000,
            ..Usage::default()
        };
        let cost = cost_usd("claude-sonnet-5-5", &odd).unwrap();
        assert!((cost - 2000.0 * 2.0 / 1e6).abs() < 1e-12);
    }

    #[test]
    fn openrouter_tells_what_its_prompt_read_from_and_wrote_to_a_cache() {
        let answer = json!({"usage": {"prompt_tokens": 3000, "completion_tokens": 500,
            "prompt_tokens_details": {"cached_tokens": 2200, "cache_write_tokens": 0}}});
        assert_eq!(
            usage_of(&answer),
            Some(Usage {
                input: 800,
                output: 500,
                cache_write: 0,
                cache_read: 2200,
                ..Usage::default()
            })
        );
        let first = json!({"usage": {"prompt_tokens": 3000, "completion_tokens": 5,
            "prompt_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 2200}}});
        let first = usage_of(&first).unwrap();
        assert_eq!(
            (first.input, first.cache_write, first.prompt()),
            (800, 2200, 3000)
        );
    }

    /// A stream that says its prompt whole first and in parts at its end (or
    /// the other way round) has one prompt, and every event's output.
    #[test]
    fn a_prompt_said_twice_in_a_stream_is_one_prompt() {
        let whole = Usage {
            input: 3000,
            output: 1,
            ..Usage::default()
        };
        let parts = Usage {
            input: 800,
            output: 500,
            cache_write: 0,
            cache_read: 2200,
            ..Usage::default()
        };
        for (first, then) in [(whole, parts), (parts, whole)] {
            let mut usage = Usage::default();
            usage.widen(first);
            usage.widen(then);
            assert_eq!(usage, parts);
            assert_eq!(usage.prompt(), 3000);
        }
        // An event saying only how far the output has got.
        let mut usage = parts;
        usage.widen(Usage {
            output: 900,
            ..Usage::default()
        });
        assert_eq!((usage.prompt(), usage.output), (3000, 900));
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
