//! Room messages, files, and pin.
use axum::{Json, http::StatusCode};
use myriad_error::AppError;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;

use crate::federation::types::*;

use super::e2e::{collect_room_e2e_recipients, load_member_e2e_keys};
use super::helpers::*;
use super::types::*;

// 消息功能

/// 发送 Room 消息
pub async fn send_room_message(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
    req: &SendRoomMessageRequest,
) -> Result<SendRoomMessageResponse, (StatusCode, Json<serde_json::Value>)> {
    // 验证载荷大小（default 4 MiB；内存节约档略低）
    let max_payload = crate::federation::limits::message_payload_limit();
    let payload_size = req.payload.to_string().len();
    if payload_size > max_payload {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(
                AppError::from_status_u16(
                    413,
                    format!(
                        "Message payload too large: {} bytes (max {})",
                        payload_size, max_payload
                    ),
                )
                .with_code("payload_too_large")
                .to_json(),
            ),
        ));
    }

    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    let message_type = req.message_type.as_deref().unwrap_or("text");
    let want_encrypt = req.encrypt.unwrap_or(false);

    // 验证成员身份（pending 邀请返回 ROOM_INVITE_PENDING，便于客户端引导接受）
    let my_role = require_active_member_role(db, room_id, &local_actor).await?;

    // observer 不能发消息
    if my_role == "observer" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Observers cannot send messages")),
        ));
    }

    let room_game = super::helpers::load_room_game_config(db, room_id)
        .await
        .map_err(|error| {
            tracing::error!("Failed to load room game config: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error", "code": "database_error"})),
            )
        })?;
    if let Err(error) = super::game::validate_room_game_message(
        message_type,
        &req.payload,
        want_encrypt,
        room_game.as_ref(),
    ) {
        tracing::error!(%error, "invalid game message");
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid game message",
                "code": "GAME_MESSAGE_INVALID"
            })),
        ));
    }

    let stored_payload = if want_encrypt {
        let encrypted = match collect_room_e2e_recipients(db, room_id, &local_actor).await {
            Err(e) => Err(e),
            Ok(recipients) if recipients.is_empty() => Err("No peer E2E keys yet".into()),
            Ok(mut all) => match load_member_e2e_keys(db, room_id, &local_actor).await {
                Ok((my_pk, _)) => {
                    if !all.iter().any(|(_, pk)| pk == &my_pk) {
                        all.push((local_actor.clone(), my_pk));
                    }
                    crate::federation::e2e::encrypt_json_for_recipients(
                        &req.payload,
                        room_id.as_bytes(),
                        &all,
                    )
                }
                Err(e) => Err(e),
            },
        };
        // encrypt=true never falls back to storing / sending plaintext.
        encrypted.map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": e,
                    "code": "e2e_required",
                })),
            )
        })?
    } else {
        req.payload.clone()
    };
    let is_encrypted = want_encrypt;

    let message_id = generate_message_id();
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

    // The local message and its durable federation effects are one commit.  A
    // successful response must never describe a message that was committed
    // without the Activity/outbox rows needed after a crash.
    let txn = db.begin().await.map_err(db_err)?;
    txn.execute_raw(Statement::from_sql_and_values(
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

    let fanout = fanout_to_remote_members(
        &txn,
        user_id,
        room_id,
        &activity_id,
        &msg_activity,
        "RoomMessage",
        "RoomMessage",
    )
    .await
    .map_err(db_err)?;

    txn.commit().await.map_err(db_err)?;

    // Only expose the message to local consumers after the durable commit.
    // Local display uses sender plaintext for E2E messages; DB / ActivityPub
    // fan-out still retain the encrypted envelope.
    // 本地展示用：E2E 时用发送端明文，避免 Aro 先渲染 ciphertext 信封、等 poll 才正常。
    // 明文落地时 is_encrypted 必须为 false，避免客户端按 flag 二次解密。
    let (ws_payload, ws_is_encrypted) = if is_encrypted {
        (req.payload.clone(), false)
    } else {
        (stored_payload.clone(), false)
    };
    let ws_msg = json!({
        "type": "message",
        "room_id": room_id,
        "message": {
            "message_id": &message_id,
            "sender_actor": &local_actor,
            "message_type": message_type,
            "payload": &ws_payload,
            "is_encrypted": ws_is_encrypted,
            "thread_id": &req.thread_id,
            "reply_to": &req.reply_to,
            "created_at": now_iso8601()
        }
    });
    crate::federation::ws_gateway::broadcast_to_room(room_id, &ws_msg).await;
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

    // 验证成员身份（pending 邀请返回 ROOM_INVITE_PENDING）
    let _my_role = require_active_member_role(db, room_id, &local_actor).await?;

    let _ = user_id;
    let limit = limit.unwrap_or(50).min(200);
    let my_keys = load_member_e2e_keys(db, room_id, &local_actor).await.ok();

    // Opening the latest page marks the room as read for this member (sidebar badge).
    // Pagination (`before`) does not advance last_read_at — that would clear unread while
    // the user is only browsing history.
    if before.is_none() {
        if let Err(e) = mark_room_read(db, room_id, &local_actor).await {
            tracing::debug!(room_id = %room_id, error = %e, "mark_room_read skipped");
        }
    }

    let rows = if let Some(before_id) = before {
        db.query_all_raw(Statement::from_sql_and_values(
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
        db.query_all_raw(Statement::from_sql_and_values(
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
        // After successful decrypt, mark is_encrypted=false so clients treat the
        // payload as display plaintext.
        let mut display_encrypted = is_encrypted;
        if is_encrypted {
            if let Some((pk, sk)) = my_keys.as_ref() {
                if let Ok(plain) = crate::federation::e2e::decrypt_json_for_recipient(
                    &payload,
                    sk,
                    pk,
                    room_id.as_bytes(),
                ) {
                    payload = plain;
                    display_encrypted = false;
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
            is_encrypted: display_encrypted,
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
pub(crate) fn room_file_kind(
    message_type: &str,
    payload: &serde_json::Value,
) -> Option<&'static str> {
    let mut mt = message_type;
    if mt.is_empty() || mt == "text" {
        if payload
            .get("transfer_id")
            .and_then(|v| v.as_str())
            .is_some()
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

pub(crate) fn room_file_status(
    has_inline: bool,
    transfer_status: Option<&str>,
    has_transfer_id: bool,
) -> String {
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

    let _my_role = require_active_member_role(db, room_id, &local_actor).await?;
    let _ = user_id;

    let limit = limit.unwrap_or(50).clamp(1, 200);
    let filter = filter.unwrap_or("all").to_ascii_lowercase();
    let q_norm = q.map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    let my_keys = load_member_e2e_keys(db, room_id, &local_actor).await.ok();

    // Local transfer status map for this room
    let transfer_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT transfer_id, filename, file_size, mime_type, status, created_at
               FROM federation_file_transfers
               WHERE room_id = $1"#,
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let mut transfer_map: std::collections::HashMap<
        String,
        (String, String, i64, Option<String>, String),
    > = std::collections::HashMap::new();
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

    // Over-fetch when this handler will drop rows (`q` / non-all filter); then truncate to `limit`.
    let fetch_limit = if q_norm.is_some() || (filter != "all") {
        (limit * 3).min(200)
    } else {
        limit
    };

    // payload column is json (not jsonb); cast for containment / ->> operators.
    let rows = if let Some(before_id) = before {
        db.query_all_raw(Statement::from_sql_and_values(
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
        db.query_all_raw(Statement::from_sql_and_values(
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

        let tr = transfer_id.as_ref().and_then(|id| transfer_map.get(id));
        let filename = payload
            .get("filename")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| tr.map(|t| t.1.clone()))
            .unwrap_or_else(|| {
                if kind == "image" {
                    "image".into()
                } else {
                    "file".into()
                }
            });

        if let Some(ref qn) = q_norm {
            if !filename.to_lowercase().contains(qn) {
                continue;
            }
        }

        let size = payload
            .get("size")
            .and_then(|v| v.as_i64())
            .or_else(|| {
                payload
                    .get("size")
                    .and_then(|v| v.as_u64())
                    .map(|u| u as i64)
            })
            .or_else(|| tr.map(|t| t.2))
            .unwrap_or(0);
        let mime_type = payload
            .get("mime_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| tr.and_then(|t| t.3.clone()));
        let status = room_file_status(has_inline, tr.map(|t| t.0.as_str()), transfer_id.is_some());
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
        // Append orphans, then sort newest-first
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

    let my_role = require_active_member_role(db, room_id, &local_actor).await?;

    if !is_admin_role(&my_role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json(
                "Only owner or admin can pin messages",
            )),
        ));
    }

    let txn = db.begin().await.map_err(db_err)?;
    let updated = txn
        .query_one_raw(Statement::from_sql_and_values(
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
                Json(AppError::public_json("Message not found")),
            )
        })?;
    let pinned_message_id: String = updated.try_get("", "message_id").map_err(db_err)?;
    if pinned_message_id.is_empty() {
        txn.rollback().await.ok();
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AppError::public_json("Pinned message id is empty")),
        ));
    }

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
    fanout_to_remote_members_required(
        &txn,
        user_id,
        room_id,
        &activity_id,
        &pin_activity,
        "RoomPin",
        "RoomPin",
    )
    .await
    .map_err(db_err)?;
    txn.commit().await.map_err(db_err)?;

    crate::federation::ws_gateway::broadcast_to_room(
        room_id,
        &json!({
            "type": "room_message_pinned",
            "room_id": room_id,
            "message_id": pinned_message_id,
            "is_pinned": req.pinned
        }),
    )
    .await;

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
    db: &impl ConnectionTrait,
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

    // Pin/unpin is owner/admin only.
    let role = get_member_role(db, room_id, actor_url_str)
        .await
        .map_err(|e| e.to_string())?;
    if !role.as_deref().map(is_admin_role).unwrap_or(false) {
        tracing::warn!(
            "[Room] rejected pin from non-admin {} in room {}",
            actor_url_str,
            room_id
        );
        return Err(format!(
            "not_member: {actor_url_str} cannot pin messages in room {room_id}"
        ));
    }

    let updated = db
        .execute_raw(Statement::from_sql_and_values(
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
