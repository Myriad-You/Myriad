//! Durable task spend. Reservations survive an interrupted provider request.
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Budget {
    pub spent_tokens: u64,
    pub reserved_tokens: u64,
    pub limit_tokens: u64,
}

/// Smallest output allowance worth a model turn.
const MIN_OUTPUT: u64 = 4096;

impl Default for Budget {
    fn default() -> Self {
        Self {
            spent_tokens: 0,
            reserved_tokens: 0,
            limit_tokens: 128_000,
        }
    }
}

impl Budget {
    pub fn remaining(&self) -> u64 {
        self.limit_tokens
            .saturating_sub(self.spent_tokens.saturating_add(self.reserved_tokens))
    }

    pub fn settle(&mut self, actual: Option<u64>) {
        self.spent_tokens = self
            .spent_tokens
            .saturating_add(actual.unwrap_or(self.reserved_tokens));
        self.reserved_tokens = 0;
    }

    pub fn reserve_model(&mut self, input_estimate: u64) -> Result<u32, String> {
        // A full edit pass on a long note is reasoning plus every replacement in one turn;
        // 8192 cut those off mid-call.
        let output = self.remaining().saturating_sub(input_estimate).min(16_384);
        // A sliver of output is spent on reasoning and cuts the call off mid-arguments: a
        // request that cannot finish is refused instead of billed.
        if output == 0 || output < MIN_OUTPUT.min(self.limit_tokens / 4) {
            return Err("The task reached its token budget".into());
        }
        self.reserved_tokens = input_estimate.saturating_add(output);
        Ok(output as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservation_survives_restart_and_limits_output() {
        let mut budget = Budget {
            limit_tokens: 100,
            ..Default::default()
        };
        assert_eq!(budget.reserve_model(60).unwrap(), 40);
        let mut restored: Budget =
            serde_json::from_value(serde_json::to_value(budget).unwrap()).unwrap();
        restored.settle(None);
        assert_eq!(restored.spent_tokens, 100);
        assert!(restored.reserve_model(1).is_err());
    }

    #[test]
    fn a_turn_too_small_to_finish_is_refused_not_billed() {
        let mut budget = Budget::default();
        budget.spent_tokens = budget.limit_tokens - 20_000;
        // 3k of output would be spent on reasoning and cut the call off.
        assert!(budget.reserve_model(17_000).is_err());
        assert_eq!(budget.reserved_tokens, 0);
        assert_eq!(budget.reserve_model(10_000).unwrap(), 10_000);
    }
}
