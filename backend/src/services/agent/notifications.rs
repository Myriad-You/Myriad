//! Agent 通知系统
//!
//! 提供实时通知推送（broadcast channel + SSE）和历史通知缓存。
//! 集成 heartbeat 任务结果、agent 执行结果等多种通知来源。
//!
//! 持久化：通知写入 `agent_notifications` 表，启动时恢复最近历史，
//! 已读状态落库，保留 30 天自动清理。内存中的环形缓冲作为热缓存。

mod bridge;

use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};

use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    IntoActiveModel, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, Statement,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};

use crate::models::entities::agent_notifications as notif_entity;

use super::notification_preferences::{self, NotificationPreferences};

/// 全局通知管理器单例
static NOTIFICATION_MANAGER: OnceLock<Arc<NotificationManager>> = OnceLock::new();

/// 通知类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    /// Agent 任务正在执行（同一 run 的通知会原位更新）
    TaskProgress,
    /// Agent 任务完成
    TaskCompleted,
    /// Agent 任务失败
    TaskFailed,
    /// Agent 任务由用户明确取消
    TaskCancelled,
    /// Heartbeat 定时任务执行结果
    HeartbeatResult,
    /// MCP 服务器状态变化
    McpServerStatus,
    /// Brew 订阅源抓取到新内容
    BrewNewItems,
    /// Brew 订阅源连续抓取失败
    BrewSourceError,
    /// Tapp 用户通知（`source_key` = `tapp_notification`）
    TappNotification,
    /// 系统更新/回滚任务状态
    UpdaterStatus,
    /// 系统提示（未知字符串也落到这里）
    SystemInfo,
    /// `waiting_for_input` 澄清
    AgentClarification,
    /// 联邦私信 / 群聊新消息
    FederationMessage,
    /// 联邦关注（新粉丝 / 关注被接受）
    FederationFollow,
    /// 联邦邀请（私信通道 / 群组）
    FederationInvite,
}

/// 通知优先级
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Urgent = 3,
}

impl NotificationType {
    fn as_str(&self) -> &'static str {
        match self {
            NotificationType::TaskProgress => "task_progress",
            NotificationType::TaskCompleted => "task_completed",
            NotificationType::TaskFailed => "task_failed",
            NotificationType::TaskCancelled => "task_cancelled",
            NotificationType::HeartbeatResult => "heartbeat_result",
            NotificationType::McpServerStatus => "mcp_server_status",
            NotificationType::BrewNewItems => "brew_new_items",
            NotificationType::BrewSourceError => "brew_source_error",
            NotificationType::TappNotification => "tapp_notification",
            NotificationType::UpdaterStatus => "updater_status",
            NotificationType::SystemInfo => "system_info",
            NotificationType::AgentClarification => "agent_clarification",
            NotificationType::FederationMessage => "federation_message",
            NotificationType::FederationFollow => "federation_follow",
            NotificationType::FederationInvite => "federation_invite",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "task_progress" => NotificationType::TaskProgress,
            "task_completed" => NotificationType::TaskCompleted,
            "task_failed" => NotificationType::TaskFailed,
            "task_cancelled" => NotificationType::TaskCancelled,
            "heartbeat_result" => NotificationType::HeartbeatResult,
            "mcp_server_status" => NotificationType::McpServerStatus,
            "brew_new_items" => NotificationType::BrewNewItems,
            "brew_source_error" => NotificationType::BrewSourceError,
            "tapp_notification" => NotificationType::TappNotification,
            "updater_status" => NotificationType::UpdaterStatus,
            "agent_clarification" => NotificationType::AgentClarification,
            "federation_message" => NotificationType::FederationMessage,
            "federation_follow" => NotificationType::FederationFollow,
            "federation_invite" => NotificationType::FederationInvite,
            _ => NotificationType::SystemInfo,
        }
    }

    fn source_key(&self) -> &'static str {
        match self {
            NotificationType::TaskProgress
            | NotificationType::TaskCompleted
            | NotificationType::TaskFailed
            | NotificationType::TaskCancelled
            | NotificationType::AgentClarification => "agent",
            NotificationType::HeartbeatResult => "heartbeat",
            NotificationType::McpServerStatus => "mcp",
            NotificationType::BrewNewItems | NotificationType::BrewSourceError => "brew",
            NotificationType::TappNotification => "tapp",
            NotificationType::UpdaterStatus => "updater",
            NotificationType::FederationMessage
            | NotificationType::FederationFollow
            | NotificationType::FederationInvite => "federation",
            NotificationType::SystemInfo => "system",
        }
    }
}

impl NotificationPriority {
    fn as_str(&self) -> &'static str {
        match self {
            NotificationPriority::Low => "low",
            NotificationPriority::Normal => "normal",
            NotificationPriority::High => "high",
            NotificationPriority::Urgent => "urgent",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "low" => NotificationPriority::Low,
            "high" => NotificationPriority::High,
            "urgent" => NotificationPriority::Urgent,
            _ => NotificationPriority::Normal,
        }
    }
}

/// 单条通知
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub notification_type: NotificationType,
    pub priority: NotificationPriority,
    pub title: String,
    pub body: String,
    /// 目标用户 ID。用户可见通知必须有明确 owner；`user_id=None` 拒绝下发。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<i32>,
    /// 可选的结构化数据（如任务 ID、链接等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub read: bool,
}

