//! Thin adapter re-exporting pure DAG scheduling from
//! [`crate::services::agent::dag_pure`].

pub use crate::services::agent::dag_pure::DagScheduler;

#[cfg(test)]
mod adapter_tests {
    use super::*;
    use crate::services::agent::types::{FailureStrategy, RecipeStep};
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
        dag.mark_failed("a", &FailureStrategy::Skip);
        let ready = dag.get_ready_steps();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "b");
    }
}
