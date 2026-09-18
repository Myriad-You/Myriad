//! Pure task-store projection and retention rules.
//!
//! Persistence / locks / DB stay in `executor/task_store.rs`. Domain owns:
//! - cancellable status set
//! - TaskStatus ↔ DB string maps
//! - lane key / session id projection
//! - terminal retention and waiting-input timeout decisions
//! - in-memory status count aggregation

pub use myriad_agent_rules::{
    TERMINAL_RETENTION_HOURS, WAITING_INPUT_TIMEOUT_HOURS, is_cancellable_task_status,
    is_terminal_past_retention, is_waiting_input_timed_out, lane_id_from_user_session,
    session_id_from_lane_id, session_id_from_lane_key, status_counts_from_iter,
    task_status_from_db_str, task_status_to_db_str, waiting_input_timeout_error,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::types::TaskStatus;
    use chrono::{Duration, Utc};

    #[test]
    fn cancellable_statuses_include_active_and_wait() {
        assert!(is_cancellable_task_status(&TaskStatus::Running));
        assert!(is_cancellable_task_status(&TaskStatus::WaitingForInput));
        assert!(is_cancellable_task_status(&TaskStatus::Paused));
        assert!(is_cancellable_task_status(&TaskStatus::Pending));
        assert!(!is_cancellable_task_status(&TaskStatus::Completed));
        assert!(!is_cancellable_task_status(&TaskStatus::Failed));
        assert!(!is_cancellable_task_status(&TaskStatus::Cancelled));
    }

    #[test]
    fn status_roundtrip_known_strings() {
        for s in [
            "pending",
            "running",
            "waiting_for_input",
            "paused",
            "completed",
            "failed",
            "cancelled",
        ] {
            assert_eq!(
                task_status_to_db_str(&task_status_from_db_str(s).expect(s)),
                s
            );
        }
        assert!(task_status_from_db_str("unknown_legacy").is_err());
        assert_ne!(
            task_status_from_db_str("unknown_legacy").ok(),
            Some(TaskStatus::Pending)
        );
    }

    #[test]
    fn session_and_lane_projection() {
        assert_eq!(
            session_id_from_lane_key("user:42:session:ses_abc"),
            Some("ses_abc".into())
        );
        assert_eq!(session_id_from_lane_key("user:42"), None);
        assert_eq!(session_id_from_lane_key("user:42:session:"), None);
        assert_eq!(session_id_from_lane_id(None), None);
        assert_eq!(
            session_id_from_lane_id(Some("user:1:session:s1")),
            Some("s1".into())
        );
        assert_eq!(
            lane_id_from_user_session(7, "ses_x"),
            Some("user:7:session:ses_x".into())
        );
        assert_eq!(lane_id_from_user_session(7, "  "), None);
        assert_eq!(lane_id_from_user_session(7, ""), None);
    }

    #[test]
    fn retention_and_waiting_timeout() {
        let now = Utc::now();
        assert!(!is_terminal_past_retention(now - Duration::hours(23), now));
        assert!(is_terminal_past_retention(now - Duration::hours(25), now));
        assert!(!is_waiting_input_timed_out(now - Duration::hours(2), now));
        assert!(is_waiting_input_timed_out(now - Duration::hours(3), now));
        assert!(waiting_input_timeout_error().contains("timed out"));
    }

    #[test]
    fn status_counts_group_wait_and_paused() {
        let statuses = [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::WaitingForInput,
            TaskStatus::Paused,
            TaskStatus::Completed,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ];
        let (total, pending, running, waiting, completed, failed, cancelled) =
            status_counts_from_iter(statuses.iter());
        assert_eq!(total, 7);
        assert_eq!(pending, 1);
        assert_eq!(running, 1);
        assert_eq!(waiting, 2);
        assert_eq!(completed, 1);
        assert_eq!(failed, 1);
        assert_eq!(cancelled, 1);
    }
}
