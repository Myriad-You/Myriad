//! 每用户通知偏好与事件目录。
//!
//! `NotificationType` 负责历史记录的展示兼容；这里的 event key 则描述真正的
//! 业务事件（例如 updater 成功和失败），用于精确过滤。

use std::collections::BTreeMap;
#[cfg(test)]
use std::{collections::HashMap, sync::LazyLock};

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use tokio::sync::RwLock;

/// 通知元数据中承载事件键的字段名。
pub const EVENT_KEY_FIELD: &str = "event_key";

/// 通知元数据 `action` 字段的取值：点击后打开 Agent 会话 / Agent 管理页。
pub const ACTION_OPEN_AGENT: &str = "open_agent";
pub const ACTION_OPEN_AGENT_MANAGE: &str = "open_agent_manage";

pub const SOURCE_KEYS: [&str; 8] = [
    "agent",
    "heartbeat",
    "mcp",
    "phantasi",
    "tapp",
    "updater",
    "federation",
    "system",
];

/// 通知事件目录的唯一来源。生产者只能经由这里构造事件键；偏好目录、
/// Merope 闸门与 `shared/notification_events.json` 都从它派生。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationEventKey {
    AgentTaskProgress,
    AgentTaskCompleted,
    AgentTaskFailed,
    AgentTaskCancelled,
    AgentClarification,
    MeropePlatformActivity,
    MeropeReportReady,
    /// She wants to talk: what she would say, while they are not with her.
    MeropeReachOut,
    HeartbeatSucceeded,
    HeartbeatFailed,
    HeartbeatSeoReview,
    McpConnected,
    McpDisconnected,
    PhantasiNewItems,
    PhantasiSourceError,
    PlatformSyncFailed,
    TappMessage,
    TappWarning,
    TappError,
    UpdaterSubmitted,
    UpdaterRunning,
    UpdaterSucceeded,
    UpdaterFailed,
    UpdaterNeedsManual,
    UpdaterUnknown,
    FederationChannelMessage,
    FederationRoomMessage,
    FederationNewFollower,
    FederationFollowAccepted,
    FederationChannelInvite,
    FederationRoomInvite,
    FederationChannelAccepted,
    FederationRoomInviteAccepted,
    FederationDeliveryFailed,
    FederationDomainRevoked,
    SystemInfo,
    SkillPruned,
    SkillImproved,
    SkillChanged,
}

