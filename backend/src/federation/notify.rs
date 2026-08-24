//! 联邦事件 → 通知中心桥接
//!
//! 将 Follow / Channel / Room 等入站事件翻译为用户通知。
//! 消息类按会话稳定 ID upsert，避免刷屏；邀请/关注独立条目。

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{json, Value};

use crate::services::agent::notifications::{
    get_notification_manager, Notification, NotificationPriority, NotificationType,
};

const ARO_TAPP_ID: &str = "com.myriad.aro";

/// 从 Actor URL 生成可读标签（优先 display_name / username@domain）
pub async fn actor_label(db: &impl ConnectionTrait, actor_url: &str) -> String {
    if let Ok(Some(row)) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT display_name, username, domain
               FROM federation_remote_actors WHERE actor_url = $1"#,
            [actor_url.into()],
        ))
        .await
    {
        let display: Option<String> = row.try_get::<String>("", "display_name").ok();
        if let Some(name) = display.filter(|s| !s.trim().is_empty()) {
            return name;
        }
        let username: Option<String> = row.try_get::<String>("", "username").ok();
        let domain: Option<String> = row.try_get::<String>("", "domain").ok();
        match (username, domain) {
            (Some(u), Some(d)) if !u.is_empty() && !d.is_empty() => return format!("{}@{}", u, d),
            (Some(u), _) if !u.is_empty() => return u,
            _ => {}
        }
    }
    actor_url
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(actor_url)
        .to_string()
}

/// True when payload is an E2E ciphertext envelope (not yet decrypted).
fn is_e2e_ciphertext_envelope(payload: &Value) -> bool {
    if !payload.is_object() {
        if let Some(s) = payload.as_str() {
            let t = s.trim();
            return t.starts_with('{') && t.contains("ciphertext") && t.contains("algorithm");
        }
        return false;
    }
    let has_ct = payload.get("ciphertext").is_some() || payload.get("cipher_text").is_some();
    let has_alg = payload
        .get("algorithm")
        .and_then(|v| v.as_str())
        .is_some_and(|a| !a.is_empty());
    has_ct && has_alg
}

/// 消息正文预览
pub fn payload_preview(message_type: &str, payload: &Value) -> String {
    match message_type {
        "image" => return "Photo".to_string(),
        "file" | "file-meta" => return "File".to_string(),
        "system" => return "System message".to_string(),
        "link" => {
            if let Some(u) = payload
                .get("url")
                .or_else(|| payload.get("href"))
                .or_else(|| payload.get("link"))
                .and_then(|v| v.as_str())
            {
                return truncate(u, 160);
            }
        }
        _ => {}
    }
    // Never dump ciphertext / algorithm envelopes into the notification tray.
    if is_e2e_ciphertext_envelope(payload) {
        return "Encrypted message".to_string();
    }
    let raw = if let Some(s) = payload.as_str() {
        s.to_string()
    } else if let Some(t) = payload.get("text").and_then(|v| v.as_str()) {
        t.to_string()
    } else if let Some(t) = payload.get("content").and_then(|v| v.as_str()) {
        t.to_string()
    } else if let Some(n) = payload.get("name").and_then(|v| v.as_str()) {
        n.to_string()
    } else if let Some(u) = payload
        .get("url")
        .or_else(|| payload.get("href"))
        .or_else(|| payload.get("link"))
        .and_then(|v| v.as_str())
    {
        u.to_string()
    } else if payload.is_null() {
        "New message".to_string()
    } else {
        let s = payload.to_string();
        // Suppress crypto-looking JSON leftovers
        if s.contains("ciphertext") && s.contains("algorithm") {
            return "Encrypted message".to_string();
        }
        if s.len() > 120 {
            format!("{}…", &s[..117])
        } else {
            s
        }
    };
    truncate(&raw, 160)
}

fn truncate(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut)
}

fn aro_route(kind: &str, id: &str) -> String {
    format!(
        "/tapp/run/{}?{}={}&view=messages",
        ARO_TAPP_ID,
        kind,
        urlencoding_lite(id)
    )
}

/// 轻量 URL 编码（id 多为安全字符；对保留字做 escape）
fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn stable_hash(s: &str) -> String {
    format!("{:x}", md5::compute(s.as_bytes()))
}

