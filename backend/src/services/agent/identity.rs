//! Agent 身份：SOUL.md / USER.md。对外说话走 `get_speaking_soul`。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::RwLock;

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

/// 全局身份管理器
pub struct IdentityManager {
    identity: Arc<RwLock<AgentIdentity>>,
}

impl IdentityManager {
    /// 创建并初始化身份管理器
    pub async fn new(data_dir: PathBuf) -> Self {
        let identity = AgentIdentity::load_from_dir(&data_dir).await;
        Self {
            identity: Arc::new(RwLock::new(identity)),
        }
    }

    /// 获取全局身份（读锁）
    pub async fn get(&self) -> AgentIdentity {
        self.identity.read().await.clone()
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

/// 获取当前 Agent 身份（如果已初始化）
pub async fn get_identity() -> Option<AgentIdentity> {
    match IDENTITY_MANAGER.get() {
        Some(manager) => Some(manager.get().await),
        None => None,
    }
}

/// User-facing soul: site persona when Merope is on, else SOUL.md.
pub async fn get_speaking_soul() -> Option<String> {
    #[cfg(test)]
    if crate::services::channel_group::g0::on() {
        return Some(crate::services::channel_group::g0::soul());
    }
    crate::services::agent::merope::resolve_speaking_soul().await
}
