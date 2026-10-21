//! Module visibility preferences shared by config HTTP handlers and the Agent service.
//!
//! Workspace crate so `services::agent` does not depend on the HTTP `api::config`
//! god-object. Types + pure normalization live here; load is SeaORM but free of Axum.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Configuration key in the `configurations` table.
pub const MODULE_VISIBILITY_PREFERENCES_KEY: &str = "module_visibility_preferences";

pub const MODULE_VISIBILITY_KEYS: [&str; 4] = ["library", "phantasi", "tapp", "agent"];
pub const MODULE_VISIBILITY_LEVELS: [&str; 3] = ["all", "authenticated", "admin"];

/// Legacy agent usage levels (compat storage; auth uses Tapp permissions).
const AGENT_GUEST_USAGE_LEVELS: [&str; 2] = ["none", "visible"];
const AGENT_USER_USAGE_LEVELS: [&str; 4] = ["none", "chat", "standard", "elevated"];

/// 旧版 Agent 使用档位（已弃用：运行时以 Tapp `user_perm_*` / 预设模板为准）
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsagePreferences {
    #[serde(default = "default_agent_guest_usage")]
    pub guest: String,
    #[serde(default = "default_agent_user_usage")]
    pub user: String,
}

fn default_agent_guest_usage() -> String {
    "none".to_string()
}
fn default_agent_user_usage() -> String {
    "standard".to_string()
}

impl Default for AgentUsagePreferences {
    fn default() -> Self {
        Self {
            guest: default_agent_guest_usage(),
            user: default_agent_user_usage(),
        }
    }
}

impl AgentUsagePreferences {
    pub fn normalized(mut self) -> Self {
        if !AGENT_GUEST_USAGE_LEVELS.contains(&self.guest.as_str()) {
            self.guest = default_agent_guest_usage();
        }
        if !AGENT_USER_USAGE_LEVELS.contains(&self.user.as_str()) {
            self.user = default_agent_user_usage();
        }
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModuleVisibilityPreferences {
    #[serde(default = "default_module_visibility_modules")]
    pub modules: HashMap<String, String>,
    /// 旧版 Agent 档位（兼容存储；鉴权请用 Tapp 权限）
    #[serde(default)]
    pub agent_usage: AgentUsagePreferences,
}

fn default_module_visibility_modules() -> HashMap<String, String> {
    HashMap::from([
        ("library".to_string(), "all".to_string()),
        ("phantasi".to_string(), "all".to_string()),
        ("tapp".to_string(), "all".to_string()),
        ("agent".to_string(), "all".to_string()),
    ])
}

impl Default for ModuleVisibilityPreferences {
    fn default() -> Self {
        Self {
            modules: default_module_visibility_modules(),
            agent_usage: AgentUsagePreferences::default(),
        }
    }
}

impl ModuleVisibilityPreferences {
    pub fn normalized(mut self) -> Self {
        let defaults = default_module_visibility_modules();
        let mut normalized = HashMap::new();

        for key in MODULE_VISIBILITY_KEYS {
            let value = self
                .modules
                .remove(key)
                .unwrap_or_else(|| defaults.get(key).cloned().unwrap_or_else(|| "all".into()));
            let value = if MODULE_VISIBILITY_LEVELS.contains(&value.as_str()) {
                value
            } else {
                defaults.get(key).cloned().unwrap_or_else(|| "all".into())
            };
            normalized.insert(key.to_string(), value);
        }

        self.modules = normalized;
        self.agent_usage = self.agent_usage.normalized();
        self
    }

    /// Agent 模块页面可见级别
    pub fn agent_visibility(&self) -> &str {
        self.module_visibility("agent")
    }

    pub fn module_visibility(&self, key: &str) -> &str {
        self.modules.get(key).map(String::as_str).unwrap_or("all")
    }
}

fn parse_module_visibility_preferences(
    value: Value,
) -> Result<ModuleVisibilityPreferences, String> {
    let preferences = serde_json::from_value::<ModuleVisibilityPreferences>(value)
        .map_err(|error| format!("invalid module visibility preferences: {error}"))?;
    for key in MODULE_VISIBILITY_KEYS {
        if let Some(level) = preferences.modules.get(key)
            && !MODULE_VISIBILITY_LEVELS.contains(&level.as_str())
        {
            return Err(format!("invalid visibility level for {key}"));
        }
    }
    Ok(preferences.normalized())
}

/// Strict loader for authorization boundaries. Missing configuration uses product
/// defaults; malformed rows and database errors are returned to the caller.
pub async fn try_load_module_visibility_preferences(
    db: &impl ConnectionTrait,
) -> Result<ModuleVisibilityPreferences, String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT value FROM configurations WHERE key = $1",
            vec![MODULE_VISIBILITY_PREFERENCES_KEY.into()],
        ))
        .await
        .map_err(|error| format!("failed to load module visibility preferences: {error}"))?;
    let Some(row) = row else {
        return Ok(ModuleVisibilityPreferences::default());
    };
    let value = row
        .try_get::<Value>("", "value")
        .map_err(|error| format!("failed to read module visibility preferences: {error}"))?;
    parse_module_visibility_preferences(value)
}

