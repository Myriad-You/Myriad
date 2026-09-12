//! Room membership (invite, join, roles, leave).
use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use crate::federation::types::*;

use super::helpers::*;
use super::types::*;

// 成员管理

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

    // 验证邀请权限（pending 邀请返回 ROOM_INVITE_PENDING）
    let my_role = require_active_member_role(db, room_id, &local_actor).await?;

    // 检查 invite_policy
    let room_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT invite_policy, max_members, name, owner_actor, is_public, home_server,
                      shared_data_config
               FROM federation_rooms WHERE room_id = $1"#,
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Room not found")),
            )
        })?;

    let policy: String = room_row.try_get("", "invite_policy").unwrap_or_default();
    let max_members: i32 = room_row.try_get("", "max_members").unwrap_or(50);
    let room_name: String = room_row.try_get("", "name").unwrap_or_default();
    let owner_actor: String = room_row.try_get("", "owner_actor").unwrap_or_default();
    let room_is_public: bool = room_row.try_get("", "is_public").unwrap_or(false);
    let room_home_server: String = room_row
        .try_get::<String>("", "home_server")
        .unwrap_or_default();
    let room_invite_policy = policy.clone();
    let room_game = super::game::parse_room_game_config(
        room_row
            .try_get::<Option<serde_json::Value>>("", "shared_data_config")
            .ok()
            .flatten()
            .as_ref(),
    );

    match policy.as_str() {
        "admin-only" if !is_admin_role(&my_role) => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("Only admins can invite")),
            ));
        }
        "member-invite" => {} // 任何成员可邀请
        "open" => {}          // 与 member-invite 相同：active 成员即可邀请
        _ if !is_admin_role(&my_role) => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("Insufficient permissions")),
            ));
        }
        _ => {}
    }

    // 检查成员上限（仅 active 占用名额；pending 邀请不计入）
    let count_row = db
        .query_one_raw(Statement::from_sql_and_values(
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
            Json(AppError::public_json("Room is full")),
        ));
    }

    let role = req.role.as_deref().unwrap_or("member");
    if !["member", "admin", "observer"].contains(&role) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid role")),
        ));
    }

    // 解析目标 Actor：本地用户名保持原逻辑，远端支持 Actor URL / acct:user@domain / @user@domain / user@domain。
    let raw_target_actor = req.actor.trim();
    if raw_target_actor.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Actor reference is required")),
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
                Json(AppError::public_json(
                    "You are already a member of this room",
                )),
            ));
        }

        // 远程成员：fetch actor + 添加记录
        let remote = crate::federation::actor::fetch_remote_actor(db, &target_actor)
            .await
            .map_err(|e| {
                tracing::error!("[Room] Failed to fetch remote actor: {}", e);
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Cannot resolve actor",
                        "code": "remote_actor_unresolved",
                    })),
                )
            })?;

        // Remote invitee stays *pending* until they accept (no RoomJoin fan-out yet).
        let insert_result = db
            .execute_raw(Statement::from_sql_and_values(
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
                Json(AppError::public_json("Actor is already a room member")),
            ));
        }

        // Snapshot *active* members so the invitee can seed federation_room_members
        // (especially the inviter/owner). Without this, remote rejects RoomMessage.
        let member_rows = db
            .query_all_raw(Statement::from_sql_and_values(
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
            .query_one_raw(Statement::from_sql_and_values(
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
                    Json(AppError::public_json(
                        "Room name is missing; cannot send invite with empty name",
                    )),
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
                "members": members_json,
                "isPublic": room_is_public,
                "invitePolicy": room_invite_policy,
                "homeServer": room_home_server,
                "game": room_game,
            }
        });

        let inbox = &remote.inbox_url;
        if !inbox.is_empty() {
            let domain = extract_domain(inbox).unwrap_or_default();
            let act_row = db
                .query_one_raw(Statement::from_sql_and_values(
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
                    .execute_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"INSERT INTO federation_delivery_queue
                           (activity_id, target_inbox, target_domain, status, created_at)
                           VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
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
                Json(AppError::public_json(
                    "You are already a member of this room",
                )),
            ));
        }

        // 查找本地用户 ID
        let local_row = db
            .query_one_raw(Statement::from_sql_and_values(
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
                Json(AppError::public_json(
                    "Local user not found. Use Actor URL or @user@domain for remote users",
                )),
            )
        })?;

        // Same-instance invite: auto-join as *active* (no accept hop).
        let insert_result = db
            .execute_raw(Statement::from_sql_and_values(
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
                Json(AppError::public_json("Actor is already a room member")),
            ));
        }

        // 向远程成员 fan-out RoomJoin。
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
                "role": role,
                "game": room_game,
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