impl Notification {
    pub fn new(
        user_id: i32,
        notification_type: NotificationType,
        priority: NotificationPriority,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id: format!("notif_{}", uuid::Uuid::new_v4().simple()),
            notification_type,
            priority,
            title: title.into(),
            body: body.into(),
            user_id: Some(user_id),
            metadata: None,
            created_at: Utc::now(),
            read: false,
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn event_key(&self) -> Option<&str> {
        self.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("event_key"))
            .and_then(|value| value.as_str())
    }
}

/// SSE 推送事件（broadcast channel 传输类型）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum NotificationEvent {
    /// 新通知
    NewNotification { notification: Notification },
    /// 通知已读
    NotificationRead { id: String, user_id: i32 },
    /// 通知被删除
    NotificationDeleted { id: String, user_id: i32 },
    /// 通知被批量清空（SSE 端按 user_id 过滤转发）
    NotificationsCleared { user_id: i32 },
    /// 订阅方落后丢弃了消息：客户端应重新拉取历史列表
    Resync { lagged_by: u64 },
    /// On-page persona speech. Not history, not a toast.
    LiveSpeech { user_id: i32, speech: LiveSpeech },
    /// Late direction for an already delivered line; never contains speech text.
    LiveSpeechMotion {
        user_id: i32,
        id: String,
        performance: serde_json::Value,
    },
    /// Ephemeral addressee state. No notification history, toast or speech.
    MeropeStateChanged {
        user_id: i32,
        mood: super::merope::MoodTransition,
        activity: String,
    },
}

/// Face-only proactive line. Never persisted in the notification center.
#[derive(Debug, Clone, Serialize)]
pub struct LiveSpeech {
    pub id: String,
    pub body: String,
    pub event_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub performance: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merope_state: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intention_id: Option<String>,
}

pub fn event_is_for_user(event: &NotificationEvent, user_id: i32) -> bool {
    match event {
        NotificationEvent::NewNotification { notification } => {
            notification.user_id == Some(user_id)
        }
        NotificationEvent::NotificationRead { user_id: owner, .. }
        | NotificationEvent::NotificationDeleted { user_id: owner, .. }
        | NotificationEvent::NotificationsCleared { user_id: owner }
        | NotificationEvent::LiveSpeech { user_id: owner, .. }
        | NotificationEvent::LiveSpeechMotion { user_id: owner, .. }
        | NotificationEvent::MeropeStateChanged { user_id: owner, .. } => *owner == user_id,
        // resync 对所有订阅者广播；由 SSE 转发层无条件下发
        NotificationEvent::Resync { .. } => true,
    }
}

/// 通知管理器
pub struct NotificationManager {
    /// 广播通道（multi-subscriber SSE）
    tx: broadcast::Sender<NotificationEvent>,
    /// 历史通知环形缓冲区（热缓存，启动时从 DB 恢复）
    history: RwLock<VecDeque<Notification>>,
    /// 最大历史记录数
    max_history: usize,
    /// 数据库连接（持久化通知历史与已读状态）
    db: Option<DatabaseConnection>,
}

impl NotificationManager {
    pub async fn notification_preferences(&self, user_id: i32) -> NotificationPreferences {
        notification_preferences::load(self.db.as_ref(), user_id).await
    }

    pub async fn update_notification_preferences(
        &self,
        user_id: i32,
        preferences: NotificationPreferences,
    ) -> Result<NotificationPreferences, String> {
        notification_preferences::save(self.db.as_ref(), user_id, preferences).await
    }

    async fn notification_is_enabled(&self, notification: &Notification) -> bool {
        let Some(user_id) = notification.user_id else {
            return false;
        };
        let preferences = self.notification_preferences(user_id).await;
        if let Some(event_key) = notification.event_key() {
            preferences.allows(event_key)
        } else {
            // 缺少精细事件键时，仍必须服从总开关和来源开关。
            preferences.enabled
                && preferences
                    .sources
                    .get(notification.notification_type.source_key())
                    .copied()
                    .unwrap_or(true)
        }
    }

    /// 创建带持久化的管理器，并从 DB 恢复最近历史
    pub async fn new_with_db(max_history: usize, db: DatabaseConnection) -> Self {
        // 512：降低高扇出时 Lagged 频率；仍会在 Lagged 时发 resync
        let (tx, _) = broadcast::channel(512);
        let mut history = VecDeque::with_capacity(max_history);

        // 启动时删除 `user_id IS NULL` 的通知：没有共享可变通知。
        match notif_entity::Entity::delete_many()
            .filter(notif_entity::Column::UserId.is_null())
            .exec(&db)
            .await
        {
            Ok(result) if result.rows_affected > 0 => tracing::warn!(
                "[Notifications] Removed {} legacy ownerless notifications",
                result.rows_affected
            ),
            Ok(_) => {}
            Err(error) => tracing::warn!(
                "[Notifications] Failed to remove legacy ownerless notifications: {}",
                error
            ),
        }

        match notif_entity::Entity::find()
            .order_by_desc(notif_entity::Column::CreatedAt)
            .limit(max_history as u64)
            .all(&db)
            .await
        {
            Ok(models) => {
                // DB 按时间倒序取出，环形缓冲需要正序（旧→新）
                for model in models.into_iter().rev() {
                    history.push_back(Self::model_to_notification(model));
                }
                if !history.is_empty() {
                    tracing::info!(
                        "[Notifications] Restored {} notifications from DB",
                        history.len()
                    );
                }
            }
            Err(e) => {
                tracing::warn!("[Notifications] Failed to restore history: {}", e);
            }
        }

        Self {
            tx,
            history: RwLock::new(history),
            max_history,
            db: Some(db),
        }
    }

