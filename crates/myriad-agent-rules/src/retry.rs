//! Retry/failure types and helpers that do not take `&RecipeStep`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 失败处理策略
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FailureStrategy {
    /// 终止整个方案
    Abort,
    /// 跳过并继续
    Skip,
    /// 使用默认值继续
    UseDefault(Value),
    /// 回退到备用能力
    Fallback(String),
}

/// 重试配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// 最大重试次数
    pub max_attempts: u32,
    /// 重试间隔（毫秒）
    pub delay_ms: u64,
    /// 指数退避
    pub exponential_backoff: bool,
}

/// Max sleep between retries (ms).
pub const RETRY_DELAY_CAP_MS: u64 = 30_000;
/// Floor for configured per-step delay_ms.
pub const RETRY_BASE_DELAY_FLOOR_MS: u64 = 100;
/// Default base delay when step.retry is absent.
pub const RETRY_DEFAULT_BASE_DELAY_MS: u64 = 500;

/// Whether the executor should attempt another retry.
///
/// `retry_count` is the number of failures so far (1 after first failure).
pub fn should_retry_step(
    retry_count: u32,
    max_attempts: u32,
    global_budget: u32,
    analysis_retryable: bool,
) -> bool {
    retry_count < max_attempts && global_budget > 0 && analysis_retryable
}

/// Compute sleep delay after `retry_count` failures (1-based failure index).
///
/// Exponential: `base * 2^(retry_count-1)` when backoff is on; then multiply by
/// analyzer `delay_multiplier` and cap at [`RETRY_DELAY_CAP_MS`].
pub fn compute_retry_delay_ms(
    base_delay: u64,
    use_backoff: bool,
    retry_count: u32,
    delay_multiplier: f64,
) -> u64 {
    let delay = if use_backoff {
        base_delay.saturating_mul(2u64.saturating_pow(retry_count.saturating_sub(1)))
    } else {
        base_delay
    };
    let adjusted = (delay as f64 * delay_multiplier) as u64;
    adjusted.min(RETRY_DELAY_CAP_MS)
}

/// Build a deterministic prepend step id for analyzer-suggested capabilities.
pub fn prepend_step_id(parent_step_id: &str, retry_count: u32) -> String {
    format!("{parent_step_id}_prepend_{retry_count}")
}

/// Assemble final error text when multiple attempts failed.
pub fn format_retry_final_error(retry_errors: &[String], last_error: &str) -> String {
    if retry_errors.len() > 1 {
        let previous: Vec<_> = retry_errors[..retry_errors.len() - 1]
            .iter()
            .map(|e| truncate_str(e, 120).to_string())
            .collect();
        format!(
            "{} (previous {} attempts: {})",
            last_error,
            previous.len(),
            previous.join("; ")
        )
    } else {
        last_error.to_string()
    }
}

fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn failure_strategy_and_retry_config_roundtrip() {
        assert_eq!(
            serde_json::to_value(&FailureStrategy::Abort).unwrap(),
            json!("abort")
        );
        assert_eq!(
            serde_json::to_value(&FailureStrategy::Skip).unwrap(),
            json!("skip")
        );
        let fallback: FailureStrategy =
            serde_json::from_value(json!({ "fallback": "ai.chat" })).unwrap();
        assert_eq!(fallback, FailureStrategy::Fallback("ai.chat".into()));
        let default: FailureStrategy = serde_json::from_value(json!({ "use_default": 1 })).unwrap();
        assert_eq!(default, FailureStrategy::UseDefault(json!(1)));

        let retry: RetryConfig = serde_json::from_value(json!({
            "max_attempts": 3,
            "delay_ms": 200,
            "exponential_backoff": true
        }))
        .unwrap();
        assert_eq!(retry.max_attempts, 3);
        assert_eq!(retry.delay_ms, 200);
        assert!(retry.exponential_backoff);
    }

    #[test]
    fn should_retry_requires_budget_and_retryable() {
        assert!(should_retry_step(1, 3, 2, true));
        assert!(!should_retry_step(3, 3, 2, true));
        assert!(!should_retry_step(1, 3, 0, true));
        assert!(!should_retry_step(1, 3, 2, false));
    }

    #[test]
    fn delay_exponential_and_cap() {
        assert_eq!(compute_retry_delay_ms(200, true, 1, 1.0), 200);
        assert_eq!(compute_retry_delay_ms(200, true, 3, 5.0), 4000);
        assert_eq!(compute_retry_delay_ms(500, false, 5, 1.0), 500);
        assert_eq!(
            compute_retry_delay_ms(20_000, true, 5, 10.0),
            RETRY_DELAY_CAP_MS
        );
        assert_eq!(RETRY_BASE_DELAY_FLOOR_MS, 100);
        assert_eq!(RETRY_DEFAULT_BASE_DELAY_MS, 500);
    }

    #[test]
    fn prepend_id_and_final_error() {
        assert_eq!(prepend_step_id("step_a", 2), "step_a_prepend_2");
        assert_eq!(format_retry_final_error(&["only".into()], "only"), "only");
        let multi = format_retry_final_error(
            &["first error".into(), "second error".into()],
            "second error",
        );
        assert!(multi.contains("second error"));
        assert!(multi.contains("previous 1 attempts"));
        assert!(multi.contains("first error"));
    }
}