impl NotificationEventKey {
    pub const ALL: [Self; 39] = [
        Self::AgentTaskProgress,
        Self::AgentTaskCompleted,
        Self::AgentTaskFailed,
        Self::AgentTaskCancelled,
        Self::AgentClarification,
        Self::MeropePlatformActivity,
        Self::MeropeReportReady,
        Self::MeropeReachOut,
        Self::HeartbeatSucceeded,
        Self::HeartbeatFailed,
        Self::HeartbeatSeoReview,
        Self::McpConnected,
        Self::McpDisconnected,
        Self::PhantasiNewItems,
        Self::PhantasiSourceError,
        Self::PlatformSyncFailed,
        Self::TappMessage,
        Self::TappWarning,
        Self::TappError,
        Self::UpdaterSubmitted,
        Self::UpdaterRunning,
        Self::UpdaterSucceeded,
        Self::UpdaterFailed,
        Self::UpdaterNeedsManual,
        Self::UpdaterUnknown,
        Self::FederationChannelMessage,
        Self::FederationRoomMessage,
        Self::FederationNewFollower,
        Self::FederationFollowAccepted,
        Self::FederationChannelInvite,
        Self::FederationRoomInvite,
        Self::FederationChannelAccepted,
        Self::FederationRoomInviteAccepted,
        Self::FederationDeliveryFailed,
        Self::FederationDomainRevoked,
        Self::SystemInfo,
        Self::SkillPruned,
        Self::SkillImproved,
        Self::SkillChanged,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::AgentTaskProgress => "agent.task_progress",
            Self::AgentTaskCompleted => "agent.task_completed",
            Self::AgentTaskFailed => "agent.task_failed",
            Self::AgentTaskCancelled => "agent.task_cancelled",
            Self::AgentClarification => "agent.clarification",
            Self::MeropePlatformActivity => "agent.merope.platform_activity",
            Self::MeropeReportReady => "agent.merope.report_ready",
            Self::MeropeReachOut => "agent.merope.reach_out",
            Self::HeartbeatSucceeded => "heartbeat.succeeded",
            Self::HeartbeatFailed => "heartbeat.failed",
            Self::HeartbeatSeoReview => "heartbeat.seo_review",
            Self::McpConnected => "mcp.connected",
            Self::McpDisconnected => "mcp.disconnected",
            Self::PhantasiNewItems => "phantasi.new_items",
            Self::PhantasiSourceError => "phantasi.source_error",
            Self::PlatformSyncFailed => "platform.sync.failed",
            Self::TappMessage => "tapp.message",
            Self::TappWarning => "tapp.warning",
            Self::TappError => "tapp.error",
            Self::UpdaterSubmitted => "updater.submitted",
            Self::UpdaterRunning => "updater.running",
            Self::UpdaterSucceeded => "updater.succeeded",
            Self::UpdaterFailed => "updater.failed",
            Self::UpdaterNeedsManual => "updater.needs_manual",
            Self::UpdaterUnknown => "updater.unknown",
            Self::FederationChannelMessage => "federation.channel_message",
            Self::FederationRoomMessage => "federation.room_message",
            Self::FederationNewFollower => "federation.new_follower",
            Self::FederationFollowAccepted => "federation.follow_accepted",
            Self::FederationChannelInvite => "federation.channel_invite",
            Self::FederationRoomInvite => "federation.room_invite",
            Self::FederationChannelAccepted => "federation.channel_accepted",
            Self::FederationRoomInviteAccepted => "federation.room_invite_accepted",
            Self::FederationDeliveryFailed => "federation.delivery_failed",
            Self::FederationDomainRevoked => "federation.domain_revoked",
            Self::SystemInfo => "system.info",
            Self::SkillPruned => "skill.pruned",
            Self::SkillImproved => "skill.improved",
            Self::SkillChanged => "skill.changed",
        }
    }

    /// 用户偏好里的来源开关。`skill.*` 归 agent：技能生命周期属于 Agent，
    /// 避免「system 开 / agent 关」时吞掉技能通知；平台同步失败归 system。
    pub const fn source(self) -> &'static str {
        match self {
            Self::AgentTaskProgress
            | Self::AgentTaskCompleted
            | Self::AgentTaskFailed
            | Self::AgentTaskCancelled
            | Self::AgentClarification
            | Self::MeropePlatformActivity
            | Self::MeropeReportReady
            | Self::MeropeReachOut
            | Self::SkillPruned
            | Self::SkillImproved
            | Self::SkillChanged => "agent",
            Self::HeartbeatSucceeded | Self::HeartbeatFailed | Self::HeartbeatSeoReview => {
                "heartbeat"
            }
            Self::McpConnected | Self::McpDisconnected => "mcp",
            Self::PhantasiNewItems | Self::PhantasiSourceError => "phantasi",
            Self::TappMessage | Self::TappWarning | Self::TappError => "tapp",
            Self::UpdaterSubmitted
            | Self::UpdaterRunning
            | Self::UpdaterSucceeded
            | Self::UpdaterFailed
            | Self::UpdaterNeedsManual
            | Self::UpdaterUnknown => "updater",
            Self::FederationChannelMessage
            | Self::FederationRoomMessage
            | Self::FederationNewFollower
            | Self::FederationFollowAccepted
            | Self::FederationChannelInvite
            | Self::FederationRoomInvite
            | Self::FederationChannelAccepted
            | Self::FederationRoomInviteAccepted
            | Self::FederationDeliveryFailed
            | Self::FederationDomainRevoked => "federation",
            Self::PlatformSyncFailed | Self::SystemInfo => "system",
        }
    }

    /// Agent 任务的终局，或需要用户回应的状态。
    pub const fn is_task_outcome(self) -> bool {
        matches!(
            self,
            Self::AgentTaskCompleted
                | Self::AgentTaskFailed
                | Self::AgentTaskCancelled
                | Self::AgentClarification
        )
    }

    /// 值得 Merope 在用户离开时主动打扰的事件。
    pub const fn interrupts_when_away(self) -> bool {
        self.is_task_outcome()
            || matches!(
                self,
                Self::PhantasiSourceError
                    | Self::PlatformSyncFailed
                    | Self::MeropePlatformActivity
                    | Self::MeropeReportReady
                    | Self::MeropeReachOut
            )
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|event| event.key() == key)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct NotificationEventDefinition {
    pub key: &'static str,
    pub source: &'static str,
}