    fn model_to_notification(model: notif_entity::Model) -> Notification {
        Notification {
            id: model.id,
            notification_type: NotificationType::from_str(&model.notification_type),
            priority: NotificationPriority::from_str(&model.priority),
            title: model.title,
            body: model.body,
            user_id: model.user_id,
            metadata: model.metadata,
            created_at: model.created_at.with_timezone(&Utc),
            read: model.read,
        }
    }

    /// 发布用户通知（实时事件 + 热缓存 + 持久化）。返回是否被通知系统接收，非阅读回执。
    pub async fn notify(&self, notification: Notification) -> bool {
        if notification.user_id.is_none() {
            tracing::error!(
                id = %notification.id,
                "[Notifications] Rejected ownerless user notification"
            );
            return false;
        }
        if !self.notification_is_enabled(&notification).await {
            tracing::debug!(
                id = %notification.id,
                event_key = notification.event_key().unwrap_or("unknown"),
                "Notification disabled by user preference"
            );
            return false;
        }
        tracing::debug!(
            id = %notification.id,
            r#type = ?notification.notification_type,
            title = %notification.title,
            "Sending notification"
        );

        // 存入历史
        {
            let mut history = self.history.write().await;
            if history.len() >= self.max_history {
                history.pop_front();
            }
            history.push_back(notification.clone());
        }

        if let Some(db) = &self.db {
            if let Err(error) = bridge::persist(db, &notification, false).await {
                tracing::warn!(id = %notification.id, %error, "notification persistence failed");
            }
        }

        // 广播到所有 SSE 订阅者
        let _ = self
            .tx
            .send(NotificationEvent::NewNotification { notification });
        true
    }

    pub fn emit_live_speech(&self, user_id: i32, speech: LiveSpeech) -> bool {
        self.tx
            .send(NotificationEvent::LiveSpeech { user_id, speech })
            .is_ok()
    }

    pub fn emit_live_speech_motion(
        &self,
        user_id: i32,
        id: String,
        performance: serde_json::Value,
    ) {
        let _ = self.tx.send(NotificationEvent::LiveSpeechMotion {
            user_id,
            id,
            performance,
        });
    }

    pub fn emit_merope_state(
        &self,
        user_id: i32,
        mood: super::merope::MoodTransition,
        activity: String,
    ) {
        let _ = self.tx.send(NotificationEvent::MeropeStateChanged {
            user_id,
            mood,
            activity,
        });
    }

    /// 创建或更新一条通知。
    ///
    /// Agent 的运行进度使用稳定 ID，避免每个步骤都堆积为一条新通知；
    /// SSE 仍复用 `new_notification` 事件，客户端按 ID 替换即可。
    pub async fn upsert(&self, notification: Notification) {
        if notification.user_id.is_none() {
            tracing::error!(
                id = %notification.id,
                "[Notifications] Rejected ownerless notification update"
            );
            return;
        }
        if !self.notification_is_enabled(&notification).await {
            let is_status_snapshot = matches!(
                notification.notification_type,
                NotificationType::TaskProgress
                    | NotificationType::TaskCompleted
                    | NotificationType::TaskFailed
                    | NotificationType::TaskCancelled
                    | NotificationType::AgentClarification
                    | NotificationType::McpServerStatus
                    | NotificationType::UpdaterStatus
            );
            if is_status_snapshot {
                let user_id = notification.user_id.expect("owner checked above");
                // 稳定 ID 的运行中通知若终态被关闭，必须移除旧快照，避免永远显示运行中。
                let _ = self.delete_notification(&notification.id, user_id).await;
            }
            return;
        }
        tracing::debug!(
            id = %notification.id,
            r#type = ?notification.notification_type,
            title = %notification.title,
            "Upserting notification"
        );

        {
            let mut history = self.history.write().await;
            if let Some(position) = history.iter().position(|n| n.id == notification.id) {
                history.remove(position);
            } else if history.len() >= self.max_history {
                history.pop_front();
            }
            history.push_back(notification.clone());
        }

        if let Some(db) = &self.db {
            if let Err(error) = bridge::persist(db, &notification, true).await {
                tracing::warn!(id = %notification.id, %error, "notification upsert failed");
            }
        }

        let _ = self
            .tx
            .send(NotificationEvent::NewNotification { notification });
    }

