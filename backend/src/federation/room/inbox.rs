//! Remote room activity handlers (inbox).
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;
use std::collections::HashSet;

use crate::federation::types::*;

use super::e2e::decrypt_room_payload_for_local_ws;
use super::helpers::*;
use super::stickers::{parse_room_stickers, stickers_to_json};

// Inbox 处理（远程 Room 事件）

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
                        .query_one_raw(Statement::from_sql_and_values(
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
                .query_one_raw(Statement::from_sql_and_values(
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
        .query_one_raw(Statement::from_sql_and_values(
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
    let home_server = object
        .get("homeServer")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| extract_host_port(owner_actor))
        .or_else(|| extract_domain(owner_actor))
        .or_else(|| extract_host_port(actor_url_str))
        .or_else(|| extract_domain(actor_url_str))
        .unwrap_or_default();
    let invite_is_public = object
        .get("isPublic")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let invite_policy = object
        .get("invitePolicy")
        .and_then(|v| v.as_str())
        .filter(|s| ["admin-only", "member-invite", "open"].contains(s))
        .unwrap_or(if invite_is_public {
            "open"
        } else {
            "admin-only"
        });
    let has_real_name = invite_name.is_some();
    let display_name = resolve_invite_room_name(invite_name.as_deref(), room_id);
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_rooms
           (room_id, name, description, owner_actor, home_server, governance_type, invite_policy,
            max_members, is_public, distribution_strategy, created_at)
           VALUES ($1, $2, NULL, $3, $4, 'owner', $6, 50, $7, 'fan-out', NOW())
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
             -- Promote is_public only when homes match (or local home empty).
             -- Blocks: existing private local room + spoofed invite claiming public.
             is_public = CASE
               WHEN federation_rooms.is_public THEN true
               WHEN EXCLUDED.is_public
                    AND (
                      btrim(federation_rooms.home_server) = ''
                      OR lower(btrim(federation_rooms.home_server))
                         = lower(btrim(EXCLUDED.home_server))
                    )
               THEN true
               ELSE federation_rooms.is_public
             END,
             invite_policy = CASE
               WHEN EXCLUDED.is_public
                    AND (
                      btrim(federation_rooms.home_server) = ''
                      OR lower(btrim(federation_rooms.home_server))
                         = lower(btrim(EXCLUDED.home_server))
                    )
               THEN EXCLUDED.invite_policy
               ELSE federation_rooms.invite_policy
             END,
             home_server = CASE
               WHEN btrim(federation_rooms.home_server) = ''
                    AND btrim(EXCLUDED.home_server) <> ''
               THEN EXCLUDED.home_server
               ELSE federation_rooms.home_server
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
            invite_policy.into(),
            invite_is_public.into(),
        ],
    ))
    .await
    .map_err(|e| e.to_string())?;

    // Seed remote members BEFORE accepting messages:
    // 1) owner  2) inviter (activity actor)  3) members[] snapshot from invite
    if !same_actor_url(owner_actor, &local_actor) {
        upsert_remote_room_member(db, room_id, owner_actor, "owner", None).await?;
    }
    if !same_actor_url(actor_url_str, &local_actor) && !same_actor_url(actor_url_str, owner_actor) {
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
            let member_role = m.get("role").and_then(|v| v.as_str()).unwrap_or("member");
            upsert_remote_room_member(db, room_id, member_actor, member_role, None).await?;
        }
    }

    // Local invitee is *pending* until they accept (does not auto-join).
    let inserted = db
        .execute_raw(Statement::from_sql_and_values(
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
        .query_one_raw(Statement::from_sql_and_values(
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
        .execute_raw(Statement::from_sql_and_values(
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

    // 广播到本地 WebSocket。
    // E2E 时尽量解密后再广播：多方信封对各收件人明文相同，任一本地成员密钥成功即可。
    // 失败则仍推密文（与 get_messages 在无密钥时行为一致），避免挡住投递。
    let mut ws_payload = payload.clone();
    let mut ws_is_encrypted = is_encrypted;
    if is_encrypted {
        if let Ok(plain) = decrypt_room_payload_for_local_ws(db, room_id, &payload).await {
            ws_payload = plain;
            // Match local-send path: display plaintext must not keep is_encrypted=true
            // or Aro may treat the bubble as still sealed / flash ciphertext on merge.
            ws_is_encrypted = false;
        }
    }
    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "message",
            "room_id": room_id,
            "message": {
                "message_id": message_id,
                "sender_actor": sender,
                "message_type": message_type,
                "payload": ws_payload,
                "is_encrypted": ws_is_encrypted,
                "thread_id": thread_id,
                "reply_to": reply_to,
                "created_at": now_iso8601()
            }
        }),
    )
    .await;

    // 新消息才通知本地成员（排除发送者若其为本地用户；actor URL 规范化比较）。
    // 预览用已解密的 ws_payload，避免通知栏出现 ciphertext JSON。
    if inserted.rows_affected() > 0 {
        let label = crate::federation::notify::actor_label(db, sender).await;
        let name = crate::federation::notify::room_name(db, room_id).await;
        let local_users = crate::federation::notify::room_local_user_ids(db, room_id).await;
        // Resolve which local user_ids belong to the sender (tolerate URL drift).
        let sender_local_ids: HashSet<i32> = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT local_user_id, actor_url FROM federation_room_members
                   WHERE room_id = $1 AND is_local = true AND local_user_id IS NOT NULL"#,
                [room_id.into()],
            ))
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|r| {
                let uid = r.try_get::<i32>("", "local_user_id").ok()?;
                let act: String = r.try_get("", "actor_url").ok()?;
                if same_actor_url(&act, sender) {
                    Some(uid)
                } else {
                    None
                }
            })
            .collect();
        for user_id in local_users {
            if sender_local_ids.contains(&user_id) {
                continue;
            }
            crate::federation::notify::notify_room_message(
                user_id,
                room_id,
                &name,
                sender,
                &label,
                message_type,
                &ws_payload,
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
        if kicker_role.as_deref() != Some("owner") && kicker_role.as_deref() != Some("admin") {
            // Still allow if kicker not known locally (roster lag) — log and proceed
            tracing::warn!(
                "[Room] kick from {} without local admin role in room {}",
                actor_url_str,
                room_id
            );
        }
    }

    db.execute_raw(Statement::from_sql_and_values(
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
        .query_one_raw(Statement::from_sql_and_values(
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
    db.execute_raw(Statement::from_sql_and_values(
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
pub(crate) async fn refanout_local_e2e_keys_to_member(
    db: &DatabaseConnection,
    room_id: &str,
    target_actor: &str,
) -> Result<(), String> {
    if target_actor.is_empty() {
        return Ok(());
    }

    let room_row = db
        .query_one_raw(Statement::from_sql_and_values(
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
        .query_one_raw(Statement::from_sql_and_values(
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
        .query_all_raw(Statement::from_sql_and_values(
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
            .query_one_raw(Statement::from_sql_and_values(
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
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_delivery_queue
                       (activity_id, target_inbox, target_domain, status, created_at)
                       VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
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
pub(crate) async fn notify_local_members_of_join(db: &DatabaseConnection, room_id: &str, joining_actor: &str) {
    // Prefer invited_by local user; fall back to local owner/admin members
    let inviter_row = db
        .query_one_raw(Statement::from_sql_and_values(
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
            .query_one_raw(Statement::from_sql_and_values(
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
            .query_all_raw(Statement::from_sql_and_values(
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
        .execute_raw(Statement::from_sql_and_values(
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
            .query_all_raw(Statement::from_sql_and_values(
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
                    .execute_raw(Statement::from_sql_and_values(
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

    // 验证发送方是成员。名称/策略/贴纸包均需 admin 或 owner。
    let sender_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT m.role, r.owner_actor,
                      COALESCE(m.membership_status, 'active') AS membership_status
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
    let membership_status: String = sender_row
        .try_get("", "membership_status")
        .unwrap_or_else(|_| "active".to_string());
    let is_owner = owner == actor_url_str || same_actor_url(&owner, actor_url_str);
    let is_admin = is_admin_role(&role) || is_owner;
    let stickers_only = changes.contains_key("stickers") && changes.keys().all(|k| k == "stickers");
    if stickers_only {
        if membership_status != "active" {
            return Err(format!(
                "Actor {} is not an active member of room {}",
                actor_url_str, room_id
            ));
        }
        // Sticker pack edits are owner/admin-only (same as other governance).
        if !is_admin {
            return Err(format!(
                "Actor {} has no sticker governance rights in room {}",
                actor_url_str, room_id
            ));
        }
        // Apply sticker pack mirror from home / peer.
        if let Some(stickers_val) = changes.get("stickers") {
            let room_row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT shared_data_config FROM federation_rooms WHERE room_id = $1",
                    [room_id.into()],
                ))
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Room {room_id} not found"))?;
            let mut shared = room_row
                .try_get::<Option<serde_json::Value>>("", "shared_data_config")
                .ok()
                .flatten()
                .unwrap_or_else(|| json!({}));
            let parsed = parse_room_stickers(&json!({ "stickers": stickers_val }));
            shared["stickers"] = stickers_to_json(&parsed);
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE federation_rooms SET shared_data_config = $2, updated_at = NOW() WHERE room_id = $1",
                [room_id.into(), shared.into()],
            ))
            .await
            .map_err(|e| e.to_string())?;

            crate::federation::ws_gateway::broadcast_to_room(
                room_id,
                &json!({
                    "type": "system",
                    "room_id": room_id,
                    "event": "stickers_changed",
                    "actor": actor_url_str,
                    "op": "sync",
                    "stickers": stickers_to_json(&parsed),
                }),
            )
            .await;
            tracing::info!(
                "[Room] Sticker pack synced in {} by {} (count={})",
                room_id,
                actor_url_str,
                parsed.len()
            );
        }
        return Ok(());
    }
    if !is_admin {
        return Err(format!(
            "Actor {} has no governance rights in room {}",
            actor_url_str, room_id
        ));
    }

    // set_member_role — only owner may mint/revoke room admins
    if let Some(role_change) = changes.get("set_member_role").and_then(|v| v.as_object()) {
        if !is_owner {
            return Err("Only owner can change member roles".to_string());
        }
        let target = role_change
            .get("actor")
            .and_then(|v| v.as_str())
            .ok_or("set_member_role missing actor")?;
        let role = role_change
            .get("role")
            .and_then(|v| v.as_str())
            .ok_or("set_member_role missing role")?;
        if role != "admin" && role != "member" {
            return Err("set_member_role role must be admin or member".to_string());
        }
        if same_actor_url(target, actor_url_str) {
            return Err("Cannot change own role via governance".to_string());
        }
        let (target_stored, target_role) = resolve_active_member_actor(db, room_id, target)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Target {target} is not an active member"))?;
        if target_role == "owner" {
            return Err("Cannot change owner role; use transfer_owner".to_string());
        }
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_room_members
               SET role = $3
               WHERE room_id = $1 AND actor_url = $2
                 AND COALESCE(membership_status, 'active') = 'active'"#,
            [
                room_id.into(),
                target_stored.clone().into(),
                role.into(),
            ],
        ))
        .await
        .map_err(|e| e.to_string())?;
        crate::federation::ws_gateway::broadcast_to_room(
            room_id,
            &json!({
                "type": "system",
                "room_id": room_id,
                "event": "member_role_changed",
                "actor": target_stored,
                "role": role,
                "changed_by": actor_url_str
            }),
        )
        .await;
    }

    // 转移 owner — 仅 owner 可发；同步成员 role 字段避免双 owner
    if let Some(new_owner) = changes.get("transfer_owner").and_then(|v| v.as_str()) {
        if !is_owner {
            return Err("Only owner can transfer ownership".to_string());
        }
        let old_owner = owner.clone();
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE federation_rooms SET owner_actor = $2, updated_at = NOW() WHERE room_id = $1",
            [room_id.into(), new_owner.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;
        // Demote previous owner role if still a member
        if !old_owner.is_empty() && !same_actor_url(&old_owner, new_owner) {
            let _ = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"UPDATE federation_room_members
                       SET role = 'admin'
                       WHERE room_id = $1 AND actor_url = $2 AND role = 'owner'"#,
                    [room_id.into(), old_owner.into()],
                ))
                .await;
        }
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
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
                .query_one_raw(Statement::from_sql_and_values(
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
        db.execute_raw(Statement::from_sql_and_values(
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