/// 私信 / Channel 新消息（按会话 upsert）
pub async fn notify_channel_message(
    user_id: i32,
    channel_id: &str,
    sender_actor: &str,
    sender_label: &str,
    message_type: &str,
    payload: &Value,
) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let preview = payload_preview(message_type, payload);
    let mut notification = Notification::new(
        user_id,
        NotificationType::FederationMessage,
        NotificationPriority::Normal,
        sender_label,
        &preview,
    )
    .with_metadata(json!({
        "event_key": "federation.channel_message",
        "route": aro_route("channel", channel_id),
        "tapp_id": ARO_TAPP_ID,
        "kind": "channel",
        "channel_id": channel_id,
        "sender_actor": sender_actor,
        "message_type": message_type,
    }));
    notification.id = format!("fed_ch_{}_u{}", stable_hash(channel_id), user_id);
    notification.read = false;
    manager.upsert(notification).await;
    crate::services::agent::merope::spawn_ingest(
        user_id,
        "federation.channel_message",
        format!("{sender_label} 发来私信：{preview}"),
    );
}

/// 群聊 / Room 新消息（按会话 + 用户 upsert）
pub async fn notify_room_message(
    user_id: i32,
    room_id: &str,
    room_name: &str,
    sender_actor: &str,
    sender_label: &str,
    message_type: &str,
    payload: &Value,
) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let preview = payload_preview(message_type, payload);
    let title = if room_name.is_empty() {
        sender_label.to_string()
    } else {
        format!("{} · {}", room_name, sender_label)
    };
    let mut notification = Notification::new(
        user_id,
        NotificationType::FederationMessage,
        NotificationPriority::Normal,
        title,
        &preview,
    )
    .with_metadata(json!({
        "event_key": "federation.room_message",
        "route": aro_route("room", room_id),
        "tapp_id": ARO_TAPP_ID,
        "kind": "room",
        "room_id": room_id,
        "sender_actor": sender_actor,
        "message_type": message_type,
    }));
    notification.id = format!("fed_rm_{}_u{}", stable_hash(room_id), user_id);
    notification.read = false;
    manager.upsert(notification).await;
    crate::services::agent::merope::spawn_ingest(
        user_id,
        "federation.room_message",
        format!("{sender_label} 在群里说话：{preview}"),
    );
}

/// 新粉丝（自动 Accept 后的关注事件）
pub async fn notify_new_follower(user_id: i32, actor_url: &str, actor_label: &str) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let mut notification = Notification::new(
        user_id,
        NotificationType::FederationFollow,
        NotificationPriority::Normal,
        "New follower",
        format!("{actor_label} followed you"),
    )
    .with_metadata(json!({
        "event_key": "federation.new_follower",
        "route": format!("/tapp/run/{}?view=feed", ARO_TAPP_ID),
        "tapp_id": ARO_TAPP_ID,
        "kind": "follow",
        "actor_url": actor_url,
        "actor_label": actor_label,
    }));
    notification.id = format!("fed_follower_{}_u{}", stable_hash(actor_url), user_id);
    notification.read = false;
    manager.upsert(notification).await;
    crate::services::agent::merope::spawn_ingest(
        user_id,
        "federation.new_follower",
        format!("{actor_label} 关注了这个人"),
    );
}

/// 我们发出的关注被接受
pub async fn notify_follow_accepted(user_id: i32, actor_url: &str, actor_label: &str) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let mut notification = Notification::new(
        user_id,
        NotificationType::FederationFollow,
        NotificationPriority::Low,
        "Follow accepted",
        format!("{actor_label} accepted your follow"),
    )
    .with_metadata(json!({
        "event_key": "federation.follow_accepted",
        "route": format!("/tapp/run/{}?view=feed", ARO_TAPP_ID),
        "tapp_id": ARO_TAPP_ID,
        "kind": "follow_accepted",
        "actor_url": actor_url,
        "actor_label": actor_label,
    }));
    notification.id = format!("fed_follow_ok_{}_u{}", stable_hash(actor_url), user_id);
    notification.read = false;
    manager.upsert(notification).await;
}

/// 远程发起的 Channel 请求（待接受）
pub async fn notify_channel_invite(
    user_id: i32,
    channel_id: &str,
    actor_url: &str,
    actor_label: &str,
) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let mut notification = Notification::new(
        user_id,
        NotificationType::FederationInvite,
        NotificationPriority::High,
        "New message request",
        format!("{actor_label} wants to message you"),
    )
    .with_metadata(json!({
        "event_key": "federation.channel_invite",
        "route": aro_route("channel", channel_id),
        "tapp_id": ARO_TAPP_ID,
        "kind": "channel_invite",
        "channel_id": channel_id,
        "actor_url": actor_url,
        "actor_label": actor_label,
        // Hint for host UI / Aro deep-link actions
        "actions": [
            { "id": "accept", "api": format!("POST /api/federation/channels/{}/accept", channel_id) },
            { "id": "reject", "api": format!("POST /api/federation/channels/{}/close", channel_id) }
        ],
    }));
    notification.id = format!("fed_inv_ch_{}_u{}", stable_hash(channel_id), user_id);
    notification.read = false;
    manager.upsert(notification).await;
}