    /// 将一个后端 run 映射为通知中心中的单条、可持续更新的任务通知。
    #[allow(clippy::too_many_arguments)]
    pub async fn notify_task_status(
        &self,
        run_id: &str,
        task_id: Option<&str>,
        user_id: i32,
        session_id: Option<&str>,
        title: &str,
        body: &str,
        progress: u8,
        status: &str,
        success: Option<bool>,
    ) {
        let notification_type = match status {
            "completed" => NotificationType::TaskCompleted,
            "failed" => NotificationType::TaskFailed,
            "cancelled" => NotificationType::TaskCancelled,
            "waiting_for_input" => NotificationType::AgentClarification,
            _ => NotificationType::TaskProgress,
        };
        let priority = match status {
            "failed" => NotificationPriority::High,
            "waiting_for_input" => NotificationPriority::High,
            _ => NotificationPriority::Normal,
        };
        let event_key = match status {
            "completed" => "agent.task_completed",
            "failed" => "agent.task_failed",
            "cancelled" => "agent.task_cancelled",
            "waiting_for_input" => "agent.clarification",
            _ => "agent.task_progress",
        };
        // Heartbeat already notifies admins via `notify_heartbeat_result`.
        if user_id == crate::services::agent::SYSTEM_USER_ID {
            return;
        }
        if event_key != "agent.task_progress" {
            crate::services::agent::merope::spawn_ingest(user_id, event_key, body);
        }
        // Looking at the Agent panel: no Agent notification of any kind, including
        // in-progress snapshots. Speech still goes through ingest → face.
        if !crate::services::agent::merope::allow_existing_notify(user_id).await {
            let _ = self
                .delete_notification(&format!("agent_run_{}", run_id), user_id)
                .await;
            return;
        }
        let merope_on = crate::services::agent::merope::is_enabled().await;
        let session_id = match session_id {
            Some(id) if !id.is_empty() => Some(id.to_string()),
            _ if merope_on => {
                crate::services::agent::merope::ingest::latest_session_id_for(user_id).await
            }
            _ => None,
        };
        let mut metadata = serde_json::json!({
            "event_key": event_key,
            "run_id": run_id,
            "task_id": task_id,
            "session_id": session_id,
            "status": status,
            "progress": progress,
            "success": success,
        });
        // Flag off must look exactly like before: no landing hint of its own,
        // the panel keeps resolving these by notification type and session id.
        if merope_on {
            metadata["action"] = serde_json::json!("open_agent");
        }
        let mut notification = Notification::new(user_id, notification_type, priority, title, body)
            .with_metadata(metadata);
        notification.id = format!("agent_run_{}", run_id);
        self.upsert(notification).await;
    }

