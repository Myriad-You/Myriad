//! 用户通知生产者。
//!
//! 将业务事件翻译为统一通知；持久化、用户隔离和 SSE 广播由 `NotificationManager`
//! 负责，生产者不直接操作数据库通知表。

use super::notifications::{
    Notification, NotificationManager, NotificationPriority, NotificationType,
};

impl NotificationManager {
    pub async fn notify_heartbeat_result(&self, task_name: &str, result: &str, success: bool) {
        let priority = if success {
            NotificationPriority::Low
        } else {
            NotificationPriority::High
        };
        for user_id in self.admin_user_ids().await {
            let notification = Notification::new(
                user_id,
                NotificationType::HeartbeatResult,
                priority,
                format!("定时任务: {}", task_name),
                result,
            )
            .with_metadata(serde_json::json!({
                "event_key": if success { "heartbeat.succeeded" } else { "heartbeat.failed" },
                "action": "open_arael_manage",
                "tab": "heartbeat",
                "success": success,
                "status": if success { "completed" } else { "failed" },
            }));
            self.notify(notification).await;
        }
    }

    pub async fn notify_brew_new_items(
        &self,
        user_id: i32,
        source_id: i32,
        source_name: &str,
        new_count: i32,
        titles: &[String],
    ) {
        let body = if titles.is_empty() {
            format!("发现 {} 篇新内容", new_count)
        } else {
            titles.join("\n")
        };
        let notification = Notification::new(
            user_id,
            NotificationType::BrewNewItems,
            NotificationPriority::Normal,
            format!("{} · {} 篇新内容", source_name, new_count),
            body,
        )
        .with_metadata(serde_json::json!({
            "event_key": "brew.new_items",
            "route": "/brew",
            "source_id": source_id,
            "new_count": new_count,
        }));
        self.notify(notification).await;
    }

    pub async fn notify_brew_source_error(
        &self,
        user_id: i32,
        source_id: i32,
        source_name: &str,
        error: &str,
    ) {
        let summary = format!("{source_name} 连续抓取失败");
        crate::services::agent::life::spawn_ingest(user_id, "brew.source_error", &summary);
        if !crate::services::agent::life::allow_existing_notify(user_id).await {
            return;
        }
        // Keep the deep link: the thing that broke is a feed, not a conversation.
        // Whatever the Agent has to say about it goes out as its own speech.
        let metadata = serde_json::json!({
            "event_key": "brew.source_error",
            "route": "/brew",
            "source_id": source_id,
            "status": "failed",
        });
        let notification = Notification::new(
            user_id,
            NotificationType::BrewSourceError,
            NotificationPriority::High,
            format!("{} 连续抓取失败", source_name),
            error,
        )
        .with_metadata(metadata);
        self.notify(notification).await;
    }

    pub async fn notify_platform_sync_error(&self, user_id: i32, platform: &str, error: &str) {
        let summary = format!("{platform} 自动刷新失败");
        crate::services::agent::life::spawn_ingest(user_id, "platform.sync.failed", &summary);
        if !crate::services::agent::life::allow_existing_notify(user_id).await {
            return;
        }
        let metadata = serde_json::json!({
            "event_key": "platform.sync.failed",
            "route": "/config?section=platforms",
            "platform": platform,
            "status": "failed",
        });
        let notification = Notification::new(
            user_id,
            NotificationType::SystemInfo,
            NotificationPriority::High,
            format!("{} 自动刷新失败", platform),
            error,
        )
        .with_metadata(metadata);
        self.notify(notification).await;
    }

