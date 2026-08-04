//! 每用户通知偏好与事件目录。
//!
//! `NotificationType` 负责历史记录的展示兼容；这里的 event key 则描述真正的
//! 业务事件（例如 updater 成功和失败），用于精确过滤。

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

pub const SOURCE_KEYS: [&str; 8] = [
    "agent",
    "heartbeat",
    "mcp",
    "brew",
    "tapp",
    "updater",
    "federation",
    "system",
];

#[derive(Debug, Clone, Copy, Serialize)]
pub struct NotificationEventDefinition {
    pub key: &'static str,
    pub source: &'static str,
}

pub const EVENT_DEFINITIONS: [NotificationEventDefinition; 32] = [
    NotificationEventDefinition {
        key: "agent.task_progress",
        source: "agent",
    },
    NotificationEventDefinition {
        key: "agent.task_completed",
        source: "agent",
    },
    NotificationEventDefinition {
        key: "agent.task_failed",
        source: "agent",
    },
    NotificationEventDefinition {
        key: "agent.task_cancelled",
        source: "agent",
    },
    NotificationEventDefinition {
        key: "agent.clarification",
        source: "agent",
    },
    NotificationEventDefinition {
        key: "heartbeat.succeeded",
        source: "heartbeat",
    },
    NotificationEventDefinition {
        key: "heartbeat.failed",
        source: "heartbeat",
    },
    NotificationEventDefinition {
        key: "mcp.connected",
        source: "mcp",
    },
    NotificationEventDefinition {
        key: "mcp.disconnected",
        source: "mcp",
    },
    NotificationEventDefinition {
        key: "brew.new_items",
        source: "brew",
    },
    NotificationEventDefinition {
        key: "brew.source_error",
        source: "brew",
    },
    NotificationEventDefinition {
        key: "tapp.message",
        source: "tapp",
    },
    NotificationEventDefinition {
        key: "tapp.warning",
        source: "tapp",
    },
    NotificationEventDefinition {
        key: "tapp.error",
        source: "tapp",
    },
    NotificationEventDefinition {
        key: "updater.submitted",
        source: "updater",
    },
    NotificationEventDefinition {
        key: "updater.running",
        source: "updater",
    },
    NotificationEventDefinition {
        key: "updater.succeeded",
        source: "updater",
    },
    NotificationEventDefinition {
        key: "updater.failed",
        source: "updater",
    },
    NotificationEventDefinition {
        key: "updater.needs_manual",
        source: "updater",
    },
    NotificationEventDefinition {
        key: "updater.unknown",
        source: "updater",
    },
    NotificationEventDefinition {
        key: "federation.channel_message",
        source: "federation",
    },
    NotificationEventDefinition {
        key: "federation.room_message",
        source: "federation",
    },
    NotificationEventDefinition {
        key: "federation.new_follower",
        source: "federation",
    },
    NotificationEventDefinition {
        key: "federation.follow_accepted",
        source: "federation",
    },
    NotificationEventDefinition {
        key: "federation.channel_invite",
        source: "federation",
    },
    NotificationEventDefinition {
        key: "federation.room_invite",
        source: "federation",
    },
    NotificationEventDefinition {
        key: "federation.channel_accepted",
        source: "federation",
    },
    NotificationEventDefinition {
        key: "federation.room_invite_accepted",
        source: "federation",
    },
    NotificationEventDefinition {
        key: "system.info",
        source: "system",
    },
    // skill.* is Arael skill lifecycle — same source as agent so FE prefs
    // (system on / agent off) cannot swallow skill notifications.
    NotificationEventDefinition {
        key: "skill.pruned",
        source: "agent",
    },
    NotificationEventDefinition {
        key: "skill.improved",
        source: "agent",
    },
    NotificationEventDefinition {
        key: "skill.changed",
        source: "agent",
    },
];

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

    pub fn allows(&self, event_key: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(definition) = EVENT_DEFINITIONS
            .iter()
            .find(|definition| definition.key == event_key)
        else {
            // 新增事件在目录和设置 UI 更新前保持可见，避免重要告警被静默丢弃。
            return true;
        };
        self.sources.get(definition.source).copied().unwrap_or(true)
            && self.events.get(event_key).copied().unwrap_or(true)
    }
}

static PREFERENCES_CACHE: LazyLock<RwLock<HashMap<i32, NotificationPreferences>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub async fn cache_restored(user_id: i32, preferences: NotificationPreferences) {
    PREFERENCES_CACHE
        .write()
        .await
        .insert(user_id, preferences.normalized());
}

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

pub async fn load(db: Option<&DatabaseConnection>, user_id: i32) -> NotificationPreferences {
    if let Some(cached) = PREFERENCES_CACHE.read().await.get(&user_id).cloned() {
        return cached;
    }
    let preferences = if let Some(db) = db {
        let result = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT notification_preferences FROM users WHERE id = $1",
                [user_id.into()],
            ))
            .await;
        match result {
            Ok(Some(row)) => row
                .try_get::<serde_json::Value>("", "notification_preferences")
                .ok()
                .and_then(|value| serde_json::from_value(value).ok())
                .map(NotificationPreferences::normalized)
                .unwrap_or_default(),
            Ok(None) => NotificationPreferences::default(),
            Err(error) => {
                tracing::warn!(
                    user_id,
                    "Failed to load notification preferences: {}",
                    error
                );
                NotificationPreferences::default()
            }
        }
    } else {
        NotificationPreferences::default()
    };
    PREFERENCES_CACHE
        .write()
        .await
        .insert(user_id, preferences.clone());
    preferences
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
    PREFERENCES_CACHE
        .write()
        .await
        .insert(user_id, preferences.clone());
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
        assert!(preferences.events["brew.source_error"]);
        assert!(!preferences.sources.contains_key("removed"));
        assert!(preferences.locations["agent"].toast);
        assert!(!preferences.locations.contains_key("removed"));
    }

    #[test]
    fn source_and_event_switches_are_both_enforced() {
        let mut preferences = NotificationPreferences::default();
        preferences.sources.insert("brew".to_string(), false);
        assert!(!preferences.allows("brew.source_error"));
        preferences.sources.insert("brew".to_string(), true);
        preferences
            .events
            .insert("brew.source_error".to_string(), false);
        assert!(!preferences.allows("brew.source_error"));
        assert!(preferences.allows("future.critical_event"));
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

        let defaults = load(Some(&db), user_id).await;
        assert!(defaults.enabled);
        assert!(defaults.allows("brew.source_error"));

        let mut changed = defaults;
        changed.sources.insert("brew".to_string(), false);
        changed.delivery.browser = false;
        changed.locations.get_mut("brew").unwrap().panel = false;
        save(Some(&db), user_id, changed).await.unwrap();
        clear_cached_for_test(user_id).await;

        let restored = load(Some(&db), user_id).await;
        assert!(!restored.sources["brew"]);
        assert!(!restored.delivery.browser);
        assert!(!restored.locations["brew"].panel);
        assert!(!restored.allows("brew.source_error"));

        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
        .unwrap();
        clear_cached_for_test(user_id).await;
    }
}
