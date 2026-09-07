//! Channel E2E session load, key exchange, and accept.
use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;

use crate::federation::types::*;

use super::buffer::buffer_early_channel_activity;
use super::types::E2eKeyExchangeResponse;

// E2E helpers (from accept_e2e)

// E2E 会话辅助

pub(crate) async fn jwt_secret_for_channel_e2e() -> String {
    let config = crate::GLOBAL_CONFIG.read().await;
    config.jwt_secret.clone()
}

/// 从 channel.properties.e2e 加载会话（private key may be sealed at rest）
pub async fn load_e2e_session(
    channel_id: &str,
    properties: Option<&serde_json::Value>,
) -> Result<crate::federation::e2e::EncryptionSession, String> {
    let e2e = properties
        .and_then(|p| p.get("e2e"))
        .ok_or("No e2e state on channel; call key-exchange first")?;
    let local_pk = e2e
        .get("local_public_key")
        .and_then(|v| v.as_str())
        .ok_or("Missing local_public_key in e2e state")?;
    let local_sk_stored = e2e
        .get("local_private_key")
        .and_then(|v| v.as_str())
        .ok_or("Missing local_private_key in e2e state")?;
    let jwt_secret = jwt_secret_for_channel_e2e().await;
    let local_sk = crate::federation::e2e::unseal_private_key(local_sk_stored, &jwt_secret)?;
    let remote_pk = e2e.get("remote_public_key").and_then(|v| v.as_str());
    crate::federation::e2e::session_from_stored(channel_id, local_pk, &local_sk, remote_pk)
}

/// 处理 myriad:KeyExchange Activity
///
/// 1. 校验公钥材料（`e2e` 模块）
/// 2. 写入 channel.properties.e2e.remote_public_key，标记会话 established
/// 3. 作为特殊 message 存入历史，并 WebSocket 广播
pub async fn handle_key_exchange(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let channel_id = object
        .get("channel")
        .and_then(|v| v.as_str())
        .ok_or("Missing channel")?;
    let public_key = object
        .get("publicKey")
        .and_then(|v| v.as_str())
        .ok_or("Missing publicKey")?;
    let algorithm = object
        .get("algorithm")
        .and_then(|v| v.as_str())
        .unwrap_or(crate::federation::e2e::E2E_ALGORITHM);

    crate::federation::e2e::validate_public_key_b64(public_key).map_err(|error| {
        tracing::error!(%error, "invalid remote E2E public key");
        "Invalid remote E2E public key".to_string()
    })?;

    // 验证发送方是该 Channel 的远程方，并读取 properties
    let ch_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.properties, c.status FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.channel_id = $1 AND ra.actor_url = $2"#,
            [channel_id.into(), actor_url_str.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    let ch_row = match ch_row {
        Some(r) => r,
        None => {
            // Align with ChannelMessage: Open may still be in flight.
            let channel_exists = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT 1 FROM federation_channels WHERE channel_id = $1",
                    [channel_id.into()],
                ))
                .await
                .map_err(|e| e.to_string())?
                .is_some();
            if !channel_exists {
                let buffered = buffer_early_channel_activity(channel_id, actor_url_str, activity);
                if buffered {
                    tracing::info!(
                        "[Channel] Buffered early KeyExchange for {} from {} (channel not yet present)",
                        channel_id,
                        actor_url_str
                    );
                }
                return Err(format!(
                    "Channel {} not yet present; retry after ChannelOpen",
                    channel_id
                ));
            }
            return Err(format!(
                "Channel {} not found or actor {} is not the remote party",
                channel_id, actor_url_str
            ));
        }
    };

    let ch_status: String = ch_row
        .try_get("", "status")
        .unwrap_or_else(|_| "pending".into());
    if ch_status == "closed" {
        return Err(format!("Channel {channel_id} is closed"));
    }

    // 合并 e2e 状态：写入 remote_public_key；若已有本地密钥则 established=true。
    //
    // 必须在行锁下重读 properties。这里和 initiate_e2e_key_exchange 都是
    // 「读整个 properties → 改 e2e → 整体写回」，两者用各自的快照互相覆盖：
    // 本函数会抹掉刚写入的 local_private_key，下一次 initiate 因此认为本地
    // 还没有密钥、重新生成一对并再发一条 KeyExchange —— 对端的 remote key 随之
    // 作废，双方陷入密钥轮换循环，而循环前加密的历史永远解不开
    // （截图里那串 "Encrypted · decrypting…" 就是这么来的）。
    let locked = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT properties FROM federation_channels WHERE channel_id = $1 FOR UPDATE",
            [channel_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Channel {channel_id} not found"))?;

    let mut properties = locked
        .try_get::<Option<serde_json::Value>>("", "properties")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}));
    if !properties.is_object() {
        properties = json!({});
    }

    let mut e2e_obj = properties.get("e2e").cloned().unwrap_or_else(|| json!({}));
    if !e2e_obj.is_object() {
        e2e_obj = json!({});
    }
    e2e_obj["remote_public_key"] = json!(public_key);
    e2e_obj["algorithm"] = json!(algorithm);
    let has_local = e2e_obj
        .get("local_private_key")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    e2e_obj["established"] = json!(has_local);
    properties["e2e"] = e2e_obj;

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_channels SET properties = $2, last_activity_at = NOW() WHERE channel_id = $1",
        [channel_id.into(), properties.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;

    let message_id = activity
        .get("id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(generate_message_id);

    let payload = json!({
        "publicKey": public_key,
        "algorithm": algorithm,
    });

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_channel_messages
           (channel_id, message_id, sender_actor, message_type, payload, is_encrypted, created_at)
           VALUES ($1, $2, $3, 'myriad:KeyExchange', $4, false, NOW())
           ON CONFLICT (message_id) DO NOTHING"#,
        [
            channel_id.into(),
            message_id.clone().into(),
            actor_url_str.into(),
            payload.clone().into(),
        ],
    ))
    .await
    .map_err(|e| e.to_string())?;

    crate::federation::ws_gateway::broadcast_to_channel(
        channel_id,
        &json!({
            "type": "key_exchange",
            "channel_id": channel_id,
            "from": actor_url_str,
            "publicKey": public_key,
            "algorithm": algorithm,
            "established": has_local
        }),
    )
    .await;

    tracing::info!(
        "[Channel] KeyExchange received in channel {} from {} (local_keys={})",
        channel_id,
        actor_url_str,
        has_local
    );
    Ok(())
}

