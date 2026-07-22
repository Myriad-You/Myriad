//! 联邦 Room 管理模块（Phase 4 — Layer 3）
//!
//! N:N 多方房间：群聊、协作、共享阅读室、联合分析等
//! 支持星型路由（Home Server fan-out）和成员治理

use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::federation::types::*;

// ==================== 请求/响应类型 ====================

/// 创建 Room 请求
#[derive(Debug, Deserialize)]
pub struct CreateRoomRequest {
    pub name: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    /// owner / democratic / open
    pub governance_type: Option<String>,
    /// admin-only / member-invite / open
    pub invite_policy: Option<String>,
    pub max_members: Option<i32>,
    pub is_public: Option<bool>,
}

/// 更新 Room 请求
#[derive(Debug, Deserialize)]
pub struct UpdateRoomRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub invite_policy: Option<String>,
    pub max_members: Option<i32>,
    pub is_public: Option<bool>,
}

/// 邀请成员请求
#[derive(Debug, Deserialize)]
pub struct InviteMemberRequest {
    /// 远程 Actor URL 或本地用户名
    pub actor: String,
    /// member / admin / observer
    pub role: Option<String>,
}

/// 发送 Room 消息请求
#[derive(Debug, Deserialize)]
pub struct SendRoomMessageRequest {
    pub message_type: Option<String>,
    pub payload: serde_json::Value,
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
    /// 是否使用 Room E2E 多方加密（需成员已完成密钥发布）
    #[serde(default)]
    pub encrypt: Option<bool>,
}

/// Room 概要
#[derive(Debug, Serialize)]
pub struct RoomSummary {
    pub room_id: String,
    pub name: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub owner_actor: String,
    pub governance_type: String,
    pub invite_policy: String,
    pub member_count: i64,
    pub max_members: i32,
    pub is_public: bool,
    pub my_role: Option<String>,
    /// active | pending (invite not yet accepted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_membership_status: Option<String>,
    pub last_message_at: Option<String>,
    pub created_at: String,
    pub unread_count: i64,
}

/// Room 详情
#[derive(Debug, Serialize)]
pub struct RoomDetail {
    pub room_id: String,
    pub name: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub owner_actor: String,
    pub home_server: String,
    pub governance_type: String,
    pub governance_config: Option<serde_json::Value>,
    pub invite_policy: String,
    pub distribution_strategy: String,
    pub max_members: i32,
    pub is_public: bool,
    pub enabled_tapps: Option<serde_json::Value>,
    pub my_role: Option<String>,
    /// active | pending
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_membership_status: Option<String>,
    pub member_count: i64,
    pub created_at: String,
}

/// Room 成员
#[derive(Debug, Serialize)]
pub struct RoomMember {
    pub actor_url: String,
    pub is_local: bool,
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub role: String,
    /// active | pending
    pub membership_status: String,
    pub joined_at: String,
    pub invited_by: Option<String>,
}

/// Room 消息条目
#[derive(Debug, Clone, Serialize)]
pub struct RoomMessageItem {
    pub message_id: String,
    pub sender_actor: String,
    pub message_type: String,
    pub payload: serde_json::Value,
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
    pub reactions: serde_json::Value,
    pub is_pinned: bool,
    pub is_encrypted: bool,
    pub created_at: String,
}

/// Room 附件库条目（群文件索引；不含 payload 字节）
#[derive(Debug, Clone, Serialize)]
pub struct RoomFileItem {
    /// Stable client key: message_id:transfer_id|filename or tr:transfer_id
    pub key: String,
    pub message_id: String,
    /// image | file
    pub kind: String,
    pub filename: String,
    pub size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub sender_actor: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_id: Option<String>,
    /// True when original message payload has inline data (bytes not returned here)
    pub has_inline: bool,
    /// ready | pending | missing
    pub status: String,
}

/// 群文件列表结果（含是否还有更早条目）
#[derive(Debug, Serialize)]
pub struct RoomFileListResult {
    pub files: Vec<RoomFileItem>,
    pub total: usize,
    pub has_more: bool,
}

/// 发送消息响应
#[derive(Debug, Serialize)]
pub struct SendRoomMessageResponse {
    pub success: bool,
    pub message_id: String,
    pub room_id: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_encrypted: bool,
    /// Outbound fan-out enqueue observability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<crate::federation::delivery::DeliveryEnqueueInfo>,
}

/// 发起 Room E2E 密钥发布响应
#[derive(Debug, Serialize)]
pub struct RoomE2eKeyExchangeResponse {
    pub success: bool,
    pub room_id: String,
    pub public_key: String,
    pub algorithm: String,
    /// 当前已登记公钥的成员数（含自己）
    pub published_key_count: usize,
}

/// Pin/Unpin Room 消息请求
#[derive(Debug, Deserialize)]
pub struct PinRoomMessageRequest {
    pub pinned: bool,
}

// ==================== 辅助函数 ====================

/// 检查用户在 Room 中的角色
pub(crate) async fn get_member_role(
    db: &DatabaseConnection,
    room_id: &str,
    actor_url: &str,
) -> Result<Option<String>, sea_orm::DbErr> {
    // Only *active* members can act (pending invites cannot send/download).
    // Legacy rows without membership_status column heal to default 'active'.
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT role, COALESCE(membership_status, 'active') AS membership_status
               FROM federation_room_members
               WHERE room_id = $1 AND actor_url = $2"#,
            [room_id.into(), actor_url.into()],
        ))
        .await?;

    if let Some(r) = row {
        let status: String = r
            .try_get("", "membership_status")
            .unwrap_or_else(|_| "active".into());
        if status == "active" {
            if let Ok(role) = r.try_get::<String>("", "role") {
                return Ok(Some(role));
            }
        } else {
            return Ok(None);
        }
    }

    // Fallback: host case / trailing-slash differences (exact SQL match fails).
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT actor_url, role, COALESCE(membership_status, 'active') AS membership_status
               FROM federation_room_members WHERE room_id = $1"#,
            [room_id.into()],
        ))
        .await?;
    for r in rows {
        let url: String = r.try_get("", "actor_url").unwrap_or_default();
        if same_actor_url(&url, actor_url) {
            let status: String = r
                .try_get("", "membership_status")
                .unwrap_or_else(|_| "active".into());
            if status != "active" {
                return Ok(None);
            }
            return Ok(r.try_get::<String>("", "role").ok());
        }
    }

    Ok(None)
}

/// Lookup membership role + status (any status, including pending).
/// Returns `None` if no row; status defaults to `active` for legacy rows.
pub(crate) async fn get_membership(
    db: &DatabaseConnection,
    room_id: &str,
    actor_url: &str,
) -> Result<Option<(String, String)>, sea_orm::DbErr> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT role, COALESCE(membership_status, 'active') AS membership_status
               FROM federation_room_members
               WHERE room_id = $1 AND actor_url = $2"#,
            [room_id.into(), actor_url.into()],
        ))
        .await?;

    if let Some(r) = row {
        let role: String = r.try_get("", "role").unwrap_or_else(|_| "member".into());
        let status: String = r
            .try_get("", "membership_status")
            .unwrap_or_else(|_| "active".into());
        return Ok(Some((role, status)));
    }

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT actor_url, role, COALESCE(membership_status, 'active') AS membership_status
               FROM federation_room_members WHERE room_id = $1"#,
            [room_id.into()],
        ))
        .await?;
    for r in rows {
        let url: String = r.try_get("", "actor_url").unwrap_or_default();
        if same_actor_url(&url, actor_url) {
            let role: String = r.try_get("", "role").unwrap_or_else(|_| "member".into());
            let status: String = r
                .try_get("", "membership_status")
                .unwrap_or_else(|_| "active".into());
            return Ok(Some((role, status)));
        }
    }
    Ok(None)
}

/// Upsert a remote (non-local) room member row (defaults to *active*).
async fn upsert_remote_room_member(
    db: &DatabaseConnection,
    room_id: &str,
    actor_url: &str,
    role: &str,
    invited_by: Option<&str>,
) -> Result<(), String> {
    upsert_remote_room_member_with_status(db, room_id, actor_url, role, invited_by, "active").await
}

/// Upsert a remote room member with explicit membership_status.
async fn upsert_remote_room_member_with_status(
    db: &DatabaseConnection,
    room_id: &str,
    actor_url: &str,
    role: &str,
    invited_by: Option<&str>,
    membership_status: &str,
) -> Result<(), String> {
    let role = if ["owner", "admin", "member", "observer"].contains(&role) {
        role
    } else {
        "member"
    };
    let status = if membership_status == "pending" {
        "pending"
    } else {
        "active"
    };
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_room_members
           (room_id, actor_url, is_local, role, invited_by, joined_at, membership_status)
           VALUES ($1, $2, false, $3, $4, NOW(), $5)
           ON CONFLICT (room_id, actor_url) DO UPDATE SET
               role = CASE
                   WHEN federation_room_members.role = 'owner' THEN federation_room_members.role
                   WHEN EXCLUDED.role = 'owner' THEN EXCLUDED.role
                   ELSE EXCLUDED.role
               END,
               membership_status = CASE
                   WHEN EXCLUDED.membership_status = 'active' THEN 'active'
                   WHEN federation_room_members.membership_status = 'active' THEN 'active'
                   ELSE EXCLUDED.membership_status
               END,
               joined_at = COALESCE(federation_room_members.joined_at, NOW())"#,
        [
            room_id.into(),
            actor_url.into(),
            role.into(),
            invited_by.into(),
            status.into(),
        ],
    ))
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Ensure the signed RoomMessage sender is a known member.
///
/// Self-heals common sync gaps (invitee never seeded inviter/owner) so messages
/// are not lost. Returns `not_member:` / `not_found:` prefixed errors for inbox
/// status mapping (4xx, no endless 500 retry storm).
async fn ensure_room_message_sender_member(
    db: &DatabaseConnection,
    room_id: &str,
    sender_actor: &str,
) -> Result<(), String> {
    if get_member_role(db, room_id, sender_actor)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Ok(());
    }

    let room_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT owner_actor FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    let Some(room_row) = room_row else {
        // Distinguish ordering race (invite still in flight) from permanent absence.
        // Callers that already verified the room exists never hit this branch.
        return Err(format!(
            "Room {room_id} not yet present; retry after RoomInvite"
        ));
    };

    let owner: String = room_row.try_get("", "owner_actor").unwrap_or_default();
    let mut heal_role: Option<&'static str> = None;

    if !owner.is_empty() && same_actor_url(&owner, sender_actor) {
        heal_role = Some("owner");
    } else {
        // Inviter of any local/remote member is clearly part of the room graph.
        let inviters = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT invited_by FROM federation_room_members WHERE room_id = $1 AND invited_by IS NOT NULL",
                [room_id.into()],
            ))
            .await
            .map_err(|e| e.to_string())?;
        for r in inviters {
            let inv: String = r.try_get("", "invited_by").unwrap_or_default();
            if !inv.is_empty() && same_actor_url(&inv, sender_actor) {
                heal_role = Some("member");
                break;
            }
        }
    }

    if let Some(role) = heal_role {
        upsert_remote_room_member(db, room_id, sender_actor, role, None).await?;
        tracing::info!(
            "[Room] Self-healed membership for {} in {} as {}",
            sender_actor,
            room_id,
            role
        );
        return Ok(());
    }

    Err(format!(
        "not_member: Actor {} is not a member of room {}",
        sender_actor, room_id
    ))
}

/// 检查是否有管理权限（owner 或 admin）
fn is_admin_role(role: &str) -> bool {
    role == "owner" || role == "admin"
}

/// Synthetic fallback used when a real room name is unavailable.
/// Matches the invite-receive and notify paths (`Room {first 8 of room_id}`).
fn fallback_room_name(room_id: &str) -> String {
    format!("Room {}", &room_id[..8.min(room_id.len())])
}

/// Blank / whitespace-only names are treated as missing.
/// Public rooms are one-way: once public, they cannot become private again.
fn validate_public_transition(
    currently_public: bool,
    requested: Option<bool>,
) -> Result<(), &'static str> {
    match requested {
        Some(false) if currently_public => {
            Err("Public rooms cannot be made private again")
        }
        _ => Ok(()),
    }
}

fn non_empty_room_name(name: Option<&str>) -> Option<String> {
    name.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Resolve display name for an invite: real non-empty name, else synthetic fallback.
fn resolve_invite_room_name(invite_name: Option<&str>, room_id: &str) -> String {
    non_empty_room_name(invite_name).unwrap_or_else(|| fallback_room_name(room_id))
}

/// True when stored name is empty/whitespace or the synthetic `Room rm_xxxx` fallback.
/// Used by unit tests to document the same condition as the invite-receive SQL CASE.
#[cfg(test)]
fn is_missing_or_fallback_room_name(name: &str, room_id: &str) -> bool {
    let trimmed = name.trim();
    trimmed.is_empty() || trimmed == fallback_room_name(room_id)
}

/// 向 Room 的所有远程成员 fan-out 一个 Activity
pub(crate) async fn fanout_to_remote_members(
    db: &DatabaseConnection,
    user_id: i32,
    room_id: &str,
    activity_id: &str,
    activity_json: &serde_json::Value,
    activity_type: &str,
    object_type: &str,
) -> Result<crate::federation::delivery::FanoutResult, sea_orm::DbErr> {
    fanout_to_remote_members_excluding(
        db,
        user_id,
        room_id,
        activity_id,
        activity_json,
        activity_type,
        object_type,
        &[],
    )
    .await
}

/// Fan-out with optional actor URL exclusions (e.g. skip invitee on RoomJoin roster announce).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fanout_to_remote_members_excluding(
    db: &DatabaseConnection,
    user_id: i32,
    room_id: &str,
    activity_id: &str,
    activity_json: &serde_json::Value,
    activity_type: &str,
    object_type: &str,
    exclude_actors: &[&str],
) -> Result<crate::federation::delivery::FanoutResult, sea_orm::DbErr> {
    let mut result = crate::federation::delivery::FanoutResult::default();

    // Ensure signing keys exist before enqueue so first outbound never races
    // the delivery worker without a keypair (join / message / leave fan-out).
    if let Ok(Some(uname_row)) = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
    {
        let username: String = uname_row.try_get("", "username").unwrap_or_default();
        if !username.is_empty() {
            if let Err(e) =
                crate::federation::actor::ensure_user_federation_keys(db, user_id, &username).await
            {
                tracing::warn!(
                    user_id = user_id,
                    username = %username,
                    room_id = %room_id,
                    activity_type = %activity_type,
                    error = %e,
                    "Failed to ensure federation keys before room fanout; delivery may retry"
                );
            }
        }
    }

    // 记录 Activity
    let act_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
               (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
               VALUES ($1, $2, $3, $4, $5, true, NOW())
               RETURNING id"#,
            [
                activity_id.into(),
                user_id.into(),
                activity_type.into(),
                object_type.into(),
                activity_json.clone().into(),
            ],
        ))
        .await?;

    let act_db_id = match act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
        Some(id) => id,
        None => {
            tracing::error!(
                "[Room] fanout failed to insert activity {} for room {}",
                activity_id,
                room_id
            );
            return Ok(result);
        }
    };

    // Count remote *active* members missing remote_actors (cannot resolve inbox)
    let unresolved_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT COUNT(*)::int AS cnt
               FROM federation_room_members rm
               LEFT JOIN federation_remote_actors ra ON rm.actor_url = ra.actor_url
               WHERE rm.room_id = $1 AND rm.is_local = false
                 AND COALESCE(rm.membership_status, 'active') = 'active'
                 AND ra.id IS NULL"#,
            [room_id.into()],
        ))
        .await?;
    result.unresolved_members = unresolved_row
        .and_then(|r| r.try_get::<i32>("", "cnt").ok())
        .unwrap_or(0) as u32;

    // 获取所有 *active* 远程成员的 inbox（pending 邀请不参与 fan-out）
    let remote_members = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT DISTINCT ra.inbox_url, ra.domain, ra.actor_url
               FROM federation_room_members rm
               JOIN federation_remote_actors ra ON rm.actor_url = ra.actor_url
               WHERE rm.room_id = $1 AND rm.is_local = false
                 AND COALESCE(rm.membership_status, 'active') = 'active'"#,
            [room_id.into()],
        ))
        .await?;

    for member_row in remote_members {
        let inbox: String = member_row.try_get("", "inbox_url").unwrap_or_default();
        let domain: String = member_row.try_get("", "domain").unwrap_or_default();
        let actor: String = member_row.try_get("", "actor_url").unwrap_or_default();
        if exclude_actors
            .iter()
            .any(|ex| !ex.is_empty() && same_actor_url(ex, &actor))
        {
            continue;
        }
        if inbox.is_empty() {
            result.skipped_empty_inbox += 1;
            tracing::warn!(
                "[Room] fanout skip empty inbox for {} in room {}",
                actor,
                room_id
            );
            continue;
        }
        result.remote_with_inbox += 1;
        match db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_delivery_queue
                   (activity_id, target_inbox, target_domain, status, created_at)
                   VALUES ($1, $2, $3, 'pending', NOW())"#,
                [act_db_id.into(), inbox.into(), domain.into()],
            ))
            .await
        {
            Ok(_) => result.enqueued += 1,
            Err(e) => {
                tracing::error!(
                    "[Room] fanout enqueue failed room={} actor={}: {}",
                    room_id,
                    actor,
                    e
                );
            }
        }
    }

    if result.enqueued == 0
        && (result.remote_with_inbox > 0
            || result.skipped_empty_inbox > 0
            || result.unresolved_members > 0)
    {
        tracing::warn!(
            "[Room] fanout queued 0 for room {} type={} unresolved={} empty_inbox={} with_inbox={}",
            room_id,
            activity_type,
            result.unresolved_members,
            result.skipped_empty_inbox,
            result.remote_with_inbox
        );
    } else {
        tracing::debug!(
            "[Room] fanout room={} type={} enqueued={}",
            room_id,
            activity_type,
            result.enqueued
        );
    }

    Ok(result)
}