/// Parse `rm_…` or shareable `rm_…@home[:port]` (optional `myriad:room:` prefix).
pub fn parse_room_join_ref(raw: &str) -> (String, Option<String>) {
    let mut s = raw.trim();
    if let Some(rest) = s.strip_prefix("myriad:room:") {
        s = rest.trim();
    }
    // Allow full public API URLs ending with /public/rooms/{id}
    if let Some(idx) = s.rfind("/public/rooms/") {
        let tail = &s[idx + "/public/rooms/".len()..];
        let id = tail.split(['?', '#', '/']).next().unwrap_or("").trim();
        if id.starts_with("rm_") {
            let home = extract_host_port(s).or_else(|| extract_domain(s));
            return (id.to_string(), home);
        }
    }
    if let Some((id, host)) = s.rsplit_once('@') {
        let id = id.trim();
        let host = host.trim();
        if id.starts_with("rm_")
            && !host.is_empty()
            && !host.contains('/')
            && !host.contains('?')
            && !host.contains('#')
            && !host.contains('@')
        {
            return (id.to_string(), Some(host.to_string()));
        }
    }
    (s.to_string(), None)
}

/// GET public room metadata (no auth). 404 if missing or not public.
pub async fn get_public_room(
    room_id: &str,
    db: &DatabaseConnection,
) -> Result<PublicRoomInfo, (StatusCode, Json<serde_json::Value>)> {
    let (room_id, _) = parse_room_join_ref(room_id);
    if !room_id.starts_with("rm_") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid room id")),
        ));
    }
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT room_id, name, description, avatar_url, owner_actor, home_server,
                      invite_policy, max_members, is_public, shared_data_config,
                      (SELECT COUNT(*) FROM federation_room_members
                       WHERE room_id = federation_rooms.room_id
                         AND COALESCE(membership_status, 'active') = 'active') AS member_count
               FROM federation_rooms
               WHERE room_id = $1 AND is_public = true"#,
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Public room not found")),
            )
        })?;

    Ok(PublicRoomInfo {
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
        invite_policy: row.try_get("", "invite_policy").unwrap_or_default(),
        max_members: row.try_get::<i32>("", "max_members").unwrap_or(50),
        is_public: true,
        member_count: row.try_get::<i64>("", "member_count").unwrap_or(0),
        game: super::game::parse_room_game_config(
            row.try_get::<Option<serde_json::Value>>("", "shared_data_config")
                .ok()
                .flatten()
                .as_ref(),
        ),
    })
}

pub(crate) async fn fetch_remote_public_room(
    home_server: &str,
    room_id: &str,
) -> Result<PublicRoomInfo, String> {
    let home = home_server.trim().trim_end_matches('/');
    if home.is_empty() {
        return Err("home_server is empty".into());
    }
    let bases: Vec<String> = if home.starts_with("http://") || home.starts_with("https://") {
        vec![home.to_string()]
    } else {
        vec![format!("https://{home}"), format!("http://{home}")]
    };
    let user_agent = format!(
        "Myriad/{} (+{})",
        env!("CARGO_PKG_VERSION"),
        get_base_url().await
    );
    let mut last_err = "unreachable".to_string();
    for base in bases {
        let url = format!("{base}/api/federation/public/rooms/{room_id}");
        let (target, client) = match crate::services::outbound_security::build_public_http_client(
            &url,
            std::time::Duration::from_secs(10),
            Some(&user_agent),
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                last_err = e;
                continue;
            }
        };
        let resp = match client
            .get(target)
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err("Public room not found on home server".into());
        }
        if !resp.status().is_success() {
            last_err = format!("home returned {}", resp.status());
            continue;
        }
        let body = crate::services::outbound_security::read_limited_body(resp, 256 * 1024)
            .await
            .map_err(|e| format!("read public room: {e}"))?;
        let info: PublicRoomInfo =
            serde_json::from_slice(&body).map_err(|e| format!("parse public room: {e}"))?;
        if !info.is_public || info.room_id != room_id {
            return Err("Remote document is not a valid public room".into());
        }
        return Ok(info);
    }
    Err(last_err)
}

