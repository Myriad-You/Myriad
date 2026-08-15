//! Thin adapter re-exporting pure DAG scheduling from
//! [`crate::services::agent::dag_pure`].
//!
//! Optional tracing on failure lives here so pure domain stays I/O-free.

pub use crate::services::agent::dag_pure::DagScheduler;

use crate::services::agent::types::FailureStrategy;

/// Adapter wrapper: mark failed + log strategy effect for operators.
pub fn mark_failed_with_log(scheduler: &mut DagScheduler, step_id: &str, strategy: &FailureStrategy) {
    let blocks = scheduler.mark_failed(step_id, strategy);
    if blocks {
        tracing::warn!(
            step_id = step_id,
            "[DAG] Step failed with Abort strategy, blocking dependent steps"
        );
    } else {
        tracing::debug!(
            step_id = step_id,
            strategy = ?strategy,
            "[DAG] Step failed with {:?} strategy, marking as completed for dependents",
            strategy
        );
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;
    use crate::services::agent::types::RecipeStep;
    use std::collections::HashMap;

    fn make_step(id: &str, depends_on: Vec<&str>) -> RecipeStep {
        RecipeStep {
            id: id.to_string(),
            order: 0,
            capability_id: format!("test.{}", id),
            action: "test".to_string(),
            params: HashMap::new(),
            depends_on: depends_on.into_iter().map(String::from).collect(),
            on_failure: FailureStrategy::Abort,
            retry: None,
            timeout_ms: None,
            model_tier: None,
            generator: None,
        }
    }

    #[test]
    fn adapter_dag_scheduler_is_shipped_pure() {
        let steps = vec![make_step("a", vec![]), make_step("b", vec!["a"])];
        let mut dag = DagScheduler::new(&steps).unwrap();
        assert!(dag.is_parallel_mode());
        assert_eq!(dag.get_ready_steps().len(), 1);
        mark_failed_with_log(&mut dag, "a", &FailureStrategy::Skip);
        let ready = dag.get_ready_steps();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "b");
    }
}