// ==================== Room CRUD ====================

/// 创建新 Room
pub async fn create_room(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    req: &CreateRoomRequest,
) -> Result<RoomDetail, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    let home_server = extract_domain(&base_url).unwrap_or_default();
    let room_id = generate_room_id();

    let governance = req.governance_type.as_deref().unwrap_or("owner");
    let invite_policy = req.invite_policy.as_deref().unwrap_or("admin-only");
    let max_members = req.max_members.unwrap_or(50);
    let is_public = req.is_public.unwrap_or(false);

    // 验证名称和描述长度
    if req.name.is_empty() || req.name.len() > 500 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Room name must be 1-500 characters"})),
        ));
    }
    if req.description.as_ref().is_some_and(|d| d.len() > 5000) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Description must be at most 5000 characters"})),
        ));
    }

    // 验证 max_members 范围
    if !(2..=5000).contains(&max_members) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "max_members must be between 2 and 5000"})),
        ));
    }

    // 验证枚举值
    if !["owner", "democratic", "open"].contains(&governance) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid governance_type"})),
        ));
    }
    if !["admin-only", "member-invite", "open"].contains(&invite_policy) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid invite_policy"})),
        ));
    }

    // 创建 Room
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_rooms
           (room_id, name, description, avatar_url, owner_actor, home_server, governance_type, invite_policy,
            max_members, is_public, distribution_strategy, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'fan-out', NOW())"#,
        [
            room_id.clone().into(),
            req.name.clone().into(),
            req.description.clone().into(),
            req.avatar_url.clone().into(),
            local_actor.clone().into(),
            home_server.clone().into(),
            governance.into(),
            invite_policy.into(),
            max_members.into(),
            is_public.into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    // 将创建者添加为 owner 成员
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_room_members
           (room_id, actor_url, is_local, local_user_id, role, joined_at, membership_status)
           VALUES ($1, $2, true, $3, 'owner', NOW(), 'active')"#,
        [
            room_id.clone().into(),
            local_actor.clone().into(),
            user_id.into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    tracing::info!("[Room] Created room {} by {}", room_id, username);

    Ok(RoomDetail {
        room_id,
        name: req.name.clone(),
        description: req.description.clone(),
        avatar_url: req.avatar_url.clone(),
        owner_actor: local_actor,
        home_server,
        governance_type: governance.to_string(),
        governance_config: None,
        invite_policy: invite_policy.to_string(),
        distribution_strategy: "fan-out".to_string(),
        max_members,
        is_public,
        enabled_tapps: None,
        my_role: Some("owner".to_string()),
        my_membership_status: Some("active".to_string()),
        member_count: 1,
        created_at: now_iso8601(),
    })
}

/// 更新 Room 信息（仅 owner/admin 可操作）
pub async fn update_room(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
    req: &UpdateRoomRequest,
) -> Result<RoomDetail, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    // 验证权限：必须是 owner 或 admin
    let my_role = get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Not a member of this room"})),
            )
        })?;

    if !is_admin_role(&my_role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Only owner or admin can update room"})),
        ));
    }

    // 验证字段
    if let Some(ref name) = req.name {
        if name.is_empty() || name.len() > 500 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Room name must be 1-500 characters"})),
            ));
        }
    }
    if let Some(ref desc) = req.description {
        if desc.len() > 5000 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Description must be at most 5000 characters"})),
            ));
        }
    }
    if let Some(ref avatar) = req.avatar_url {
        if avatar.len() > 2048 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Avatar URL too long"})),
            ));
        }
    }
    if let Some(ref policy) = req.invite_policy {
        if !["admin-only", "member-invite", "open"].contains(&policy.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid invite_policy"})),
            ));
        }
    }
    if let Some(max) = req.max_members {
        if !(2..=5000).contains(&max) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "max_members must be between 2 and 5000"})),
            ));
        }
    }

    // Public is one-way: once public, cannot go private again.
    let currently_public: bool = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT is_public FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?
        .and_then(|r| r.try_get::<bool>("", "is_public").ok())
        .unwrap_or(false);

    if let Err(msg) = validate_public_transition(currently_public, req.is_public) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))));
    }

    // 构建动态 SET 子句
    let mut set_parts = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();
    let mut idx = 1u32;

    if let Some(ref name) = req.name {
        set_parts.push(format!("name = ${}", idx));
        values.push(name.clone().into());
        idx += 1;
    }
    if let Some(ref desc) = req.description {
        set_parts.push(format!("description = ${}", idx));
        values.push(desc.clone().into());
        idx += 1;
    }
    if let Some(ref avatar) = req.avatar_url {
        set_parts.push(format!("avatar_url = ${}", idx));
        values.push(avatar.clone().into());
        idx += 1;
    }
    if let Some(ref policy) = req.invite_policy {
        set_parts.push(format!("invite_policy = ${}", idx));
        values.push(policy.clone().into());
        idx += 1;
    }
    if let Some(max) = req.max_members {
        set_parts.push(format!("max_members = ${}", idx));
        values.push(max.into());
        idx += 1;
    }
    // Only allow true (or no-op true→true). false is blocked when already public.
    if let Some(true) = req.is_public {
        set_parts.push(format!("is_public = ${}", idx));
        values.push(true.into());
        idx += 1;
    }

    if set_parts.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "No fields to update"})),
        ));
    }

    set_parts.push("updated_at = NOW()".to_string());
    let set_clause = set_parts.join(", ");
    let sql = format!(
        "UPDATE federation_rooms SET {} WHERE room_id = ${}",
        set_clause, idx
    );
    values.push(room_id.into());

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &sql,
        values,
    ))
    .await
    .map_err(db_err)?;

    tracing::info!("[Room] Updated room {} by {}", room_id, username);

    // Fan-out governance changes so remote members update name/description/etc.
    let mut changes = serde_json::Map::new();
    if let Some(ref name) = req.name {
        changes.insert("name".into(), json!(name));
    }
    if let Some(ref desc) = req.description {
        changes.insert("description".into(), json!(desc));
    }
    if let Some(ref avatar) = req.avatar_url {
        changes.insert("avatar_url".into(), json!(avatar));
    }
    if let Some(ref policy) = req.invite_policy {
        changes.insert("invite_policy".into(), json!(policy));
    }
    if let Some(max) = req.max_members {
        changes.insert("max_members".into(), json!(max));
    }
    // Only fan-out successful public=true transitions (never public→private).
    if req.is_public == Some(true) {
        changes.insert("is_public".into(), json!(true));
    }

    if !changes.is_empty() {
        let changes_val = serde_json::Value::Object(changes);
        crate::federation::ws_gateway::broadcast_to_room(
            room_id,
            &json!({
                "type": "system",
                "room_id": room_id,
                "event": "governance_changed",
                "actor": &local_actor,
                "changes": &changes_val
            }),
        )
        .await;

        let activity_id = generate_activity_id(&base_url);
        let gov_activity = json!({
            "@context": build_context(),
            "type": "myriad:RoomGovernance",
            "id": &activity_id,
            "actor": &local_actor,
            "object": {
                "type": "myriad:RoomGovernance",
                "room": room_id,
                "changes": changes_val
            }
        });
        if let Err(e) = fanout_to_remote_members(
            db,
            user_id,
            room_id,
            &activity_id,
            &gov_activity,
            "RoomGovernance",
            "RoomGovernance",
        )
        .await
        {
            tracing::warn!(
                "[Room] Failed to fan-out governance for room {}: {}",
                room_id,
                e
            );
        }
    }

    // 返回更新后的详情
    get_room(user_id, username, room_id, db).await
}

/// 解散 Room（仅 owner 可操作）
pub async fn delete_room(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT owner_actor FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Room not found"})),
            )
        })?;

    let owner_actor: String = row.try_get("", "owner_actor").unwrap_or_default();
    let my_role = get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .unwrap_or_default();

    if !same_actor_url(&owner_actor, &local_actor) || my_role != "owner" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Only the room owner can delete room"})),
        ));
    }

    // Fan-out dissolve while remote members still exist for delivery targets
    let activity_id = generate_activity_id(&base_url);
    let dissolve_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomDissolve",
        "id": &activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:Room",
            "id": room_id
        }
    });
    if let Err(e) = fanout_to_remote_members(
        db,
        user_id,
        room_id,
        &activity_id,
        &dissolve_activity,
        "RoomDissolve",
        "Room",
    )
    .await
    {
        tracing::warn!("[Room] dissolve fanout failed for {}: {}", room_id, e);
    }

    let delete_notice = json!({
        "type": "room_deleted",
        "room_id": room_id,
        "deleted_by": local_actor
    });
    crate::federation::ws_gateway::broadcast_to_room(room_id, &delete_notice).await;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_room_messages WHERE room_id = $1",
        [room_id.into()],
    ))
    .await
    .map_err(db_err)?;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_room_members WHERE room_id = $1",
        [room_id.into()],
    ))
    .await
    .map_err(db_err)?;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_rooms WHERE room_id = $1",
        [room_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // Stop outbound KeyExchange / Room* retries aimed at this id
    let _ = crate::federation::delivery::cancel_pending_deliveries_for_resource(
        db,
        room_id,
        "cancelled: local room dissolved",
    )
    .await;

    tracing::info!("[Room] Deleted room {} by {}", room_id, username);

    Ok(json!({ "success": true, "room_id": room_id }))
}

/// Remote owner dissolved the room — wipe local copy and notify open clients.
pub async fn handle_room_dissolve(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let room_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("room").and_then(|v| v.as_str()))
        .ok_or("Missing room id")?;

    // Prefer owner check; tolerate missing local room row (already gone)
    let owner_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT owner_actor FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if let Some(row) = owner_row.as_ref() {
        let owner: String = row.try_get("", "owner_actor").unwrap_or_default();
        if !owner.is_empty() && !same_actor_url(&owner, actor_url_str) {
            // Also accept if actor is local owner role member (roster lag)
            let role = get_member_role(db, room_id, actor_url_str)
                .await
                .map_err(|e| e.to_string())?;
            if role.as_deref() != Some("owner") {
                return Err(format!(
                    "Actor {} is not owner of room {} (owner={})",
                    actor_url_str, room_id, owner
                ));
            }
        }
    }

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "room_deleted",
            "room_id": room_id,
            "deleted_by": actor_url_str
        }),
    )
    .await;

    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM federation_room_messages WHERE room_id = $1",
            [room_id.into()],
        ))
        .await;
    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM federation_room_members WHERE room_id = $1",
            [room_id.into()],
        ))
        .await;
    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await;

    let _ = crate::federation::delivery::cancel_pending_deliveries_for_resource(
        db,
        room_id,
        "cancelled: remote room dissolved",
    )
    .await;

    tracing::info!(
        "[Room] Dissolved room {} (remote notice from {})",
        room_id,
        actor_url_str
    );
    Ok(())
}

/// 获取用户参与的所有 Room
pub async fn list_rooms(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
) -> Result<Vec<RoomSummary>, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT r.room_id, r.name, r.description, r.avatar_url, r.owner_actor,
                      r.governance_type, r.invite_policy, r.max_members, r.is_public,
                      r.created_at,
                      rm.role AS my_role,
                      COALESCE(rm.membership_status, 'active') AS my_membership_status,
                      (SELECT COUNT(*) FROM federation_room_members
                       WHERE room_id = r.room_id
                         AND COALESCE(membership_status, 'active') = 'active') AS member_count,
                      (SELECT MAX(created_at) FROM federation_room_messages WHERE room_id = r.room_id) AS last_message_at,
                      COALESCE((SELECT COUNT(*) FROM federation_room_messages msg
                                WHERE msg.room_id = r.room_id
                                  AND msg.sender_actor != $2
                                  AND msg.created_at > COALESCE(
                                      (SELECT MAX(m2.created_at) FROM federation_room_messages m2
                                       WHERE m2.room_id = r.room_id AND m2.sender_actor = $2), r.created_at)
                      ), 0) AS unread_count
               FROM federation_rooms r
               JOIN federation_room_members rm ON rm.room_id = r.room_id AND rm.actor_url = $2
               WHERE rm.is_local = true AND rm.local_user_id = $1
               ORDER BY COALESCE(
                   (SELECT MAX(created_at) FROM federation_room_messages WHERE room_id = r.room_id),
                   r.created_at
               ) DESC"#,
            [user_id.into(), local_actor.into()],
        ))
        .await
        .map_err(db_err)?;

    let mut rooms = Vec::new();
    for row in rows {
        rooms.push(RoomSummary {
            room_id: row.try_get("", "room_id").unwrap_or_default(),
            name: row.try_get("", "name").unwrap_or_default(),
            description: row
                .try_get::<Option<String>>("", "description")
                .unwrap_or(None),
            avatar_url: row
                .try_get::<Option<String>>("", "avatar_url")
                .unwrap_or(None),
            owner_actor: row.try_get("", "owner_actor").unwrap_or_default(),
            governance_type: row.try_get("", "governance_type").unwrap_or_default(),
            invite_policy: row.try_get("", "invite_policy").unwrap_or_default(),
            member_count: row.try_get::<i64>("", "member_count").unwrap_or(0),
            max_members: row.try_get::<i32>("", "max_members").unwrap_or(50),
            is_public: row.try_get::<bool>("", "is_public").unwrap_or(false),
            my_role: row.try_get::<Option<String>>("", "my_role").unwrap_or(None),
            my_membership_status: row
                .try_get::<Option<String>>("", "my_membership_status")
                .unwrap_or(None),
            last_message_at: row
                .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "last_message_at")
                .ok()
                .flatten()
                .map(|t| t.to_rfc3339()),
            created_at: row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
            unread_count: row.try_get::<i64>("", "unread_count").unwrap_or(0),
        });
    }

    Ok(rooms)
}