    /// 清理过期通知（保留最近 keep_days 天）
    pub async fn cleanup_old(&self, keep_days: i64) {
        let cutoff = Utc::now() - chrono::Duration::days(keep_days);
        self.history
            .write()
            .await
            .retain(|notification| notification.created_at >= cutoff);
        let Some(db) = &self.db else { return };
        match notif_entity::Entity::delete_many()
            .filter(notif_entity::Column::CreatedAt.lt(cutoff))
            .exec(db)
            .await
        {
            Ok(res) if res.rows_affected > 0 => {
                tracing::info!(
                    "[Notifications] Cleaned {} notifications older than {} days",
                    res.rows_affected,
                    keep_days
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("[Notifications] Cleanup failed: {}", e),
        }
    }

    /// 订阅通知流（用于 SSE endpoint）
    pub fn subscribe(&self) -> broadcast::Receiver<NotificationEvent> {
        self.tx.subscribe()
    }

    /// 获取指定用户的历史通知（最新的 N 条）。旧的无 owner 通知不会下发。
    pub async fn get_history_for_user(&self, user_id: i32, limit: usize) -> Vec<Notification> {
        if let Some(db) = &self.db {
            match notif_entity::Entity::find()
                .filter(notif_entity::Column::UserId.eq(user_id))
                .order_by_desc(notif_entity::Column::CreatedAt)
                .limit(limit as u64)
                .all(db)
                .await
            {
                Ok(models) => {
                    return models
                        .into_iter()
                        .map(Self::model_to_notification)
                        .collect();
                }
                Err(error) => {
                    tracing::warn!(
                        user_id,
                        "[Notifications] History query failed, using hot cache: {}",
                        error
                    );
                }
            }
        }
        let history = self.history.read().await;
        history
            .iter()
            .rev()
            .filter(|n| n.user_id == Some(user_id))
            .take(limit)
            .cloned()
            .collect()
    }

    /// 获取指定用户的未读通知数
    pub async fn unread_count_for_user(&self, user_id: i32) -> usize {
        if let Some(db) = &self.db {
            match notif_entity::Entity::find()
                .filter(notif_entity::Column::UserId.eq(user_id))
                .filter(notif_entity::Column::Read.eq(false))
                .count(db)
                .await
            {
                Ok(count) => return count as usize,
                Err(error) => tracing::warn!(
                    user_id,
                    "[Notifications] Unread count query failed, using hot cache: {}",
                    error
                ),
            }
        }
        let history = self.history.read().await;
        history
            .iter()
            .filter(|n| !n.read && n.user_id == Some(user_id))
            .count()
    }

    pub async fn total_count_for_user(&self, user_id: i32) -> usize {
        if let Some(db) = &self.db {
            match notif_entity::Entity::find()
                .filter(notif_entity::Column::UserId.eq(user_id))
                .count(db)
                .await
            {
                Ok(count) => return count as usize,
                Err(error) => tracing::warn!(
                    user_id,
                    "[Notifications] Total count query failed, using hot cache: {}",
                    error
                ),
            }
        }
        self.history
            .read()
            .await
            .iter()
            .filter(|notification| notification.user_id == Some(user_id))
            .count()
    }

    /// 标记通知已读（带用户归属校验）
    pub async fn mark_read(&self, notification_id: &str, user_id: i32) -> Result<bool, String> {
        let updated_db = if let Some(db) = &self.db {
            match notif_entity::Entity::update_many()
                .col_expr(
                    notif_entity::Column::Read,
                    sea_orm::sea_query::Expr::value(true),
                )
                .filter(notif_entity::Column::Id.eq(notification_id))
                .filter(notif_entity::Column::UserId.eq(user_id))
                .exec(db)
                .await
            {
                Ok(result) => result.rows_affected > 0,
                Err(error) => {
                    tracing::warn!("[Notifications] Mark read failed: {}", error);
                    return Err(error.to_string());
                }
            }
        } else {
            false
        };
        let found = {
            let mut history = self.history.write().await;
            if let Some(n) = history
                .iter_mut()
                .find(|n| n.id == notification_id && n.user_id == Some(user_id))
            {
                n.read = true;
                true
            } else {
                false
            }
        };
        if found || updated_db {
            let _ = self.tx.send(NotificationEvent::NotificationRead {
                id: notification_id.to_string(),
                user_id,
            });
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 删除单条通知（带用户归属校验）
    pub async fn delete_notification(
        &self,
        notification_id: &str,
        user_id: i32,
    ) -> Result<bool, String> {
        // 持久化模式先写 DB；失败时保留热缓存，避免返回成功后重启又“复活”。
        let removed_from_db = if let Some(db) = &self.db {
            match notif_entity::Entity::delete_many()
                .filter(notif_entity::Column::Id.eq(notification_id))
                .filter(notif_entity::Column::UserId.eq(user_id))
                .exec(db)
                .await
            {
                Ok(res) => res.rows_affected > 0,
                Err(e) => {
                    tracing::warn!("[Notifications] Delete failed: {}", e);
                    return Err(e.to_string());
                }
            }
        } else {
            false
        };
        let removed_from_memory = {
            let mut history = self.history.write().await;
            let before = history.len();
            history.retain(|n| !(n.id == notification_id && n.user_id == Some(user_id)));
            history.len() < before
        };

        let removed = removed_from_memory || removed_from_db;
        if removed {
            let _ = self.tx.send(NotificationEvent::NotificationDeleted {
                id: notification_id.to_string(),
                user_id,
            });
        }
        Ok(removed)
    }

    /// 清空该用户自己的全部通知，返回删除条数
    pub async fn clear_all(&self, user_id: i32) -> Result<u64, String> {
        let deleted_from_db = if let Some(db) = &self.db {
            match notif_entity::Entity::delete_many()
                .filter(notif_entity::Column::UserId.eq(user_id))
                .exec(db)
                .await
            {
                Ok(res) => res.rows_affected,
                Err(e) => {
                    tracing::warn!("[Notifications] Clear failed: {}", e);
                    return Err(e.to_string());
                }
            }
        } else {
            0
        };

        let deleted_from_memory = {
            let mut history = self.history.write().await;
            let before = history.len();
            history.retain(|n| n.user_id != Some(user_id));
            (before - history.len()) as u64
        };

        let _ = self
            .tx
            .send(NotificationEvent::NotificationsCleared { user_id });
        Ok(if self.db.is_some() {
            deleted_from_db
        } else {
            deleted_from_memory
        })
    }

    pub(crate) async fn admin_user_ids(&self) -> Vec<i32> {
        let Some(db) = &self.db else {
            return Vec::new();
        };
        let statement = Statement::from_string(
            db.get_database_backend(),
            "SELECT id FROM users WHERE is_admin = true ORDER BY id".to_string(),
        );
        match db.query_all_raw(statement).await {
            Ok(rows) => rows
                .iter()
                .filter_map(|row| row.try_get::<i32>("", "id").ok())
                .collect(),
            Err(error) => {
                tracing::warn!(
                    "[Notifications] Failed to resolve admin recipients: {}",
                    error
                );
                Vec::new()
            }
        }
    }

    /// 返回尚未到达终态的 updater 通知，用于 backend 重启后恢复状态跟踪。
    pub(crate) async fn pending_updater_jobs(&self) -> Vec<(i32, String, String)> {
        let Some(db) = &self.db else {
            return Vec::new();
        };
        match notif_entity::Entity::find()
            .filter(
                notif_entity::Column::NotificationType.eq(NotificationType::UpdaterStatus.as_str()),
            )
            .all(db)
            .await
        {
            Ok(models) => models
                .into_iter()
                .filter_map(|model| {
                    let user_id = model.user_id?;
                    let metadata = model.metadata?;
                    let status = metadata.get("status").and_then(|value| value.as_str())?;
                    if matches!(status, "succeeded" | "failed" | "needs_manual" | "unknown") {
                        return None;
                    }
                    let job_id = metadata
                        .get("job_id")
                        .and_then(|value| value.as_str())?
                        .to_string();
                    let kind = metadata
                        .get("kind")
                        .and_then(|value| value.as_str())
                        .unwrap_or("update")
                        .to_string();
                    Some((user_id, job_id, kind))
                })
                .collect(),
            Err(error) => {
                tracing::warn!(
                    "[Notifications] Failed to restore updater trackers: {}",
                    error
                );
                Vec::new()
            }
        }
    }
}

/// Trusted background producers persist events without an SSE listener or cleanup jobs.
pub async fn init_notification_publisher(db: DatabaseConnection) {
    let (tx, _) = broadcast::channel(32);
    let manager = NotificationManager {
        tx,
        history: RwLock::new(VecDeque::new()),
        max_history: 200,
        db: Some(db),
    };
    let _ = NOTIFICATION_MANAGER.set(Arc::new(manager));
}

/// 初始化全局通知管理器（带持久化 + 每日过期清理）
pub async fn init_notifications(db: DatabaseConnection) {
    let manager = Arc::new(NotificationManager::new_with_db(200, db).await);
    let _ = NOTIFICATION_MANAGER.set(manager.clone());
    bridge::spawn(manager.clone());

    // 每日清理 30 天前的通知
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            manager.cleanup_old(30).await;
        }
    });

    tracing::info!("[Notifications] Manager initialized (persistent)");
}

/// 获取全局通知管理器
pub fn get_notification_manager() -> Option<&'static Arc<NotificationManager>> {
    NOTIFICATION_MANAGER.get()
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn live_speech_reports_transport_acceptance_and_disconnect() {
        let manager = test_manager();
        let speech = || LiveSpeech {
            id: "delivery-test".into(),
            body: "hello".into(),
            event_key: "agent.merope.greeting".into(),
            performance: None,
            merope_state: None,
            intention_id: None,
        };
        assert!(!manager.emit_live_speech(2, speech()));
        let mut stream = manager.subscribe();
        assert!(manager.emit_live_speech(2, speech()));
        assert!(matches!(
            stream.recv().await.unwrap(),
            NotificationEvent::LiveSpeech { user_id: 2, .. }
        ));
        drop(stream);
        assert!(!manager.emit_live_speech(2, speech()));
    }