/// Load preferences from `configurations`, always returning a normalized value.
pub async fn load_module_visibility_preferences(
    db: &impl ConnectionTrait,
) -> ModuleVisibilityPreferences {
    match try_load_module_visibility_preferences(db).await {
        Ok(preferences) => preferences,
        Err(error) => {
            tracing::warn!(%error, "Using default module visibility preferences");
            ModuleVisibilityPreferences::default()
        }
    }
}

/// Agent-facing alias (same as [`load_module_visibility_preferences`]).
pub async fn load_module_visibility_preferences_for_agent(
    db: &impl ConnectionTrait,
) -> ModuleVisibilityPreferences {
    load_module_visibility_preferences(db).await
}

/// Authorization-boundary Agent visibility. Load errors stay errors and must
/// not become the default `all`.
pub fn agent_visibility_for_authorization(
    loaded: Result<ModuleVisibilityPreferences, String>,
) -> Result<String, String> {
    loaded.map(|prefs| prefs.agent_visibility().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_all_visible() {
        let p = ModuleVisibilityPreferences::default();
        for key in MODULE_VISIBILITY_KEYS {
            assert_eq!(p.modules.get(key).map(String::as_str), Some("all"), "{key}");
        }
        assert_eq!(p.agent_visibility(), "all");
    }

    #[test]
    fn normalized_drops_unknown_modules_and_levels() {
        let mut modules = HashMap::new();
        modules.insert("library".into(), "admin".into());
        modules.insert("agent".into(), "not-a-level".into());
        modules.insert("evil".into(), "all".into());
        let p = ModuleVisibilityPreferences {
            modules,
            agent_usage: AgentUsagePreferences {
                guest: "bogus".into(),
                user: "elevated".into(),
            },
        }
        .normalized();

        assert_eq!(p.modules.get("library").map(String::as_str), Some("admin"));
        // invalid level → default all
        assert_eq!(p.modules.get("agent").map(String::as_str), Some("all"));
        assert!(!p.modules.contains_key("evil"));
        assert_eq!(p.agent_usage.guest, "none");
        assert_eq!(p.agent_usage.user, "elevated");
        // missing keys filled
        assert_eq!(p.modules.get("phantasi").map(String::as_str), Some("all"));
    }

    #[test]
    fn strict_parser_rejects_levels_that_would_expand_access() {
        assert!(
            parse_module_visibility_preferences(serde_json::json!({
                "modules": { "phantasi": "invalid" }
            }))
            .is_err()
        );
        assert_eq!(
            parse_module_visibility_preferences(serde_json::json!({
                "modules": { "phantasi": "admin" }
            }))
            .unwrap()
            .module_visibility("phantasi"),
            "admin"
        );
    }

    #[test]
    fn serde_roundtrip_preserves_normalized_shape() {
        let p = ModuleVisibilityPreferences::default().normalized();
        let v = serde_json::to_value(&p).unwrap();
        let back: ModuleVisibilityPreferences = serde_json::from_value(v).unwrap();
        assert_eq!(back.normalized(), p);
    }

    #[test]
    fn retired_journal_fields_still_parse_and_drop_on_save() {
        let stored = serde_json::json!({
            "modules": { "phantasi": "admin" },
            "journalBoards": { "feeds": "admin", "notes": "invalid" },
            "journalSourceSort": "bogus"
        });
        let loaded = parse_module_visibility_preferences(stored).unwrap();
        assert_eq!(loaded.module_visibility("phantasi"), "admin");
        let saved = serde_json::to_value(&loaded).unwrap();
        assert!(saved.get("journalBoards").is_none());
        assert!(saved.get("journalSourceSort").is_none());
    }

    #[test]
    fn config_key_is_stable() {
        assert_eq!(
            MODULE_VISIBILITY_PREFERENCES_KEY,
            "module_visibility_preferences"
        );
    }

    #[test]
    fn authorization_visibility_does_not_become_all_on_error() {
        let err: Result<ModuleVisibilityPreferences, String> = Err("db down".into());
        assert!(agent_visibility_for_authorization(err.clone()).is_err());
        assert_ne!(
            agent_visibility_for_authorization(err).unwrap_or_else(|_| "denied".into()),
            "all"
        );
        let lenient = ModuleVisibilityPreferences::default();
        assert_eq!(lenient.agent_visibility(), "all");
        assert_eq!(
            agent_visibility_for_authorization(Ok(ModuleVisibilityPreferences {
                modules: HashMap::from([("agent".into(), "admin".into())]),
                ..Default::default()
            }))
            .unwrap(),
            "admin"
        );
    }
}