/// 获取 Room 详情
pub async fn get_room(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
) -> Result<RoomDetail, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT r.room_id, r.name, r.description, r.avatar_url, r.owner_actor, r.home_server,
                      r.governance_type, r.governance_config, r.invite_policy,
                      r.distribution_strategy, r.max_members, r.is_public,
                      r.enabled_tapps, r.created_at,
                      rm.role AS my_role,
                      COALESCE(rm.membership_status, 'active') AS my_membership_status,
                      (SELECT COUNT(*) FROM federation_room_members
                       WHERE room_id = r.room_id
                         AND COALESCE(membership_status, 'active') = 'active') AS member_count
               FROM federation_rooms r
               LEFT JOIN federation_room_members rm ON rm.room_id = r.room_id AND rm.actor_url = $3
               WHERE r.room_id = $1
                 AND (r.is_public = true
                      OR EXISTS (SELECT 1 FROM federation_room_members
                                 WHERE room_id = r.room_id AND is_local = true AND local_user_id = $2))"#,
            [room_id.into(), user_id.into(), local_actor.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (StatusCode::NOT_FOUND, Json(json!({"error": "Room not found or access denied"})))
        })?;

    Ok(RoomDetail {
        room_id: row.try_get("", "room_id").unwrap_or_default(),
        name: row.try_get("", "name").unwrap_or_default(),
        description: row
            .try_get::<Option<String>>("", "description")
            .unwrap_or(None),
        avatar_url: row
            .try_get::<Option<String>>("", "avatar_url")
            .unwrap_or(None),
        owner_actor: row.try_get("", "owner_actor").unwrap_or_default(),
        home_server: row.try_get("", "home_server").unwrap_or_default(),
        governance_type: row.try_get("", "governance_type").unwrap_or_default(),
        governance_config: row
            .try_get::<Option<serde_json::Value>>("", "governance_config")
            .unwrap_or(None),
        invite_policy: row.try_get("", "invite_policy").unwrap_or_default(),
        distribution_strategy: row.try_get("", "distribution_strategy").unwrap_or_default(),
        max_members: row.try_get::<i32>("", "max_members").unwrap_or(50),
        is_public: row.try_get::<bool>("", "is_public").unwrap_or(false),
        enabled_tapps: row
            .try_get::<Option<serde_json::Value>>("", "enabled_tapps")
            .unwrap_or(None),
        my_role: row.try_get::<Option<String>>("", "my_role").unwrap_or(None),
        my_membership_status: row
            .try_get::<Option<String>>("", "my_membership_status")
            .unwrap_or(None),
        member_count: row.try_get::<i64>("", "member_count").unwrap_or(0),
        created_at: row
            .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
            .map(|t| t.to_rfc3339())
            .unwrap_or_default(),
    })
}

/// 获取 Room 成员列表
pub async fn get_members(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
) -> Result<Vec<RoomMember>, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    // Allow active *or* pending members (invitees need roster + accept UI)
    let membership = get_membership(db, room_id, &local_actor)
        .await
        .map_err(db_err)?;
    if membership.is_none() {
        // 检查是否是公开 Room
        let is_public = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT 1 FROM federation_rooms WHERE room_id = $1 AND is_public = true",
                [room_id.into()],
            ))
            .await
            .map_err(db_err)?;
        if is_public.is_none() {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Not a member of this room"})),
            ));
        }
    }

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT rm.actor_url, rm.is_local, rm.role, rm.joined_at, rm.invited_by,
                      COALESCE(rm.membership_status, 'active') AS membership_status,
                      COALESCE(
                          NULLIF(ra.display_name, ''),
                          ra.username,
                          u.display_name,
                          u.username
                      ) AS display_name,
                      COALESCE(
                          NULLIF(ra.avatar_url, ''),
                          NULLIF(
                              CASE
                                  WHEN u.avatar_url LIKE 'https://ui-avatars.com/%'
                                       OR u.avatar_url LIKE 'http://ui-avatars.com/%'
                                  THEN NULL
                                  ELSE u.avatar_url
                              END,
                              ''
                          ),
                          (
                              SELECT NULLIF(ui.avatar_url, '')
                              FROM user_identities ui
                              WHERE ui.user_id = u.id
                                AND ui.avatar_url IS NOT NULL
                                AND ui.avatar_url <> ''
                              ORDER BY ui.is_primary DESC, ui.last_login_at DESC NULLS LAST, ui.linked_at DESC
                              LIMIT 1
                          ),
                          NULLIF(u.avatar_url, '')
                      ) AS avatar_url
               FROM federation_room_members rm
               LEFT JOIN federation_remote_actors ra ON rm.actor_url = ra.actor_url
               LEFT JOIN users u ON rm.local_user_id = u.id
               WHERE rm.room_id = $1
               ORDER BY
                 CASE COALESCE(rm.membership_status, 'active') WHEN 'active' THEN 0 ELSE 1 END,
                 CASE rm.role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 WHEN 'member' THEN 2 ELSE 3 END,
                 rm.joined_at"#,
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let _ = user_id; // validated via actor_url
    let members = rows
        .iter()
        .map(|r| RoomMember {
            actor_url: r.try_get("", "actor_url").unwrap_or_default(),
            is_local: r.try_get::<bool>("", "is_local").unwrap_or(false),
            display_name: r
                .try_get::<Option<String>>("", "display_name")
                .unwrap_or(None),
            avatar_url: r
                .try_get::<Option<String>>("", "avatar_url")
                .unwrap_or(None),
            role: r.try_get("", "role").unwrap_or_default(),
            membership_status: r
                .try_get::<String>("", "membership_status")
                .unwrap_or_else(|_| "active".into()),
            joined_at: r
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "joined_at")
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
            invited_by: r
                .try_get::<Option<String>>("", "invited_by")
                .unwrap_or(None),
        })
        .collect();

    Ok(members)
}

// ==================== 成员管理 ====================

/// 邀请成员加入 Room
pub async fn invite_member(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
    req: &InviteMemberRequest,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    // 验证邀请权限
    let my_role = get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Not a member"})),
            )
        })?;

    // 检查 invite_policy
    let room_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT invite_policy, max_members, name, owner_actor FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Room not found"})),
            )
        })?;

    let policy: String = room_row.try_get("", "invite_policy").unwrap_or_default();
    let max_members: i32 = room_row.try_get("", "max_members").unwrap_or(50);
    let room_name: String = room_row.try_get("", "name").unwrap_or_default();
    let owner_actor: String = room_row.try_get("", "owner_actor").unwrap_or_default();

    match policy.as_str() {
        "admin-only" if !is_admin_role(&my_role) => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Only admins can invite"})),
            ));
        }
        "member-invite" => {} // 任何成员可邀请
        "open" => {}          // 无限制
        _ if !is_admin_role(&my_role) => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Insufficient permissions"})),
            ));
        }
        _ => {}
    }

    // 检查成员上限（仅 active 占用名额；pending 邀请不计入）
    let count_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT COUNT(*)::int AS cnt FROM federation_room_members
               WHERE room_id = $1 AND COALESCE(membership_status, 'active') = 'active'"#,
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let current_count: i32 = count_row
        .and_then(|r| r.try_get::<i32>("", "cnt").ok())
        .unwrap_or(0);

    if current_count >= max_members {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Room is full"})),
        ));
    }

    let role = req.role.as_deref().unwrap_or("member");
    if !["member", "admin", "observer"].contains(&role) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid role"})),
        ));
    }

    // 解析目标 Actor：本地用户名保持原逻辑，远端支持 Actor URL / acct:user@domain / @user@domain / user@domain。
    let raw_target_actor = req.actor.trim();
    if raw_target_actor.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Actor reference is required"})),
        ));
    }

    let resolved_remote_actor = if raw_target_actor.starts_with("http://")
        || raw_target_actor.starts_with("https://")
        || raw_target_actor.starts_with("acct:")
        || raw_target_actor.contains('@')
    {
        Some(crate::federation::follow::resolve_actor_reference(raw_target_actor).await?)
    } else {
        None
    };
    let is_remote_invite = resolved_remote_actor.is_some();

    if let Some(target_actor) = resolved_remote_actor {
        if same_actor_url(&target_actor, &local_actor) {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "You are already a member of this room"})),
            ));
        }

        // 远程成员：fetch actor + 添加记录
        let remote = crate::federation::actor::fetch_remote_actor(db, &target_actor)
            .await
            .map_err(|e| {
                tracing::error!("[Room] Failed to fetch remote actor: {}", e);
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("Cannot resolve actor: {}", e)})),
                )
            })?;

        // Remote invitee stays *pending* until they accept (no RoomJoin fan-out yet).
        let insert_result = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_room_members
	               (room_id, actor_url, is_local, role, invited_by, joined_at, membership_status)
               VALUES ($1, $2, false, $3, $4, NOW(), 'pending')
               ON CONFLICT (room_id, actor_url) DO NOTHING"#,
                [
                    room_id.into(),
                    remote.actor_url.clone().into(),
                    role.into(),
                    local_actor.clone().into(),
                ],
            ))
            .await
            .map_err(db_err)?;

        if insert_result.rows_affected() == 0 {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "Actor is already a room member"})),
            ));
        }

        // Snapshot *active* members so the invitee can seed federation_room_members
        // (especially the inviter/owner). Without this, remote rejects RoomMessage.
        let member_rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT actor_url, role FROM federation_room_members
                   WHERE room_id = $1 AND COALESCE(membership_status, 'active') = 'active'"#,
                [room_id.into()],
            ))
            .await
            .map_err(db_err)?;
        let members_json: Vec<serde_json::Value> = member_rows
            .iter()
            .map(|r| {
                json!({
                    "actor": r.try_get::<String>("", "actor_url").unwrap_or_default(),
                    "role": r.try_get::<String>("", "role").unwrap_or_else(|_| "member".into()),
                })
            })
            .collect();

        // Re-read room name immediately before building the Activity so invites
        // always carry a non-empty display name (never blank / null).
        let fresh_name_row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT name FROM federation_rooms WHERE room_id = $1",
                [room_id.into()],
            ))
            .await
            .map_err(db_err)?;
        let invite_room_name = fresh_name_row
            .and_then(|r| r.try_get::<String>("", "name").ok())
            .and_then(|n| non_empty_room_name(Some(&n)))
            .or_else(|| non_empty_room_name(Some(&room_name)));
        let invite_room_name = match invite_room_name {
            Some(n) => n,
            None => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "Room name is missing; cannot send invite with empty name"
                    })),
                ));
            }
        };

        // 发送 RoomInvite Activity 给远程方
        let activity_id = generate_activity_id(&base_url);
        let invite_activity = json!({
            "@context": build_context(),
            "type": "myriad:RoomInvite",
            "id": &activity_id,
            "actor": &local_actor,
            "to": [&remote.actor_url],
            "object": {
                "type": "myriad:Room",
                "id": room_id,
                "name": &invite_room_name,
                "owner": &owner_actor,
                "role": role,
                "members": members_json
            }
        });

        let inbox = &remote.inbox_url;
        if !inbox.is_empty() {
            let domain = extract_domain(inbox).unwrap_or_default();
            let act_row = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_activities
                       (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                       VALUES ($1, $2, 'RoomInvite', 'Room', $3, true, NOW())
                       RETURNING id"#,
                    [activity_id.into(), user_id.into(), invite_activity.into()],
                ))
                .await
                .map_err(db_err)?;

            if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
                let _ = db
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"INSERT INTO federation_delivery_queue
                           (activity_id, target_inbox, target_domain, status, created_at)
                           VALUES ($1, $2, $3, 'pending', NOW())"#,
                        [act_id.into(), inbox.into(), domain.into()],
                    ))
                    .await;
            }
        }

        // Do NOT fan-out RoomJoin until the invitee accepts (pending handshake).
        // Local WS: show pending invite on inviter's open clients.
        crate::federation::ws_gateway::broadcast_to_room(
            room_id,
            &json!({
                "type": "system",
                "room_id": room_id,
                "event": "member_invited",
                "actor": &remote.actor_url,
                "role": role,
                "membership_status": "pending"
            }),
        )
        .await;

        tracing::info!(
            "[Room] Invited remote {} to room {} as {} (pending accept)",
            remote.actor_url,
            room_id,
            role
        );
    } else {
        // 本地成员 — 解析用户名 → actor_url
        let local_target_actor = actor_url(&base_url, raw_target_actor);
        if same_actor_url(&local_target_actor, &local_actor) {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "You are already a member of this room"})),
            ));
        }

        // 查找本地用户 ID
        let local_row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT id FROM users WHERE username = $1",
                [raw_target_actor.into()],
            ))
            .await
            .map_err(db_err)?;

        let target_user_id: Option<i32> = local_row.and_then(|r| r.try_get("", "id").ok());
        let target_user_id = target_user_id.ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Local user not found. Use Actor URL or @user@domain for remote users"})),
            )
        })?;

        // Same-instance invite: auto-join as *active* (no accept hop).
        let insert_result = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_room_members
	               (room_id, actor_url, is_local, local_user_id, role, invited_by, joined_at, membership_status)
               VALUES ($1, $2, true, $3, $4, $5, NOW(), 'active')
               ON CONFLICT (room_id, actor_url) DO NOTHING"#,
                [
                    room_id.into(),
                    local_target_actor.clone().into(),
                    target_user_id.into(),
                    role.into(),
                    local_actor.clone().into(),
                ],
            ))
            .await
            .map_err(db_err)?;

        if insert_result.rows_affected() == 0 {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "Actor is already a room member"})),
            ));
        }

        // Tell remote members about this same-instance join (3+ federated roster)
        let join_activity_id = generate_activity_id(&base_url);
        let join_activity = json!({
            "@context": build_context(),
            "type": "myriad:RoomJoin",
            "id": &join_activity_id,
            "actor": &local_actor,
            "object": {
                "type": "myriad:Room",
                "id": room_id,
                "member": &local_target_actor,
                "role": role
            }
        });
        if let Err(e) = fanout_to_remote_members(
            db,
            user_id,
            room_id,
            &join_activity_id,
            &join_activity,
            "RoomJoin",
            "Room",
        )
        .await
        {
            tracing::warn!(
                "[Room] local-invite RoomJoin fanout failed for {}: {}",
                room_id,
                e
            );
        }

        crate::federation::ws_gateway::broadcast_to_room(
            room_id,
            &json!({
                "type": "system",
                "room_id": room_id,
                "event": "member_joined",
                "actor": &local_target_actor,
                "role": role,
                "membership_status": "active"
            }),
        )
        .await;

        tracing::info!(
            "[Room] Invited local {} to room {} as {} (active)",
            raw_target_actor,
            room_id,
            role
        );
    }

    // 广播系统消息（远程路径已发 member_invited；本地再发一次无害）
    let system_msg = json!({
        "type": "system",
        "room_id": room_id,
        "event": "member_invited",
        "actor": &req.actor,
        "role": role,
        "invited_by": &local_actor
    });
    crate::federation::ws_gateway::broadcast_to_room(room_id, &system_msg).await;

    Ok(json!({
        "success": true,
        "room_id": room_id,
        "invited": &req.actor,
        "role": role,
        "membership_status": if is_remote_invite { "pending" } else { "active" }
    }))
}

