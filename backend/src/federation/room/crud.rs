//! Room CRUD and dissolve.
use axum::{Json, http::StatusCode};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;

use crate::federation::types::*;

use super::helpers::*;
use super::types::*;

// Room CRUD

/// 创建新 Room
pub async fn create_room(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    req: &CreateRoomRequest,
) -> Result<RoomDetail, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    let home_server = extract_host_port(&base_url)
        .or_else(|| extract_domain(&base_url))
        .unwrap_or_default();
    let room_id = generate_room_id();

    let governance = req.governance_type.as_deref().unwrap_or("owner");
    let invite_policy = req.invite_policy.as_deref().unwrap_or("admin-only");
    let mut max_members = req.max_members.unwrap_or(50);
    let is_public = req.is_public.unwrap_or(false);
    let mut shared_data_config = None;
    if let Some(game) = &req.game {
        super::game::validate_room_game_config(
            &game.tapp_id,
            &game.protocol,
            game.max_players,
            game.max_message_bytes,
        )
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": error, "code": "GAME_CONFIG_INVALID"})),
            )
        })?;
        if let Some(players) = game.max_players {
            max_members = players;
        }
        shared_data_config = Some(json!({ "game": game }));
    }

    // 验证名称和描述长度
    if req.name.is_empty() || req.name.len() > 500 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Room name must be 1-500 characters")),
        ));
    }
    if req.description.as_ref().is_some_and(|d| d.len() > 5000) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Description must be at most 5000 characters",
            )),
        ));
    }

    // 验证 max_members 范围
    if !(2..=5000).contains(&max_members) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "max_members must be between 2 and 5000",
            )),
        ));
    }

    // 验证枚举值
    if !["owner", "democratic", "open"].contains(&governance) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid governance_type")),
        ));
    }
    if !["admin-only", "member-invite", "open"].contains(&invite_policy) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid invite_policy")),
        ));
    }

    let txn = db.begin().await.map_err(db_err)?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_rooms
           (room_id, name, description, avatar_url, owner_actor, home_server, governance_type, invite_policy,
            max_members, is_public, distribution_strategy, shared_data_config, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'fan-out', $11, NOW())"#,
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
            shared_data_config.clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    txn.execute_raw(Statement::from_sql_and_values(
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
    txn.commit().await.map_err(db_err)?;

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
        shared_data_config,
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

    // 验证权限：必须是 owner 或 admin（pending 邀请返回 ROOM_INVITE_PENDING）
    let my_role = require_active_member_role(db, room_id, &local_actor).await?;

    if !is_admin_role(&my_role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Only owner or admin can update room")),
        ));
    }

    // 验证字段
    if let Some(ref name) = req.name {
        if name.is_empty() || name.len() > 500 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Room name must be 1-500 characters")),
            ));
        }
    }
    if let Some(ref desc) = req.description {
        if desc.len() > 5000 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json(
                    "Description must be at most 5000 characters",
                )),
            ));
        }
    }
    if let Some(ref avatar) = req.avatar_url {
        // https URLs stay short; data:image/* uploads need more room (base64).
        let max = if avatar.starts_with("data:image/") {
            600_000
        } else {
            2048
        };
        if avatar.is_empty() || avatar.len() > max {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Invalid avatar (empty or too large)")),
            ));
        }
        if !avatar.starts_with("data:image/")
            && !avatar.starts_with("https://")
            && !avatar.starts_with("http://")
        {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json(
                    "Avatar must be http(s) URL or data:image",
                )),
            ));
        }
    }
    if let Some(ref policy) = req.invite_policy {
        if !["admin-only", "member-invite", "open"].contains(&policy.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Invalid invite_policy")),
            ));
        }
    }
    if let Some(max) = req.max_members {
        if !(2..=5000).contains(&max) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json(
                    "max_members must be between 2 and 5000",
                )),
            ));
        }
    }

    // Public is one-way: once public, cannot go private again.
    let currently_public: bool = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT is_public FROM federation_rooms WHERE room_id = $1",
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?
        .and_then(|r| r.try_get::<bool>("", "is_public").ok())
        .unwrap_or(false);

    if let Err(msg) = validate_public_transition(currently_public, req.is_public) {
        return Err((StatusCode::BAD_REQUEST, Json(AppError::public_json(msg))));
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
            Json(AppError::public_json("No fields to update")),
        ));
    }

    set_parts.push("updated_at = NOW()".to_string());
    let set_clause = set_parts.join(", ");
    let sql = format!(
        "UPDATE federation_rooms SET {} WHERE room_id = ${}",
        set_clause, idx
    );
    values.push(room_id.into());

    db.execute_raw(Statement::from_sql_and_values(
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
    // Fan-out when the request sets `is_public: true` (public→private already rejected).
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
    let my_role = get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .unwrap_or_default();

    if !same_actor_url(&owner_actor, &local_actor) || my_role != "owner" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Only the room owner can delete room")),
        ));
    }

    // Cancel stale traffic (messages / KeyExchange / invites) *before* dissolve
    // fan-out. cancel_pending_deliveries_for_resource also excludes RoomDissolve /
    // ChannelClose, but ordering first is defense-in-depth so dissolve rows are
    // never present when we cancel.
    let _ = crate::federation::delivery::cancel_pending_deliveries_for_resource(
        db,
        room_id,
        "cancelled: local room dissolved",
    )
    .await;

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

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_room_messages WHERE room_id = $1",
        [room_id.into()],
    ))
    .await
    .map_err(db_err)?;

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_room_members WHERE room_id = $1",
        [room_id.into()],
    ))
    .await
    .map_err(db_err)?;

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_rooms WHERE room_id = $1",
        [room_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // Do NOT cancel again after fan-out — object.id = room_id matches LIKE %room_id%.
    // Exclusion + pre-cancel above keep dissolve pending until the delivery worker finishes.

    tracing::info!("[Room] Deleted room {} by {}", room_id, username);

    Ok(json!({ "success": true, "room_id": room_id }))
}

