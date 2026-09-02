use crate::services::agent::executor_utils_pure::truncate_str;

use crate::services::agent::types::{QuestionType, RiskLevel, UserQuestion};

use crate::services::agent::SYSTEM_USER_ID;

use chrono::{DateTime, Utc};

use serde_json::{json, Value};

use std::collections::HashMap;

/// Whether an unconfirmed dynamically-generated step must be blocked.
///
/// Aligns with [`Agent::system_sensitive_gate`]:
/// - **System / heartbeat** (`SYSTEM_USER_ID`): Medium auto-runs (same as plan-time
/// gate); High / Critical hard-block with a clear error (not silent skip).
/// - **Interactive users**: Medium and above block until confirmed.
///
/// Only Low may auto-run for normal users without an extra confirmation gate.
pub fn should_block_unconfirmed_dynamic_step(user_id: i32, risk: RiskLevel) -> bool {
    if user_id == SYSTEM_USER_ID {
        matches!(risk, RiskLevel::High | RiskLevel::Critical)
    } else {
        matches!(
            risk,
            RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dynamic_risk_gate_aligns_with_system_sensitive_gate() {
        // System/heartbeat: Medium auto-run; High+ blocked (matches system_sensitive_gate).
        assert!(should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::Critical
        ));
        assert!(should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::High
        ));
        assert!(!should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::Medium
        ));
        assert!(!should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::Low
        ));
        // Interactive: Medium+ blocked until confirmed.
        assert!(should_block_unconfirmed_dynamic_step(7, RiskLevel::High));
        assert!(should_block_unconfirmed_dynamic_step(7, RiskLevel::Medium));
        assert!(!should_block_unconfirmed_dynamic_step(7, RiskLevel::Low));
    }
}