/// Self-join a room when `invite_policy = open` (or public open rooms).
pub async fn join_room(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    // Already active?
    if let Ok(Some((role, status))) = get_membership(db, room_id, &local_actor).await {
        if status == "active" {
            return Ok(json!({
                "success": true,
                "room_id": room_id,
                "membership_status": "active",
                "role": role,
                "already_member": true
            }));
        }
        if status == "pending" {
            // Pending invite: accept instead
            return accept_room_invite(user_id, username, room_id, db).await;
        }
    }

    let room_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT invite_policy, max_members, is_public, name
               FROM federation_rooms WHERE room_id = $1"#,
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Room not found"})),
            )
        })?;

    let invite_policy: String = room_row.try_get("", "invite_policy").unwrap_or_default();
    let is_public: bool = room_row.try_get("", "is_public").unwrap_or(false);
    let max_members: i32 = room_row.try_get("", "max_members").unwrap_or(50);

    // Self-join without invite: open policy OR public rooms (join by room id).
    if invite_policy != "open" && !is_public {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "This room requires an invite",
                "invite_policy": invite_policy,
                "is_public": is_public
            })),
        ));
    }

    let count_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT COUNT(*)::int AS cnt FROM federation_room_members
               WHERE room_id = $1 AND COALESCE(membership_status, 'active') = 'active'"#,
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?;
    let current: i32 = count_row
        .and_then(|r| r.try_get::<i32>("", "cnt").ok())
        .unwrap_or(0);
    if current >= max_members {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Room is full"})),
        ));
    }

    let insert_result = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_room_members
               (room_id, actor_url, is_local, local_user_id, role, joined_at, membership_status)
               VALUES ($1, $2, true, $3, 'member', NOW(), 'active')
               ON CONFLICT (room_id, actor_url) DO UPDATE SET
                   membership_status = 'active',
                   is_local = true,
                   local_user_id = EXCLUDED.local_user_id,
                   joined_at = COALESCE(federation_room_members.joined_at, NOW())"#,
            [
                room_id.into(),
                local_actor.clone().into(),
                user_id.into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    if insert_result.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "Could not join room"})),
        ));
    }

    let join_activity_id = generate_activity_id(&base_url);
    let join_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomJoin",
        "id": &join_activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:Room",
            "id": room_id,
            "member": &local_actor,
            "role": "member"
        }
    });
    if let Err(e) = fanout_to_remote_members(
        db,
        user_id,
        room_id,
        &join_activity_id,
        &join_activity,
        "RoomJoin",
        "Room",
    )
    .await
    {
        tracing::warn!("[Room] open-join RoomJoin fanout failed for {}: {}", room_id, e);
    }

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "system",
            "room_id": room_id,
            "event": "member_joined",
            "actor": &local_actor,
            "role": "member",
            "membership_status": "active"
        }),
    )
    .await;

    tracing::info!(
        "[Room] {} self-joined open room {}",
        local_actor,
        room_id
    );

    Ok(json!({
        "success": true,
        "room_id": room_id,
        "membership_status": "active",
        "role": "member"
    }))
}

/// Accept a pending room invite (local user → active + RoomJoin fan-out).
pub async fn accept_room_invite(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let membership = get_membership(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "No membership for this room"})),
            )
        })?;
    let (role, status) = membership;
    if status == "active" {
        return Ok(json!({
            "success": true,
            "room_id": room_id,
            "membership_status": "active",
            "role": role,
            "already_active": true
        }));
    }
    if status != "pending" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Cannot accept invite in status {}", status)})),
        ));
    }

    // Activate local membership
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_room_members
           SET membership_status = 'active', joined_at = NOW()
           WHERE room_id = $1 AND actor_url = $2"#,
        [room_id.into(), local_actor.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    // Announce join to all *active* remote peers (inviter + others)
    let join_activity_id = generate_activity_id(&base_url);
    let join_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomJoin",
        "id": &join_activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:Room",
            "id": room_id,
            "member": &local_actor,
            "role": &role
        }
    });
    if let Err(e) = fanout_to_remote_members(
        db,
        user_id,
        room_id,
        &join_activity_id,
        &join_activity,
        "RoomJoin",
        "Room",
    )
    .await
    {
        tracing::warn!(
            "[Room] accept RoomJoin fanout failed for {}: {}",
            room_id,
            e
        );
    }

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "system",
            "room_id": room_id,
            "event": "member_joined",
            "actor": &local_actor,
            "role": &role,
            "membership_status": "active"
        }),
    )
    .await;

    // Dismiss pending invite notification
    let notif_id =
        crate::federation::notify::room_invite_notification_id(room_id, user_id);
    crate::federation::notify::mark_invite_notification_read(user_id, &notif_id).await;

    tracing::info!(
        "[Room] {} accepted invite to room {} as {}",
        local_actor,
        room_id,
        role
    );

    Ok(json!({
        "success": true,
        "room_id": room_id,
        "membership_status": "active",
        "role": role
    }))
}

/// Reject a pending room invite (delete membership + notify inviter).
pub async fn reject_room_invite(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT role, invited_by, COALESCE(membership_status, 'active') AS membership_status
               FROM federation_room_members
               WHERE room_id = $1 AND actor_url = $2 AND is_local = true AND local_user_id = $3"#,
            [room_id.into(), local_actor.clone().into(), user_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "No local membership for this room"})),
            )
        })?;

    let status: String = row
        .try_get("", "membership_status")
        .unwrap_or_else(|_| "active".into());
    if status != "pending" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Only pending invites can be rejected"})),
        ));
    }
    let invited_by: Option<String> = row.try_get("", "invited_by").ok().flatten();
    let role: String = row.try_get("", "role").unwrap_or_else(|_| "member".into());

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"DELETE FROM federation_room_members
           WHERE room_id = $1 AND actor_url = $2 AND COALESCE(membership_status, 'active') = 'pending'"#,
        [room_id.into(), local_actor.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    // Notify inviter (if remote) via standard AP Reject
    if let Some(ref inviter) = invited_by {
        if !same_actor_url(inviter, &local_actor) {
            if let Ok(remote) = crate::federation::actor::fetch_remote_actor(db, inviter).await {
                let inbox = remote.inbox_url.clone();
                if !inbox.is_empty() {
                    let activity_id = generate_activity_id(&base_url);
                    let reject_activity = json!({
                        "@context": build_context(),
                        "type": "Reject",
                        "id": &activity_id,
                        "actor": &local_actor,
                        "to": [inviter],
                        "object": {
                            "type": "myriad:RoomInvite",
                            "id": room_id,
                            "room": room_id,
                            "role": &role
                        }
                    });
                    let domain = extract_domain(&inbox).unwrap_or_default();
                    if let Ok(Some(act_row)) = db
                        .query_one(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"INSERT INTO federation_activities
                               (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                               VALUES ($1, $2, 'Reject', 'RoomInvite', $3, true, NOW())
                               RETURNING id"#,
                            [
                                activity_id.into(),
                                user_id.into(),
                                reject_activity.into(),
                            ],
                        ))
                        .await
                    {
                        if let Ok(act_id) = act_row.try_get::<i32>("", "id") {
                            let _ = db
                                .execute(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    r#"INSERT INTO federation_delivery_queue
                                       (activity_id, target_inbox, target_domain, status, created_at)
                                       VALUES ($1, $2, $3, 'pending', NOW())"#,
                                    [act_id.into(), inbox.into(), domain.into()],
                                ))
                                .await;
                        }
                    }
                }
            }
        }
    }

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "system",
            "room_id": room_id,
            "event": "member_removed",
            "actor": &local_actor,
            "reason": "invite_rejected"
        }),
    )
    .await;

    let notif_id =
        crate::federation::notify::room_invite_notification_id(room_id, user_id);
    crate::federation::notify::mark_invite_notification_read(user_id, &notif_id).await;

    tracing::info!(
        "[Room] {} rejected invite to room {}",
        local_actor,
        room_id
    );

    Ok(json!({
        "success": true,
        "room_id": room_id,
        "membership_status": "rejected"
    }))
}

/// 移除成员
pub async fn remove_member(
    user_id: i32,
    username: &str,
    room_id: &str,
    target_actor: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    // 验证权限
    let my_role = get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Not a member"})),
            )
        })?;

    if !is_admin_role(&my_role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Only admins can remove members"})),
        ));
    }

    // 不能移除 owner
    let target_role = get_member_role(db, room_id, target_actor)
        .await
        .map_err(db_err)?;

    if target_role.as_deref() == Some("owner") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot remove the room owner"})),
        ));
    }

    let _ = user_id; // validated via my_role check

    // Fan-out kick before local delete so remotes still have membership for delivery targets
    // (fanout uses remaining remote members; kicked target is still in the list here —
    // we send RoomLeave with object.member so remotes remove the target, not the admin).
    let activity_id = generate_activity_id(&base_url);
    let leave_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomLeave",
        "id": &activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:Room",
            "id": room_id,
            "member": target_actor,
            "removedBy": &local_actor
        }
    });
    if let Err(e) = fanout_to_remote_members(
        db,
        user_id,
        room_id,
        &activity_id,
        &leave_activity,
        "RoomLeave",
        "Room",
    )
    .await
    {
        tracing::warn!("[Room] kick fanout failed for {}: {}", room_id, e);
    }

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_room_members WHERE room_id = $1 AND actor_url = $2",
        [room_id.into(), target_actor.into()],
    ))
    .await
    .map_err(db_err)?;

    // 广播系统消息
    let system_msg = json!({
        "type": "system",
        "room_id": room_id,
        "event": "member_removed",
        "actor": target_actor,
        "removed_by": &local_actor
    });
    crate::federation::ws_gateway::broadcast_to_room(room_id, &system_msg).await;

    tracing::info!("[Room] Removed {} from room {}", target_actor, room_id);

    Ok(json!({ "success": true, "room_id": room_id, "removed": target_actor }))
}

/// Transfer room ownership to another member (local or remote actor URL).
pub async fn transfer_room_ownership(
    user_id: i32,
    username: &str,
    room_id: &str,
    new_owner_actor: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    let new_owner = new_owner_actor.trim();
    if new_owner.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "new_owner is required"})),
        ));
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT owner_actor FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Room not found"})),
            )
        })?;
    let owner_actor: String = row.try_get("", "owner_actor").unwrap_or_default();
    if !same_actor_url(&owner_actor, &local_actor) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Only the room owner can transfer ownership"})),
        ));
    }
    if same_actor_url(&owner_actor, new_owner) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Already the owner"})),
        ));
    }

    // Resolve target: full actor URL or local username
    let target_actor = if new_owner.starts_with("http://")
        || new_owner.starts_with("https://")
        || new_owner.starts_with("acct:")
        || new_owner.contains('@')
    {
        crate::federation::follow::resolve_actor_reference(new_owner).await?
    } else {
        actor_url(&base_url, new_owner)
    };

    let target_role = get_member_role(db, room_id, &target_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "New owner must already be a room member"})),
            )
        })?;
    if target_role == "observer" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot transfer ownership to an observer"})),
        ));
    }

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_rooms SET owner_actor = $2, updated_at = NOW() WHERE room_id = $1",
        [room_id.into(), target_actor.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_room_members
               SET role = 'admin'
               WHERE room_id = $1 AND actor_url = $2 AND role = 'owner'"#,
            [room_id.into(), owner_actor.clone().into()],
        ))
        .await;
    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_room_members
               SET role = 'owner'
               WHERE room_id = $1 AND actor_url = $2"#,
            [room_id.into(), target_actor.clone().into()],
        ))
        .await;

    let changes = json!({ "transfer_owner": &target_actor });
    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "system",
            "room_id": room_id,
            "event": "governance_changed",
            "actor": &local_actor,
            "changes": &changes
        }),
    )
    .await;

    let activity_id = generate_activity_id(&base_url);
    let gov_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomGovernance",
        "id": &activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:RoomGovernance",
            "room": room_id,
            "changes": changes
        }
    });
    if let Err(e) = fanout_to_remote_members(
        db,
        user_id,
        room_id,
        &activity_id,
        &gov_activity,
        "RoomGovernance",
        "RoomGovernance",
    )
    .await
    {
        tracing::warn!("[Room] ownership transfer fanout failed: {}", e);
    }

    tracing::info!(
        "[Room] ownership of {} transferred {} → {}",
        room_id,
        owner_actor,
        target_actor
    );

    Ok(json!({
        "success": true,
        "room_id": room_id,
        "previous_owner": owner_actor,
        "new_owner": target_actor
    }))
}

/// 离开 Room
pub async fn leave_room(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    // Pending invite → reject path (no RoomLeave spam to peers who never saw us)
    if let Ok(Some((_, status))) = get_membership(db, room_id, &local_actor).await {
        if status == "pending" {
            return reject_room_invite(user_id, username, room_id, db).await;
        }
    }

    let my_role = get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Not a member"})),
            )
        })?;

    if my_role == "owner" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Owner cannot leave. Transfer ownership or delete the room."})),
        ));
    }

    let _ = user_id;

    // Fan-out leave while we still appear as a member (delivery targets = other remotes)
    let activity_id = generate_activity_id(&base_url);
    let leave_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomLeave",
        "id": &activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:Room",
            "id": room_id
        }
    });
    if let Err(e) = fanout_to_remote_members(
        db,
        user_id,
        room_id,
        &activity_id,
        &leave_activity,
        "RoomLeave",
        "Room",
    )
    .await
    {
        tracing::warn!("[Room] leave fanout failed for {}: {}", room_id, e);
    }

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_room_members WHERE room_id = $1 AND actor_url = $2",
        [room_id.into(), local_actor.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    // 广播
    let system_msg = json!({
        "type": "system",
        "room_id": room_id,
        "event": "member_left",
        "actor": &local_actor
    });
    crate::federation::ws_gateway::broadcast_to_room(room_id, &system_msg).await;

    tracing::info!("[Room] {} left room {}", username, room_id);

    Ok(json!({ "success": true, "room_id": room_id }))
}

