//! Pure retry decision helpers for the agent executor.
//!
//! The async retry loop (sleep, SSE, breaker, execute_step) stays in
//! `executor/retry.rs`. Domain owns:
//! - whether another attempt is allowed
//! - delay computation (base + exponential + multiplier, cap)
//! - default max_attempts by capability
//! - prepend step id formatting
//! - multi-attempt error message assembly

use crate::services::agent::types::{RecipeStep, RetryConfig as StepRetryConfig};

pub use myriad_agent_rules::{
    compute_retry_delay_ms, format_retry_final_error, prepend_step_id, should_retry_step,
    RETRY_BASE_DELAY_FLOOR_MS, RETRY_DEFAULT_BASE_DELAY_MS, RETRY_DELAY_CAP_MS,
};

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
