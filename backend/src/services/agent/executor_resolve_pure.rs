use crate::services::agent::types::RiskLevel;

/// 心跳（`SYSTEM_USER_ID`）无人值守时始终不能执行的能力：它们会创建、修改或
/// 触发新的自动执行，一次注入就能借心跳自我扩散。
pub const UNATTENDED_DENIED_CAPABILITIES: &[&str] = &[
    "heartbeat.create",
    "heartbeat.update",
    "heartbeat.delete",
    "heartbeat.toggle",
    "scheduler.create",
    "scheduler.trigger",
    "task.submit",
    "tapp.install",
];

/// 心跳能否不经人工确认执行一个需要确认的能力。
///
/// High / Critical 与 [`UNATTENDED_DENIED_CAPABILITIES`] 一律不行；其余 Low、Medium
/// 照常自动执行（例如 `http.fetch`）。
pub fn unattended_may_auto_run(capability_id: &str, risk: RiskLevel) -> bool {
    !matches!(risk, RiskLevel::High | RiskLevel::Critical)
        && !UNATTENDED_DENIED_CAPABILITIES.contains(&capability_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_auto_runs_low_and_medium_but_never_self_spreading_work() {
        assert!(!unattended_may_auto_run("cache.clear", RiskLevel::High));
        assert!(!unattended_may_auto_run(
            "system.shutdown",
            RiskLevel::Critical
        ));
        assert!(unattended_may_auto_run("http.fetch", RiskLevel::Medium));
        assert!(unattended_may_auto_run("storage.set", RiskLevel::Low));
        for id in UNATTENDED_DENIED_CAPABILITIES {
            assert!(!unattended_may_auto_run(id, RiskLevel::Medium), "{id}");
            assert!(!unattended_may_auto_run(id, RiskLevel::Low), "{id}");
        }
    }
}