// ==================== 消息功能 ====================

/// 最大消息载荷大小: 32 MiB（与 channel MAX_MESSAGE_PAYLOAD 对齐；Tapp 包分享）
const MAX_ROOM_MESSAGE_PAYLOAD: usize = 32 * 1024 * 1024;

/// 发送 Room 消息
pub async fn send_room_message(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
    req: &SendRoomMessageRequest,
) -> Result<SendRoomMessageResponse, (StatusCode, Json<serde_json::Value>)> {
    // 验证载荷大小
    let payload_size = req.payload.to_string().len();
    if payload_size > MAX_ROOM_MESSAGE_PAYLOAD {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(
                json!({"error": format!("Message payload too large: {} bytes (max {})", payload_size, MAX_ROOM_MESSAGE_PAYLOAD)}),
            ),
        ));
    }

    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    let message_type = req.message_type.as_deref().unwrap_or("text");
    let want_encrypt = req.encrypt.unwrap_or(false);

    // 验证成员身份
    let my_role = get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Not a member"})),
            )
        })?;

    // observer 不能发消息
    if my_role == "observer" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Observers cannot send messages"})),
        ));
    }

    // E2E 未就绪时降级明文，避免 Aro 默认 encrypt=true 导致发消息 400
    let (stored_payload, is_encrypted) = if want_encrypt {
        let recipients = match collect_room_e2e_recipients(db, room_id, &local_actor).await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(room_id = %room_id, error = %e, "E2E recipients unavailable; plaintext");
                Vec::new()
            }
        };
        if recipients.is_empty() {
            tracing::debug!(room_id = %room_id, "No peer E2E keys yet; sending plaintext");
            (req.payload.clone(), false)
        } else {
        // 也给自己 wrap 一份，便于本端历史解密
        let mut all = recipients;
        if let Ok((my_pk, _)) = load_member_e2e_keys(db, room_id, &local_actor).await {
            if !all.iter().any(|(_, pk)| pk == &my_pk) {
                all.push((local_actor.clone(), my_pk));
            }
        }
        match crate::federation::e2e::encrypt_json_for_recipients(
            &req.payload,
            room_id.as_bytes(),
            &all,
        ) {
            Ok(encrypted) => (encrypted, true),
            Err(e) => {
                tracing::warn!(room_id = %room_id, error = %e, "E2E encrypt failed; plaintext");
                (req.payload.clone(), false)
            }
        }
        }
    } else {
        (req.payload.clone(), false)
    };

    let message_id = generate_message_id();

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_room_messages
           (room_id, message_id, sender_actor, message_type, payload, thread_id, reply_to,
            reactions, is_pinned, is_encrypted, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, '{}', false, $8, NOW())"#,
        [
            room_id.into(),
            message_id.clone().into(),
            local_actor.clone().into(),
            message_type.into(),
            stored_payload.clone().into(),
            req.thread_id.clone().into(),
            req.reply_to.clone().into(),
            is_encrypted.into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    // 广播给 WebSocket 连接
    let ws_msg = json!({
        "type": "message",
        "room_id": room_id,
        "message": {
            "message_id": &message_id,
            "sender_actor": &local_actor,
            "message_type": message_type,
            "payload": &stored_payload,
            "is_encrypted": is_encrypted,
            "thread_id": &req.thread_id,
            "reply_to": &req.reply_to,
            "created_at": now_iso8601()
        }
    });
    crate::federation::ws_gateway::broadcast_to_room(room_id, &ws_msg).await;

    // Fan-out 到远程成员
    let activity_id = generate_activity_id(&base_url);
    let msg_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomMessage",
        "id": &activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:RoomMessage",
            "room": room_id,
            "messageId": &message_id,
            "messageType": message_type,
            "from": &local_actor,
            "payload": &stored_payload,
            "isEncrypted": is_encrypted,
            "threadId": &req.thread_id,
            "replyTo": &req.reply_to,
            "timestamp": now_iso8601()
        }
    });

    let fanout = match fanout_to_remote_members(
        db,
        user_id,
        room_id,
        &activity_id,
        &msg_activity,
        "RoomMessage",
        "RoomMessage",
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("[Room] fanout error room={}: {}", room_id, e);
            crate::federation::delivery::FanoutResult::default()
        }
    };
    let delivery = fanout.to_enqueue_info();

    Ok(SendRoomMessageResponse {
        success: true,
        message_id,
        room_id: room_id.to_string(),
        is_encrypted,
        delivery: Some(delivery),
    })
}

/// 获取 Room 消息历史
pub async fn get_room_messages(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
    before: Option<&str>,
    limit: Option<i64>,
) -> Result<Vec<RoomMessageItem>, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    // 验证成员身份
    let is_member = get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?;
    if is_member.is_none() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Not a member"})),
        ));
    }

    let _ = user_id;
    let limit = limit.unwrap_or(50).min(200);
    let my_keys = load_member_e2e_keys(db, room_id, &local_actor).await.ok();

    let rows = if let Some(before_id) = before {
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT message_id, sender_actor, message_type, payload, thread_id, reply_to,
                      reactions, is_pinned, is_encrypted, created_at
               FROM federation_room_messages
               WHERE room_id = $1
                 AND created_at < (SELECT created_at FROM federation_room_messages WHERE message_id = $2)
               ORDER BY created_at DESC
               LIMIT $3"#,
            [room_id.into(), before_id.into(), limit.into()],
        ))
        .await
        .map_err(db_err)?
    } else {
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT message_id, sender_actor, message_type, payload, thread_id, reply_to,
                      reactions, is_pinned, is_encrypted, created_at
               FROM federation_room_messages
               WHERE room_id = $1
               ORDER BY created_at DESC
               LIMIT $2"#,
            [room_id.into(), limit.into()],
        ))
        .await
        .map_err(db_err)?
    };

    let mut messages: Vec<RoomMessageItem> = Vec::with_capacity(rows.len());
    for r in rows {
        let is_encrypted: bool = r.try_get("", "is_encrypted").unwrap_or(false);
        let mut payload: serde_json::Value = r.try_get("", "payload").unwrap_or(json!(null));
        if is_encrypted {
            if let Some((pk, sk)) = my_keys.as_ref() {
                if let Ok(plain) = crate::federation::e2e::decrypt_json_for_recipient(
                    &payload,
                    sk,
                    pk,
                    room_id.as_bytes(),
                ) {
                    payload = plain;
                }
            }
        }
        messages.push(RoomMessageItem {
            message_id: r.try_get("", "message_id").unwrap_or_default(),
            sender_actor: r.try_get("", "sender_actor").unwrap_or_default(),
            message_type: r.try_get("", "message_type").unwrap_or_default(),
            payload,
            thread_id: r.try_get::<Option<String>>("", "thread_id").unwrap_or(None),
            reply_to: r.try_get::<Option<String>>("", "reply_to").unwrap_or(None),
            reactions: r.try_get("", "reactions").unwrap_or(json!({})),
            is_pinned: r.try_get::<bool>("", "is_pinned").unwrap_or(false),
            is_encrypted,
            created_at: r
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        });
    }

    messages.reverse();
    Ok(messages)
}

/// Classify attachment kind from message_type + payload (mirrors Aro client).
fn room_file_kind(message_type: &str, payload: &serde_json::Value) -> Option<&'static str> {
    let mut mt = message_type;
    if mt.is_empty() || mt == "text" {
        if payload.get("transfer_id").and_then(|v| v.as_str()).is_some()
            && payload.get("filename").and_then(|v| v.as_str()).is_some()
        {
            mt = "file-meta";
        } else if payload
            .get("mime_type")
            .and_then(|v| v.as_str())
            .map(|m| m.starts_with("image/"))
            .unwrap_or(false)
            && payload.get("data").is_some()
        {
            mt = "image";
        } else if payload.get("data").is_some()
            && payload.get("filename").and_then(|v| v.as_str()).is_some()
        {
            mt = "file";
        }
    }
    match mt {
        "image" => Some("image"),
        "file" | "file-meta" => Some("file"),
        _ => None,
    }
}

fn room_file_status(has_inline: bool, transfer_status: Option<&str>, has_transfer_id: bool) -> String {
    if has_inline {
        return "ready".into();
    }
    match transfer_status {
        Some("completed") => "ready".into(),
        Some("pending") | Some("transferring") | Some("in-progress") => "pending".into(),
        Some("failed") | Some("cancelled") => "missing".into(),
        _ if has_transfer_id => "ready".into(),
        _ => "missing".into(),
    }
}

/// 群文件索引：从 room 消息（image/file/file-meta）+ 本机 transfers 聚合。
/// 不返回 payload.data 字节；客户端下载走 transfer 或聊天窗口内联。
///
/// Query params (caller):
/// - `before`: message_id cursor (older than that message's created_at)
/// - `limit`: page size (default 50, max 200)
/// - `filter`: all | image | file
/// - `q`: case-insensitive filename substring
#[allow(clippy::too_many_arguments)]
pub async fn list_room_files(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
    before: Option<&str>,
    limit: Option<i64>,
    filter: Option<&str>,
    q: Option<&str>,
) -> Result<RoomFileListResult, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    if get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .is_none()
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Not a member"})),
        ));
    }
    let _ = user_id;

    let limit = limit.unwrap_or(50).clamp(1, 200);
    let filter = filter.unwrap_or("all").to_ascii_lowercase();
    let q_norm = q
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let my_keys = load_member_e2e_keys(db, room_id, &local_actor).await.ok();

    // Local transfer status map for this room
    let transfer_rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT transfer_id, filename, file_size, mime_type, status, created_at
               FROM federation_file_transfers
               WHERE room_id = $1"#,
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let mut transfer_map: std::collections::HashMap<String, (String, String, i64, Option<String>, String)> =
        std::collections::HashMap::new();
    // transfer_id -> (status, filename, file_size, mime, created_at)
    for r in &transfer_rows {
        let tid: String = r.try_get("", "transfer_id").unwrap_or_default();
        if tid.is_empty() {
            continue;
        }
        transfer_map.insert(
            tid,
            (
                r.try_get("", "status").unwrap_or_else(|_| "pending".into()),
                r.try_get("", "filename").unwrap_or_else(|_| "file".into()),
                r.try_get("", "file_size").unwrap_or(0),
                r.try_get::<Option<String>>("", "mime_type").unwrap_or(None),
                r.try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
            ),
        );
    }

    // Fetch a bit more than limit so client-side filter still fills a page when possible
    let fetch_limit = if q_norm.is_some() || (filter != "all") {
        (limit * 3).min(200)
    } else {
        limit
    };

    // payload column is json (not jsonb); cast for containment / ->> operators.
    let rows = if let Some(before_id) = before {
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT message_id, sender_actor, message_type, payload, is_encrypted, created_at
               FROM federation_room_messages
               WHERE room_id = $1
                 AND created_at < (SELECT created_at FROM federation_room_messages WHERE message_id = $2)
                 AND (
                   message_type IN ('image', 'file', 'file-meta')
                   OR (
                     message_type IN ('text', '')
                     AND (
                       (payload::jsonb) ? 'transfer_id'
                       OR ((payload::jsonb) ? 'data' AND (payload::jsonb) ? 'filename')
                       OR ((payload::jsonb) ? 'data'
                           AND COALESCE(payload::jsonb->>'mime_type','') LIKE 'image/%')
                     )
                   )
                 )
               ORDER BY created_at DESC
               LIMIT $3"#,
            [room_id.into(), before_id.into(), fetch_limit.into()],
        ))
        .await
        .map_err(db_err)?
    } else {
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT message_id, sender_actor, message_type, payload, is_encrypted, created_at
               FROM federation_room_messages
               WHERE room_id = $1
                 AND (
                   message_type IN ('image', 'file', 'file-meta')
                   OR (
                     message_type IN ('text', '')
                     AND (
                       (payload::jsonb) ? 'transfer_id'
                       OR ((payload::jsonb) ? 'data' AND (payload::jsonb) ? 'filename')
                       OR ((payload::jsonb) ? 'data'
                           AND COALESCE(payload::jsonb->>'mime_type','') LIKE 'image/%')
                     )
                   )
                 )
               ORDER BY created_at DESC
               LIMIT $2"#,
            [room_id.into(), fetch_limit.into()],
        ))
        .await
        .map_err(db_err)?
    };

    let raw_count = rows.len() as i64;
    let mut known_transfer_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut files: Vec<RoomFileItem> = Vec::with_capacity(rows.len());

    for r in rows {
        let is_encrypted: bool = r.try_get("", "is_encrypted").unwrap_or(false);
        let mut payload: serde_json::Value = r.try_get("", "payload").unwrap_or(json!({}));
        if is_encrypted {
            if let Some((pk, sk)) = my_keys.as_ref() {
                if let Ok(plain) = crate::federation::e2e::decrypt_json_for_recipient(
                    &payload,
                    sk,
                    pk,
                    room_id.as_bytes(),
                ) {
                    payload = plain;
                } else {
                    // Cannot index ciphertext body
                    continue;
                }
            } else {
                continue;
            }
        }
        if !payload.is_object() {
            payload = json!({});
        }

        let message_type: String = r.try_get("", "message_type").unwrap_or_default();
        let kind = match room_file_kind(&message_type, &payload) {
            Some(k) => k,
            None => continue,
        };
        if filter == "image" && kind != "image" {
            continue;
        }
        if filter == "file" && kind != "file" {
            continue;
        }

        let transfer_id = payload
            .get("transfer_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let has_inline = payload.get("data").map(|d| !d.is_null()).unwrap_or(false)
            && payload
                .get("data")
                .and_then(|d| d.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(true); // non-string data still counts as present

        let tr = transfer_id
            .as_ref()
            .and_then(|id| transfer_map.get(id));
        let filename = payload
            .get("filename")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| tr.map(|t| t.1.clone()))
            .unwrap_or_else(|| if kind == "image" { "image".into() } else { "file".into() });

        if let Some(ref qn) = q_norm {
            if !filename.to_lowercase().contains(qn) {
                continue;
            }
        }

        let size = payload
            .get("size")
            .and_then(|v| v.as_i64())
            .or_else(|| payload.get("size").and_then(|v| v.as_u64()).map(|u| u as i64))
            .or_else(|| tr.map(|t| t.2))
            .unwrap_or(0);
        let mime_type = payload
            .get("mime_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| tr.and_then(|t| t.3.clone()));
        let status = room_file_status(
            has_inline,
            tr.map(|t| t.0.as_str()),
            transfer_id.is_some(),
        );
        let message_id: String = r.try_get("", "message_id").unwrap_or_default();
        let sender_actor: String = r.try_get("", "sender_actor").unwrap_or_default();
        let created_at = r
            .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
            .map(|t| t.to_rfc3339())
            .unwrap_or_default();

        if let Some(ref tid) = transfer_id {
            known_transfer_ids.insert(tid.clone());
        }

        let key = format!(
            "{}:{}",
            message_id,
            transfer_id.as_deref().unwrap_or(&filename)
        );
        files.push(RoomFileItem {
            key,
            message_id,
            kind: kind.into(),
            filename,
            size,
            mime_type,
            sender_actor,
            created_at,
            transfer_id,
            has_inline,
            status,
        });

        if files.len() as i64 >= limit {
            break;
        }
    }

    // First page only: orphan transfers not yet linked by a file-meta message
    if before.is_none() {
        let mut orphans: Vec<RoomFileItem> = Vec::new();
        for (tid, (status, filename, size, mime, created_at)) in &transfer_map {
            if known_transfer_ids.contains(tid) {
                continue;
            }
            if !matches!(
                status.as_str(),
                "completed" | "pending" | "transferring" | "in-progress"
            ) {
                continue;
            }
            if filter == "image" {
                continue; // orphans are always file kind
            }
            if let Some(ref qn) = q_norm {
                if !filename.to_lowercase().contains(qn) {
                    continue;
                }
            }
            orphans.push(RoomFileItem {
                key: format!("tr:{}", tid),
                message_id: String::new(),
                kind: "file".into(),
                filename: filename.clone(),
                size: *size,
                mime_type: mime.clone(),
                sender_actor: String::new(),
                created_at: created_at.clone(),
                transfer_id: Some(tid.clone()),
                has_inline: false,
                status: room_file_status(false, Some(status.as_str()), true),
            });
        }
        orphans.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        // Prepend orphans (newest first merge)
        if !orphans.is_empty() {
            files.extend(orphans);
            files.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            if files.len() as i64 > limit {
                files.truncate(limit as usize);
            }
        }
    }

    let has_more = raw_count >= fetch_limit;
    let total = files.len();
    Ok(RoomFileListResult {
        files,
        total,
        has_more,
    })
}

/// Pin/Unpin Room 消息（owner/admin 可操作）
pub async fn pin_room_message(
    user_id: i32,
    username: &str,
    room_id: &str,
    message_id: &str,
    db: &DatabaseConnection,
    req: &PinRoomMessageRequest,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let my_role = get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Not a member of this room"})),
            )
        })?;

    if !is_admin_role(&my_role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Only owner or admin can pin messages"})),
        ));
    }

    let updated = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_room_messages
               SET is_pinned = $3
               WHERE room_id = $1 AND message_id = $2
               RETURNING message_id"#,
            [room_id.into(), message_id.into(), req.pinned.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Message not found"})),
            )
        })?;

    let pinned_message_id: String = updated.try_get("", "message_id").unwrap_or_default();
    let ws_msg = json!({
        "type": "room_message_pinned",
        "room_id": room_id,
        "message_id": pinned_message_id,
        "is_pinned": req.pinned
    });
    crate::federation::ws_gateway::broadcast_to_room(room_id, &ws_msg).await;

    // Fan-out pin state to remote members (was local-only)
    let activity_id = generate_activity_id(&base_url);
    let pin_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomPin",
        "id": &activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:RoomPin",
            "room": room_id,
            "messageId": message_id,
            "pinned": req.pinned
        }
    });
    if let Err(e) = fanout_to_remote_members(
        db,
        user_id,
        room_id,
        &activity_id,
        &pin_activity,
        "RoomPin",
        "RoomPin",
    )
    .await
    {
        tracing::warn!("[Room] pin fanout failed for {}: {}", room_id, e);
    }

    tracing::info!(
        "[Room] {} set pinned={} for message {} in room {}",
        username,
        req.pinned,
        message_id,
        room_id
    );

    Ok(json!({
        "success": true,
        "room_id": room_id,
        "message_id": message_id,
        "is_pinned": req.pinned
    }))
}

