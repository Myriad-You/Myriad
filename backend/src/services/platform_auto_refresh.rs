//! Myriad core platform auto-refresh tasks.
//!
//! The existing Tapp scheduler owns timing, retries and execution history. Core
//! platform refreshes use a reserved namespace so they can reuse that engine
//! without pretending to be an installed Tapp or depending on a browser.

use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use serde_json::json;
use std::collections::{HashMap, HashSet};

use crate::models::entities::tapp_scheduled_tasks::{
    self, ExecutionTarget, MissedPolicy, ScheduleType, TaskScope,
};

pub const CORE_PLATFORM_SYNC_TAPP_ID: &str = "myriad.core.platform-sync";
const CORE_PLATFORM_SYNC_TASK_PREFIX: &str = "platform-sync:";
const MIN_INTERVAL_HOURS: i32 = 1;
const MAX_INTERVAL_HOURS: i32 = 168;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformAutoRefreshSummary {
    pub enabled_tasks: usize,
    pub disabled_tasks: usize,
    pub interval_hours: i32,
}

pub fn clamp_interval_hours(interval_hours: i32) -> i32 {
    interval_hours.clamp(MIN_INTERVAL_HOURS, MAX_INTERVAL_HOURS)
}

pub fn normalize_platform_name(name: &str) -> Option<&'static str> {
    match name.trim().to_ascii_lowercase().as_str() {
        "github" => Some("github"),
        "bilibili" => Some("bilibili"),
        "steam" => Some("steam"),
        "youtube" | "yt" => Some("youtube"),
        "netease" | "netease music" | "netease_music" => Some("netease"),
        "bangumi" => Some("bangumi"),
        "x" | "twitter" => Some("x"),
        "discord" => Some("discord"),
        "mal" | "myanimelist" => Some("mal"),
        "xbox" => Some("xbox"),
        "psn" | "playstation" | "playstation network" => Some("psn"),
        _ => None,
    }
}

pub fn core_platform_from_task(task: &tapp_scheduled_tasks::Model) -> Option<&str> {
    if task.tapp_id != CORE_PLATFORM_SYNC_TAPP_ID {
        return None;
    }
    task.task_id.strip_prefix(CORE_PLATFORM_SYNC_TASK_PREFIX)
}

pub fn is_core_platform_sync_task(task: &tapp_scheduled_tasks::Model) -> bool {
    core_platform_from_task(task).is_some()
}

fn task_id(platform: &str) -> String {
    format!("{CORE_PLATFORM_SYNC_TASK_PREFIX}{platform}")
}

fn backend_actions(platform: &str) -> serde_json::Value {
    json!([{
        "action": "platform.sync",
        "platform": platform,
    }])
}

