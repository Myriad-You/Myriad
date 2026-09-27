//! Step failure types, re-exported from the rules crate.

pub use myriad_agent_rules::StepError;
#[cfg(test)]
pub use myriad_agent_rules::StepOutcome;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_producer_marks_an_unknown_outcome_the_same_way() {
        assert_eq!(
            myriad_mcp::OUTCOME_UNKNOWN_PREFIX,
            myriad_agent_rules::OUTCOME_UNKNOWN_PREFIX
        );
        let mcp = StepError::from_handler(format!(
            "{} MCP call timed out",
            myriad_mcp::OUTCOME_UNKNOWN_PREFIX
        ));
        assert_eq!(mcp.outcome, StepOutcome::Unknown);
    }
}