/// Inbound pin/unpin for federated rooms.
pub async fn handle_room_pin(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let room_id = object
        .get("room")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("id").and_then(|v| v.as_str()))
        .ok_or("Missing room id")?;
    let message_id = object
        .get("messageId")
        .and_then(|v| v.as_str())
        .ok_or("Missing messageId")?;
    let pinned = object
        .get("pinned")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // Best-effort: actor should be admin; tolerate roster lag
    let role = get_member_role(db, room_id, actor_url_str)
        .await
        .map_err(|e| e.to_string())?;
    if role.as_deref() != Some("owner") && role.as_deref() != Some("admin") {
        tracing::warn!(
            "[Room] pin from non-admin {} in room {} (applying anyway if message exists)",
            actor_url_str,
            room_id
        );
    }

    let updated = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_room_messages
               SET is_pinned = $3
               WHERE room_id = $1 AND message_id = $2"#,
            [room_id.into(), message_id.into(), pinned.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if updated.rows_affected() == 0 {
        tracing::debug!(
            "[Room] pin for unknown message {} in room {}",
            message_id,
            room_id
        );
        return Ok(());
    }

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "room_message_pinned",
            "room_id": room_id,
            "message_id": message_id,
            "is_pinned": pinned
        }),
    )
    .await;

    tracing::info!(
        "[Room] remote pin message {} in {} pinned={} by {}",
        message_id,
        room_id,
        pinned,
        actor_url_str
    );
    Ok(())
}

// ==================== Inbox 处理（远程 Room 事件）====================

/// 处理远程 RoomInvite
pub async fn handle_room_invite(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let room_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Missing room id")?;
    let role = object
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("member");
    // Blank / whitespace-only names are treated as missing (not as a real name).
    let invite_name = non_empty_room_name(object.get("name").and_then(|v| v.as_str()));
    let owner_actor = object
        .get("owner")
        .and_then(|v| v.as_str())
        .unwrap_or(actor_url_str);

    // 查找本地接收者（从 "to" 字段推断）
    let to = activity.get("to").and_then(|v| v.as_array());
    let local_user_id: Option<i32> = if let Some(targets) = to {
        let mut found_id = None;
        for target in targets {
            if let Some(url) = target.as_str() {
                // 尝试从 /users/xxx 提取用户名并查找
                if let Some(uname) = url.rsplit('/').next() {
                    if let Ok(Some(row)) = db
                        .query_one(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "SELECT id FROM users WHERE username = $1",
                            [uname.into()],
                        ))
                        .await
                    {
                        found_id = row.try_get::<i32>("", "id").ok();
                        break;
                    }
                }
            }
        }
        found_id
    } else {
        None
    };

    let target_user_id: i32 = match local_user_id {
        Some(uid) => uid,
        None => {
            // 个人实例回退：无法从 "to" 解析出收件人时路由到第一个本地用户
            let fallback = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT id FROM users ORDER BY id LIMIT 1",
                    [],
                ))
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "No local users found".to_string())?;
            fallback.try_get("", "id").map_err(|e| e.to_string())?
        }
    };
    let base_url_val = {
        let config = crate::GLOBAL_CONFIG.read().await;
        config
            .base_url
            .clone()
            .unwrap_or_else(|| format!("http://{}:{}", config.server_host, config.server_port))
    };

    // 查找或创建本地用户对应的 actor_url
    let local_actor = if let Ok(Some(row)) = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users WHERE id = $1",
            [target_user_id.into()],
        ))
        .await
    {
        let uname: String = row.try_get("", "username").unwrap_or_default();
        actor_url(&base_url_val, &uname)
    } else {
        actor_url(&base_url_val, "unknown")
    };

    // Ensure Room row exists; if it already exists with empty/fallback name and
    // the invite carries a real name, upgrade it (ON CONFLICT DO NOTHING left
    // invitees stuck on "Room rm_xxxx" forever after a partial insert).
    let home_server = extract_domain(owner_actor)
        .or_else(|| extract_domain(actor_url_str))
        .unwrap_or_default();
    let has_real_name = invite_name.is_some();
    let display_name = resolve_invite_room_name(invite_name.as_deref(), room_id);
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_rooms
           (room_id, name, description, owner_actor, home_server, governance_type, invite_policy,
            max_members, is_public, distribution_strategy, created_at)
           VALUES ($1, $2, NULL, $3, $4, 'owner', 'admin-only', 50, false, 'fan-out', NOW())
           ON CONFLICT (room_id) DO UPDATE SET
             name = CASE
               WHEN $5::boolean
                    AND (
                      federation_rooms.name IS NULL
                      OR btrim(federation_rooms.name) = ''
                      OR federation_rooms.name = ('Room ' || left($1, 8))
                    )
               THEN EXCLUDED.name
               ELSE federation_rooms.name
             END,
             updated_at = CASE
               WHEN $5::boolean
                    AND (
                      federation_rooms.name IS NULL
                      OR btrim(federation_rooms.name) = ''
                      OR federation_rooms.name = ('Room ' || left($1, 8))
                    )
               THEN NOW()
               ELSE federation_rooms.updated_at
             END"#,
        [
            room_id.into(),
            display_name.into(),
            owner_actor.into(),
            home_server.into(),
            has_real_name.into(),
        ],
    ))
    .await
    .map_err(|e| e.to_string())?;

    // Seed remote members BEFORE accepting messages:
    // 1) owner  2) inviter (activity actor)  3) members[] snapshot from invite
    if !same_actor_url(owner_actor, &local_actor) {
        upsert_remote_room_member(db, room_id, owner_actor, "owner", None).await?;
    }
    if !same_actor_url(actor_url_str, &local_actor)
        && !same_actor_url(actor_url_str, owner_actor)
    {
        upsert_remote_room_member(db, room_id, actor_url_str, "admin", None).await?;
    } else if !same_actor_url(actor_url_str, &local_actor) {
        // inviter is owner — already upserted; ensure role stays owner
        upsert_remote_room_member(db, room_id, actor_url_str, "owner", None).await?;
    }

    if let Some(members) = object.get("members").and_then(|v| v.as_array()) {
        for m in members {
            let Some(member_actor) = m
                .get("actor")
                .or_else(|| m.get("actorUrl"))
                .or_else(|| m.get("actor_url"))
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            if same_actor_url(member_actor, &local_actor) {
                continue;
            }
            let member_role = m
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("member");
            upsert_remote_room_member(db, room_id, member_actor, member_role, None).await?;
        }
    }

    // Local invitee is *pending* until they accept (does not auto-join).
    let inserted = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_room_members
           (room_id, actor_url, is_local, local_user_id, role, invited_by, joined_at, membership_status)
           VALUES ($1, $2, true, $3, $4, $5, NOW(), 'pending')
           ON CONFLICT (room_id, actor_url) DO NOTHING"#,
            [
                room_id.into(),
                local_actor.clone().into(),
                target_user_id.into(),
                role.into(),
                actor_url_str.into(),
            ],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if inserted.rows_affected() > 0 {
        let label = crate::federation::notify::actor_label(db, actor_url_str).await;
        let name = crate::federation::notify::room_name(db, room_id).await;
        crate::federation::notify::notify_room_invite(
            target_user_id,
            room_id,
            &name,
            actor_url_str,
            &label,
        )
        .await;
        // Surface pending room in invitee's Aro (list_rooms will include it).
        crate::federation::ws_gateway::broadcast_to_room(
            room_id,
            &json!({
                "type": "system",
                "room_id": room_id,
                "event": "member_invited",
                "actor": &local_actor,
                "role": role,
                "membership_status": "pending",
                "invited_by": actor_url_str
            }),
        )
        .await;
    }

    tracing::info!(
        "[Room] Received invite to room {} from {} (pending local member, seeded remotes)",
        room_id,
        actor_url_str
    );
    Ok(())
}

/// 处理远程 RoomMessage
pub async fn handle_room_message(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let room_id = object
        .get("room")
        .and_then(|v| v.as_str())
        .ok_or("Missing room")?;

    // Prefer signed activity actor; fall back to object.from
    let sender_actor = object
        .get("from")
        .and_then(|v| v.as_str())
        .unwrap_or(actor_url_str);

    // object.from must match signed actor (prevent spoofed from field)
    if !same_actor_url(sender_actor, actor_url_str) {
        return Err(format!(
            "not_member: RoomMessage from {} does not match signed actor {}",
            sender_actor, actor_url_str
        ));
    }

    // Room may still be in-flight (invite race) — ask peer to retry (503), not permanent 404
    let room_exists = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .is_some();
    if !room_exists {
        return Err(format!(
            "Room {room_id} not yet present; retry after RoomInvite"
        ));
    }

    // Membership check + self-heal for invitee missing inviter/owner rows
    ensure_room_message_sender_member(db, room_id, actor_url_str).await?;

    let fallback_msg_id = generate_message_id();
    let message_id = object
        .get("messageId")
        .and_then(|v| v.as_str())
        .unwrap_or(&fallback_msg_id);
    // Prefer signed actor URL for storage consistency
    let sender = actor_url_str;
    let message_type = object
        .get("messageType")
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    let payload = object.get("payload").cloned().unwrap_or(json!(null));
    let thread_id = object.get("threadId").and_then(|v| v.as_str());
    let reply_to = object.get("replyTo").and_then(|v| v.as_str());
    let is_encrypted = object
        .get("isEncrypted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let inserted = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_room_messages
           (room_id, message_id, sender_actor, message_type, payload, thread_id, reply_to,
            reactions, is_pinned, is_encrypted, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, '{}', false, $8, NOW())
           ON CONFLICT (message_id) DO NOTHING"#,
            [
                room_id.into(),
                message_id.into(),
                sender.into(),
                message_type.into(),
                payload.clone().into(),
                thread_id.into(),
                reply_to.into(),
                is_encrypted.into(),
            ],
        ))
        .await
        .map_err(|e| e.to_string())?;

    // 广播到本地 WebSocket
    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "message",
            "room_id": room_id,
            "message": {
                "message_id": message_id,
                "sender_actor": sender,
                "message_type": message_type,
                "payload": payload,
                "is_encrypted": is_encrypted,
                "thread_id": thread_id,
                "reply_to": reply_to,
                "created_at": now_iso8601()
            }
        }),
    )
    .await;

    // 新消息才通知本地成员（排除发送者若其为本地用户）
    if inserted.rows_affected() > 0 {
        let label = crate::federation::notify::actor_label(db, sender).await;
        let name = crate::federation::notify::room_name(db, room_id).await;
        let local_users = crate::federation::notify::room_local_user_ids(db, room_id).await;
        // 若发送者绑定了本地 user，跳过该 user
        let sender_local: Option<i32> = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT local_user_id FROM federation_room_members
                   WHERE room_id = $1 AND actor_url = $2 AND is_local = true"#,
                [room_id.into(), sender.into()],
            ))
            .await
            .ok()
            .flatten()
            .and_then(|r| r.try_get::<i32>("", "local_user_id").ok());
        for user_id in local_users {
            if sender_local == Some(user_id) {
                continue;
            }
            crate::federation::notify::notify_room_message(
                user_id,
                room_id,
                &name,
                sender,
                &label,
                message_type,
                &payload,
            )
            .await;
        }
    }

    tracing::info!("[Room] Received message {} in room {}", message_id, room_id);
    Ok(())
}

