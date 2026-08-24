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

pub const MODULE_VISIBILITY_KEYS: [&str; 5] =
    ["library", "brew", "reports", "tapp", "agent"];
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
        ("brew".to_string(), "all".to_string()),
        ("reports".to_string(), "all".to_string()),
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
        self.modules
            .get("agent")
            .map(String::as_str)
            .unwrap_or("all")
    }
}

/// Load preferences from `configurations`, always returning a normalized value.
pub async fn load_module_visibility_preferences(
    db: &impl ConnectionTrait,
) -> ModuleVisibilityPreferences {
    let sql = "SELECT value FROM configurations WHERE key = $1";
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![MODULE_VISIBILITY_PREFERENCES_KEY.into()],
        ))
        .await;

    match result {
        Ok(Some(row)) => match row.try_get::<Value>("", "value") {
            Ok(value) => serde_json::from_value::<ModuleVisibilityPreferences>(value)
                .map(ModuleVisibilityPreferences::normalized)
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "Invalid module visibility preferences, using defaults: {}",
                        e
                    );
                    ModuleVisibilityPreferences::default()
                }),
            Err(e) => {
                tracing::warn!("Failed to read module visibility preferences: {}", e);
                ModuleVisibilityPreferences::default()
            }
        },
        Ok(None) => ModuleVisibilityPreferences::default(),
        Err(e) => {
            tracing::warn!("Failed to load module visibility preferences: {}", e);
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
        assert_eq!(p.modules.get("brew").map(String::as_str), Some("all"));
    }

    #[test]
    fn serde_roundtrip_preserves_normalized_shape() {
        let p = ModuleVisibilityPreferences::default().normalized();
        let v = serde_json::to_value(&p).unwrap();
        let back: ModuleVisibilityPreferences = serde_json::from_value(v).unwrap();
        assert_eq!(back.normalized(), p);
    }

    #[test]
    fn config_key_is_stable() {
        assert_eq!(
            MODULE_VISIBILITY_PREFERENCES_KEY,
            "module_visibility_preferences"
        );
    }
}