/// Normalize home_server for comparison (strip scheme, trailing slash, lowercase).
pub(crate) fn normalize_home_server(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches('/');
    if let Some(rest) = s.strip_prefix("https://") {
        s = rest;
    } else if let Some(rest) = s.strip_prefix("http://") {
        s = rest;
    }
    s.trim_end_matches('/').to_ascii_lowercase()
}

pub(crate) fn home_servers_match(a: &str, b: &str) -> bool {
    let a = normalize_home_server(a);
    let b = normalize_home_server(b);
    !a.is_empty() && a == b
}

/// Validate a remote public room document before materialize (pure, unit-tested).
///
/// Returns normalized `(home_server, invite_policy, max_members)` on success.
pub(crate) fn validate_remote_public_room_doc(
    info: &PublicRoomInfo,
    fetched_from: &str,
) -> Result<(String, String, i32), String> {
    if !info.is_public {
        return Err("remote document is not public".into());
    }
    if info.room_id.is_empty() || !info.room_id.starts_with("rm_") {
        return Err("invalid remote room id".into());
    }
    let home = if info.home_server.trim().is_empty() {
        extract_host_port(&info.owner_actor)
            .or_else(|| extract_domain(&info.owner_actor))
            .unwrap_or_else(|| fetched_from.to_string())
    } else {
        info.home_server.clone()
    };
    // Document home must agree with the host we contacted (or be empty → use fetch host).
    if !home.is_empty() && !home_servers_match(&home, fetched_from) {
        return Err(format!(
            "remote home_server mismatch: document={home} fetched_from={fetched_from}"
        ));
    }
    let home = if home.is_empty() {
        fetched_from.to_string()
    } else {
        home
    };
    let invite_policy = match info.invite_policy.as_str() {
        "admin-only" | "member-invite" | "open" => info.invite_policy.clone(),
        _ => "open".into(),
    };
    let max_members = if (2..=5000).contains(&info.max_members) {
        info.max_members
    } else {
        50
    };
    Ok((home, invite_policy, max_members))
}

/// existing_home 空则允许，否则须与 document home 匹配。不判断本机是否为 home。
pub(crate) fn may_promote_private_room_to_public(existing_home: &str, document_home: &str) -> bool {
    if existing_home.trim().is_empty() {
        return true;
    }
    home_servers_match(existing_home, document_home)
}