/// 处理远程 RoomLeave（自愿离开或被踢）
///
/// - 自愿离开：`actor` 即离开者
/// - 踢人：`object.member` = 被踢者，`actor` = 操作者（admin）
pub async fn handle_room_leave(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let room_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("room").and_then(|v| v.as_str()))
        .ok_or("Missing room id")?;

    let removed = object
        .get("member")
        .and_then(|v| v.as_str())
        .unwrap_or(actor_url_str);
    let is_kick = object.get("member").and_then(|v| v.as_str()).is_some()
        && !same_actor_url(removed, actor_url_str);

    if is_kick {
        // Kicker must be admin/owner on this instance's roster (best-effort)
        let kicker_role = get_member_role(db, room_id, actor_url_str)
            .await
            .map_err(|e| e.to_string())?;
        if kicker_role.as_deref() != Some("owner")
            && kicker_role.as_deref() != Some("admin")
        {
            // Still allow if kicker not known locally (roster lag) — log and proceed
            tracing::warn!(
                "[Room] kick from {} without local admin role in room {}",
                actor_url_str,
                room_id
            );
        }
    }

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_room_members WHERE room_id = $1 AND actor_url = $2",
        [room_id.into(), removed.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;

    let event = if is_kick {
        "member_removed"
    } else {
        "member_left"
    };
    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "system",
            "room_id": room_id,
            "event": event,
            "actor": removed,
            "removed_by": if is_kick { serde_json::Value::String(actor_url_str.to_string()) } else { serde_json::Value::Null }
        }),
    )
    .await;

    tracing::info!(
        "[Room] {} {} room {} ({})",
        removed,
        if is_kick { "removed from" } else { "left" },
        room_id,
        actor_url_str
    );
    Ok(())
}

/// 处理远程 RoomJoin (myriad:RoomJoin)
///
/// - 自报加入：`actor` = 新成员
/// - 名册同步：`object.member` = 新成员，`actor` = 邀请者（3+ 方 roster）
pub async fn handle_room_join(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let room_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("room").and_then(|v| v.as_str()))
        .ok_or("Missing room id")?;
    let role = object
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("member");
    let joining = object
        .get("member")
        .and_then(|v| v.as_str())
        .unwrap_or(actor_url_str);

    // 验证 Room 存在
    let room_exists = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if room_exists.is_none() {
        // Alignment: RoomJoin can race ahead of RoomInvite. Soft-ack used to drop the
        // join forever (202) so inviter never saw accept. Signal retry instead.
        tracing::info!(
            "[Room] RoomJoin for {} from {} (joining={}) — room not yet present; signaling retry",
            room_id,
            actor_url_str,
            joining
        );
        return Err(format!(
            "Room {room_id} not yet present; retry after RoomInvite"
        ));
    }

    // Ensure remote_actors row so future fanout can resolve inbox
    if !joining.is_empty() {
        if let Err(e) = crate::federation::actor::fetch_remote_actor(db, joining).await {
            tracing::warn!(
                "[Room] fetch_remote_actor for join {} failed: {} (membership still recorded)",
                joining,
                e
            );
        }
    }

    // Was this a pending invite on our roster? (inviter-side accept signal)
    let prior = get_membership(db, room_id, joining)
        .await
        .map_err(|e| e.to_string())?;
    let was_pending = prior
        .as_ref()
        .map(|(_, st)| st == "pending")
        .unwrap_or(false);
    let is_new = prior.is_none();

    // 加入/激活成员（pending → active on accept-side RoomJoin)
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_room_members
           (room_id, actor_url, is_local, role, joined_at, membership_status)
           VALUES ($1, $2, false, $3, NOW(), 'active')
           ON CONFLICT (room_id, actor_url) DO UPDATE SET
               role = EXCLUDED.role,
               membership_status = 'active',
               joined_at = COALESCE(federation_room_members.joined_at, NOW())"#,
        [room_id.into(), joining.into(), role.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "system",
            "room_id": room_id,
            "event": "member_joined",
            "actor": joining,
            "role": role,
            "membership_status": "active"
        }),
    )
    .await;

    // Notify local inviter / owner when a pending invite is accepted
    if was_pending || is_new {
        notify_local_members_of_join(db, room_id, joining).await;
        // Alignment: pending invitees were skipped by KeyExchange fan-out (active-only).
        // When they become active, push any locally published E2E keys so they can decrypt.
        if let Err(e) = refanout_local_e2e_keys_to_member(db, room_id, joining).await {
            tracing::warn!(
                "[Room] E2E key re-fanout to new member {} in {} failed: {}",
                joining,
                room_id,
                e
            );
        }
    }

    tracing::info!(
        "[Room] {} joined room {} as {} (announced by {}, status=active)",
        joining,
        room_id,
        role,
        actor_url_str
    );
    Ok(())
}

/// After a remote member becomes active, deliver KeyExchange for every *local*
/// published room E2E key (skipped earlier while they were pending).
async fn refanout_local_e2e_keys_to_member(
    db: &DatabaseConnection,
    room_id: &str,
    target_actor: &str,
) -> Result<(), String> {
    if target_actor.is_empty() {
        return Ok(());
    }

    let room_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT shared_data_config FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;
    let Some(room_row) = room_row else {
        return Ok(());
    };
    let shared = room_row
        .try_get::<Option<serde_json::Value>>("", "shared_data_config")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}));
    let keys = shared
        .get("e2e")
        .and_then(|e| e.get("published_keys"))
        .and_then(|v| v.as_object())
        .cloned();
    let Some(keys) = keys else {
        return Ok(());
    };
    if keys.is_empty() {
        return Ok(());
    }

    // Resolve target inbox
    let target = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT inbox_url, domain FROM federation_remote_actors WHERE actor_url = $1",
            [target_actor.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;
    let Some(target) = target else {
        // Try fetch once
        let _ = crate::federation::actor::fetch_remote_actor(db, target_actor).await;
        return Ok(());
    };
    let inbox: String = target.try_get("", "inbox_url").unwrap_or_default();
    let domain: String = target.try_get("", "domain").unwrap_or_default();
    if inbox.is_empty() {
        return Ok(());
    }

    // Local active members that own a published key
    let local_members = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT actor_url, local_user_id FROM federation_room_members
               WHERE room_id = $1 AND is_local = true
                 AND COALESCE(membership_status, 'active') = 'active'
                 AND local_user_id IS NOT NULL"#,
            [room_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    let base_url = get_base_url().await;
    let mut sent = 0u32;
    for lm in local_members {
        let local_actor: String = lm.try_get("", "actor_url").unwrap_or_default();
        let user_id: i32 = match lm.try_get::<i32>("", "local_user_id") {
            Ok(id) => id,
            Err(_) => continue,
        };
        if local_actor.is_empty() {
            continue;
        }
        // Match published key by actor URL (same_actor_url for host variants)
        let public_key = keys.iter().find_map(|(k, v)| {
            if same_actor_url(k, &local_actor) {
                v.as_str().map(|s| s.to_string())
            } else {
                None
            }
        });
        let Some(public_key) = public_key else {
            continue;
        };

        let activity_id = generate_activity_id(&base_url);
        let kx_object = crate::federation::e2e::KeyExchangePayload::for_room(
            room_id,
            &public_key,
            Some(now_iso8601()),
        )
        .to_json();
        let kx_activity = json!({
            "@context": build_context(),
            "type": "myriad:KeyExchange",
            "id": &activity_id,
            "actor": &local_actor,
            "to": [target_actor],
            "object": kx_object
        });

        let act_row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'KeyExchange', 'KeyExchange', $3, true, NOW())
                   RETURNING id"#,
                [
                    activity_id.clone().into(),
                    user_id.into(),
                    kx_activity.into(),
                ],
            ))
            .await
            .map_err(|e| e.to_string())?;
        if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_delivery_queue
                       (activity_id, target_inbox, target_domain, status, created_at)
                       VALUES ($1, $2, $3, 'pending', NOW())"#,
                    [act_id.into(), inbox.clone().into(), domain.clone().into()],
                ))
                .await;
            sent += 1;
        }
    }

    if sent > 0 {
        tracing::info!(
            "[Room] Re-fanout {} E2E key(s) to new member {} in {}",
            sent,
            target_actor,
            room_id
        );
    }
    Ok(())
}

/// Notify local users (inviter preferred, else owner) that someone joined/accepted.
async fn notify_local_members_of_join(
    db: &DatabaseConnection,
    room_id: &str,
    joining_actor: &str,
) {
    // Prefer invited_by local user; fall back to local owner/admin members
    let inviter_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT invited_by FROM federation_room_members
               WHERE room_id = $1 AND actor_url = $2"#,
            [room_id.into(), joining_actor.into()],
        ))
        .await
        .ok()
        .flatten();
    let invited_by: Option<String> = inviter_row
        .and_then(|r| r.try_get::<Option<String>>("", "invited_by").ok())
        .flatten();

    let mut targets: Vec<(i32, bool)> = Vec::new(); // (user_id, is_inviter)

    if let Some(ref inv) = invited_by {
        if let Ok(Some(row)) = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT local_user_id FROM federation_room_members
                   WHERE room_id = $1 AND is_local = true AND actor_url = $2
                     AND local_user_id IS NOT NULL"#,
                [room_id.into(), inv.clone().into()],
            ))
            .await
        {
            if let Ok(uid) = row.try_get::<i32>("", "local_user_id") {
                targets.push((uid, true));
            }
        }
    }

    if targets.is_empty() {
        // Fall back: notify all local owners/admins (cap 5)
        if let Ok(rows) = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT local_user_id FROM federation_room_members
                   WHERE room_id = $1 AND is_local = true
                     AND local_user_id IS NOT NULL
                     AND role IN ('owner', 'admin')
                     AND COALESCE(membership_status, 'active') = 'active'
                   LIMIT 5"#,
                [room_id.into()],
            ))
            .await
        {
            for r in rows {
                if let Ok(uid) = r.try_get::<i32>("", "local_user_id") {
                    targets.push((uid, false));
                }
            }
        }
    }

    if targets.is_empty() {
        return;
    }

    let label = crate::federation::notify::actor_label(db, joining_actor).await;
    let name = crate::federation::notify::room_name(db, room_id).await;
    for (uid, _) in targets {
        crate::federation::notify::notify_room_invite_accepted(
            uid,
            room_id,
            &name,
            joining_actor,
            &label,
        )
        .await;
    }
}

/// Inbound Reject for a room invite: remove pending remote member on inviter's side.
pub async fn handle_room_invite_reject(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let room_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("room").and_then(|v| v.as_str()))
        .ok_or("Missing room id on Reject")?;

    // Only delete if this actor was pending on our side
    let result = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"DELETE FROM federation_room_members
               WHERE room_id = $1 AND actor_url = $2
                 AND COALESCE(membership_status, 'active') = 'pending'"#,
            [room_id.into(), actor_url_str.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if result.rows_affected() == 0 {
        // Fallback same_actor_url match
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT actor_url FROM federation_room_members
                   WHERE room_id = $1 AND COALESCE(membership_status, 'active') = 'pending'"#,
                [room_id.into()],
            ))
            .await
            .map_err(|e| e.to_string())?;
        for r in rows {
            let url: String = r.try_get("", "actor_url").unwrap_or_default();
            if same_actor_url(&url, actor_url_str) {
                let _ = db
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        "DELETE FROM federation_room_members WHERE room_id = $1 AND actor_url = $2",
                        [room_id.into(), url.into()],
                    ))
                    .await;
                break;
            }
        }
    }

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "system",
            "room_id": room_id,
            "event": "member_removed",
            "actor": actor_url_str,
            "reason": "invite_rejected"
        }),
    )
    .await;

    tracing::info!(
        "[Room] Invite rejected by {} for room {}",
        actor_url_str,
        room_id
    );
    Ok(())
}

/// 处理 RoomGovernance Activity (myriad:RoomGovernance)
///
/// 治理变更：name / description / avatar_url / invite_policy / max_members / is_public /
/// transfer_owner。仅 owner 或 admin 角色可执行；transfer_owner 仅 owner 可执行。
pub async fn handle_room_governance(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let room_id = object
        .get("room")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("id").and_then(|v| v.as_str()))
        .ok_or("Missing room id")?;
    let changes = object
        .get("changes")
        .and_then(|v| v.as_object())
        .ok_or("Missing changes object")?;

    // 验证发送方是 owner / admin
    let sender_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT m.role, r.owner_actor
               FROM federation_room_members m
               JOIN federation_rooms r ON r.room_id = m.room_id
               WHERE m.room_id = $1 AND m.actor_url = $2"#,
            [room_id.into(), actor_url_str.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "Actor {} is not a member of room {}",
                actor_url_str, room_id
            )
        })?;

    let role: String = sender_row.try_get("", "role").unwrap_or_default();
    let owner: String = sender_row.try_get("", "owner_actor").unwrap_or_default();
    let is_owner = owner == actor_url_str;
    let is_admin = role == "admin" || is_owner;
    if !is_admin {
        return Err(format!(
            "Actor {} has no governance rights in room {}",
            actor_url_str, room_id
        ));
    }

    // 转移 owner — 仅 owner 可发；同步成员 role 字段避免双 owner
    if let Some(new_owner) = changes.get("transfer_owner").and_then(|v| v.as_str()) {
        if !is_owner {
            return Err("Only owner can transfer ownership".to_string());
        }
        let old_owner = owner.clone();
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE federation_rooms SET owner_actor = $2, updated_at = NOW() WHERE room_id = $1",
            [room_id.into(), new_owner.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;
        // Demote previous owner role if still a member
        if !old_owner.is_empty() && !same_actor_url(&old_owner, new_owner) {
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"UPDATE federation_room_members
                       SET role = 'admin'
                       WHERE room_id = $1 AND actor_url = $2 AND role = 'owner'"#,
                    [room_id.into(), old_owner.into()],
                ))
                .await;
        }
        let _ = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"UPDATE federation_room_members
                   SET role = 'owner'
                   WHERE room_id = $1 AND actor_url = $2"#,
                [room_id.into(), new_owner.into()],
            ))
            .await;
    }

    // 字段更新（白名单）
    let mut updates: Vec<(&str, sea_orm::Value)> = Vec::new();
    // Ignore blank/whitespace name so governance cannot wipe a good name.
    if let Some(v) = non_empty_room_name(changes.get("name").and_then(|v| v.as_str())) {
        updates.push(("name", v.into()));
    }
    if let Some(v) = changes.get("description").and_then(|v| v.as_str()) {
        updates.push(("description", v.to_string().into()));
    }
    if let Some(v) = changes.get("avatar_url").and_then(|v| v.as_str()) {
        updates.push(("avatar_url", v.to_string().into()));
    }
    if let Some(v) = changes.get("invite_policy").and_then(|v| v.as_str()) {
        updates.push(("invite_policy", v.to_string().into()));
    }
    if let Some(v) = changes.get("max_members").and_then(|v| v.as_i64()) {
        updates.push(("max_members", (v as i32).into()));
    }
    if let Some(v) = changes.get("is_public").and_then(|v| v.as_bool()) {
        // Remote governance: never allow public → private.
        if !v {
            let currently_public: bool = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT is_public FROM federation_rooms WHERE room_id = $1",
                    [room_id.into()],
                ))
                .await
                .ok()
                .flatten()
                .and_then(|r| r.try_get::<bool>("", "is_public").ok())
                .unwrap_or(false);
            if currently_public {
                tracing::warn!(
                    room_id = %room_id,
                    "[Room] Ignoring remote governance that would un-public a room"
                );
            } else {
                // still private; no-op false is fine to skip
            }
        } else {
            updates.push(("is_public", true.into()));
        }
    }

    for (col, val) in updates {
        let sql = format!(
            "UPDATE federation_rooms SET {} = $2, updated_at = NOW() WHERE room_id = $1",
            col
        );
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            [room_id.into(), val],
        ))
        .await
        .map_err(|e| e.to_string())?;
    }

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "system",
            "room_id": room_id,
            "event": "governance_changed",
            "actor": actor_url_str,
            "changes": changes
        }),
    )
    .await;

    tracing::info!(
        "[Room] Governance change in {} by {}: {:?}",
        room_id,
        actor_url_str,
        changes
    );
    Ok(())
}