// accept / initiate e2e (merged)

/// 接受 Channel（本地用户确认）
pub async fn accept_channel(
    user_id: i32,
    username: &str,
    channel_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.status, c.initiated_by, ra.actor_url, ra.inbox_url
               FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.user_id = $1 AND c.channel_id = $2"#,
            [user_id.into(), channel_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Channel not found"})),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    if status != "pending" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Cannot accept this invite",
                "code": "invite_invalid_status",
            })),
        ));
    }

    let remote_actor_url: String = row.try_get("", "actor_url").unwrap_or_default();
    let remote_inbox: Option<String> = row
        .try_get::<Option<String>>("", "inbox_url")
        .unwrap_or(None);

    // 更新状态
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_channels SET status = 'accepted' WHERE channel_id = $1",
        [channel_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // 发送 Accept Activity
    let local_actor = actor_url(&base_url, username);
    let activity_id = generate_activity_id(&base_url);
    let accept = json!({
        "@context": build_context(),
        "type": "Accept",
        "id": &activity_id,
        "actor": &local_actor,
        "to": [&remote_actor_url],
        "object": {
            "type": "myriad:ChannelOpen",
            "id": channel_id
        }
    });

    if let Some(inbox) = remote_inbox {
        let domain = extract_domain(&inbox).unwrap_or_default();
        let act_row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'Accept', 'ChannelOpen', $3, true, NOW())
                   RETURNING id"#,
                [
                    activity_id.clone().into(),
                    user_id.into(),
                    accept.clone().into(),
                ],
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

    let notif_id = crate::federation::notify::channel_invite_notification_id(channel_id, user_id);
    crate::federation::notify::mark_invite_notification_read(user_id, &notif_id).await;

    Ok(json!({
        "success": true,
        "channel_id": channel_id,
        "status": "accepted"
    }))
}

