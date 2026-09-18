//! Room E2E multi-party encryption.
use axum::{Json, http::StatusCode};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;

use crate::federation::types::*;

use super::helpers::*;
use super::types::*;

// Room E2E 多方加密

pub(crate) async fn jwt_secret_for_e2e_seal() -> String {
    let config = crate::GLOBAL_CONFIG.read().await;
    config.jwt_secret.clone()
}

/// 用任一本地成员密钥解密多方信封，供 WebSocket 广播展示（明文各收件人相同）。
pub(crate) async fn decrypt_room_payload_for_local_ws(
    db: &impl ConnectionTrait,
    room_id: &str,
    encrypted_payload: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT actor_url FROM federation_room_members
               WHERE room_id = $1 AND is_local = true
                 AND COALESCE(membership_status, 'active') = 'active'"#,
            [room_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    let mut last_err = "no local members with e2e keys".to_string();
    for row in rows {
        let actor: String = row.try_get("", "actor_url").unwrap_or_default();
        if actor.is_empty() {
            continue;
        }
        match load_member_e2e_keys(db, room_id, &actor).await {
            Ok((pk, sk)) => {
                match crate::federation::e2e::decrypt_json_for_recipient(
                    encrypted_payload,
                    &sk,
                    &pk,
                    room_id.as_bytes(),
                ) {
                    Ok(plain) => return Ok(plain),
                    Err(e) => last_err = e,
                }
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// 从成员 custom_permissions 读取本地 E2E 密钥对
pub(crate) async fn load_member_e2e_keys(
    db: &impl ConnectionTrait,
    room_id: &str,
    actor_url: &str,
) -> Result<(String, String), String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
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
pub(crate) async fn collect_room_e2e_recipients(
    db: &impl ConnectionTrait,
    room_id: &str,
    exclude_actor: &str,
) -> Result<Vec<(String, String)>, String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
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

/// Publish Room E2E keys: reuse existing local key first; skip fan-out/WS when already published.
pub async fn initiate_e2e_key_exchange(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
) -> Result<RoomE2eKeyExchangeResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    // Alignment: pending invitees must not fan-out KeyExchange (remote may lack room row)
    let my_role = require_active_member_role(db, room_id, &local_actor).await?;
    if my_role == "observer" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Observers cannot publish E2E keys")),
        ));
    }

    // 1) 读取成员行；已有本地密钥则复用，避免每次打开会话轮换公钥导致解密失败。
    //
    // Lock room then member. `handle_key_exchange` only `FOR UPDATE`s `federation_rooms`.
    let txn = db.begin().await.map_err(db_err)?;
    let room_row = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT shared_data_config FROM federation_rooms WHERE room_id = $1 FOR UPDATE",
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

    let member_row = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT custom_permissions FROM federation_room_members
               WHERE room_id = $1 AND actor_url = $2
               FOR UPDATE"#,
            [room_id.into(), local_actor.clone().into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Member row not found")),
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

    let (public_key, sealed_sk) = if let (Some(pk), Some(sk_stored)) = (existing_pk, existing_sk) {
        match crate::federation::e2e::unseal_private_key(&sk_stored, &jwt_secret) {
            Ok(_) => (pk, sk_stored),
            Err(_) => {
                let session = crate::federation::e2e::create_session(room_id);
                let sealed = crate::federation::e2e::seal_private_key(
                    &session.local_keypair.private_key,
                    &jwt_secret,
                )
                .map_err(|e| {
                    tracing::error!("Failed to seal room E2E key: {e}");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Failed to seal E2E key", "code": "e2e_key_failed"})),
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
            tracing::error!("Failed to seal room E2E key: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to seal E2E key", "code": "e2e_key_failed"})),
            )
        })?;
        (session.local_keypair.public_key, sealed)
    };

    if !perms.is_object() {
        perms = json!({});
    }
    perms["e2e"] = json!({
        "local_public_key": public_key,
        "local_private_key": sealed_sk,
        "sealed": true,
        "algorithm": crate::federation::e2e::E2E_ALGORITHM,
    });

    txn.execute_raw(Statement::from_sql_and_values(
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

    // 2) 登记到 room.shared_data_config.e2e.published_keys（沿用上面已锁定的行）
    let mut shared = room_row
        .try_get::<Option<serde_json::Value>>("", "shared_data_config")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}));
    if !shared.is_object() {
        shared = json!({});
    }
    if !shared["e2e"].is_object() {
        shared["e2e"] = json!({ "published_keys": {} });
    }
    if !shared["e2e"]["published_keys"].is_object() {
        shared["e2e"]["published_keys"] = json!({});
    }
    let already_published = shared["e2e"]["published_keys"]
        .get(&local_actor)
        .and_then(|v| v.as_str())
        == Some(public_key.as_str());
    shared["e2e"]["published_keys"][&local_actor] = json!(public_key);
    shared["e2e"]["algorithm"] = json!(crate::federation::e2e::E2E_ALGORITHM);

    let published_key_count = shared["e2e"]["published_keys"]
        .as_object()
        .map(|o| o.len())
        .unwrap_or(0);

    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_rooms SET shared_data_config = $2, updated_at = NOW() WHERE room_id = $1",
        [room_id.into(), shared.into()],
    ))
    .await
    .map_err(db_err)?;

    // Skip KeyExchange fan-out when this actor already published the same key
    // (previous successful commit already queued delivery).
    if !already_published {
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
        fanout_to_remote_members(
            &txn,
            user_id,
            room_id,
            &activity_id,
            &kx_activity,
            "KeyExchange",
            "KeyExchange",
        )
        .await
        .map_err(db_err)?;
    }
    txn.commit().await.map_err(db_err)?;

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
    db: &impl ConnectionTrait,
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

    // Missing room → transient `not yet present; retry after RoomInvite`.
    // `not_found:` here is a TOCTOU on the second SELECT after the row was seen.
    let room_exists = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM federation_rooms WHERE room_id = $1 FOR UPDATE",
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

    // 行锁下读改写：本地 initiate_e2e_key_exchange 会并发改同一份
    // shared_data_config。没有锁时两边用各自的快照整体覆盖，谁后写谁获胜，
    // published_keys 会丢掉一方的公钥 —— 丢失方从此收不到能解开的消息。
    let room_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT shared_data_config FROM federation_rooms WHERE room_id = $1 FOR UPDATE",
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
    if !shared.is_object() {
        shared = json!({});
    }
    if !shared["e2e"].is_object() {
        shared["e2e"] = json!({ "published_keys": {} });
    }
    if !shared["e2e"]["published_keys"].is_object() {
        shared["e2e"]["published_keys"] = json!({});
    }
    shared["e2e"]["published_keys"][actor_url_str] = json!(public_key);
    shared["e2e"]["algorithm"] = json!(algorithm);
    let published_key_count = shared["e2e"]["published_keys"]
        .as_object()
        .map(|o| o.len())
        .unwrap_or(0);

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
use myriad_error::AppError;