// ==================== Room E2E 多方加密 ====================

async fn jwt_secret_for_e2e_seal() -> String {
    let config = crate::GLOBAL_CONFIG.read().await;
    config.jwt_secret.clone()
}

/// 从成员 custom_permissions 读取本地 E2E 密钥对
async fn load_member_e2e_keys(
    db: &DatabaseConnection,
    room_id: &str,
    actor_url: &str,
) -> Result<(String, String), String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT custom_permissions FROM federation_room_members WHERE room_id = $1 AND actor_url = $2",
            [room_id.into(), actor_url.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Member row not found")?;

    let perms = row
        .try_get::<Option<serde_json::Value>>("", "custom_permissions")
        .ok()
        .flatten()
        .ok_or("No custom_permissions / e2e keys")?;
    let e2e = perms.get("e2e").ok_or("No e2e state on member")?;
    let pk = e2e
        .get("local_public_key")
        .and_then(|v| v.as_str())
        .ok_or("Missing local_public_key")?
        .to_string();
    let sk_stored = e2e
        .get("local_private_key")
        .and_then(|v| v.as_str())
        .ok_or("Missing local_private_key")?;
    let jwt_secret = jwt_secret_for_e2e_seal().await;
    let sk = crate::federation::e2e::unseal_private_key(sk_stored, &jwt_secret)?;
    Ok((pk, sk))
}

/// 收集房间已发布的对端公钥（不含 exclude_actor）
async fn collect_room_e2e_recipients(
    db: &DatabaseConnection,
    room_id: &str,
    exclude_actor: &str,
) -> Result<Vec<(String, String)>, String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT shared_data_config FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Room not found")?;

    let shared = row
        .try_get::<Option<serde_json::Value>>("", "shared_data_config")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}));
    let published = shared
        .pointer("/e2e/published_keys")
        .and_then(|v| v.as_object())
        .ok_or("No published E2E keys on room; members must run key-exchange")?;

    let mut out = Vec::new();
    for (actor, pk_val) in published {
        if actor == exclude_actor {
            continue;
        }
        if let Some(pk) = pk_val.as_str() {
            crate::federation::e2e::validate_public_key_b64(pk)
                .map_err(|e| format!("bad key for {actor}: {e}"))?;
            out.push((actor.clone(), pk.to_string()));
        }
    }
    Ok(out)
}

/// 发起 Room E2E 密钥发布：生成本地密钥、登记到 published_keys、fan-out KeyExchange
pub async fn initiate_e2e_key_exchange(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
) -> Result<RoomE2eKeyExchangeResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let membership = get_membership(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Not a member"})),
            )
        })?;
    let (my_role, my_status) = membership;
    // Alignment: pending invitees must not fan-out KeyExchange (remote may lack room row)
    if my_status != "active" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!(
                    "Membership is {my_status}; accept the invite before E2E key exchange"
                )
            })),
        ));
    }
    if my_role == "observer" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Observers cannot publish E2E keys"})),
        ));
    }

    // 1) 读取成员行；已有本地密钥则复用，避免每次打开会话轮换公钥导致解密失败
    let member_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT custom_permissions FROM federation_room_members WHERE room_id = $1 AND actor_url = $2",
            [room_id.into(), local_actor.clone().into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Member row not found"})),
            )
        })?;

    let mut perms = member_row
        .try_get::<Option<serde_json::Value>>("", "custom_permissions")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}));

    let jwt_secret = jwt_secret_for_e2e_seal().await;
    let existing_pk = perms
        .get("e2e")
        .and_then(|e| e.get("local_public_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let existing_sk = perms
        .get("e2e")
        .and_then(|e| e.get("local_private_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let (public_key, sealed_sk) =
        if let (Some(pk), Some(sk_stored)) = (existing_pk, existing_sk) {
            match crate::federation::e2e::unseal_private_key(&sk_stored, &jwt_secret) {
                Ok(_) => (pk, sk_stored),
                Err(_) => {
                    let session = crate::federation::e2e::create_session(room_id);
                    let sealed = crate::federation::e2e::seal_private_key(
                        &session.local_keypair.private_key,
                        &jwt_secret,
                    )
                    .map_err(|e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({"error": format!("Failed to seal E2E key: {}", e)})),
                        )
                    })?;
                    (session.local_keypair.public_key, sealed)
                }
            }
        } else {
            let session = crate::federation::e2e::create_session(room_id);
            let sealed = crate::federation::e2e::seal_private_key(
                &session.local_keypair.private_key,
                &jwt_secret,
            )
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("Failed to seal E2E key: {}", e)})),
                )
            })?;
            (session.local_keypair.public_key, sealed)
        };

    perms["e2e"] = json!({
        "local_public_key": public_key,
        "local_private_key": sealed_sk,
        "sealed": true,
        "algorithm": crate::federation::e2e::E2E_ALGORITHM,
    });

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_room_members SET custom_permissions = $3 WHERE room_id = $1 AND actor_url = $2",
        [
            room_id.into(),
            local_actor.clone().into(),
            perms.into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    // 2) 登记到 room.shared_data_config.e2e.published_keys
    let room_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT shared_data_config FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Room not found"})),
            )
        })?;

    let mut shared = room_row
        .try_get::<Option<serde_json::Value>>("", "shared_data_config")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}));
    if shared.get("e2e").is_none() {
        shared["e2e"] = json!({ "published_keys": {} });
    }
    if shared["e2e"].get("published_keys").is_none() {
        shared["e2e"]["published_keys"] = json!({});
    }
    shared["e2e"]["published_keys"][&local_actor] = json!(public_key);
    shared["e2e"]["algorithm"] = json!(crate::federation::e2e::E2E_ALGORITHM);

    let published_key_count = shared["e2e"]["published_keys"]
        .as_object()
        .map(|o| o.len())
        .unwrap_or(0);

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_rooms SET shared_data_config = $2, updated_at = NOW() WHERE room_id = $1",
        [room_id.into(), shared.into()],
    ))
    .await
    .map_err(db_err)?;

    // 3) Fan-out KeyExchange activity
    let activity_id = generate_activity_id(&base_url);
    let kx_object = crate::federation::e2e::KeyExchangePayload::for_room(
        room_id,
        &public_key,
        Some(now_iso8601()),
    )
    .to_json();
    let kx_activity = json!({
        "@context": build_context(),
        "type": "myriad:KeyExchange",
        "id": &activity_id,
        "actor": &local_actor,
        "object": kx_object
    });

    let _ = fanout_to_remote_members(
        db,
        user_id,
        room_id,
        &activity_id,
        &kx_activity,
        "KeyExchange",
        "KeyExchange",
    )
    .await;

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "key_exchange",
            "room_id": room_id,
            "from": local_actor,
            "publicKey": public_key,
            "algorithm": crate::federation::e2e::E2E_ALGORITHM,
            "published_key_count": published_key_count,
            "direction": "outbound"
        }),
    )
    .await;

    tracing::info!(
        "[Room] E2E key published for {} in room {} (published={})",
        username,
        room_id,
        published_key_count
    );

    Ok(RoomE2eKeyExchangeResponse {
        success: true,
        room_id: room_id.to_string(),
        public_key,
        algorithm: crate::federation::e2e::E2E_ALGORITHM.to_string(),
        published_key_count,
    })
}

/// 处理 Room 的 myriad:KeyExchange：登记对方公钥到 shared_data_config
pub async fn handle_key_exchange(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let room_id = object
        .get("room")
        .and_then(|v| v.as_str())
        .ok_or("Missing room")?;
    let public_key = object
        .get("publicKey")
        .and_then(|v| v.as_str())
        .ok_or("Missing publicKey")?;
    let algorithm = object
        .get("algorithm")
        .and_then(|v| v.as_str())
        .unwrap_or(crate::federation::e2e::E2E_ALGORITHM);

    crate::federation::e2e::validate_public_key_b64(public_key)
        .map_err(|e| format!("Invalid remote E2E public key: {e}"))?;

    // Room may not exist yet if RoomInvite is still in flight — ask peer to retry
    // (transient). Permanent not_found only after room row is known-absent and we
    // already completed invite handling (see ensure below).
    let room_exists = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .is_some();
    if !room_exists {
        return Err(format!(
            "Room {room_id} not yet present; retry after RoomInvite"
        ));
    }

    // 发送方必须是成员（含 inviter/owner self-heal）
    ensure_room_message_sender_member(db, room_id, actor_url_str).await?;

    let room_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT shared_data_config FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("not_found: Room {room_id} not found"))?;

    let mut shared = room_row
        .try_get::<Option<serde_json::Value>>("", "shared_data_config")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}));
    if shared.get("e2e").is_none() {
        shared["e2e"] = json!({ "published_keys": {} });
    }
    if shared["e2e"].get("published_keys").is_none() {
        shared["e2e"]["published_keys"] = json!({});
    }
    shared["e2e"]["published_keys"][actor_url_str] = json!(public_key);
    shared["e2e"]["algorithm"] = json!(algorithm);
    let published_key_count = shared["e2e"]["published_keys"]
        .as_object()
        .map(|o| o.len())
        .unwrap_or(0);

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_rooms SET shared_data_config = $2, updated_at = NOW() WHERE room_id = $1",
        [room_id.into(), shared.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "key_exchange",
            "room_id": room_id,
            "from": actor_url_str,
            "publicKey": public_key,
            "algorithm": algorithm,
            "published_key_count": published_key_count
        }),
    )
    .await;

    tracing::info!(
        "[Room] KeyExchange received in room {} from {} (published={})",
        room_id,
        actor_url_str,
        published_key_count
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_transition_is_one_way() {
        assert!(validate_public_transition(false, None).is_ok());
        assert!(validate_public_transition(false, Some(true)).is_ok());
        assert!(validate_public_transition(false, Some(false)).is_ok()); // still private
        assert!(validate_public_transition(true, Some(true)).is_ok()); // no-op
        assert!(validate_public_transition(true, None).is_ok());
        assert!(validate_public_transition(true, Some(false)).is_err());
    }

    #[test]
    fn non_empty_room_name_trims_and_rejects_blank() {
        assert_eq!(non_empty_room_name(Some(" 测试群 ")).as_deref(), Some("测试群"));
        assert_eq!(non_empty_room_name(Some("   ")), None);
        assert_eq!(non_empty_room_name(Some("")), None);
        assert_eq!(non_empty_room_name(None), None);
    }

    #[test]
    fn resolve_invite_room_name_prefers_real_name() {
        let room_id = "rm_08355abcdef";
        assert_eq!(
            resolve_invite_room_name(Some("测试群"), room_id),
            "测试群"
        );
        assert_eq!(
            resolve_invite_room_name(Some("  "), room_id),
            "Room rm_08355"
        );
        assert_eq!(
            resolve_invite_room_name(None, room_id),
            "Room rm_08355"
        );
    }

    #[test]
    fn is_missing_or_fallback_detects_placeholder() {
        let room_id = "rm_08355abcdef";
        assert!(is_missing_or_fallback_room_name("", room_id));
        assert!(is_missing_or_fallback_room_name("  ", room_id));
        assert!(is_missing_or_fallback_room_name("Room rm_08355", room_id));
        assert!(!is_missing_or_fallback_room_name("测试群", room_id));
        assert!(!is_missing_or_fallback_room_name("Room other", room_id));
    }

    #[test]
    fn invite_object_name_parsing_matches_handle_room_invite() {
        // Mirrors handle_room_invite: read object.name, treat blank as missing.
        // "rm_abc12345" → first 8 chars = "rm_abc12"
        let room_id = "rm_abc12345";
        let with_name = json!({"id": room_id, "name": "  测试群  "});
        let name = non_empty_room_name(with_name.get("name").and_then(|v| v.as_str()));
        assert_eq!(
            resolve_invite_room_name(name.as_deref(), room_id),
            "测试群"
        );

        let blank = json!({"id": room_id, "name": "  "});
        let name = non_empty_room_name(blank.get("name").and_then(|v| v.as_str()));
        assert_eq!(
            resolve_invite_room_name(name.as_deref(), room_id),
            "Room rm_abc12"
        );

        let missing = json!({"id": room_id});
        let name = non_empty_room_name(missing.get("name").and_then(|v| v.as_str()));
        assert_eq!(
            resolve_invite_room_name(name.as_deref(), room_id),
            "Room rm_abc12"
        );
    }

    #[test]
    fn is_admin_role_owner_and_admin_only() {
        assert!(is_admin_role("owner"));
        assert!(is_admin_role("admin"));
        assert!(!is_admin_role("member"));
        assert!(!is_admin_role("moderator"));
        assert!(!is_admin_role(""));

    }

    #[test]
    fn fallback_room_name_uses_first_8_chars() {
        assert_eq!(fallback_room_name("rm_abcdefghij"), "Room rm_abcde");
        assert_eq!(fallback_room_name("short"), "Room short");

    }

    #[test]
    fn validate_public_transition_one_way() {
        assert!(validate_public_transition(true, Some(false)).is_err());
        assert!(validate_public_transition(true, Some(true)).is_ok());
        assert!(validate_public_transition(false, Some(true)).is_ok());
        assert!(validate_public_transition(false, None).is_ok());

    }

    #[test]
    fn non_empty_room_name_trims_v2() {
        assert_eq!(non_empty_room_name(Some("  hi  ")).as_deref(), Some("hi"));
        assert_eq!(non_empty_room_name(Some("   ")), None);
        assert_eq!(non_empty_room_name(None), None);

    }

    #[test]
    fn w175_is_admin_role() {
        assert!(is_admin_role("owner"));
        assert!(is_admin_role("admin"));
        assert!(!is_admin_role("member"));
        assert!(!is_admin_role(""));

    }


    #[test]
    fn w175_fallback_room_name() {
        assert_eq!(fallback_room_name("rm_abcdefghij"), "Room rm_abcde");
        assert_eq!(fallback_room_name("short"), "Room short");

    }


    #[test]
    fn w175_public_transition_one_way() {
        assert!(validate_public_transition(true, Some(false)).is_err());
        assert!(validate_public_transition(true, Some(true)).is_ok());
        assert!(validate_public_transition(false, Some(true)).is_ok());
        assert!(validate_public_transition(false, None).is_ok());

    }


    #[test]
    fn w175_non_empty_room_name() {
        assert_eq!(non_empty_room_name(Some("  hi  ")).as_deref(), Some("hi"));
        assert_eq!(non_empty_room_name(Some("  ")), None);
        assert_eq!(non_empty_room_name(None), None);

    }

}

