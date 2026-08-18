//! Agent 身份系统
//!
//! 支持全局身份 (SOUL.md / USER.md) 和角色专属身份 (agents/*.md)。
//!
//! 角色身份文件位于 `data/agent/agents/` 目录下，文件名与 `AgentProfile.id` 对应：
//! - `data-worker.md` → DataWorker
//! - `content-worker.md` → ContentWorker
//! - `creative-worker.md` → CreativeWorker
//! - `system-worker.md` → SystemWorker
//!
//! Orchestrator 使用全局 SOUL.md 作为身份。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::RwLock;

use super::routing::AgentRole;

/// Agent 身份数据
#[derive(Debug, Clone)]
pub struct AgentIdentity {
    /// SOUL.md 内容（Agent 性格、语气、行为边界）
    pub soul: Option<String>,
    /// USER.md 内容（用户偏好、语言、响应风格）
    pub user_profile: Option<String>,
}

impl AgentIdentity {
    /// 从目录加载身份文件
    pub async fn load_from_dir(dir: &Path) -> Self {
        let soul = Self::read_file(&dir.join("SOUL.md")).await;
        let user_profile = Self::read_file(&dir.join("USER.md")).await;

        if soul.is_some() {
            tracing::info!("[Identity] Loaded SOUL.md from {}", dir.display());
        }
        if user_profile.is_some() {
            tracing::info!("[Identity] Loaded USER.md from {}", dir.display());
        }

        Self { soul, user_profile }
    }

    /// 获取 Role 提示词（SOUL.md 内容，或 None 使用默认模板）
    pub fn role_prompt(&self) -> Option<&str> {
        self.soul.as_deref()
    }

    /// 获取用户上下文提示词片段
    pub fn user_context(&self) -> Option<&str> {
        self.user_profile.as_deref()
    }

    async fn read_file(path: &Path) -> Option<String> {
        match tokio::fs::read_to_string(path).await {
            Ok(content) if !content.trim().is_empty() => Some(content),
            Ok(_) => None,
            Err(_) => None,
        }
    }
}

/// 全局身份管理器（支持全局身份 + 角色身份）
pub struct IdentityManager {
    /// 全局身份 (SOUL.md + USER.md)
    identity: Arc<RwLock<AgentIdentity>>,
    /// 角色专属身份 (agents/*.md)
    role_identities: Arc<RwLock<HashMap<AgentRole, String>>>,
}

/// AgentRole.id → 文件名映射
fn role_file_id(role: AgentRole) -> Option<&'static str> {
    match role {
        AgentRole::DataWorker => Some("data-worker"),
        AgentRole::ContentWorker => Some("content-worker"),
        AgentRole::CreativeWorker => Some("creative-worker"),
        AgentRole::SystemWorker => Some("system-worker"),
        AgentRole::Orchestrator => None, // 使用全局 SOUL.md
    }
}

impl IdentityManager {
    /// 创建并初始化身份管理器
    pub async fn new(data_dir: PathBuf) -> Self {
        let identity = AgentIdentity::load_from_dir(&data_dir).await;
        let role_identities = Self::load_role_identities(&data_dir).await;

        let role_count = role_identities.len();
        if role_count > 0 {
            tracing::info!(
                "[Identity] Loaded {} role identities from agents/",
                role_count
            );
        }

        Self {
            identity: Arc::new(RwLock::new(identity)),
            role_identities: Arc::new(RwLock::new(role_identities)),
        }
    }

    /// 获取全局身份（读锁）
    pub async fn get(&self) -> AgentIdentity {
        self.identity.read().await.clone()
    }

    /// 获取所有角色的简短描述（用于 Planner 注入上下文）
    ///
    /// 返回格式：`" Data Worker: <第一行>\n Content Worker: <第一行>"`
    pub async fn get_role_summaries(&self) -> String {
        let roles = self.role_identities.read().await;
        let mut summaries = Vec::new();
        for (role, content) in roles.iter() {
            let first_line = content
                .lines()
                .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
                .unwrap_or("(no description)");
            summaries.push(format!(
                "{} {}: {}",
                role.icon(),
                role.display_name(),
                first_line
            ));
        }
        summaries.sort(); // 稳定排序
        summaries.join("\n")
    }

    /// 获取指定角色的身份文本
    pub async fn get_role_soul(&self, role: AgentRole) -> Option<String> {
        let roles = self.role_identities.read().await;
        roles.get(&role).cloned()
    }

    /// 加载 agents/ 目录下的角色身份文件
    async fn load_role_identities(data_dir: &Path) -> HashMap<AgentRole, String> {
        let agents_dir = data_dir.join("agents");
        let mut map = HashMap::new();

        let roles = [
            AgentRole::DataWorker,
            AgentRole::ContentWorker,
            AgentRole::CreativeWorker,
            AgentRole::SystemWorker,
        ];

        for role in roles {
            if let Some(file_id) = role_file_id(role) {
                let path = agents_dir.join(format!("{}.md", file_id));
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    let content = content.trim().to_string();
                    if !content.is_empty() {
                        tracing::debug!(
                            role = role.display_name(),
                            file = %path.display(),
                            "[Identity] Loaded role identity"
                        );
                        map.insert(role, content);
                    }
                }
            }
        }

        map
    }
}

/// 全局身份管理器实例
static IDENTITY_MANAGER: once_cell::sync::OnceCell<IdentityManager> =
    once_cell::sync::OnceCell::new();

/// 初始化全局身份管理器
pub async fn init_identity(data_dir: PathBuf) {
    let manager = IdentityManager::new(data_dir).await;
    let _ = IDENTITY_MANAGER.set(manager);
}

/// 获取全局身份管理器
pub fn get_identity_manager() -> Option<&'static IdentityManager> {
    IDENTITY_MANAGER.get()
}

/// 获取当前 Agent 身份（如果已初始化）
pub async fn get_identity() -> Option<AgentIdentity> {
    match IDENTITY_MANAGER.get() {
        Some(manager) => Some(manager.get().await),
        None => None,
    }
}

/// User-facing soul: site persona when Agent life is on, else SOUL.md.
pub async fn get_speaking_soul() -> Option<String> {
    crate::services::agent::life::resolve_speaking_soul().await
}

/// 获取指定角色的身份文本
pub async fn get_role_identity(role: AgentRole) -> Option<String> {
    match IDENTITY_MANAGER.get() {
        Some(manager) => manager.get_role_soul(role).await,
        None => None,
    }
}