    #[tokio::test]
    async fn notification_acceptance_distinguishes_rejected_and_retained() {
        let manager = test_manager();
        let mut notification = Notification::new(
            2,
            NotificationType::SystemInfo,
            NotificationPriority::Normal,
            "test",
            "hello",
        );
        notification.user_id = None;
        assert!(!manager.notify(notification.clone()).await);
        assert!(manager.get_history_for_user(2, 10).await.is_empty());
        notification.user_id = Some(2);
        assert!(manager.notify(notification).await);
        assert_eq!(manager.get_history_for_user(2, 10).await.len(), 1);
    }

    use super::*;

    #[test]
    fn looking_at_the_panel_skips_every_agent_task_notification() {
        let src = include_str!("notifications.rs");
        let notify = src
            .split("pub async fn notify_task_status")
            .nth(1)
            .and_then(|rest| rest.split("/// 清理过期通知").next())
            .expect("notify_task_status");
        let skip = notify.find("allow_existing_notify").expect("looking skip");
        assert!(
            !notify[..skip].contains("is_valuable_event"),
            "progress must not notify while looking at the Agent panel"
        );
        assert!(notify.contains("delete_notification"));
    }

    fn test_manager() -> NotificationManager {
        let (tx, _) = broadcast::channel(8);
        NotificationManager {
            tx,
            history: RwLock::new(VecDeque::new()),
            max_history: 10,
            db: None,
        }
    }