/// 处理收到的 ChannelAccept Activity（myriad:ChannelAccept）
///
/// 远程方接受了我方发起的 Channel：把 status 置为 'accepted'。
/// 与外部 `Accept`（object=myriad:ChannelOpen）等价的快捷形式，
/// 来自仅实现 MFP 扩展的对端实例。
pub async fn handle_channel_accept(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let channel_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| object.as_str())
        .ok_or("Missing channel id")?;

    // 验证发送方确为该 Channel 的远程方
    let ch_check = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.status FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.channel_id = $1 AND ra.actor_url = $2"#,
            [channel_id.into(), actor_url_str.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if ch_check.is_none() {
        return Err(format!(
            "Channel {} not found or actor {} is not the remote party",
            channel_id, actor_url_str
        ));
    }

    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_channels
               SET status = 'accepted', last_activity_at = NOW()
               WHERE channel_id = $1 AND status = 'pending'"#,
            [channel_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if result.rows_affected() > 0 {
        crate::federation::ws_gateway::broadcast_to_channel(
            channel_id,
            &json!({
                "type": "channel_accepted",
                "channel_id": channel_id
            }),
        )
        .await;
        tracing::info!(
            "[Channel] {} accepted by remote {}",
            channel_id,
            actor_url_str
        );
    }

    Ok(())
}

/// 发起 Channel E2E 密钥交换：生成 X25519 密钥对、写入 properties、投递 myriad:KeyExchange
pub async fn initiate_e2e_key_exchange(
    user_id: i32,
    username: &str,
    channel_id: &str,
    db: &DatabaseConnection,
) -> Result<E2eKeyExchangeResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // 全程持有 channel 行锁：入站 handle_key_exchange 会并发改写同一个
    // properties。两条路径各自「读 → 改 e2e → 整体写回」，谁后写谁获胜，于是
    // 本地私钥或对端公钥会被对方手里的旧快照抹掉。丢了 local_private_key，
    // 下一次调用就认为本地还没有密钥、重新生成一对并再发一条 KeyExchange，
    // 双方陷入密钥轮换循环 —— 轮换之前加密的消息从此永远解不开。
    let txn = db.begin().await.map_err(db_err)?;
    let ch_row = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.status, c.properties, ra.actor_url, ra.inbox_url
               FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.user_id = $1 AND c.channel_id = $2
               FOR UPDATE OF c"#,
            [user_id.into(), channel_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Channel not found"})),
            )
        })?;

    let status: String = ch_row.try_get("", "status").unwrap_or_default();
    // Alignment: do not fan-out KeyExchange while local channel is still pending
    // (remote may not have applied ChannelOpen yet → not_found storm).
    if !["active", "accepted"].contains(&status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!(
                    "Channel is {status}; wait until active/accepted before E2E key exchange"
                )
            })),
        ));
    }

    let remote_inbox: Option<String> = ch_row
        .try_get::<Option<String>>("", "inbox_url")
        .unwrap_or(None);
    let remote_actor_url: String = ch_row.try_get("", "actor_url").unwrap_or_default();
    let mut properties = ch_row
        .try_get::<Option<serde_json::Value>>("", "properties")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}));

    // 保留已有 remote key（若对端先发起）
    let existing_remote = properties
        .get("e2e")
        .and_then(|e| e.get("remote_public_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 已有本地密钥则复用：打开会话时自动 key-exchange 若每次轮换密钥，
    // 对端仍握旧公钥 → 解密失败，且历史里堆满 outbound KeyExchange。
    let existing_local_pk = properties
        .get("e2e")
        .and_then(|e| e.get("local_public_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let existing_local_sk = properties
        .get("e2e")
        .and_then(|e| e.get("local_private_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let jwt_secret = jwt_secret_for_channel_e2e().await;
    // new_keypair: only write a history KeyExchange bubble when the local key is mint-new
    let (public_key, sealed_sk, established, new_keypair) = if let (Some(pk), Some(sk_stored)) =
        (existing_local_pk, existing_local_sk)
    {
        // Validate we can still unseal; if seal secret rotated, mint a new pair.
        match crate::federation::e2e::unseal_private_key(&sk_stored, &jwt_secret) {
            Ok(_sk) => {
                let established = existing_remote
                    .as_ref()
                    .map(|r| crate::federation::e2e::validate_public_key_b64(r).is_ok())
                    .unwrap_or(false);
                (pk, sk_stored, established, false)
            }
            Err(_) => {
                let mut session = crate::federation::e2e::create_session(channel_id);
                let mut established = false;
                if let Some(ref remote_pk) = existing_remote {
                    if crate::federation::e2e::accept_key_exchange(&mut session, remote_pk).is_ok()
                    {
                        established = true;
                    }
                }
                let sealed = crate::federation::e2e::seal_private_key(
                    &session.local_keypair.private_key,
                    &jwt_secret,
                )
                .map_err(|e| {
                    tracing::error!("Failed to seal channel E2E key: {e}");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Failed to seal E2E key", "code": "e2e_key_failed"})),
                    )
                })?;
                (session.local_keypair.public_key, sealed, established, true)
            }
        }
    } else {
        let mut session = crate::federation::e2e::create_session(channel_id);
        let mut established = false;
        if let Some(ref remote_pk) = existing_remote {
            if crate::federation::e2e::accept_key_exchange(&mut session, remote_pk).is_ok() {
                established = true;
            }
        }
        let sealed = crate::federation::e2e::seal_private_key(
            &session.local_keypair.private_key,
            &jwt_secret,
        )
        .map_err(|e| {
            tracing::error!("Failed to seal channel E2E key: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to seal E2E key", "code": "e2e_key_failed"})),
            )
        })?;
        (session.local_keypair.public_key, sealed, established, true)
    };

    let e2e_state = json!({
        "local_public_key": public_key,
        "local_private_key": sealed_sk,
        "remote_public_key": existing_remote,
        "established": established,
        "sealed": true,
        "algorithm": crate::federation::e2e::E2E_ALGORITHM,
    });
    if !properties.is_object() {
        properties = json!({});
    }
    properties["e2e"] = e2e_state;

    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_channels SET properties = $2, last_activity_at = NOW() WHERE channel_id = $1 AND user_id = $3",
        [channel_id.into(), properties.into(), user_id.into()],
    ))
    .await
    .map_err(db_err)?;
    // Commit before the KeyExchange fan-out: a peer that answers instantly must
    // not race an uncommitted local key (and must not block on our row lock).
    txn.commit().await.map_err(db_err)?;

    let local_actor = actor_url(&base_url, username);
    let activity_id = generate_activity_id(&base_url);
    let kx_object = crate::federation::e2e::KeyExchangePayload::for_channel(
        channel_id,
        &public_key,
        Some(now_iso8601()),
    )
    .to_json();
    let kx_activity = json!({
        "@context": build_context(),
        "type": "myriad:KeyExchange",
        "id": &activity_id,
        "actor": &local_actor,
        "to": [&remote_actor_url],
        "object": kx_object
    });

    // 本地历史：仅在新生成密钥时记一条（复用密钥时重复写入会刷屏且误导）
    if new_keypair {
        let message_id = generate_message_id();
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_channel_messages
               (channel_id, message_id, sender_actor, message_type, payload, is_encrypted, created_at)
               VALUES ($1, $2, $3, 'myriad:KeyExchange', $4, false, NOW())"#,
                [
                    channel_id.into(),
                    message_id.into(),
                    local_actor.clone().into(),
                    json!({
                        "publicKey": &public_key,
                        "algorithm": crate::federation::e2e::E2E_ALGORITHM,
                        "direction": "outbound"
                    })
                    .into(),
                ],
            ))
            .await;
    }

    if let Some(inbox) = remote_inbox {
        let domain = extract_domain(&inbox).unwrap_or_default();
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
                    kx_activity.clone().into(),
                ],
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

    crate::federation::ws_gateway::broadcast_to_channel(
        channel_id,
        &json!({
            "type": "key_exchange",
            "channel_id": channel_id,
            "from": local_actor,
            "publicKey": &public_key,
            "algorithm": crate::federation::e2e::E2E_ALGORITHM,
            "established": established,
            "direction": "outbound"
        }),
    )
    .await;

    tracing::info!(
        "[Channel] E2E key exchange initiated for {} (established={}, new_keypair={})",
        channel_id,
        established,
        new_keypair
    );

    Ok(E2eKeyExchangeResponse {
        success: true,
        channel_id: channel_id.to_string(),
        public_key,
        algorithm: crate::federation::e2e::E2E_ALGORITHM.to_string(),
        established,
    })
}
