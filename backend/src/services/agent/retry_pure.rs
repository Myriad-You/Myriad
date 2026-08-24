//! Pure retry decision helpers for the agent executor.
//!
//! The async retry loop (sleep, SSE, breaker, execute_step) stays in
//! `executor/retry.rs`. Domain owns:
//! - whether another attempt is allowed
//! - delay computation (base + exponential + multiplier, cap)
//! - default max_attempts by capability
//! - prepend step id formatting
//! - multi-attempt error message assembly

use crate::services::agent::executor_utils_pure::truncate_str;
use crate::services::agent::types::{RecipeStep, RetryConfig as StepRetryConfig};

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

/// Base delay and whether exponential backoff is enabled for a step.
pub fn step_retry_delay_config(step: &RecipeStep) -> (u64, bool) {
    let base_delay = step
        .retry
        .as_ref()
        .map(|r: &StepRetryConfig| r.delay_ms.max(RETRY_BASE_DELAY_FLOOR_MS))
        .unwrap_or(RETRY_DEFAULT_BASE_DELAY_MS);
    let use_backoff = step
        .retry
        .as_ref()
        .map(|r: &StepRetryConfig| r.exponential_backoff)
        .unwrap_or(true);
    (base_delay, use_backoff)
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

/// Default max attempts for a step (shared by serial / DAG / resume paths).
///
/// Explicit `step.retry.max_attempts` is clamped to 3. Otherwise AI / skill /
/// prompt.generate get 2 attempts; other capabilities get 1.
pub fn default_max_retries(step: &RecipeStep) -> u32 {
    step.retry
        .as_ref()
        .map(|r| r.max_attempts.min(3))
        .unwrap_or_else(|| {
            if step.capability_id.starts_with("ai.")
                || step.capability_id.starts_with("skill:")
                || step.capability_id == "prompt.generate"
            {
                2
            } else {
                1
            }
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::types::{FailureStrategy, RecipeStep, RetryConfig};

    fn step(cap: &str, retry: Option<RetryConfig>) -> RecipeStep {
        RecipeStep {
            id: "s1".into(),
            order: 1,
            capability_id: cap.into(),
            action: "execute".into(),
            params: Default::default(),
            depends_on: vec![],
            on_failure: FailureStrategy::Abort,
            retry,
            timeout_ms: None,
            generator: None,
            model_tier: None,
        }
    }

    #[test]
    fn should_retry_requires_budget_and_retryable() {
        assert!(should_retry_step(1, 3, 2, true));
        assert!(!should_retry_step(3, 3, 2, true)); // count reached max
        assert!(!should_retry_step(1, 3, 0, true)); // no budget
        assert!(!should_retry_step(1, 3, 2, false)); // not retryable
    }

    #[test]
    fn delay_exponential_and_cap() {
        let (base, backoff) = step_retry_delay_config(&step(
            "http.fetch",
            Some(RetryConfig {
                max_attempts: 3,
                delay_ms: 200,
                exponential_backoff: true,
            }),
        ));
        assert_eq!(base, 200);
        assert!(backoff);
        // retry_count=1 → 200 * 2^0 = 200; *1.0
        assert_eq!(compute_retry_delay_ms(200, true, 1, 1.0), 200);
        // retry_count=3 → 200 * 4 = 800; *5.0 = 4000
        assert_eq!(compute_retry_delay_ms(200, true, 3, 5.0), 4000);
        // no backoff
        assert_eq!(compute_retry_delay_ms(500, false, 5, 1.0), 500);
        // floor when delay_ms < 100
        let (base2, _) = step_retry_delay_config(&step(
            "x",
            Some(RetryConfig {
                max_attempts: 2,
                delay_ms: 10,
                exponential_backoff: false,
            }),
        ));
        assert_eq!(base2, RETRY_BASE_DELAY_FLOOR_MS);
        // cap
        assert_eq!(
            compute_retry_delay_ms(20_000, true, 5, 10.0),
            RETRY_DELAY_CAP_MS
        );
    }

    #[test]
    fn default_max_retries_by_capability() {
        assert_eq!(default_max_retries(&step("ai.chat", None)), 2);
        assert_eq!(default_max_retries(&step("skill:foo", None)), 2);
        assert_eq!(default_max_retries(&step("prompt.generate", None)), 2);
        assert_eq!(default_max_retries(&step("http.fetch", None)), 1);
        assert_eq!(
            default_max_retries(&step(
                "http.fetch",
                Some(RetryConfig {
                    max_attempts: 10,
                    delay_ms: 100,
                    exponential_backoff: true,
                })
            )),
            3
        );
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
