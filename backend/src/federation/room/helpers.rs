//! Room helpers (membership, fanout, name validation).
use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use crate::federation::types::*;

// 辅助函数

/// Structured 403 when the actor is not an *active* member.
/// Pending invitees get `code: ROOM_INVITE_PENDING` so clients can show accept/reject UI
/// instead of a generic "Not a member" dead-end.
pub(crate) fn not_active_member_err(
    membership: Option<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some((_, status)) = membership.as_ref() {
        if status == "pending" {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Room invite pending acceptance",
                    "code": "ROOM_INVITE_PENDING",
                    "membership_status": "pending",
                    "hint": "Accept the invite via POST /api/federation/rooms/{room_id}/accept",
                })),
            );
        }
        if status != "active" {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": format!("Membership status is {status}"),
                    "code": "ROOM_MEMBERSHIP_INACTIVE",
                    "membership_status": status,
                })),
            );
        }
    }
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "Not a member",
            "code": "NOT_A_MEMBER",
        })),
    )
}

/// Require *active* membership; returns role or a structured 403 for pending/absent.
pub(crate) async fn require_active_member_role(
    db: &impl ConnectionTrait,
    room_id: &str,
    actor_url: &str,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    match get_membership(db, room_id, actor_url)
        .await
        .map_err(db_err)?
    {
        Some((role, status)) if status == "active" => Ok(role),
        other => Err(not_active_member_err(other)),
    }
}

/// Advance this member's room read cursor (used by list_rooms unread_count).
pub(crate) async fn mark_room_read(
    db: &impl ConnectionTrait,
    room_id: &str,
    actor_url: &str,
) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_room_members
           SET last_read_at = NOW()
           WHERE room_id = $1
             AND actor_url = $2
             AND COALESCE(membership_status, 'active') = 'active'"#,
        [room_id.into(), actor_url.into()],
    ))
    .await?;
    Ok(())
}

/// Resolve active membership to the *stored* actor_url + role (host/case tolerant).
pub(crate) async fn resolve_active_member_actor(
    db: &impl ConnectionTrait,
    room_id: &str,
    actor_url: &str,
) -> Result<Option<(String, String)>, sea_orm::DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT actor_url, role, COALESCE(membership_status, 'active') AS membership_status
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
            let stored: String = r.try_get("", "actor_url").unwrap_or_default();
            let role: String = r.try_get("", "role").unwrap_or_else(|_| "member".into());
            if !stored.is_empty() {
                return Ok(Some((stored, role)));
            }
        } else {
            return Ok(None);
        }
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
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
            let role: String = r.try_get("", "role").unwrap_or_else(|_| "member".into());
            return Ok(Some((url, role)));
        }
    }
    Ok(None)
}

/// 检查用户在 Room 中的角色
pub(crate) async fn get_member_role(
    db: &impl ConnectionTrait,
    room_id: &str,
    actor_url: &str,
) -> Result<Option<String>, sea_orm::DbErr> {
    // Only *active* members can act (pending invites cannot send/download).
    // Legacy rows without membership_status column heal to default 'active'.
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
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
        .query_all_raw(Statement::from_sql_and_values(
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
    db: &impl ConnectionTrait,
    room_id: &str,
    actor_url: &str,
) -> Result<Option<(String, String)>, sea_orm::DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
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
        .query_all_raw(Statement::from_sql_and_values(
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
pub(crate) async fn upsert_remote_room_member(
    db: &impl ConnectionTrait,
    room_id: &str,
    actor_url: &str,
    role: &str,
    invited_by: Option<&str>,
) -> Result<(), String> {
    upsert_remote_room_member_with_status(db, room_id, actor_url, role, invited_by, "active").await
}

/// Upsert a remote room member with explicit membership_status.
pub(crate) async fn upsert_remote_room_member_with_status(
    db: &impl ConnectionTrait,
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
    db.execute_raw(Statement::from_sql_and_values(
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
pub(crate) async fn ensure_room_message_sender_member(
    db: &impl ConnectionTrait,
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
        .query_one_raw(Statement::from_sql_and_values(
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
            .query_all_raw(Statement::from_sql_and_values(
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
pub(crate) fn is_admin_role(role: &str) -> bool {
    role == "owner" || role == "admin"
}

/// Synthetic fallback used when a real room name is unavailable.
/// Matches the invite-receive and notify paths (`Room {first 8 of room_id}`).
pub(crate) fn fallback_room_name(room_id: &str) -> String {
    format!("Room {}", &room_id[..8.min(room_id.len())])
}

/// Blank / whitespace-only names are treated as missing.
/// Public rooms are one-way: once public, they cannot become private again.
pub(crate) fn validate_public_transition(
    currently_public: bool,
    requested: Option<bool>,
) -> Result<(), &'static str> {
    match requested {
        Some(false) if currently_public => Err("Public rooms cannot be made private again"),
        _ => Ok(()),
    }
}

pub(crate) fn non_empty_room_name(name: Option<&str>) -> Option<String> {
    name.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Resolve display name for an invite: real non-empty name, else synthetic fallback.
pub(crate) fn resolve_invite_room_name(invite_name: Option<&str>, room_id: &str) -> String {
    non_empty_room_name(invite_name).unwrap_or_else(|| fallback_room_name(room_id))
}

/// True when stored name is empty/whitespace or the synthetic `Room rm_xxxx` fallback.
/// Used by unit tests to document the same condition as the invite-receive SQL CASE.
#[cfg(test)]
pub(crate) fn is_missing_or_fallback_room_name(name: &str, room_id: &str) -> bool {
    let trimmed = name.trim();
    trimmed.is_empty() || trimmed == fallback_room_name(room_id)
}

/// 向 Room 的所有远程成员 fan-out 一个 Activity
pub(crate) async fn fanout_to_remote_members(
    db: &impl ConnectionTrait,
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
    db: &impl ConnectionTrait,
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
        .query_one_raw(Statement::from_sql_and_values(
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
        .query_one_raw(Statement::from_sql_and_values(
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
            return Err(sea_orm::DbErr::Custom(format!(
                "room fanout activity insert returned no id: activity={activity_id} room={room_id}"
            )));
        }
    };

    // Count remote *active* members missing remote_actors (cannot resolve inbox)
    let unresolved_row = db
        .query_one_raw(Statement::from_sql_and_values(
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
        .query_all_raw(Statement::from_sql_and_values(
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
        let inserted = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_delivery_queue
                   (activity_id, target_inbox, target_domain, status, created_at)
                   VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                [act_db_id.into(), inbox.into(), domain.into()],
            ))
            .await?;
        result.enqueued += inserted.rows_affected() as u32;
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