    #[tokio::test]
    async fn upsert_replaces_task_progress_instead_of_duplicating_it() {
        let manager = test_manager();

        let mut first = Notification::new(
            9,
            NotificationType::TaskProgress,
            NotificationPriority::Normal,
            "running",
            "10%",
        );
        first.id = "agent_run_test".to_string();
        manager.upsert(first).await;

        let mut second = Notification::new(
            9,
            NotificationType::TaskCompleted,
            NotificationPriority::Normal,
            "done",
            "100%",
        );
        second.id = "agent_run_test".to_string();
        manager.upsert(second).await;

        let history = manager.get_history_for_user(9, 10).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, "agent_run_test");
        assert_eq!(history[0].body, "100%");
        assert!(matches!(
            history[0].notification_type,
            NotificationType::TaskCompleted
        ));
    }

    #[tokio::test]
    async fn users_cannot_read_delete_or_clear_each_others_notifications() {
        let manager = test_manager();
        for user_id in [1, 2] {
            let mut notification = Notification::new(
                user_id,
                NotificationType::SystemInfo,
                NotificationPriority::Normal,
                format!("user {}", user_id),
                "private",
            );
            notification.id = format!("private_{}", user_id);
            manager.notify(notification).await;
        }

        assert_eq!(manager.get_history_for_user(1, 10).await.len(), 1);
        assert_eq!(manager.get_history_for_user(2, 10).await.len(), 1);
        assert!(!manager.delete_notification("private_2", 1).await.unwrap());
        manager.clear_all(1).await.unwrap();
        assert!(manager.get_history_for_user(1, 10).await.is_empty());
        assert_eq!(manager.get_history_for_user(2, 10).await.len(), 1);
    }

    #[tokio::test]
    async fn ownerless_notifications_are_not_user_visible() {
        let manager = test_manager();
        manager
            .notify({
                let mut notification = Notification::new(
                    1,
                    NotificationType::SystemInfo,
                    NotificationPriority::Normal,
                    "legacy broadcast",
                    "must not leak",
                );
                notification.user_id = None;
                notification
            })
            .await;

        assert!(manager.get_history_for_user(1, 10).await.is_empty());
        assert_eq!(manager.total_count_for_user(1).await, 0);
    }

    #[test]
    fn realtime_events_are_filtered_by_owner() {
        let notification = Notification::new(
            2,
            NotificationType::SystemInfo,
            NotificationPriority::Normal,
            "private",
            "body",
        );
        let event = NotificationEvent::NewNotification { notification };
        assert!(event_is_for_user(&event, 2));
        assert!(!event_is_for_user(&event, 1));

        let deleted = NotificationEvent::NotificationDeleted {
            id: "n".to_string(),
            user_id: 2,
        };
        assert!(event_is_for_user(&deleted, 2));
        assert!(!event_is_for_user(&deleted, 1));

        // resync is broadcast to every subscriber so clients can re-list history
        let resync = NotificationEvent::Resync { lagged_by: 3 };
        assert!(event_is_for_user(&resync, 1));
        assert!(event_is_for_user(&resync, 99));

        let live = NotificationEvent::LiveSpeech {
            user_id: 2,
            speech: LiveSpeech {
                id: "spk_1".into(),
                body: "见到你了。".into(),
                event_key: "agent.merope.greeting".into(),
                performance: None,
                merope_state: None,
                intention_id: None,
            },
        };
        assert!(event_is_for_user(&live, 2));
        assert!(!event_is_for_user(&live, 1));
        let refinement = NotificationEvent::LiveSpeechMotion {
            user_id: 2,
            id: "spk_1".into(),
            performance: serde_json::json!({"phrases": []}),
        };
        assert!(event_is_for_user(&refinement, 2));
        assert!(!event_is_for_user(&refinement, 1));
        let wire = serde_json::to_value(refinement).unwrap();
        assert_eq!(wire["event"], "live_speech_motion");
        assert!(wire.get("body").is_none());
        assert!(wire.get("speech").is_none());
    }

    #[tokio::test]
    async fn mood_updates_are_owner_only_ephemeral_state_not_speech_or_history() {
        let manager = test_manager();
        let mut stream = manager.subscribe();
        let before = super::super::merope::Affect::at_rest(Default::default());
        let mood = super::super::merope::MoodTransition::from_affect(
            &before,
            &before,
            "user_appraisal",
            12,
        );
        manager.emit_merope_state(2, mood, "idle".into());
        let event = stream.recv().await.unwrap();
        assert!(event_is_for_user(&event, 2));
        assert!(!event_is_for_user(&event, 1));
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["event"], "merope_state_changed");
        assert_eq!(json["mood"]["revision"], 12);
        assert!(json.get("speech").is_none());
        assert!(json.get("notification").is_none());
        assert!(manager.get_history_for_user(2, 10).await.is_empty());
    }

    #[tokio::test]
    async fn brew_and_tapp_producers_target_only_their_owner() {
        let manager = test_manager();
        manager
            .notify_brew_new_items(
                42,
                7,
                "Example Feed",
                2,
                &["First".to_string(), "Second".to_string()],
            )
            .await;
        manager
            .notify_tapp(42, "demo.tapp", Some("Scheduled"), "done", "info")
            .await;

        let owner_history = manager.get_history_for_user(42, 10).await;
        assert_eq!(owner_history.len(), 2);
        assert!(owner_history.iter().any(|notification| matches!(
            notification.notification_type,
            NotificationType::BrewNewItems
        )));
        assert!(owner_history.iter().any(|notification| matches!(
            notification.notification_type,
            NotificationType::TappNotification
        )));
        assert!(manager.get_history_for_user(41, 10).await.is_empty());
    }

    #[tokio::test]
    async fn updater_producer_targets_only_the_initiating_admin() {
        let manager = test_manager();
        manager
            .notify_updater_job(42, "job-1", "update", "running", "正在更新")
            .await;
        manager
            .notify_updater_job(42, "job-1", "update", "succeeded", "更新完成")
            .await;

        let owner_history = manager.get_history_for_user(42, 10).await;
        assert_eq!(owner_history.len(), 1);
        assert!(matches!(
            owner_history[0].notification_type,
            NotificationType::UpdaterStatus
        ));
        assert_eq!(
            owner_history[0]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("status"))
                .and_then(|value| value.as_str()),
            Some("succeeded")
        );
        assert!(manager.get_history_for_user(41, 10).await.is_empty());
    }

    #[tokio::test]
    async fn disabled_event_is_not_persisted_or_streamed() {
        let manager = test_manager();
        let mut stream = manager.subscribe();
        let user_id = 9001;
        let mut preferences = NotificationPreferences::default();
        preferences
            .events
            .insert("brew.new_items".to_string(), false);
        notification_preferences::set_cached_for_test(user_id, preferences).await;

        manager
            .notify(
                Notification::new(
                    user_id,
                    NotificationType::BrewNewItems,
                    NotificationPriority::Normal,
                    "new items",
                    "body",
                )
                .with_metadata(serde_json::json!({"event_key": "brew.new_items"})),
            )
            .await;

        assert!(manager.get_history_for_user(user_id, 10).await.is_empty());
        assert!(matches!(
            stream.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn master_switch_also_blocks_legacy_notifications_without_event_key() {
        let manager = test_manager();
        let user_id = 9004;
        let preferences = NotificationPreferences {
            enabled: false,
            ..NotificationPreferences::default()
        };
        notification_preferences::set_cached_for_test(user_id, preferences).await;

        manager
            .notify(Notification::new(
                user_id,
                NotificationType::SystemInfo,
                NotificationPriority::Normal,
                "legacy",
                "body",
            ))
            .await;

        assert!(manager.get_history_for_user(user_id, 10).await.is_empty());
    }

    #[tokio::test]
    async fn disabled_terminal_event_removes_stale_progress_snapshot() {
        let manager = test_manager();
        let user_id = 9002;
        let mut progress = Notification::new(
            user_id,
            NotificationType::TaskProgress,
            NotificationPriority::Normal,
            "running",
            "50%",
        )
        .with_metadata(serde_json::json!({"event_key": "agent.task_progress"}));
        progress.id = "stable-run".to_string();
        manager.upsert(progress).await;

        let mut preferences = NotificationPreferences::default();
        preferences
            .events
            .insert("agent.task_completed".to_string(), false);
        notification_preferences::set_cached_for_test(user_id, preferences).await;
        let mut completed = Notification::new(
            user_id,
            NotificationType::TaskCompleted,
            NotificationPriority::Normal,
            "done",
            "100%",
        )
        .with_metadata(serde_json::json!({"event_key": "agent.task_completed"}));
        completed.id = "stable-run".to_string();
        manager.upsert(completed).await;

        assert!(manager.get_history_for_user(user_id, 10).await.is_empty());
    }

    #[tokio::test]
    async fn disabling_stable_message_events_keeps_existing_history() {
        let manager = test_manager();
        let user_id = 9003;
        let mut previous = Notification::new(
            user_id,
            NotificationType::FederationMessage,
            NotificationPriority::Normal,
            "Aro",
            "previous message",
        );
        previous.id = "stable-message".to_string();
        manager.upsert(previous).await;

        let mut preferences = NotificationPreferences::default();
        preferences
            .events
            .insert("federation.channel_message".to_string(), false);
        notification_preferences::set_cached_for_test(user_id, preferences).await;
        let mut incoming = Notification::new(
            user_id,
            NotificationType::FederationMessage,
            NotificationPriority::Normal,
            "Aro",
            "disabled new message",
        )
        .with_metadata(serde_json::json!({
            "event_key": "federation.channel_message"
        }));
        incoming.id = "stable-message".to_string();
        manager.upsert(incoming).await;

        let history = manager.get_history_for_user(user_id, 10).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].body, "previous message");
    }

    #[tokio::test]
    async fn federation_message_upserts_per_conversation() {
        let manager = test_manager();
        let mut first = Notification::new(
            7,
            NotificationType::FederationMessage,
            NotificationPriority::Normal,
            "Alice",
            "hi",
        )
        .with_metadata(serde_json::json!({
            "route": "/tapp/run/com.myriad.aro?channel=ch1&view=messages",
            "kind": "channel",
            "channel_id": "ch1",
        }));
        first.id = "fed_ch_test_u7".to_string();
        manager.upsert(first).await;

        let mut second = Notification::new(
            7,
            NotificationType::FederationMessage,
            NotificationPriority::Normal,
            "Alice",
            "hello again",
        )
        .with_metadata(serde_json::json!({
            "route": "/tapp/run/com.myriad.aro?channel=ch1&view=messages",
            "kind": "channel",
            "channel_id": "ch1",
        }));
        second.id = "fed_ch_test_u7".to_string();
        manager.upsert(second).await;

        let history = manager.get_history_for_user(7, 10).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].body, "hello again");
        assert!(matches!(
            history[0].notification_type,
            NotificationType::FederationMessage
        ));
        assert!(manager.get_history_for_user(8, 10).await.is_empty());
    }

    #[test]
    fn notification_type_storage_names_round_trip() {
        for notification_type in [
            NotificationType::TaskProgress,
            NotificationType::TaskCompleted,
            NotificationType::TaskFailed,
            NotificationType::TaskCancelled,
            NotificationType::HeartbeatResult,
            NotificationType::McpServerStatus,
            NotificationType::BrewNewItems,
            NotificationType::BrewSourceError,
            NotificationType::TappNotification,
            NotificationType::UpdaterStatus,
            NotificationType::SystemInfo,
            NotificationType::AgentClarification,
            NotificationType::FederationMessage,
            NotificationType::FederationFollow,
            NotificationType::FederationInvite,
        ] {
            let stored = notification_type.as_str();
            assert_eq!(NotificationType::from_str(stored).as_str(), stored);
        }
    }
}

/// First-party, ephemeral cross-process observation; never returned to a TAPP.
pub async fn publish_persona_observation(
    db: &DatabaseConnection,
    user_id: i32,
    event_key: &str,
    summary: &str,
) {
    bridge::publish_persona_observation(db, user_id, event_key, summary).await;
}