/// Remote owner dissolved the room — wipe local copy and notify open clients.
pub async fn handle_room_dissolve(
    db: &impl ConnectionTrait,
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
        .query_one_raw(Statement::from_sql_and_values(
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

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_room_messages WHERE room_id = $1",
        [room_id.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_room_members WHERE room_id = $1",
        [room_id.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_rooms WHERE room_id = $1",
        [room_id.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;

    crate::federation::delivery::cancel_pending_deliveries_for_resource(
        db,
        room_id,
        "cancelled: remote room dissolved",
    )
    .await
    .map_err(|e| e.to_string())?;

    // Best-effort live hint after every durable database operation succeeded.
    // The transaction remains authoritative; clients can recover by reloading.
    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "room_deleted",
            "room_id": room_id,
            "deleted_by": actor_url_str
        }),
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
        .query_all_raw(Statement::from_sql_and_values(
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
                      -- Unread = messages from others after this member's last_read_at.
                      -- (Previously used "last message I sent", so opening a group never cleared the badge.)
                      COALESCE((SELECT COUNT(*) FROM federation_room_messages msg
                                WHERE msg.room_id = r.room_id
                                  AND msg.sender_actor IS DISTINCT FROM $2
                                  AND msg.created_at > COALESCE(rm.last_read_at, r.created_at)
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
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT r.room_id, r.name, r.description, r.avatar_url, r.owner_actor, r.home_server,
                      r.governance_type, r.governance_config, r.invite_policy,
                      r.distribution_strategy, r.max_members, r.is_public,
                      r.enabled_tapps, r.shared_data_config, r.created_at,
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
            (StatusCode::NOT_FOUND, Json(AppError::public_json("Room not found or access denied")))
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
        // 整份 `shared_data_config`（含 stickers / e2e.published_keys / game）。
        shared_data_config: row
            .try_get::<Option<serde_json::Value>>("", "shared_data_config")
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
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT 1 FROM federation_rooms WHERE room_id = $1 AND is_public = true",
                [room_id.into()],
            ))
            .await
            .map_err(db_err)?;
        if is_public.is_none() {
            return Err((
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("Not a member of this room")),
            ));
        }
    }

    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(r#"SELECT rm.actor_url, rm.is_local, rm.role, rm.joined_at, rm.invited_by,
                      COALESCE(rm.membership_status, 'active') AS membership_status,
                      COALESCE(
                          NULLIF(ra.display_name, ''),
                          ra.username,
                          u.display_name,
                          u.username
                      ) AS display_name,
                      COALESCE(
                          NULLIF(ra.avatar_url, ''),
                          {avatar}
                      ) AS avatar_url
               FROM federation_room_members rm
               LEFT JOIN federation_remote_actors ra ON rm.actor_url = ra.actor_url
               LEFT JOIN users u ON rm.local_user_id = u.id
               WHERE rm.room_id = $1
               ORDER BY
                 CASE COALESCE(rm.membership_status, 'active') WHEN 'active' THEN 0 ELSE 1 END,
                 CASE rm.role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 WHEN 'member' THEN 2 ELSE 3 END,
                 rm.joined_at"#, avatar = crate::services::avatar::avatar_snapshot_expr("u")),
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let _ = user_id; // membership 已按 actor_url 查过；公开房间可跳过
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
use myriad_error::AppError;