/// 被邀请加入 Room
pub async fn notify_room_invite(
    user_id: i32,
    room_id: &str,
    room_name: &str,
    actor_url: &str,
    actor_label: &str,
) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let body = if room_name.is_empty() {
        format!("{actor_label} invited you to a group")
    } else {
        format!("{actor_label} invited you to {room_name}")
    };
    let mut notification = Notification::new(
        user_id,
        NotificationType::FederationInvite,
        NotificationPriority::High,
        "Group invite",
        body,
    )
    .with_metadata(json!({
        "event_key": "federation.room_invite",
        "route": aro_route("room", room_id),
        "tapp_id": ARO_TAPP_ID,
        "kind": "room_invite",
        "room_id": room_id,
        "room_name": room_name,
        "actor_url": actor_url,
        "actor_label": actor_label,
        "actions": [
            { "id": "accept", "api": format!("POST /api/federation/rooms/{}/accept", room_id) },
            { "id": "reject", "api": format!("POST /api/federation/rooms/{}/reject", room_id) }
        ],
    }));
    notification.id = format!("fed_inv_rm_{}_u{}", stable_hash(room_id), user_id);
    notification.read = false;
    manager.upsert(notification).await;
}

/// Mark a stable invite notification as read (after accept/reject).
pub async fn mark_invite_notification_read(user_id: i32, notif_id: &str) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let _ = manager.mark_read(notif_id, user_id).await;
}

pub fn room_invite_notification_id(room_id: &str, user_id: i32) -> String {
    format!("fed_inv_rm_{}_u{}", stable_hash(room_id), user_id)
}

pub fn channel_invite_notification_id(channel_id: &str, user_id: i32) -> String {
    format!("fed_inv_ch_{}_u{}", stable_hash(channel_id), user_id)
}

/// 远程成员接受了群组邀请（邀请方本地通知）
pub async fn notify_room_invite_accepted(
    user_id: i32,
    room_id: &str,
    room_name: &str,
    actor_url: &str,
    actor_label: &str,
) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let body = if room_name.is_empty() {
        format!("{actor_label} accepted your group invite")
    } else {
        format!("{actor_label} joined {room_name}")
    };
    let mut notification = Notification::new(
        user_id,
        NotificationType::FederationInvite,
        NotificationPriority::Normal,
        "Group invite accepted",
        body,
    )
    .with_metadata(json!({
        "event_key": "federation.room_invite_accepted",
        "route": aro_route("room", room_id),
        "tapp_id": ARO_TAPP_ID,
        "kind": "room_invite_accepted",
        "room_id": room_id,
        "room_name": room_name,
        "actor_url": actor_url,
        "actor_label": actor_label,
    }));
    notification.id = format!(
        "fed_rm_ok_{}_{}_u{}",
        stable_hash(room_id),
        stable_hash(actor_url),
        user_id
    );
    notification.read = false;
    manager.upsert(notification).await;
}

/// Channel 已被对方接受
pub async fn notify_channel_accepted(user_id: i32, channel_id: &str, actor_label: &str) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let body = if actor_label.is_empty() {
        "Your message request was accepted".to_string()
    } else {
        format!("{actor_label} accepted your message request")
    };
    let mut notification = Notification::new(
        user_id,
        NotificationType::FederationInvite,
        NotificationPriority::Normal,
        "Direct messages are ready",
        body,
    )
    .with_metadata(json!({
        "event_key": "federation.channel_accepted",
        "route": aro_route("channel", channel_id),
        "tapp_id": ARO_TAPP_ID,
        "kind": "channel_accepted",
        "channel_id": channel_id,
        "actor_label": actor_label,
    }));
    notification.id = format!("fed_ch_ok_{}_u{}", stable_hash(channel_id), user_id);
    notification.read = false;
    manager.upsert(notification).await;
}