pub const EVENT_DEFINITIONS: [NotificationEventDefinition; NotificationEventKey::ALL.len()] = {
    let mut definitions = [NotificationEventDefinition {
        key: "",
        source: "",
    }; NotificationEventKey::ALL.len()];
    let mut index = 0;
    while index < definitions.len() {
        let event = NotificationEventKey::ALL[index];
        definitions[index] = NotificationEventDefinition {
            key: event.key(),
            source: event.source(),
        };
        index += 1;
    }
    definitions
};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationDeliveryPreferences {
    #[serde(default = "default_true")]
    pub island: bool,
    #[serde(default = "default_true", alias = "high_priority_toast")]
    pub toast: bool,
    #[serde(default = "default_true")]
    pub browser: bool,
}

impl Default for NotificationDeliveryPreferences {
    fn default() -> Self {
        Self {
            island: true,
            toast: true,
            browser: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationLocationPreferences {
    #[serde(default = "default_true")]
    pub panel: bool,
    #[serde(default = "default_true")]
    pub toast: bool,
    #[serde(default = "default_true")]
    pub island: bool,
    #[serde(default = "default_true")]
    pub browser: bool,
}

impl Default for NotificationLocationPreferences {
    fn default() -> Self {
        Self {
            panel: true,
            toast: true,
            island: true,
            browser: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPreferences {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub sources: BTreeMap<String, bool>,
    #[serde(default)]
    pub events: BTreeMap<String, bool>,
    #[serde(default)]
    pub delivery: NotificationDeliveryPreferences,
    #[serde(default)]
    pub locations: BTreeMap<String, NotificationLocationPreferences>,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            enabled: true,
            sources: SOURCE_KEYS
                .into_iter()
                .map(|key| (key.to_string(), true))
                .collect(),
            events: EVENT_DEFINITIONS
                .into_iter()
                .map(|event| (event.key.to_string(), true))
                .collect(),
            delivery: NotificationDeliveryPreferences::default(),
            locations: SOURCE_KEYS
                .into_iter()
                .map(|key| (key.to_string(), NotificationLocationPreferences::default()))
                .collect(),
        }
    }
}

impl NotificationPreferences {
    pub fn normalized(self) -> Self {
        let defaults = Self::default();
        Self {
            enabled: self.enabled,
            sources: SOURCE_KEYS
                .into_iter()
                .map(|key| {
                    (
                        key.to_string(),
                        self.sources
                            .get(key)
                            .copied()
                            .unwrap_or(defaults.sources[key]),
                    )
                })
                .collect(),
            events: EVENT_DEFINITIONS
                .into_iter()
                .map(|event| {
                    (
                        event.key.to_string(),
                        self.events
                            .get(event.key)
                            .copied()
                            .unwrap_or(defaults.events[event.key]),
                    )
                })
                .collect(),
            delivery: self.delivery,
            locations: SOURCE_KEYS
                .into_iter()
                .map(|key| {
                    (
                        key.to_string(),
                        self.locations
                            .get(key)
                            .cloned()
                            .unwrap_or_else(NotificationLocationPreferences::default),
                    )
                })
                .collect(),
        }
    }

    /// `fallback_source` 是按通知类型推出的来源。事件键未登记（或缺失）时，
    /// 只能按它判定：未登记的键没有独立开关可查，但绝不能绕过来源开关。
    pub fn allows(&self, event_key: Option<&str>, fallback_source: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(definition) = event_key.and_then(|event_key| {
            EVENT_DEFINITIONS
                .iter()
                .find(|definition| definition.key == event_key)
        }) else {
            return self.source_allows(fallback_source);
        };
        self.source_allows(definition.source)
            && self.events.get(definition.key).copied().unwrap_or(true)
    }

    fn source_allows(&self, source: &str) -> bool {
        self.sources.get(source).copied().unwrap_or(true)
    }
}

// Production always reads durable preferences. Only database-free unit tests
// inject local values; there is no per-user preference cache in a live process.
#[cfg(test)]
static PREFERENCES_CACHE: LazyLock<RwLock<HashMap<i32, NotificationPreferences>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

#[cfg(test)]
pub async fn set_cached_for_test(user_id: i32, preferences: NotificationPreferences) {
    PREFERENCES_CACHE
        .write()
        .await
        .insert(user_id, preferences.normalized());
}

#[cfg(test)]
async fn clear_cached_for_test(user_id: i32) {
    PREFERENCES_CACHE.write().await.remove(&user_id);
}

pub(crate) fn preferences_from_stored_value(
    value: serde_json::Value,
) -> Result<NotificationPreferences, String> {
    serde_json::from_value(value)
        .map(NotificationPreferences::normalized)
        .map_err(|error| format!("invalid notification preferences: {error}"))
}

pub async fn load(
    db: Option<&DatabaseConnection>,
    user_id: i32,
) -> Result<NotificationPreferences, String> {
    // Persistent producers can run in other processes. A process-local cache
    // must not retain an old opt-in forever after the user changes preferences.
    #[cfg(test)]
    if db.is_none() {
        if let Some(cached) = PREFERENCES_CACHE.read().await.get(&user_id).cloned() {
            return Ok(cached);
        }
        return Ok(NotificationPreferences::default());
    }
    let Some(db) = db else {
        return Err("Database is not connected".to_string());
    };
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT notification_preferences FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await;
    match result {
        Ok(Some(row)) => {
            let value = row
                .try_get::<serde_json::Value>("", "notification_preferences")
                .map_err(|error| error.to_string())?;
            preferences_from_stored_value(value)
        }
        Ok(None) => Ok(NotificationPreferences::default()),
        Err(error) => Err(error.to_string()),
    }
}

pub async fn save(
    db: Option<&DatabaseConnection>,
    user_id: i32,
    preferences: NotificationPreferences,
) -> Result<NotificationPreferences, String> {
    let preferences = preferences.normalized();
    let Some(db) = db else {
        return Err("Database is not connected".to_string());
    };
    let value = serde_json::to_value(&preferences).map_err(|error| error.to_string())?;
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET notification_preferences = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
            [value.into(), user_id.into()],
        ))
        .await
        .map_err(|error| error.to_string())?;
    if result.rows_affected() == 0 {
        return Err("User not found".to_string());
    }
    Ok(preferences)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Database;
    use sea_orm_migration::MigratorTrait;

    #[test]
    fn normalization_fills_new_catalog_entries_and_drops_unknown_ones() {
        let preferences = NotificationPreferences {
            sources: BTreeMap::from([("agent".to_string(), false), ("removed".to_string(), false)]),
            events: BTreeMap::from([("agent.task_failed".to_string(), false)]),
            ..NotificationPreferences::default()
        }
        .normalized();

        assert!(!preferences.sources["agent"]);
        assert!(!preferences.events["agent.task_failed"]);
        assert!(preferences.events["phantasi.source_error"]);
        assert!(preferences.events["platform.sync.failed"]);
        assert!(!preferences.sources.contains_key("removed"));
        assert!(
            EVENT_DEFINITIONS
                .iter()
                .any(|definition| definition.key == "platform.sync.failed")
        );
        assert!(preferences.allows(Some("platform.sync.failed"), "system"));
        let mut off = preferences.clone();
        off.events.insert("platform.sync.failed".to_string(), false);
        assert!(!off.allows(Some("platform.sync.failed"), "system"));
        assert!(preferences.locations["agent"].toast);
        assert!(!preferences.locations.contains_key("removed"));
    }

    #[test]
    fn source_and_event_switches_are_both_enforced() {
        let mut preferences = NotificationPreferences::default();
        preferences.sources.insert("phantasi".to_string(), false);
        assert!(!preferences.allows(Some("phantasi.source_error"), "phantasi"));
        preferences.sources.insert("phantasi".to_string(), true);
        preferences
            .events
            .insert("phantasi.source_error".to_string(), false);
        assert!(!preferences.allows(Some("phantasi.source_error"), "phantasi"));
        assert!(preferences.allows(Some("future.critical_event"), "system"));
    }

    #[test]
    fn unregistered_event_key_still_honours_its_source_switch() {
        let mut preferences = NotificationPreferences::default();
        preferences.sources.insert("federation".to_string(), false);
        assert!(!preferences.allows(Some("federation.not_in_catalog"), "federation"));
        assert!(!preferences.allows(None, "federation"));
        assert!(preferences.allows(Some("federation.not_in_catalog"), "system"));

        preferences.sources.insert("federation".to_string(), true);
        preferences.enabled = false;
        assert!(!preferences.allows(Some("federation.not_in_catalog"), "federation"));
    }

    #[test]
    fn federation_failures_follow_the_federation_switch_not_their_type() {
        // Both are sent as `SystemInfo`; the catalog, not the type, owns their source.
        let mut preferences = NotificationPreferences::default();
        preferences.sources.insert("federation".to_string(), false);
        for event in [
            NotificationEventKey::FederationDeliveryFailed,
            NotificationEventKey::FederationDomainRevoked,
        ] {
            assert_eq!(event.source(), "federation");
            assert!(!preferences.allows(Some(event.key()), "system"));
        }
    }

    #[test]
    fn catalog_is_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for event in NotificationEventKey::ALL {
            assert!(seen.insert(event.key()), "duplicate key {}", event.key());
            assert!(SOURCE_KEYS.contains(&event.source()), "{}", event.key());
            assert_eq!(NotificationEventKey::from_key(event.key()), Some(event));
        }
        assert_eq!(EVENT_DEFINITIONS.len(), NotificationEventKey::ALL.len());
        assert!(NotificationEventKey::from_key("federation.not_in_catalog").is_none());
    }

    /// `shared/notification_events.json` is what the frontend table is checked
    /// against; it must be exactly this catalog.
    #[test]
    fn catalog_matches_shared_contract() {
        let spec: serde_json::Value =
            serde_json::from_str(include_str!("../../../../shared/notification_events.json"))
                .expect("shared/notification_events.json");
        let expected = serde_json::json!({
            "schemaVersion": 1,
            "sources": SOURCE_KEYS,
            "events": EVENT_DEFINITIONS.as_slice(),
            "actions": [ACTION_OPEN_AGENT, ACTION_OPEN_AGENT_MANAGE],
        });
        assert_eq!(
            spec, expected,
            "shared/notification_events.json drifted from NotificationEventKey"
        );
    }

    /// Every event key produced in code must come from `NotificationEventKey`
    /// (`Notification::with_event`). A hand-written `"event_key"` literal would
    /// let a producer emit a key the catalog and the preference UI never saw.
    #[test]
    fn production_code_emits_event_keys_only_through_the_catalog() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        walk(&root, &mut files);
        assert!(files.len() > 100, "scan found too few sources");
        // The field-name constant itself, and the DB column of the same name.
        let allowed = [
            "services/agent/notification_preferences.rs",
            "db/schema_check/tables_agent.rs",
        ];
        let literal = concat!("\"event", "_key\"");
        let mut offenders = Vec::new();
        for path in files {
            let relative = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if relative.ends_with("tests.rs")
                || relative.ends_with("_test.rs")
                || allowed.contains(&relative.as_str())
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let production = source
                .find("#[cfg(test)]\nmod tests")
                .map_or(source.as_str(), |end| &source[..end]);
            for (line_no, line) in production.lines().enumerate() {
                if line.contains(literal) && !line.trim_start().starts_with("//") {
                    offenders.push(format!("{relative}:{}: {}", line_no + 1, line.trim()));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "use Notification::with_event(NotificationEventKey::..) instead:\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn legacy_high_priority_toast_setting_migrates_to_unified_toast_switch() {
        let preferences: NotificationPreferences = serde_json::from_value(serde_json::json!({
            "delivery": {
                "island": true,
                "high_priority_toast": false,
                "browser": true
            }
        }))
        .unwrap();
        let normalized = preferences.normalized();

        assert!(!normalized.delivery.toast);
        assert!(normalized.locations.values().all(|location| location.panel));
        assert!(normalized.locations.values().all(|location| location.toast));
    }

    #[tokio::test]
    async fn postgres_jsonb_round_trip_when_test_database_is_provided() {
        let Ok(database_url) = std::env::var("NOTIFICATION_TEST_DATABASE_URL") else {
            return;
        };
        let db = Database::connect(&database_url).await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();
        let username = format!("notification-regression-{}", uuid::Uuid::new_v4().simple());
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username) VALUES ($1) RETURNING id",
                [username.into()],
            ))
            .await
            .unwrap()
            .unwrap();
        let user_id = row.try_get::<i32>("", "id").unwrap();

        let defaults = load(Some(&db), user_id).await.unwrap();
        assert!(defaults.enabled);
        assert!(defaults.allows(Some("phantasi.source_error"), "phantasi"));

        let mut changed = defaults;
        changed.sources.insert("phantasi".to_string(), false);
        changed.delivery.browser = false;
        changed.locations.get_mut("phantasi").unwrap().panel = false;
        save(Some(&db), user_id, changed).await.unwrap();
        clear_cached_for_test(user_id).await;

        let restored = load(Some(&db), user_id).await.unwrap();
        assert!(!restored.sources["phantasi"]);
        assert!(!restored.delivery.browser);
        assert!(!restored.locations["phantasi"].panel);
        assert!(!restored.allows(Some("phantasi.source_error"), "phantasi"));

        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
        .unwrap();
        clear_cached_for_test(user_id).await;
    }

    #[test]
    fn stored_json_parse_error_is_not_all_on_default() {
        let error = preferences_from_stored_value(serde_json::json!([1, 2, 3])).unwrap_err();
        assert!(
            error.contains("invalid notification preferences"),
            "{error}"
        );
        let off = preferences_from_stored_value(serde_json::json!({
            "enabled": false,
            "sources": { "agent": false }
        }))
        .unwrap();
        assert!(!off.enabled);
        assert!(!off.sources["agent"]);
    }
}