    /// Skill 自动淘汰 / AI 改进完成时通知管理员
    pub async fn notify_skill_evolution(&self, skill_id: &str, action: &str, detail: &str) {
        let (title, event_key, priority) = match action {
            "pruned" => (
                format!("技能已自动淘汰: {}", skill_id),
                "skill.pruned",
                NotificationPriority::Normal,
            ),
            "improved" => (
                format!("技能已自动改进: {}", skill_id),
                "skill.improved",
                NotificationPriority::Low,
            ),
            other => (
                format!("技能变更 ({}): {}", other, skill_id),
                "skill.changed",
                NotificationPriority::Low,
            ),
        };
        for user_id in self.admin_user_ids().await {
            let notification = Notification::new(
                user_id,
                NotificationType::SystemInfo,
                priority,
                title.clone(),
                detail,
            )
            .with_metadata(serde_json::json!({
                "event_key": event_key,
                "action": "open_arael_manage",
                "tab": "skills",
                "skill_id": skill_id,
                "status": action,
            }));
            self.notify(notification).await;
        }
    }

    pub async fn notify_mcp_server_status(&self, server_id: &str, connected: bool, detail: &str) {
        for user_id in self.admin_user_ids().await {
            let mut notification = Notification::new(
                user_id,
                NotificationType::McpServerStatus,
                if connected {
                    NotificationPriority::Low
                } else {
                    NotificationPriority::High
                },
                if connected {
                    format!("MCP {} 已连接", server_id)
                } else {
                    format!("MCP {} 连接失败", server_id)
                },
                detail,
            )
            .with_metadata(serde_json::json!({
                "event_key": if connected { "mcp.connected" } else { "mcp.disconnected" },
                // About hosts Updater/MCP operator surface
                "route": "/config?section=about",
                "server_id": server_id,
                "status": if connected { "connected" } else { "failed" },
            }));
            notification.id = format!("mcp_{:x}_u{}", md5::compute(server_id.as_bytes()), user_id);
            self.upsert(notification).await;
        }
    }

    pub async fn notify_tapp(
        &self,
        user_id: i32,
        tapp_id: &str,
        title: Option<&str>,
        message: &str,
        notification_type: &str,
    ) -> String {
        let priority = match notification_type {
            "error" | "danger" => NotificationPriority::High,
            "warning" => NotificationPriority::Normal,
            _ => NotificationPriority::Low,
        };
        let notification = Notification::new(
            user_id,
            NotificationType::TappNotification,
            priority,
            title.unwrap_or("Tapp 通知"),
            message,
        )
        .with_metadata(serde_json::json!({
            "event_key": match notification_type {
                "error" | "danger" => "tapp.error",
                "warning" => "tapp.warning",
                _ => "tapp.message",
            },
            "route": format!("/tapp/run/{}", tapp_id),
            "tapp_id": tapp_id,
            "tapp_notification_type": notification_type,
        }));
        let notification_id = notification.id.clone();
        self.notify(notification).await;
        notification_id
    }

    pub async fn notify_updater_job(
        &self,
        user_id: i32,
        job_id: &str,
        kind: &str,
        status: &str,
        detail: &str,
    ) {
        let (title, priority) = match status {
            "succeeded" => ("系统更新任务已完成", NotificationPriority::Normal),
            "failed" => ("系统更新任务失败", NotificationPriority::High),
            "needs_manual" => ("系统更新需要人工处理", NotificationPriority::Urgent),
            "running" => ("系统更新任务执行中", NotificationPriority::Low),
            "unknown" => ("系统更新任务状态需确认", NotificationPriority::High),
            _ => ("系统更新任务已提交", NotificationPriority::Low),
        };
        let mut notification = Notification::new(
            user_id,
            NotificationType::UpdaterStatus,
            priority,
            title,
            detail,
        )
        .with_metadata(serde_json::json!({
            "event_key": match status {
                "succeeded" => "updater.succeeded",
                "failed" => "updater.failed",
                "needs_manual" => "updater.needs_manual",
                "running" => "updater.running",
                "unknown" => "updater.unknown",
                _ => "updater.submitted",
            },
            // Deep-link into About (Updater panel lives there)
            "route": "/config?section=about",
            "job_id": job_id,
            "kind": kind,
            "status": status,
        }));
        notification.id = format!("upd_{:x}_u{}", md5::compute(job_id.as_bytes()), user_id);
        self.upsert(notification).await;
    }
}