/// Materialize a remote public room row + owner member for local join.
///
/// On conflict: never flip a local private room public unless the existing
/// `home_server` already matches the fetched document's home (authoritative
/// remote). Never overwrite owner/home when they already identify a different
/// authority (blocks evil-home escalation via known room_id).
pub(crate) async fn materialize_remote_public_room(
    db: &DatabaseConnection,
    info: &PublicRoomInfo,
    // Host we actually fetched from (may differ from document.home_server).
    fetched_from: &str,
) -> Result<(), String> {
    let (home, invite_policy, max_members) = validate_remote_public_room_doc(info, fetched_from)?;
    let name = non_empty_room_name(Some(&info.name))
        .unwrap_or_else(|| resolve_invite_room_name(None, &info.room_id));

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_rooms
           (room_id, name, description, avatar_url, owner_actor, home_server, governance_type,
            invite_policy, max_members, is_public, distribution_strategy, shared_data_config, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, 'owner', $7, $8, true, 'fan-out', $9, NOW())
           ON CONFLICT (room_id) DO UPDATE SET
             name = CASE
               WHEN btrim(EXCLUDED.name) <> ''
                    AND (
                      lower(btrim(federation_rooms.home_server)) = lower(btrim(EXCLUDED.home_server))
                      OR btrim(federation_rooms.home_server) = ''
                    )
               THEN EXCLUDED.name
               ELSE federation_rooms.name
             END,
             description = CASE
               WHEN lower(btrim(federation_rooms.home_server)) = lower(btrim(EXCLUDED.home_server))
                    OR btrim(federation_rooms.home_server) = ''
               THEN COALESCE(EXCLUDED.description, federation_rooms.description)
               ELSE federation_rooms.description
             END,
             avatar_url = CASE
               WHEN lower(btrim(federation_rooms.home_server)) = lower(btrim(EXCLUDED.home_server))
                    OR btrim(federation_rooms.home_server) = ''
               THEN COALESCE(EXCLUDED.avatar_url, federation_rooms.avatar_url)
               ELSE federation_rooms.avatar_url
             END,
             -- Never steal ownership of an existing local/private room from a random home.
             owner_actor = CASE
               WHEN btrim(federation_rooms.owner_actor) = '' THEN EXCLUDED.owner_actor
               WHEN lower(btrim(federation_rooms.home_server)) = lower(btrim(EXCLUDED.home_server))
               THEN EXCLUDED.owner_actor
               ELSE federation_rooms.owner_actor
             END,
             home_server = CASE
               WHEN btrim(federation_rooms.home_server) = '' THEN EXCLUDED.home_server
               ELSE federation_rooms.home_server
             END,
             invite_policy = CASE
               WHEN federation_rooms.is_public
                    OR lower(btrim(federation_rooms.home_server)) = lower(btrim(EXCLUDED.home_server))
               THEN EXCLUDED.invite_policy
               ELSE federation_rooms.invite_policy
             END,
             max_members = CASE
               WHEN federation_rooms.is_public
                    OR lower(btrim(federation_rooms.home_server)) = lower(btrim(EXCLUDED.home_server))
               THEN EXCLUDED.max_members
               ELSE federation_rooms.max_members
             END,
             -- Only promote to public when existing home matches the document home
             -- (or home was empty). Blocks: private local rm_X + evil home claiming public.
             is_public = CASE
               WHEN federation_rooms.is_public THEN true
               WHEN lower(btrim(federation_rooms.home_server)) = lower(btrim(EXCLUDED.home_server))
                    OR btrim(federation_rooms.home_server) = ''
               THEN true
               ELSE federation_rooms.is_public
             END,
             shared_data_config = CASE
               WHEN EXCLUDED.shared_data_config IS NULL THEN federation_rooms.shared_data_config
               WHEN lower(btrim(federation_rooms.home_server)) = lower(btrim(EXCLUDED.home_server))
                    OR btrim(federation_rooms.home_server) = ''
               -- Column is `json`, which has no `||`. Merge through jsonb and cast
               -- back so every CASE branch resolves to `json`.
               THEN (COALESCE(federation_rooms.shared_data_config::jsonb, '{}'::jsonb)
                     || EXCLUDED.shared_data_config::jsonb)::json
               ELSE federation_rooms.shared_data_config
             END,
             updated_at = NOW()"#,
        [
            info.room_id.clone().into(),
            name.into(),
            info.description.clone().into(),
            info.avatar_url.clone().into(),
            info.owner_actor.clone().into(),
            home.into(),
            invite_policy.into(),
            max_members.into(),
            info.game
                .as_ref()
                .map(|game| serde_json::json!({ "game": game }))
                .into(),
        ],
    ))
    .await
    .map_err(|e| e.to_string())?;

    if !info.owner_actor.is_empty() {
        upsert_remote_room_member(db, &info.room_id, &info.owner_actor, "owner", None).await?;
        if let Err(e) = crate::federation::actor::fetch_remote_actor(db, &info.owner_actor).await {
            tracing::warn!(
                "[Room] fetch owner actor for public join {} failed: {}",
                info.owner_actor,
                e
            );
        }
    }
    Ok(())
}

