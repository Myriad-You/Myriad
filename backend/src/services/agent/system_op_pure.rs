//! Pure parameter projection for agent system_op handlers.
//!
//! Handlers keep DB/scheduler/FS. Domain owns:
//! - scheduler create schedule-type / execution-target / schedule-config build
//! - brew.schedule action validation
//! - heartbeat task-id param keys

use serde_json::{json, Value};
use std::collections::HashMap;

/// Supported scheduler schedule types (matches DB enum names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentScheduleType {
    Cron,
    Interval,
    Once,
    Daily,
}

impl AgentScheduleType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cron => "cron",
            Self::Interval => "interval",
            Self::Once => "once",
            Self::Daily => "daily",
        }
    }
}

/// Parse scheduleType (or infer cron from legacy cronExpression).
pub fn parse_schedule_type(
    schedule_type_name: Option<&str>,
    has_legacy_cron: bool,
) -> Result<AgentScheduleType, String> {
    let name = schedule_type_name
        .or_else(|| has_legacy_cron.then_some("cron"))
        .ok_or_else(|| "Missing scheduleType parameter".to_string())?;
    match name.to_ascii_lowercase().as_str() {
        "cron" => Ok(AgentScheduleType::Cron),
        "interval" => Ok(AgentScheduleType::Interval),
        "once" => Ok(AgentScheduleType::Once),
        "daily" => Ok(AgentScheduleType::Daily),
        _ => Err(format!("Invalid scheduleType: {name}")),
    }
}

/// Execution target for scheduled tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentExecutionTarget {
    Backend,
    Frontend,
    Both,
}

impl AgentExecutionTarget {
    pub fn requires_backend_actions(self) -> bool {
        matches!(self, Self::Backend | Self::Both)
    }
}

/// Parse executionTarget, defaulting by whether backend actions are present.
pub fn parse_execution_target(
    name: Option<&str>,
    has_backend_actions: bool,
) -> Result<AgentExecutionTarget, String> {
    let default = if has_backend_actions {
        "backend"
    } else {
        "frontend"
    };
    let name = name.unwrap_or(default);
    match name.to_ascii_lowercase().as_str() {
        "backend" => Ok(AgentExecutionTarget::Backend),
        "frontend" => Ok(AgentExecutionTarget::Frontend),
        "both" => Ok(AgentExecutionTarget::Both),
        _ => Err(format!("Invalid executionTarget: {name}")),
    }
}

/// Build schedule config object from params + typed schedule kind.
pub fn build_schedule_config(
    schedule_type: AgentScheduleType,
    schedule_obj: Option<&Value>,
    legacy_cron: Option<&str>,
    interval: Option<i64>,
    at: Option<i64>,
    daily_time: Option<&str>,
) -> Result<Value, String> {
    if let Some(value) = schedule_obj {
        if value.is_object() {
            return Ok(value.clone());
        }
    }
    match schedule_type {
        AgentScheduleType::Cron => Ok(json!({
            "cron": legacy_cron.ok_or("Missing cron schedule")?
        })),
        AgentScheduleType::Interval => Ok(json!({
            "interval": interval.ok_or("Missing interval schedule")?
        })),
        AgentScheduleType::Once => Ok(json!({
            "at": at.ok_or("Missing at schedule")?
        })),
        AgentScheduleType::Daily => Ok(json!({
            "time": daily_time.ok_or("Missing daily time schedule")?
        })),
    }
}

/// Extract optional task id from heartbeat params (`id` / `taskId` / `task_id`).
pub fn heartbeat_task_id(params: &HashMap<String, Value>) -> Option<&str> {
    params
        .get("id")
        .or_else(|| params.get("taskId"))
        .or_else(|| params.get("task_id"))
        .and_then(Value::as_str)
}

/// Whether at least one heartbeat update field is present.
pub fn heartbeat_update_has_fields(
    name: Option<&str>,
    schedule: Option<&str>,
    action: Option<&str>,
    enabled: Option<bool>,
) -> bool {
    name.is_some() || schedule.is_some() || action.is_some() || enabled.is_some()
}

/// brew.schedule actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrewScheduleAction {
    Start,
    Stop,
    Refresh,
    Status,
}

/// Parse brew.schedule action string (default status).
pub fn parse_brew_schedule_action(action: Option<&str>) -> Result<BrewScheduleAction, String> {
    match action.unwrap_or("status") {
        "start" => Ok(BrewScheduleAction::Start),
        "stop" => Ok(BrewScheduleAction::Stop),
        "refresh" => Ok(BrewScheduleAction::Refresh),
        "status" => Ok(BrewScheduleAction::Status),
        other => Err(format!("Unknown brew schedule action: {other}")),
    }
}

/// Normalize backendActions / action field into optional array-bearing Value.
pub fn extract_raw_backend_actions(params: &HashMap<String, Value>) -> Option<Value> {
    params
        .get("backendActions")
        .or_else(|| params.get("backend_actions"))
        .cloned()
        .or_else(|| {
            params.get("action").cloned().map(|action| match action {
                Value::Array(_) => action,
                _ => Value::Array(vec![action]),
            })
        })
}

/// Legacy cron expression keys used by older agent plans.
pub fn extract_legacy_cron(params: &HashMap<String, Value>) -> Option<&str> {
    params
        .get("cronExpression")
        .or_else(|| params.get("cron"))
        .or_else(|| params.get("schedule").filter(|value| value.is_string()))
        .and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_type_and_execution_target() {
        assert_eq!(
            parse_schedule_type(Some("CRON"), false).unwrap(),
            AgentScheduleType::Cron
        );
        assert_eq!(
            parse_schedule_type(None, true).unwrap(),
            AgentScheduleType::Cron
        );
        assert!(parse_schedule_type(None, false).is_err());
        assert!(parse_schedule_type(Some("weekly"), false).is_err());

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
        assert_eq!(
            extract_legacy_cron(&HashMap::from([("cron".into(), json!("0 0 * * *"))])),
            Some("0 0 * * *")
        );
    }
}
