//! Pure task-store projection and retention rules.
//!
//! Persistence / locks / DB stay in `executor/task_store.rs`. Domain owns:
//! - cancellable status set
//! - TaskStatus ↔ DB string maps
//! - lane key / session id projection
//! - terminal retention and waiting-input timeout decisions
//! - in-memory status count aggregation

use crate::services::agent::types::TaskStatus;
use chrono::{DateTime, Utc};

/// Completed/failed/cancelled tasks older than this (hours) are removed.
pub const TERMINAL_RETENTION_HOURS: i64 = 24;
/// WaitingForInput tasks older than this (hours) become failed terminal.
pub const WAITING_INPUT_TIMEOUT_HOURS: i64 = 2;

/// User-visible error when a wait-for-input task times out.
pub const WAITING_INPUT_TIMEOUT_ERROR: &str = "任务等待用户输入超时（2小时），已自动取消";

/// Statuses that interrupt/cancel should target.
pub fn is_cancellable_task_status(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Pending
            | TaskStatus::Running
            | TaskStatus::WaitingForInput
            | TaskStatus::Paused
    )
}

/// Map persisted status string → [`TaskStatus`].
///
/// Unknown strings fall back to [`TaskStatus::Pending`] (legacy rows).
pub fn task_status_from_db_str(status: &str) -> TaskStatus {
    match status {
        "pending" => TaskStatus::Pending,
        "running" => TaskStatus::Running,
        "waiting_for_input" => TaskStatus::WaitingForInput,
        "paused" => TaskStatus::Paused,
        "completed" => TaskStatus::Completed,
        "failed" => TaskStatus::Failed,
        "cancelled" => TaskStatus::Cancelled,
        _ => TaskStatus::Pending,
    }
}

/// Map [`TaskStatus`] → persisted DB string.
pub fn task_status_to_db_str(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::WaitingForInput => "waiting_for_input",
        TaskStatus::Paused => "paused",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

/// Session id embedded in `user:{id}:session:{session_id}` lane keys.
///
/// Accepts `Option` for stored `lane_id` columns and bare lane key strings
/// via [`session_id_from_lane_key`].
pub fn session_id_from_lane_id(lane_id: Option<&str>) -> Option<String> {
    lane_id.and_then(session_id_from_lane_key)
}

/// Extract session id from a lane key of the form `user:{id}:session:{session_id}`.
pub fn session_id_from_lane_key(lane_key: &str) -> Option<String> {
    lane_key
        .split_once(":session:")
        .map(|(_, session_id)| session_id.to_string())
        .filter(|s| !s.is_empty())
}

/// Reconstruct lane id from user + session (legacy rows without `lane_id`).
pub fn lane_id_from_user_session(user_id: i32, session_id: &str) -> Option<String> {
    let sid = session_id.trim();
    if sid.is_empty() {
        return None;
    }
    Some(format!("user:{user_id}:session:{sid}"))
}

/// Whether a terminal task's `completed_at` is past retention and should be purged.
pub fn is_terminal_past_retention(
    completed_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    (now - completed_at).num_hours() > TERMINAL_RETENTION_HOURS
}

/// Whether a WaitingForInput task has exceeded the response window.
pub fn is_waiting_input_timed_out(started_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    (now - started_at).num_hours() > WAITING_INPUT_TIMEOUT_HOURS
}

/// In-memory status tallies: (total, pending, running, waiting, completed, failed, cancelled).
///
/// `WaitingForInput` and `Paused` both count as **waiting**.
pub fn status_counts_from_iter<'a, I>(statuses: I) -> (usize, usize, usize, usize, usize, usize, usize)
where
    I: IntoIterator<Item = &'a TaskStatus>,
{
    let mut total = 0usize;
    let mut pending = 0usize;
    let mut running = 0usize;
    let mut waiting = 0usize;
    let mut completed = 0usize;
    let mut failed = 0usize;
    let mut cancelled = 0usize;
    for status in statuses {
        total += 1;
        match status {
            TaskStatus::Pending => pending += 1,
            TaskStatus::Running => running += 1,
            TaskStatus::WaitingForInput | TaskStatus::Paused => waiting += 1,
            TaskStatus::Completed => completed += 1,
            TaskStatus::Failed => failed += 1,
            TaskStatus::Cancelled => cancelled += 1,
        }
    }
    (
        total, pending, running, waiting, completed, failed, cancelled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

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
            assert_eq!(task_status_to_db_str(&task_status_from_db_str(s)), s);
        }
        assert_eq!(task_status_from_db_str("unknown_legacy"), TaskStatus::Pending);
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
        assert!(!is_terminal_past_retention(
            now - Duration::hours(23),
            now
        ));
        assert!(is_terminal_past_retention(
            now - Duration::hours(25),
            now
        ));
        assert!(!is_waiting_input_timed_out(
            now - Duration::hours(2),
            now
        ));
        assert!(is_waiting_input_timed_out(
            now - Duration::hours(3),
            now
        ));
        assert!(WAITING_INPUT_TIMEOUT_ERROR.contains("超时"));
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