/// Self-join a room when `invite_policy = open` (or public rooms).
///
/// Path may be bare `rm_…` or shareable `rm_…@home[:port]`. When the room is not
/// on this instance, `home_server` (path or body) is required to fetch public
/// metadata and materialize a local row before joining.
pub async fn join_room(
    user_id: i32,
    username: &str,
    room_id_raw: &str,
    db: &DatabaseConnection,
    req: Option<&JoinRoomRequest>,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    let (room_id, home_from_ref) = parse_room_join_ref(room_id_raw);
    if !room_id.starts_with("rm_") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid room id")),
        ));
    }
    let home_hint = req
        .and_then(|r| r.home_server.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or(home_from_ref);

    // Already active?
    if let Ok(Some((role, status))) = get_membership(db, &room_id, &local_actor).await {
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
            return accept_room_invite(user_id, username, &room_id, db).await;
        }
    }

    let mut room_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT invite_policy, max_members, is_public, name, home_server, owner_actor
               FROM federation_rooms WHERE room_id = $1"#,
            [room_id.clone().into()],
        ))
        .await
        .map_err(db_err)?;

    let local_home = extract_host_port(&base_url)
        .or_else(|| extract_domain(&base_url))
        .unwrap_or_default();

    // Remote public join: materialize only when the room row is missing.
    // Never trust an arbitrary home_server to rewrite an existing private room.
    if room_row.is_none() {
        let Some(home) = home_hint.clone() else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Room not found on this instance",
                    "code": "ROOM_NOT_FOUND",
                    "hint": "For remote public groups, use room_id@home_server (shown when sharing)"
                })),
            ));
        };
        if !local_home.is_empty() && home_servers_match(&home, &local_home) {
            return Err((
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Room not found")),
            ));
        }
        let info = fetch_remote_public_room(&home, &room_id)
            .await
            .map_err(|e| {
                tracing::warn!(
                    room_id = %room_id,
                    home = %home,
                    error = %e,
                    "[Room] remote public room fetch failed"
                );
                let not_public = e == "Public room not found on home server";
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": if not_public {
                            "Public room not found on home server"
                        } else {
                            "Home server unreachable"
                        },
                        "code": if not_public {
                            "REMOTE_NOT_PUBLIC"
                        } else {
                            "REMOTE_HOME_UNREACHABLE"
                        },
                    })),
                )
            })?;
        materialize_remote_public_room(db, &info, &home)
            .await
            .map_err(|e| {
                tracing::error!(
                    room_id = %room_id,
                    home = %home,
                    error = %e,
                    "[Room] failed to materialize remote public room"
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "Failed to join room",
                        "code": "ROOM_MATERIALIZE_FAILED",
                    })),
                )
            })?;
        room_row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT invite_policy, max_members, is_public, name, home_server, owner_actor
                   FROM federation_rooms WHERE room_id = $1"#,
                [room_id.clone().into()],
            ))
            .await
            .map_err(db_err)?;
    } else if let Some(ref row) = room_row {
        // Federated invite stub may still be private while home has since gone
        // public. Refresh ONLY from the stub's recorded home_server (never from
        // attacker-supplied home_hint), and only if we are not the home authority.
        let is_public: bool = row.try_get("", "is_public").unwrap_or(false);
        let existing_home: String = row.try_get("", "home_server").unwrap_or_default();
        // Only remote stubs (not local authority) can be upgraded public from home.
        let is_remote_stub =
            !existing_home.is_empty() && !home_servers_match(&existing_home, &local_home);
        if !is_public
            && is_remote_stub
            && may_promote_private_room_to_public(&existing_home, &existing_home)
        {
            // Require join ref home to match stub home when provided (never evil home).
            if let Some(ref hint) = home_hint {
                if !home_servers_match(hint, &existing_home) {
                    tracing::debug!(
                        room_id = %room_id,
                        hint = %hint,
                        existing_home = %existing_home,
                        "[Room] ignoring home_hint that does not match room home_server"
                    );
                } else if let Ok(info) = fetch_remote_public_room(&existing_home, &room_id).await {
                    if !may_promote_private_room_to_public(&existing_home, &info.home_server)
                        && !info.home_server.trim().is_empty()
                    {
                        tracing::warn!(
                            room_id = %room_id,
                            existing_home = %existing_home,
                            doc_home = %info.home_server,
                            "[Room] refusing public promote: document home mismatch"
                        );
                    } else if let Err(e) =
                        materialize_remote_public_room(db, &info, &existing_home).await
                    {
                        tracing::warn!(
                            room_id = %room_id,
                            error = %e,
                            "[Room] public refresh from authoritative home failed"
                        );
                    } else {
                        room_row = db
                            .query_one_raw(Statement::from_sql_and_values(
                                DatabaseBackend::Postgres,
                                r#"SELECT invite_policy, max_members, is_public, name, home_server, owner_actor
                                   FROM federation_rooms WHERE room_id = $1"#,
                                [room_id.clone().into()],
                            ))
                            .await
                            .map_err(db_err)?;
                    }
                }
            }
        }
    }

    let room_row = room_row.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Room not found")),
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
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT COUNT(*)::int AS cnt FROM federation_room_members
               WHERE room_id = $1 AND COALESCE(membership_status, 'active') = 'active'"#,
            [room_id.clone().into()],
        ))
        .await
        .map_err(db_err)?;
    let current: i32 = count_row
        .and_then(|r| r.try_get::<i32>("", "cnt").ok())
        .unwrap_or(0);
    if current >= max_members {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Room is full")),
        ));
    }

    let insert_result = db
        .execute_raw(Statement::from_sql_and_values(
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
                room_id.clone().into(),
                local_actor.clone().into(),
                user_id.into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    if insert_result.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(AppError::public_json("Could not join room")),
        ));
    }

    let join_game = load_room_game_config(db, &room_id).await.ok().flatten();
    let join_activity_id = generate_activity_id(&base_url);
    let join_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomJoin",
        "id": &join_activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:Room",
            "id": &room_id,
            "member": &local_actor,
            "role": "member",
            "game": join_game,
        }
    });
    if let Err(e) = fanout_to_remote_members(
        db,
        user_id,
        &room_id,
        &join_activity_id,
        &join_activity,
        "RoomJoin",
        "Room",
    )
    .await
    {
        tracing::warn!(
            "[Room] open-join RoomJoin fanout failed for {}: {}",
            room_id,
            e
        );
    }

    crate::federation::ws_gateway::broadcast_to_room(
        &room_id,
        &json!({
            "type": "system",
            "room_id": &room_id,
            "event": "member_joined",
            "actor": &local_actor,
            "role": "member",
            "membership_status": "active"
        }),
    )
    .await;

    tracing::info!("[Room] {} self-joined open room {}", local_actor, room_id);

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
                Json(AppError::public_json("No membership for this room")),
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
            Json(json!({
                "error": "Cannot accept this invite",
                "code": "invite_invalid_status",
            })),
        ));
    }

    // Activate local membership
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_room_members
           SET membership_status = 'active', joined_at = NOW()
           WHERE room_id = $1 AND actor_url = $2"#,
        [room_id.into(), local_actor.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    // Announce join to all *active* remote peers (inviter + others)
    let join_game = load_room_game_config(db, room_id).await.ok().flatten();
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
            "role": &role,
            "game": join_game,
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
    let notif_id = crate::federation::notify::room_invite_notification_id(room_id, user_id);
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
        .query_one_raw(Statement::from_sql_and_values(
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
                Json(AppError::public_json("No local membership for this room")),
            )
        })?;

    let status: String = row
        .try_get("", "membership_status")
        .unwrap_or_else(|_| "active".into());
    if status != "pending" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Only pending invites can be rejected",
            )),
        ));
    }
    let invited_by: Option<String> = row.try_get("", "invited_by").ok().flatten();
    let role: String = row.try_get("", "role").unwrap_or_else(|_| "member".into());

    db.execute_raw(Statement::from_sql_and_values(
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
                        .query_one_raw(Statement::from_sql_and_values(
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
                                .execute_raw(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    r#"INSERT INTO federation_delivery_queue
                                       (activity_id, target_inbox, target_domain, status, created_at)
                                       VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
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

    let notif_id = crate::federation::notify::room_invite_notification_id(room_id, user_id);
    crate::federation::notify::mark_invite_notification_read(user_id, &notif_id).await;

    tracing::info!("[Room] {} rejected invite to room {}", local_actor, room_id);

    Ok(json!({
        "success": true,
        "room_id": room_id,
        "membership_status": "rejected"
    }))
}

/// Set a member's role (`admin` | `member`). Owner only; cannot change owner
/// (use transfer ownership). Admins cannot mint more admins.
pub async fn set_member_role(
    user_id: i32,
    username: &str,
    room_id: &str,
    target_actor: &str,
    new_role: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    let new_role = new_role.trim();
    if new_role != "admin" && new_role != "member" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Role must be admin or member")),
        ));
    }

    let my_role = require_active_member_role(db, room_id, &local_actor).await?;

    // Only owner can promote/demote admins. (Admins cannot mint more admins.)
    if my_role != "owner" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json(
                "Only the room owner can change member roles",
            )),
        ));
    }

    let (target_stored, target_role) = resolve_active_member_actor(db, room_id, target_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Target is not an active member")),
            )
        })?;

    if target_role == "owner" || same_actor_url(&target_stored, &local_actor) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Cannot change the owner's role; use transfer ownership",
            )),
        ));
    }

    if target_role == new_role {
        return Ok(json!({
            "success": true,
            "room_id": room_id,
            "actor": target_stored,
            "role": new_role,
            "unchanged": true
        }));
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
            new_role.into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    let system_msg = json!({
        "type": "system",
        "room_id": room_id,
        "event": "member_role_changed",
        "actor": &target_stored,
        "role": new_role,
        "changed_by": &local_actor
    });
    crate::federation::ws_gateway::broadcast_to_room(room_id, &system_msg).await;

    // fan-out RoomGovernance.set_member_role（仅 owner 可发）。
    let activity_id = generate_activity_id(&base_url);
    let gov_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomGovernance",
        "id": &activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:RoomGovernance",
            "room": room_id,
            "changes": {
                "set_member_role": {
                    "actor": &target_stored,
                    "role": new_role
                }
            }
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
            "[Room] set_member_role fanout failed for {}: {}",
            room_id,
            e
        );
    }

    tracing::info!(
        "[Room] {} set {} role to {} in {}",
        local_actor,
        target_stored,
        new_role,
        room_id
    );

    Ok(json!({
        "success": true,
        "room_id": room_id,
        "actor": target_stored,
        "role": new_role
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

    // 验证权限（pending 邀请返回 ROOM_INVITE_PENDING）
    let my_role = require_active_member_role(db, room_id, &local_actor).await?;

    if !is_admin_role(&my_role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Only admins can remove members")),
        ));
    }

    // 不能移除 owner
    let target_role = get_member_role(db, room_id, target_actor)
        .await
        .map_err(db_err)?;

    if target_role.as_deref() == Some("owner") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Cannot remove the room owner")),
        ));
    }

    let _ = user_id; // fan-out 投递用 user_id；角色校验不读它

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

    db.execute_raw(Statement::from_sql_and_values(
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
            Json(AppError::public_json("new_owner is required")),
        ));
    }

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT owner_actor FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Room not found")),
            )
        })?;
    let owner_actor: String = row.try_get("", "owner_actor").unwrap_or_default();
    if !same_actor_url(&owner_actor, &local_actor) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json(
                "Only the room owner can transfer ownership",
            )),
        ));
    }
    if same_actor_url(&owner_actor, new_owner) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Already the owner")),
        ));
    }

    // Resolve target: full actor URL or local username
    let resolved_target = if new_owner.starts_with("http://")
        || new_owner.starts_with("https://")
        || new_owner.starts_with("acct:")
        || new_owner.contains('@')
    {
        crate::federation::follow::resolve_actor_reference(new_owner).await?
    } else {
        actor_url(&base_url, new_owner)
    };

    // Use the *stored* actor_url for role UPDATEs (host/case/slash may differ
    // from the request while still matching via same_actor_url).
    let (target_actor, target_role) = resolve_active_member_actor(db, room_id, &resolved_target)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json(
                    "New owner must already be a room member",
                )),
            )
        })?;
    if target_role == "observer" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Cannot transfer ownership to an observer",
            )),
        ));
    }
    // Canonical stored owner URL (same_actor_url may have matched differently)
    let owner_stored = resolve_active_member_actor(db, room_id, &owner_actor)
        .await
        .map_err(db_err)?
        .map(|(url, _)| url)
        .unwrap_or_else(|| owner_actor.clone());

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_rooms SET owner_actor = $2, updated_at = NOW() WHERE room_id = $1",
        [room_id.into(), target_actor.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    let demoted = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_room_members
               SET role = 'admin'
               WHERE room_id = $1 AND actor_url = $2 AND role = 'owner'"#,
            [room_id.into(), owner_stored.clone().into()],
        ))
        .await
        .map_err(db_err)?;
    if demoted.rows_affected() == 0 {
        // Fallback: demote any same_actor owner row
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT actor_url FROM federation_room_members
                   WHERE room_id = $1 AND role = 'owner'"#,
                [room_id.into()],
            ))
            .await
            .map_err(db_err)?;
        for r in rows {
            let url: String = r.try_get("", "actor_url").unwrap_or_default();
            if same_actor_url(&url, &owner_actor) {
                let _ = db
                    .execute_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"UPDATE federation_room_members SET role = 'admin'
                           WHERE room_id = $1 AND actor_url = $2"#,
                        [room_id.into(), url.into()],
                    ))
                    .await;
            }
        }
    }
    let promoted = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_room_members
               SET role = 'owner'
               WHERE room_id = $1 AND actor_url = $2"#,
            [room_id.into(), target_actor.clone().into()],
        ))
        .await
        .map_err(db_err)?;
    if promoted.rows_affected() == 0 {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AppError::public_json(
                "Failed to promote new owner membership row",
            )),
        ));
    }

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
                Json(AppError::public_json("Not a member")),
            )
        })?;

    if my_role == "owner" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Owner cannot leave. Transfer ownership or delete the room.",
            )),
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

    db.execute_raw(Statement::from_sql_and_values(
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
use myriad_error::AppError;
