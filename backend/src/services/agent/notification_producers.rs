//! 用户通知生产者。
//!
//! 将业务事件翻译为统一通知；持久化、用户隔离和 SSE 广播由 `NotificationManager`
//! 负责，生产者不直接操作数据库通知表。

use super::notification_preferences::{
    ACTION_OPEN_AGENT, ACTION_OPEN_AGENT_MANAGE, NotificationEventKey,
};
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
                format!("Scheduled task: {task_name}"),
                result,
            )
            .with_event(
                if success {
                    NotificationEventKey::HeartbeatSucceeded
                } else {
                    NotificationEventKey::HeartbeatFailed
                },
                serde_json::json!({
                    "action": ACTION_OPEN_AGENT_MANAGE,
                    "tab": "heartbeat",
                    "task_name": task_name,
                    "success": success,
                    "status": if success { "completed" } else { "failed" },
                }),
            );
            self.notify(notification).await;
        }
    }

    pub async fn notify_seo_review_draft(
        &self,
        why: &str,
        site_description: Option<&str>,
        site_keywords: Option<&str>,
        site_ai_intro: Option<&str>,
    ) {
        let mut body = why.trim().to_string();
        let mut push_field = |label: &str, value: Option<&str>| {
            if let Some(text) = value.map(str::trim).filter(|s| !s.is_empty()) {
                if !body.is_empty() {
                    body.push_str("\n\n");
                }
                body.push_str(label);
                body.push('\n');
                body.push_str(text);
            }
        };
        push_field("site_description", site_description);
        push_field("site_keywords", site_keywords);
        push_field("site_ai_intro", site_ai_intro);
        if body.chars().count() > 4000 {
            body = body.chars().take(4000).collect();
        }

        for user_id in self.admin_user_ids().await {
            let mut metadata = serde_json::json!({
                "task_name": "SEO review",
                "actions": [{ "id": "apply" }],
            });
            if let Some(text) = site_description.map(str::trim).filter(|s| !s.is_empty()) {
                metadata["site_description"] = serde_json::json!(text);
            }
            if let Some(text) = site_keywords.map(str::trim).filter(|s| !s.is_empty()) {
                metadata["site_keywords"] = serde_json::json!(text);
            }
            if let Some(text) = site_ai_intro.map(str::trim).filter(|s| !s.is_empty()) {
                metadata["site_ai_intro"] = serde_json::json!(text);
            }
            let mut notification = Notification::new(
                user_id,
                NotificationType::HeartbeatResult,
                NotificationPriority::Normal,
                "Agent SEO",
                body.clone(),
            )
            .with_event(NotificationEventKey::HeartbeatSeoReview, metadata);
            notification.id = format!("seo_review_draft_u{user_id}");
            notification.read = false;
            self.upsert(notification).await;
        }
    }

    pub async fn notify_phantasi_new_items(
        &self,
        user_id: i32,
        source_id: i32,
        source_name: &str,
        new_count: i32,
        titles: &[String],
    ) {
        let body = if titles.is_empty() {
            format!("{new_count} new items found")
        } else {
            titles.join("\n")
        };
        let notification = Notification::new(
            user_id,
            NotificationType::PhantasiNewItems,
            NotificationPriority::Normal,
            format!("{source_name} · {new_count} new items"),
            body,
        )
        .with_event(
            NotificationEventKey::PhantasiNewItems,
            serde_json::json!({
                "source_id": source_id,
                "source_name": source_name,
                "new_count": new_count,
            }),
        );
        self.notify(notification).await;
    }

    pub async fn notify_phantasi_source_error(
        &self,
        user_id: i32,
        source_id: i32,
        source_name: &str,
        error: &str,
    ) {
        let summary = format!("{source_name} feed failed repeatedly");
        let event = NotificationEventKey::PhantasiSourceError;
        crate::services::agent::merope::spawn_ingest(user_id, event.key(), &summary);
        if !crate::services::agent::merope::allow_existing_notify(user_id).await {
            return;
        }
        // With Merope on, the click opens her conversation. There is no journal
        // page to open otherwise.
        let mut metadata = serde_json::json!({
            "source_id": source_id,
            "source_name": source_name,
            "status": "failed",
        });
        if crate::services::agent::merope::is_enabled().await {
            metadata["action"] = serde_json::json!(ACTION_OPEN_AGENT);
            metadata["session_id"] = serde_json::json!(
                crate::services::agent::merope::ingest::latest_session_id_for(user_id).await
            );
        }
        let notification = Notification::new(
            user_id,
            NotificationType::PhantasiSourceError,
            NotificationPriority::High,
            format!("{source_name} feed failed repeatedly"),
            error,
        )
        .with_event(event, metadata);
        self.notify(notification).await;
    }

    pub async fn notify_platform_sync_error(&self, user_id: i32, platform: &str, error: &str) {
        let summary = format!("{platform} auto-refresh failed");
        let event = NotificationEventKey::PlatformSyncFailed;
        crate::services::agent::merope::spawn_ingest(user_id, event.key(), &summary);
        if !crate::services::agent::merope::allow_existing_notify(user_id).await {
            return;
        }
        let mut metadata = serde_json::json!({
            "platform": platform,
            "status": "failed",
        });
        if crate::services::agent::merope::is_enabled().await {
            metadata["action"] = serde_json::json!(ACTION_OPEN_AGENT);
            metadata["session_id"] = serde_json::json!(
                crate::services::agent::merope::ingest::latest_session_id_for(user_id).await
            );
        } else {
            metadata["route"] = serde_json::json!("/config?section=platforms");
        }
        let notification = Notification::new(
            user_id,
            NotificationType::SystemInfo,
            NotificationPriority::High,
            format!("{platform} auto-refresh failed"),
            error,
        )
        .with_event(event, metadata);
        self.notify(notification).await;
    }

    /// One source asked her to judge too often: she stopped judging what it
    /// tells her for a while (`consciousness::judging`). Said to the
    /// site's admins once each time it happens.
    pub async fn notify_judging_paused(&self, source: &str, per_hour: usize, minutes: u64) {
        for user_id in self.admin_user_ids().await {
            let notification = Notification::new(
                user_id,
                NotificationType::SystemInfo,
                NotificationPriority::High,
                format!("She stopped judging events from {source} for {minutes} minutes"),
                format!(
                    "{source} asked her to judge more than {per_hour} events within an hour, \
                     which usually means it is telling the same thing over and over. \
                     What it sends meanwhile is let go unjudged."
                ),
            )
            .with_event(
                NotificationEventKey::SystemInfo,
                serde_json::json!({
                    "action": ACTION_OPEN_AGENT,
                    "source": source,
                    "status": "paused",
                }),
            );
            self.notify(notification).await;
        }
    }

    /// Nothing of her own life ran while she should be living on the site
    /// (`alive::watch`), or she is living again: one notification of the
    /// site owner's, changed in place, so one stop is told once and its end
    /// is not another. Only times and counts: `status` and the seconds are
    /// what the page words it from.
    pub async fn notify_her_life(
        &self,
        owner: i32,
        said: crate::services::agent::alive::watch::Said,
    ) {
        use crate::services::agent::alive::watch::Said;
        let at = |seconds: i64| {
            chrono::DateTime::from_timestamp(seconds, 0)
                .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
                .unwrap_or_default()
        };
        let (priority, title, body, metadata) = match said {
            Said::Stopped { from } => (
                NotificationPriority::High,
                "She has stopped living on the site".to_string(),
                format!(
                    "None of her own drivers has run since {}. She should be living here: \
                     her switch is on and the site is her host.",
                    at(from)
                ),
                serde_json::json!({
                    "action": ACTION_OPEN_AGENT_MANAGE,
                    "status": "her_life_stopped",
                    "from": at(from),
                }),
            ),
            Said::Back { from, since } => (
                NotificationPriority::Normal,
                "She is living on the site again".to_string(),
                format!(
                    "Her own drivers stopped at {} and have run without a gap since {}.",
                    at(from),
                    at(since)
                ),
                serde_json::json!({
                    "action": ACTION_OPEN_AGENT_MANAGE,
                    "status": "her_life_back",
                    "from": at(from),
                    "since": at(since),
                    "stopped_secs": since - from,
                }),
            ),
        };
        let mut notification = Notification::new(
            owner,
            NotificationType::SystemInfo,
            priority,
            title,
            body,
        )
        .with_event(NotificationEventKey::SystemInfo, metadata);
        notification.id = format!("her_life_u{owner}");
        self.upsert(notification).await;
    }

    /// Skill 自动淘汰 / AI 改进完成时通知管理员
    pub async fn notify_skill_evolution(&self, skill_id: &str, action: &str, detail: &str) {
        let (title, event, priority) = match action {
            "pruned" => (
                format!("Skill removed: {skill_id}"),
                NotificationEventKey::SkillPruned,
                NotificationPriority::Normal,
            ),
            "improved" => (
                format!("Skill improved: {skill_id}"),
                NotificationEventKey::SkillImproved,
                NotificationPriority::Low,
            ),
            other => (
                format!("Skill changed ({other}): {skill_id}"),
                NotificationEventKey::SkillChanged,
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
            .with_event(
                event,
                serde_json::json!({
                    "action": ACTION_OPEN_AGENT_MANAGE,
                    "tab": "skills",
                    "skill_id": skill_id,
                    "status": action,
                }),
            );
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
                    format!("MCP {server_id} connected")
                } else {
                    format!("MCP {server_id} disconnected")
                },
                detail,
            )
            .with_event(
                if connected {
                    NotificationEventKey::McpConnected
                } else {
                    NotificationEventKey::McpDisconnected
                },
                serde_json::json!({
                    // route: About section (Updater lives there; MCP panel is Advanced).
                    "route": "/config?section=about",
                    "server_id": server_id,
                    "status": if connected { "connected" } else { "failed" },
                }),
            );
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
            title.unwrap_or("App notification"),
            message,
        )
        .with_event(
            match notification_type {
                "error" | "danger" => NotificationEventKey::TappError,
                "warning" => NotificationEventKey::TappWarning,
                _ => NotificationEventKey::TappMessage,
            },
            serde_json::json!({
                "route": format!("/tapp/run/{}", tapp_id),
                "tapp_id": tapp_id,
                "tapp_notification_type": notification_type,
            }),
        );
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
            "succeeded" => ("System update finished", NotificationPriority::Normal),
            "failed" => ("System update failed", NotificationPriority::High),
            "needs_manual" => (
                "System update needs attention",
                NotificationPriority::Urgent,
            ),
            "running" => ("System update is running", NotificationPriority::Low),
            "unknown" => ("System update status unknown", NotificationPriority::High),
            _ => ("System update submitted", NotificationPriority::Low),
        };
        let mut notification = Notification::new(
            user_id,
            NotificationType::UpdaterStatus,
            priority,
            title,
            detail,
        )
        .with_event(
            match status {
                "succeeded" => NotificationEventKey::UpdaterSucceeded,
                "failed" => NotificationEventKey::UpdaterFailed,
                "needs_manual" => NotificationEventKey::UpdaterNeedsManual,
                "running" => NotificationEventKey::UpdaterRunning,
                "unknown" => NotificationEventKey::UpdaterUnknown,
                _ => NotificationEventKey::UpdaterSubmitted,
            },
            serde_json::json!({
                // Deep-link into About (Updater panel lives there)
                "route": "/config?section=about",
                "job_id": job_id,
                "kind": kind,
                "status": status,
            }),
        );
        notification.id = format!("upd_{:x}_u{}", md5::compute(job_id.as_bytes()), user_id);
        self.upsert(notification).await;
    }
}
