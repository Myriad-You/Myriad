//! Pure declarative data transform pipeline for Tapp data.transform.
//!
//! Filter/sort/map/aggregate steps are free of HTTP and storage I/O so agent
//! and API paths share one evaluator. Handlers load/save items and call
//! [`apply_pipeline`].

pub use myriad_tapp_rules::{
    apply_map_op, apply_pipeline, apply_process_step, items_from_agent_input, items_from_value,
    parse_pipeline_steps_lenient, DataTransformError, MapOp, ProcessStep, MAX_MAP_OPERATIONS,
    MAX_PIPELINE_STEPS,
};

#[cfg(test)]
mod tests {
    use super::{
        items_from_agent_input, items_from_value, parse_pipeline_steps_lenient, DataTransformError,
        MAX_PIPELINE_STEPS,
    };
    use serde_json::json;

    #[test]
    fn agent_input_and_lenient_pipeline_json() {
        assert_eq!(
            items_from_agent_input(json!([{ "a": 1 }, { "a": 2 }])).len(),
            2
        );
        assert_eq!(
            items_from_agent_input(json!({ "items": [{ "x": 1 }], "meta": true })).len(),
            1
        );
        assert!(items_from_agent_input(json!({ "not": "items" })).is_empty());
        assert!(items_from_agent_input(json!("scalar")).is_empty());

        let steps = parse_pipeline_steps_lenient(&[
            json!({ "type": "filter", "field": "a", "operator": "eq", "value": 1 }),
            json!({ "type": "unknown_noop" }),
            json!({ "type": "limit", "count": 5 }),
        ])
        .unwrap();
        assert_eq!(steps.len(), 2);

        let too_many = (0..=MAX_PIPELINE_STEPS)
            .map(|_| json!({ "type": "limit", "count": 1 }))
            .collect::<Vec<_>>();
        assert_eq!(
            parse_pipeline_steps_lenient(&too_many).unwrap_err(),
            DataTransformError::TooManySteps
        );
    }

    #[test]
    fn items_from_value_wraps_objects() {
        assert_eq!(items_from_value(json!([1, 2])).len(), 2);
        assert_eq!(items_from_value(json!({ "a": 1 })).len(), 1);
    }
}