/// 出站投递最终失败（dead letter）— 按 domain+activity_type upsert，避免刷屏
pub async fn notify_delivery_failed(
    user_id: i32,
    activity_type: &str,
    target_domain: &str,
    error: &str,
) {
    if user_id <= 0 {
        return;
    }
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let domain = if target_domain.is_empty() {
        "remote"
    } else {
        target_domain
    };
    let kind = if activity_type.is_empty() {
        "Activity"
    } else {
        activity_type
    };
    let err_short = truncate(error, 140);
    let body = format!("Delivery to {domain} failed");
    let mut notification = Notification::new(
        user_id,
        NotificationType::SystemInfo,
        NotificationPriority::High,
        "Federation delivery failed",
        body,
    )
    .with_metadata(json!({
        "event_key": "federation.delivery_failed",
        "route": "/tapp/run/com.myriad.aro?view=messages",
        "tapp_id": ARO_TAPP_ID,
        "kind": "delivery_failed",
        "activity_type": kind,
        "target_domain": domain,
        "error": err_short,
    }));
    // Same domain+type collapses into one unread entry (latest error wins)
    notification.id = format!(
        "fed_dlv_dead_u{}_{}",
        user_id,
        stable_hash(&format!("{}|{}", domain, kind))
    );
    notification.read = false;
    manager.upsert(notification).await;
}

/// 域关系吊销 — 该域下所有未完成投递被一次性取消，按 domain upsert
///
/// The revocation sweep kills every unfinished delivery to the domain in one
/// statement, so the owners of those rows never reach the per-row dead-letter
/// path. Without this they would lose queued activities silently.
pub async fn notify_domain_relationship_revoked(
    user_id: i32,
    target_domain: &str,
    cancelled_deliveries: i64,
) {
    if user_id <= 0 {
        return;
    }
    let Some(manager) = get_notification_manager() else {
        return;
    };
    let domain = if target_domain.is_empty() {
        "remote"
    } else {
        target_domain
    };
    let body = format!(
        "{domain} could not be reached for a long time. Federation was unlinked and {cancelled_deliveries} queued items were cancelled."
    );
    let mut notification = Notification::new(
        user_id,
        NotificationType::SystemInfo,
        NotificationPriority::High,
        "Federation unlinked",
        body,
    )
    .with_metadata(json!({
        "event_key": "federation.domain_revoked",
        "route": "/tapp/run/com.myriad.aro?view=messages",
        "tapp_id": ARO_TAPP_ID,
        "kind": "domain_revoked",
        "target_domain": domain,
        "cancelled_deliveries": cancelled_deliveries,
    }));
    // One entry per user+domain; a later revocation of the same domain replaces it.
    notification.id = format!("fed_dom_revoked_u{}_{}", user_id, stable_hash(domain));
    notification.read = false;
    manager.upsert(notification).await;
}

/// 查询 Room 本地成员 user_id 列表
pub async fn room_local_user_ids(db: &impl ConnectionTrait, room_id: &str) -> Vec<i32> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT DISTINCT local_user_id FROM federation_room_members
               WHERE room_id = $1 AND is_local = true AND local_user_id IS NOT NULL"#,
            [room_id.into()],
        ))
        .await
        .unwrap_or_default();
    rows.into_iter()
        .filter_map(|r| r.try_get::<i32>("", "local_user_id").ok())
        .collect()
}

/// 查询 Room 名称
pub async fn room_name(db: &impl ConnectionTrait, room_id: &str) -> String {
    if let Ok(Some(row)) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT name FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
    {
        if let Ok(name) = row.try_get::<String>("", "name") {
            if !name.is_empty() {
                return name;
            }
        }
    }
    format!("Room {}", &room_id[..8.min(room_id.len())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payload_preview_handles_common_shapes() {
        assert_eq!(
            payload_preview("text", &json!({"text": "hello world"})),
            "hello world"
        );
        assert_eq!(payload_preview("image", &json!({})), "Photo");
        assert_eq!(payload_preview("file", &json!({})), "File");
        assert_eq!(payload_preview("text", &json!("plain")), "plain");
        let long = "x".repeat(200);
        let preview = payload_preview("text", &json!(long));
        assert!(preview.ends_with('…'));
        assert!(preview.chars().count() <= 160);
        // Ciphertext envelopes must not leak into notification body
        assert_eq!(
            payload_preview(
                "text",
                &json!({
                    "algorithm": "x25519-chacha20poly1305",
                    "ciphertext": "abc123",
                    "ephemeral_key": "xyz"
                })
            ),
            "Encrypted message"
        );
        assert_eq!(
            payload_preview("link", &json!({"url": "https://example.com/a"})),
            "https://example.com/a"
        );
    }

    #[test]
    fn aro_route_encodes_id() {
        let route = aro_route("channel", "ch/with space");
        assert!(route.starts_with("/tapp/run/com.myriad.aro?channel="));
        assert!(route.contains("view=messages"));
        assert!(route.contains("%2F") || route.contains("ch"));
    }
}
