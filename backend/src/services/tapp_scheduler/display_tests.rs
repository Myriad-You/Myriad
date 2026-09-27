// Display impl for ExecutionTarget and unit tests for scheduler frontend contracts.

use crate::models::entities::tapp_scheduled_tasks::ExecutionTarget;

/// 实现 ToString 用于 ExecutionTarget
impl std::fmt::Display for ExecutionTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionTarget::Backend => write!(f, "backend"),
            ExecutionTarget::Frontend => write!(f, "frontend"),
            ExecutionTarget::Both => write!(f, "both"),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use serde_json::json;

    use crate::config::ModelTier;
    use crate::models::entities::tapp_scheduled_tasks::ScheduleType;

    use super::super::types_frontend::*;

    #[test]
    fn frontend_message_matches_scheduler_client_contract() {
        let message = FrontendTaskMessage {
            msg_type: "task:execute".to_string(),
            task: TaskExecutionContext {
                id: 7,
                task_id: "refresh".to_string(),
                tapp_id: "com.example.app".to_string(),
                user_id: 42,
                scope: "user".to_string(),
                scheduled_at: "2026-07-10T00:00:00Z".to_string(),
                executed_at: "2026-07-10T00:00:01Z".to_string(),
                is_compensation: false,
                payload: Some(json!({ "source": "timer" })),
            },
            payload: Some(json!({ "source": "timer" })),
            scheduled_at: "2026-07-10T00:00:00Z".to_string(),
            execution_id: 99,
            target_users: Some(vec![42]),
        };

        let value = serde_json::to_value(message).expect("message should serialize");
        assert_eq!(value["type"], "task:execute");
        assert_eq!(value["task"]["taskId"], "refresh");
        assert_eq!(value["task"]["tappId"], "com.example.app");
        // Outer + inner times both RFC3339 strings (not epoch ms)
        assert_eq!(value["scheduledAt"], "2026-07-10T00:00:00Z");
        assert_eq!(value["task"]["scheduledAt"], "2026-07-10T00:00:00Z");
        assert_eq!(value["task"]["executedAt"], "2026-07-10T00:00:01Z");
        assert_eq!(value["executionId"], 99);
        assert_eq!(value["payload"]["source"], "timer");
        assert!(value.get("execution_id").is_none());
    }

    #[test]
    fn shared_delivery_uses_subject_scoped_mailboxes() {
        assert_eq!(
            scheduler_mailbox_recipient("scheduler_ws_abc"),
            "connection:scheduler_ws_abc"
        );
    }

    #[test]
    fn interval_next_run_uses_milliseconds() {
        let from = DateTime::from_timestamp_millis(1_000_000).unwrap();
        let next = TappSchedulerEngine::calculate_next_run(
            &ScheduleType::Interval,
            &json!({ "interval": 30_000 }),
            from,
        )
        .expect("valid interval")
        .expect("next run");

        assert_eq!(next.timestamp_millis(), 1_030_000);
    }

    fn next_cron(expr: &str) -> Result<Option<String>, String> {
        // 2026-09-26 是周六
        let from = DateTime::parse_from_rfc3339("2026-09-26T01:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        TappSchedulerEngine::calculate_next_run(&ScheduleType::Cron, &json!({ "cron": expr }), from)
            .map(|next| next.map(|next| next.format("%m-%d %H:%M").to_string()))
    }

    #[test]
    fn cron_accepts_documented_five_field_unix_syntax() {
        let cases = [
            ("0 */6 * * *", "09-26 06:00"),
            ("*/30 * * * *", "09-26 02:00"),
            // 周字段按 Unix 编号：1 = 周一，0 与 7 都是周日
            ("0 9 * * 1", "09-28 09:00"),
            ("0 9 * * 0", "09-27 09:00"),
            ("0 9 * * 7", "09-27 09:00"),
            ("0 9 * * 1-5", "09-28 09:00"),
            ("0 9 * * 5-7", "09-26 09:00"),
            ("0 9 * * mon", "09-28 09:00"),
            ("0 9 * * */3", "09-26 09:00"),
            // 6 段（带秒）照旧透传
            ("0 0 9 * * *", "09-26 09:00"),
        ];
        for (expr, expected) in cases {
            assert_eq!(next_cron(expr), Ok(Some(expected.to_string())), "{expr}");
        }
        for expr in ["0 9 * * 8", "0 9 * * 5-1", "0 9 * *"] {
            assert!(next_cron(expr).is_err(), "{expr}");
        }
    }

    #[test]
    fn backend_action_pipeline_is_bounded_for_recovery_lease() {
        let actions = (0..=MAX_SCHEDULER_BACKEND_ACTIONS)
            .map(|index| {
                json!({
                    "type": "storage.set",
                    "key": format!("key{index}"),
                    "value": index,
                })
            })
            .collect::<Vec<_>>();

        assert!(normalize_backend_actions_parsed(Some(json!(actions))).is_err());
    }

    #[test]
    fn execute_parse_rejects_non_array_and_oversize() {
        assert!(parse_backend_action_wrappers(&None).unwrap().is_empty());
        assert_eq!(
            parse_backend_action_wrappers(&Some(json!({ "type": "storage.get" })))
                .unwrap_err(),
            "backendActions must be an array"
        );
        let too_many = (0..=MAX_SCHEDULER_BACKEND_ACTIONS)
            .map(|index| {
                json!({
                    "action": "storage.set",
                    "key": format!("key{index}"),
                    "value": index,
                })
            })
            .collect::<Vec<_>>();
        assert!(parse_backend_action_wrappers(&Some(json!(too_many))).is_err());
        let engine = include_str!("engine.rs");
        let attempt = engine
            .split("let (mut status, mut error, wrappers, authority)")
            .nth(1)
            .and_then(|rest| rest.split("ExecutionTarget::Frontend").next())
            .expect("execute attempt");
        assert!(attempt.contains("parse_backend_action_wrappers"));
        assert!(attempt.contains("validate_task_execution_permissions"));
        assert!(attempt.contains("execute_backend_actions"));
        assert!(!attempt.contains("actions_json.clone()"));
    }

    #[test]
    fn scheduled_ai_requires_matching_manifest_declaration() {
        let (_normalized, wrappers) = normalize_backend_actions_parsed(Some(json!([{
            "type": "ai.generate",
            "prompt": "Summarize {{input}}"
        }])))
        .unwrap();
        let without_ai = json!({
            "permissions": ["scheduler:register", "ai:generate"]
        });
        assert!(validate_backend_action_declarations_of(&without_ai, &wrappers).is_err());

        let declared = json!({
            "permissions": ["scheduler:register", "ai:generate"],
            "ai": {
                "protocolVersion": 2,
                "operations": ["generate"],
                "modelTier": "pro",
                "contextSources": [],
                "outputFormats": ["text"]
            }
        });
        assert_eq!(
            validate_backend_action_declarations_of(&declared, &wrappers).unwrap(),
            Some(ModelTier::Pro)
        );
    }
}
