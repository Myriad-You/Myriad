//! Room shared stickers.
use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use crate::federation::types::*;

use super::helpers::*;
use super::types::*;

/// Soft caps for room-shared stickers (personal packs are larger / client-only).
/// Only room owner/admin may edit the pack; all active members can send stickers.
pub(crate) const ROOM_STICKER_MAX_COUNT: usize = 50;
pub(crate) const ROOM_STICKER_MAX_DATA_LEN: usize = 120_000;

// Room shared stickers

pub(crate) fn parse_room_stickers(shared: &serde_json::Value) -> Vec<RoomStickerItem> {
    shared
        .get("stickers")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value::<RoomStickerItem>(v.clone()).ok())
                .filter(|s| {
                    !s.id.is_empty()
                        && s.data.starts_with("data:image/")
                        && s.data.len() <= ROOM_STICKER_MAX_DATA_LEN
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn stickers_to_json(list: &[RoomStickerItem]) -> serde_json::Value {
    serde_json::to_value(list).unwrap_or_else(|_| json!([]))
}

pub(crate) async fn load_room_shared_config(
    db: &impl ConnectionTrait,
    room_id: &str,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let room_row = db
        .query_one_raw(Statement::from_sql_and_values(
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
    Ok(room_row
        .try_get::<Option<serde_json::Value>>("", "shared_data_config")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({})))
}

pub(crate) async fn save_room_shared_config(
    db: &impl ConnectionTrait,
    room_id: &str,
    shared: serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_rooms SET shared_data_config = $2, updated_at = NOW() WHERE room_id = $1",
        [room_id.into(), shared.into()],
    ))
    .await
    .map_err(db_err)?;
    Ok(())
}

pub(crate) async fn broadcast_and_fanout_stickers(
    db: &impl ConnectionTrait,
    user_id: i32,
    room_id: &str,
    local_actor: &str,
    base_url: &str,
    stickers: &[RoomStickerItem],
    op: &str,
) {
    let stickers_val = stickers_to_json(stickers);
    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "system",
            "room_id": room_id,
            "event": "stickers_changed",
            "actor": local_actor,
            "op": op,
            "stickers": &stickers_val,
        }),
    )
    .await;

    // Fan-out as RoomGovernance so remote homes mirror the pack.
    // handle_room_governance applies `stickers` only from owner/admin actors.
    let activity_id = generate_activity_id(base_url);
    let gov_activity = json!({
        "@context": build_context(),
        "type": "myriad:RoomGovernance",
        "id": &activity_id,
        "actor": local_actor,
        "object": {
            "type": "myriad:RoomGovernance",
            "room": room_id,
            "changes": {
                "stickers": stickers_val
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
            "[Room] Failed to fan-out stickers for room {}: {}",
            room_id,
            e
        );
    }
}

/// POST /rooms/{id}/stickers — room owner/admin only (edit shared pack).
pub async fn add_room_sticker(
    user_id: i32,
    username: &str,
    room_id: &str,
    req: AddRoomStickerRequest,
    db: &impl ConnectionTrait,
) -> Result<RoomStickersResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let (stored_actor, role) = resolve_active_member_actor(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Only active members can manage stickers"})),
            )
        })?;
    if !is_admin_role(&role) {
        // Also accept room owner when member.role is not labeled "owner".
        let owner_ok = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT owner_actor FROM federation_rooms WHERE room_id = $1",
                [room_id.into()],
            ))
            .await
            .map_err(db_err)?
            .and_then(|row| {
                row.try_get::<String>("", "owner_actor")
                    .ok()
                    .filter(|o| same_actor_url(o, &stored_actor) || o == &stored_actor)
            })
            .is_some();
        if !owner_ok {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Only room owner or admin can edit the group sticker pack"})),
            ));
        }
    }

    let data = req.data.trim().to_string();
    if !data.starts_with("data:image/") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Sticker must be a data:image URL"})),
        ));
    }
    if data.len() > ROOM_STICKER_MAX_DATA_LEN {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Sticker image too large"})),
        ));
    }

    let mut shared = load_room_shared_config(db, room_id).await?;
    let mut stickers = parse_room_stickers(&shared);
    if stickers.len() >= ROOM_STICKER_MAX_COUNT {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Room sticker pack full (max {})", ROOM_STICKER_MAX_COUNT)
            })),
        ));
    }

    let name = req
        .name
        .as_ref()
        .map(|s| s.trim().chars().take(120).collect::<String>())
        .filter(|s| !s.is_empty());

    let entry = RoomStickerItem {
        id: format!("stk_{}", uuid::Uuid::new_v4().simple()),
        data,
        name,
        actor: stored_actor.clone(),
        created_at: now_iso8601(),
    };
    let local_actor = stored_actor;
    stickers.insert(0, entry);
    if stickers.len() > ROOM_STICKER_MAX_COUNT {
        stickers.truncate(ROOM_STICKER_MAX_COUNT);
    }
    shared["stickers"] = stickers_to_json(&stickers);
    save_room_shared_config(db, room_id, shared).await?;

    broadcast_and_fanout_stickers(
        db,
        user_id,
        room_id,
        &local_actor,
        &base_url,
        &stickers,
        "add",
    )
    .await;

    tracing::info!(
        "[Room] Sticker shared in {} by {} (count={})",
        room_id,
        username,
        stickers.len()
    );

    Ok(RoomStickersResponse {
        success: true,
        room_id: room_id.to_string(),
        stickers,
    })
}

/// DELETE /rooms/{id}/stickers/{sticker_id} — room owner/admin only.
pub async fn remove_room_sticker(
    user_id: i32,
    username: &str,
    room_id: &str,
    sticker_id: &str,
    db: &impl ConnectionTrait,
) -> Result<RoomStickersResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    if sticker_id.is_empty() || sticker_id.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid sticker id"})),
        ));
    }

    let (stored_actor, role) = resolve_active_member_actor(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Only active members can manage stickers"})),
            )
        })?;
    let local_actor = stored_actor;

    if !is_admin_role(&role) {
        let owner_ok = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT owner_actor FROM federation_rooms WHERE room_id = $1",
                [room_id.into()],
            ))
            .await
            .map_err(db_err)?
            .and_then(|row| {
                row.try_get::<String>("", "owner_actor")
                    .ok()
                    .filter(|o| same_actor_url(o, &local_actor) || o == &local_actor)
            })
            .is_some();
        if !owner_ok {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Only room owner or admin can edit the group sticker pack"})),
            ));
        }
    }

    let mut shared = load_room_shared_config(db, room_id).await?;
    let mut stickers = parse_room_stickers(&shared);
    let found = stickers.iter().any(|s| s.id == sticker_id);
    if !found {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Sticker not found"})),
        ));
    }

    stickers.retain(|s| s.id != sticker_id);
    shared["stickers"] = stickers_to_json(&stickers);
    save_room_shared_config(db, room_id, shared).await?;

    broadcast_and_fanout_stickers(
        db,
        user_id,
        room_id,
        &local_actor,
        &base_url,
        &stickers,
        "remove",
    )
    .await;

    Ok(RoomStickersResponse {
        success: true,
        room_id: room_id.to_string(),
        stickers,
    })
}