/// Reconcile persisted core tasks with the administrator's platform settings.
///
/// `platform_names` should be **configured** platforms (credentials ready).
/// Report-page `enabled` is unrelated — callers must not filter on it.
///
/// Existing rows are disabled rather than deleted so execution history remains
/// inspectable. Newly enabled or rescheduled tasks run after one full interval;
/// a small per-platform stagger avoids a burst of external API calls.
pub async fn reconcile_platform_auto_refresh(
    db: &DatabaseConnection,
    user_id: i32,
    enabled: bool,
    interval_hours: i32,
    platform_names: &[String],
) -> Result<PlatformAutoRefreshSummary, String> {
    let interval_hours = clamp_interval_hours(interval_hours);
    let interval_ms = i64::from(interval_hours) * 60 * 60 * 1000;
    let schedule_config = json!({ "interval": interval_ms });
    let now = Utc::now();

    let desired_platforms: Vec<&'static str> = if enabled {
        let mut seen = HashSet::new();
        platform_names
            .iter()
            .filter_map(|name| normalize_platform_name(name))
            .filter(|platform| seen.insert(*platform))
            .collect()
    } else {
        Vec::new()
    };
    let desired: HashSet<&str> = desired_platforms.iter().copied().collect();

    let existing_tasks = tapp_scheduled_tasks::Entity::find()
        .filter(tapp_scheduled_tasks::Column::TappId.eq(CORE_PLATFORM_SYNC_TAPP_ID))
        .all(db)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to load core platform tasks");
            "Failed to load core platform tasks".to_string()
        })?;

    let mut current_user_tasks = HashMap::new();
    for task in existing_tasks {
        let platform = core_platform_from_task(&task).map(str::to_string);
        if task.user_id == user_id {
            if let Some(platform) = platform {
                current_user_tasks.insert(platform, task);
                continue;
            }
        }

        if task.enabled || task.next_run_at.is_some() {
            let mut active: tapp_scheduled_tasks::ActiveModel = task.into();
            active.enabled = Set(false);
            active.next_run_at = Set(None);
            active.updated_at = Set(now.into());
            active.update(db).await.map_err(|error| {
                tracing::error!(%error, "Failed to disable stale core task");
                "Failed to disable stale core task".to_string()
            })?;
        }
    }

    let mut enabled_tasks = 0usize;
    for (index, platform) in desired_platforms.iter().enumerate() {
        let next_run_at = now
            + Duration::hours(i64::from(interval_hours))
            + Duration::minutes(i64::try_from(index).unwrap_or(i64::MAX).min(30));
        if let Some(task) = current_user_tasks.remove(*platform) {
            let schedule_changed = task.schedule_config != schedule_config;
            let must_reschedule = !task.enabled || schedule_changed || task.next_run_at.is_none();
            let mut active: tapp_scheduled_tasks::ActiveModel = task.into();
            active.name = Set(format!("Auto-refresh {platform} data"));
            active.schedule_type = Set(ScheduleType::Interval);
            active.schedule_config = Set(schedule_config.clone());
            active.payload = Set(None);
            active.execution_target = Set(ExecutionTarget::Backend);
            active.backend_actions = Set(Some(backend_actions(platform)));
            active.enabled = Set(true);
            active.missed_policy = Set(MissedPolicy::Skip);
            active.scope = Set(TaskScope::User);
            active.retry_config = Set(Some(json!({
                "max_retries": 2,
                "retry_delay": 60_000,
            })));
            if must_reschedule {
                active.next_run_at = Set(Some(next_run_at.into()));
            }
            active.updated_at = Set(now.into());
            active.update(db).await.map_err(|error| {
                tracing::error!(%error, platform, "Failed to update core platform task");
                format!("Failed to update {platform} core task")
            })?;
        } else {
            let task = tapp_scheduled_tasks::ActiveModel {
                task_id: Set(task_id(platform)),
                tapp_id: Set(CORE_PLATFORM_SYNC_TAPP_ID.to_string()),
                user_id: Set(user_id),
                name: Set(format!("Auto-refresh {platform} data")),
                schedule_type: Set(ScheduleType::Interval),
                schedule_config: Set(schedule_config.clone()),
                payload: Set(None),
                execution_target: Set(ExecutionTarget::Backend),
                backend_actions: Set(Some(backend_actions(platform))),
                enabled: Set(true),
                missed_policy: Set(MissedPolicy::Skip),
                scope: Set(TaskScope::User),
                retry_config: Set(Some(json!({
                    "max_retries": 2,
                    "retry_delay": 60_000,
                }))),
                next_run_at: Set(Some(next_run_at.into())),
                stats: Set(json!({
                    "totalRuns": 0,
                    "successRuns": 0,
                    "failedRuns": 0,
                    "missedRuns": 0,
                })),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                ..Default::default()
            };
            task.insert(db).await.map_err(|error| {
                tracing::error!(%error, platform, "Failed to create core platform task");
                format!("Failed to create {platform} core task")
            })?;
        }
        enabled_tasks += 1;
    }

    let mut disabled_tasks = 0usize;
    for (platform, task) in current_user_tasks {
        if desired.contains(platform.as_str()) {
            continue;
        }
        let mut active: tapp_scheduled_tasks::ActiveModel = task.into();
        active.enabled = Set(false);
        active.next_run_at = Set(None);
        active.updated_at = Set(now.into());
        active.update(db).await.map_err(|error| {
            tracing::error!(%error, platform, "Failed to disable core platform task");
            format!("Failed to disable {platform} core task")
        })?;
        disabled_tasks += 1;
    }

    tracing::info!(
        enabled,
        interval_hours,
        enabled_tasks,
        disabled_tasks,
        "[CoreScheduler] Platform auto-refresh tasks reconciled"
    );

    Ok(PlatformAutoRefreshSummary {
        enabled_tasks,
        disabled_tasks,
        interval_hours,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_config_and_api_platform_names() {
        assert_eq!(normalize_platform_name("Netease Music"), Some("netease"));
        assert_eq!(normalize_platform_name("MyAnimeList"), Some("mal"));
        assert_eq!(normalize_platform_name("PlayStation"), Some("psn"));
        assert_eq!(normalize_platform_name("YouTube"), Some("youtube"));
        assert_eq!(normalize_platform_name("yt"), Some("youtube"));
        assert_eq!(normalize_platform_name("unknown"), None);
    }

    #[test]
    fn clamps_unsafe_refresh_intervals() {
        assert_eq!(clamp_interval_hours(0), 1);
        assert_eq!(clamp_interval_hours(24), 24);
        assert_eq!(clamp_interval_hours(999), 168);
    }
}
