//! Pure parameter projection for agent system_op handlers.
//!
//! Handlers keep DB/scheduler/FS. Domain owns:
//! - scheduler create schedule-type / execution-target / schedule-config build
//! - brew.schedule action validation
//! - heartbeat task-id param keys

use serde_json::{json, Value};
use std::collections::HashMap;

pub use myriad_agent_rules::{
    build_schedule_config, extract_raw_backend_actions, heartbeat_task_id,
    heartbeat_update_has_fields, parse_brew_schedule_action, parse_execution_target,
    parse_schedule_type, AgentExecutionTarget, AgentScheduleType, BrewScheduleAction,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_type_and_execution_target() {
        assert_eq!(
            parse_schedule_type(Some("CRON")).unwrap(),
            AgentScheduleType::Cron
        );
        assert!(parse_schedule_type(None).is_err());
        assert!(parse_schedule_type(Some("weekly")).is_err());

        assert_eq!(
            parse_execution_target(None, true).unwrap(),
            AgentExecutionTarget::Backend
        );
        assert_eq!(
            parse_execution_target(None, false).unwrap(),
            AgentExecutionTarget::Frontend
        );
        assert!(parse_execution_target(Some("sideways"), false).is_err());
        assert!(AgentExecutionTarget::Both.requires_backend_actions());
        assert!(!AgentExecutionTarget::Frontend.requires_backend_actions());
    }

    #[test]
    fn build_schedule_config_variants() {
        let obj = json!({ "cron": "0 * * * *" });
        assert_eq!(
            build_schedule_config(AgentScheduleType::Cron, Some(&obj), None, None, None, None)
                .unwrap(),
            obj
        );
        assert_eq!(
            build_schedule_config(
                AgentScheduleType::Cron,
                None,
                Some("*/5 * * * *"),
                None,
                None,
                None
            )
            .unwrap()["cron"],
            "*/5 * * * *"
        );
        assert_eq!(
            build_schedule_config(
                AgentScheduleType::Interval,
                None,
                None,
                Some(60),
                None,
                None
            )
            .unwrap()["interval"],
            60
        );
        assert_eq!(
            build_schedule_config(AgentScheduleType::Once, None, None, None, Some(99), None)
                .unwrap()["at"],
            99
        );
        assert_eq!(
            build_schedule_config(
                AgentScheduleType::Daily,
                None,
                None,
                None,
                None,
                Some("08:00")
            )
            .unwrap()["time"],
            "08:00"
        );
        assert!(
            build_schedule_config(AgentScheduleType::Cron, None, None, None, None, None).is_err()
        );
    }

    #[test]
    fn heartbeat_and_brew_schedule_helpers() {
        let mut params = HashMap::new();
        params.insert("taskId".into(), json!("hb-1"));
        assert_eq!(heartbeat_task_id(&params), Some("hb-1"));
        assert!(!heartbeat_update_has_fields(None, None, None, None));
        assert!(heartbeat_update_has_fields(Some("n"), None, None, None));

        assert_eq!(
            parse_brew_schedule_action(None).unwrap(),
            BrewScheduleAction::Status
        );
        assert_eq!(
            parse_brew_schedule_action(Some("refresh")).unwrap(),
            BrewScheduleAction::Refresh
        );
        assert!(parse_brew_schedule_action(Some("explode")).is_err());

        let mut params = HashMap::new();
        params.insert("action".into(), json!({ "type": "x" }));
        let raw = extract_raw_backend_actions(&params).unwrap();
        assert!(raw.is_array());
    }
}
