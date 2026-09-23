//! Local HTTP handlers for initiating, uploading, listing, downloading, and cancelling transfers.

use axum::{Json, http::StatusCode};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use myriad_error::AppError;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;
use tokio::fs;

use crate::federation::types::*;

use super::storage::{
    ChunkFileState, DEFAULT_CHUNK_SIZE, MAX_FILE_SIZE, UploadSessionAction, admit_chunk_bytes,
    admit_new_transfer, bad_request, final_file_path, finalize_uploaded_transfer,
    is_strictly_under, is_valid_transfer_id, lock_transfer_admission, lock_transfer_session,
    part_file_path, path_to_db, prepare_chunk_file, resolve_transfer_path, run_transfer_file_work,
    safe_filename,
    storage_err, storage_root, stored_bytes, upload_session_action, verify_chunk_bytes,
};
use super::types::{
    InitTransferRequest, TransferDetail, TransferFileContent, TransferSummary, UploadChunkRequest,
};

/// 在 Channel 上发起文件传输
///
/// 创建传输记录 + 发送 `myriad:FileTransfer` Activity
pub async fn initiate_transfer(
    user_id: i32,
    username: &str,
    channel_id: &str,
    db: &DatabaseConnection,
    req: &InitTransferRequest,
) -> Result<TransferDetail, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // Channel must exist; status active|accepted (channel_type is unread).
    let ch_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.channel_type, c.status, ra.actor_url, ra.inbox_url
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
                Json(AppError::public_json("Channel not found")),
            )
        })?;

    let status: String = ch_row.try_get("", "status").unwrap_or_default();
    if !["active", "accepted"].contains(&status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Channel is not ready to send files",
                "code": "channel_not_ready",
            })),
        ));
    }

    // 验证文件大小
    if req.file_size <= 0 || req.file_size > MAX_FILE_SIZE {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                AppError::bad_request(format!(
                    "File size must be between 1 byte and {} bytes",
                    MAX_FILE_SIZE
                ))
                .with_code("file_too_large")
                .to_json(),
            ),
        ));
    }

    let remote_actor_url: String = ch_row.try_get("", "actor_url").unwrap_or_default();
    let remote_inbox: Option<String> = ch_row
        .try_get::<Option<String>>("", "inbox_url")
        .unwrap_or(None);

    // 计算分块数
    let chunks_total =
        ((req.file_size + DEFAULT_CHUNK_SIZE - 1) / DEFAULT_CHUNK_SIZE).max(1) as i32;

    // 创建传输记录
    let transfer_id = generate_transfer_id();
    let final_path = final_file_path(&transfer_id, &req.filename).map_err(bad_request)?;
    let local_path = path_to_db(&final_path);

    let txn = db.begin().await.map_err(db_err)?;
    lock_transfer_admission(&txn).await.map_err(db_err)?;
    admit_new_transfer(&txn, req.file_size, Some(user_id)).await?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_file_transfers
           (transfer_id, channel_id, filename, file_size, mime_type,
            checksum_sha256, status, direction, chunks_total, chunks_completed, local_path, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, 'pending', 'outbound', $7, 0, $8, NOW())"#,
        [
            transfer_id.clone().into(),
            channel_id.into(),
            req.filename.clone().into(),
            req.file_size.into(),
            req.mime_type.clone().into(),
            req.checksum.clone().into(),
            chunks_total.into(),
            local_path.into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    let local_actor = actor_url(&base_url, username);
    let activity_id = generate_activity_id(&base_url);

    let file_activity = json!({
        "@context": build_context(),
        "type": "myriad:FileTransfer",
        "id": &activity_id,
        "actor": &local_actor,
        "to": [&remote_actor_url],
        "object": {
            "type": "myriad:FileMeta",
            "transferId": &transfer_id,
            "channelId": channel_id,
            "filename": &req.filename,
            "fileSize": req.file_size,
            "mimeType": &req.mime_type,
            "checksum": &req.checksum,
            "chunksTotal": chunks_total,
            "chunkSize": DEFAULT_CHUNK_SIZE,
            "protocol": "mfp/1.0"
        }
    });

    let inbox = remote_inbox.filter(|s| !s.is_empty()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Remote inbox missing; cannot notify peer of transfer",
                "code": "remote_inbox_missing",
            })),
        )
    })?;
    let act_id = insert_local_activity(
        &txn,
        user_id,
        &activity_id,
        "FileTransfer",
        Some("FileMeta"),
        file_activity,
    )
    .await
    .map_err(db_err)?;
    enqueue_delivery(&txn, act_id, &inbox, "pending")
        .await
        .map_err(db_err)?;
    txn.commit().await.map_err(db_err)?;

    Ok(TransferDetail {
        transfer_id,
        channel_id: channel_id.to_string(),
        room_id: None,
        filename: req.filename.clone(),
        file_size: req.file_size,
        mime_type: req.mime_type.clone(),
        checksum: req.checksum.clone(),
        status: "pending".to_string(),
        direction: "outbound".to_string(),
        chunks_total,
        chunks_received: 0,
        bytes_transferred: 0,
        progress: 0.0,
        created_at: now_iso8601(),
        completed_at: None,
    })
}

/// Room 分块传输：成员发起；完成后同实例成员可下；远程成员经 FileTransfer fan-out 收块。
pub async fn initiate_room_transfer(
    user_id: i32,
    username: &str,
    room_id: &str,
    db: &DatabaseConnection,
    req: &InitTransferRequest,
) -> Result<TransferDetail, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let role = crate::federation::room::get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("Not a room member")),
            )
        })?;
    if role == "observer" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Observers cannot upload files")),
        ));
    }

    if req.file_size <= 0 || req.file_size > MAX_FILE_SIZE {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                AppError::bad_request(format!(
                    "File size must be between 1 byte and {} bytes",
                    MAX_FILE_SIZE
                ))
                .with_code("file_too_large")
                .to_json(),
            ),
        ));
    }

    let chunks_total =
        ((req.file_size + DEFAULT_CHUNK_SIZE - 1) / DEFAULT_CHUNK_SIZE).max(1) as i32;
    let transfer_id = generate_transfer_id();
    let final_path = final_file_path(&transfer_id, &req.filename).map_err(bad_request)?;
    let local_path = path_to_db(&final_path);

    let txn = db.begin().await.map_err(db_err)?;
    lock_transfer_admission(&txn).await.map_err(db_err)?;
    admit_new_transfer(&txn, req.file_size, Some(user_id)).await?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_file_transfers
           (transfer_id, channel_id, room_id, owner_user_id, filename, file_size, mime_type,
            checksum_sha256, status, direction, chunks_total, chunks_completed, local_path, created_at)
           VALUES ($1, '', $2, $3, $4, $5, $6, $7, 'pending', 'outbound', $8, 0, $9, NOW())"#,
        [
            transfer_id.clone().into(),
            room_id.into(),
            user_id.into(),
            req.filename.clone().into(),
            req.file_size.into(),
            req.mime_type.clone().into(),
            req.checksum.clone().into(),
            chunks_total.into(),
            local_path.into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    let activity_id = generate_activity_id(&base_url);
    let file_activity = json!({
        "@context": build_context(),
        "type": "myriad:FileTransfer",
        "id": &activity_id,
        "actor": &local_actor,
        "object": {
            "type": "myriad:FileMeta",
            "transferId": &transfer_id,
            "roomId": room_id,
            "filename": &req.filename,
            "fileSize": req.file_size,
            "mimeType": &req.mime_type,
            "checksum": &req.checksum,
            "chunksTotal": chunks_total,
            "chunkSize": DEFAULT_CHUNK_SIZE,
            "protocol": "mfp/1.0"
        }
    });
    crate::federation::room::fanout_to_remote_members_required(
        &txn,
        user_id,
        room_id,
        &activity_id,
        &file_activity,
        "FileTransfer",
        "FileMeta",
    )
    .await
    .map_err(db_err)?;
    txn.commit().await.map_err(db_err)?;

    Ok(TransferDetail {
        transfer_id: transfer_id.clone(),
        channel_id: String::new(),
        room_id: Some(room_id.to_string()),
        filename: req.filename.clone(),
        file_size: req.file_size,
        mime_type: req.mime_type.clone(),
        checksum: req.checksum.clone(),
        status: "pending".to_string(),
        direction: "outbound".to_string(),
        chunks_total,
        chunks_received: 0,
        bytes_transferred: 0,
        progress: 0.0,
        created_at: now_iso8601(),
        completed_at: None,
    })
}

/// 上传文件分块
pub async fn upload_chunk(
    user_id: i32,
    username: &str,
    transfer_id: &str,
    db: &DatabaseConnection,
    req: &UploadChunkRequest,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    // reject unsafe transferId before any path construction / DB-driven FS write
    if !is_valid_transfer_id(transfer_id) {
        return Err(bad_request(
            "Invalid transferId: must be 1-128 chars of [A-Za-z0-9_-] only",
        ));
    }

    let txn = db.begin().await.map_err(db_err)?;
    lock_transfer_session(&txn, transfer_id)
        .await
        .map_err(db_err)?;

    let row = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ft.status, ft.chunks_total, ft.chunks_completed,
                      ft.file_size, ft.channel_id, ft.room_id, ft.owner_user_id,
                      ft.filename, ft.checksum_sha256, ft.local_path,
                      c.user_id AS channel_user_id,
                      ra.actor_url AS remote_actor_url, ra.inbox_url AS remote_inbox
               FROM federation_file_transfers ft
               LEFT JOIN federation_channels c
                 ON c.channel_id = ft.channel_id AND ft.channel_id <> ''
               LEFT JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE ft.transfer_id = $1"#,
            [transfer_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Transfer not found")),
            )
        })?;

    let room_id: Option<String> = row
        .try_get::<Option<String>>("", "room_id")
        .unwrap_or(None)
        .filter(|s| !s.is_empty());
    let owner_user_id: Option<i32> = row
        .try_get::<Option<i32>>("", "owner_user_id")
        .unwrap_or(None);
    let channel_user_id: Option<i32> = row
        .try_get::<Option<i32>>("", "channel_user_id")
        .unwrap_or(None);

    let allowed = if room_id.is_some() {
        owner_user_id == Some(user_id)
    } else {
        channel_user_id == Some(user_id)
    };
    if !allowed {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Not your transfer")),
        ));
    }

    let chunks_total: i32 = row.try_get("", "chunks_total").unwrap_or(1);
    let chunks_completed: i32 = row.try_get("", "chunks_completed").unwrap_or(0);
    let status: String = row.try_get("", "status").unwrap_or_default();
    let file_size: i64 = row.try_get("", "file_size").unwrap_or(0);
    let channel_id: String = row.try_get("", "channel_id").unwrap_or_default();
    let filename: String = row
        .try_get("", "filename")
        .unwrap_or_else(|_| "file".to_string());
    let checksum: Option<String> = row
        .try_get::<Option<String>>("", "checksum_sha256")
        .unwrap_or(None);
    let local_path: Option<String> = row
        .try_get::<Option<String>>("", "local_path")
        .unwrap_or(None);
    let remote_actor_url: String = row
        .try_get::<Option<String>>("", "remote_actor_url")
        .unwrap_or(None)
        .unwrap_or_default();
    let remote_inbox: Option<String> = row
        .try_get::<Option<String>>("", "remote_inbox")
        .unwrap_or(None);

    if chunks_total <= 0 {
        return Err(bad_request("Invalid transfer chunk count"));
    }
    if req.chunk_index < 0 || req.chunk_index >= chunks_total {
        return Err(bad_request("Chunk index out of range"));
    }
    let session_action =
        upload_session_action(&status, chunks_completed, chunks_total, req.chunk_index)?;

    if req.chunk_size <= 0 || req.chunk_size > DEFAULT_CHUNK_SIZE {
        return Err(bad_request(format!(
            "chunk_size must be between 1 and {} bytes",
            DEFAULT_CHUNK_SIZE
        )));
    }

    // reserve decoded chunk budget before base64 decode / disk write
    let _chunk_budget = admit_chunk_bytes(req.chunk_size)?;

    let decoded = BASE64
        .decode(req.chunk_data.as_bytes())
        .map_err(|_| bad_request("Invalid base64 chunk_data"))?;
    if decoded.len() as i64 != req.chunk_size {
        return Err(bad_request("chunk_size does not match decoded data length"));
    }

    let is_last_chunk = req.chunk_index == chunks_total - 1;
    let expected_size = if is_last_chunk {
        file_size - (DEFAULT_CHUNK_SIZE * (chunks_total as i64 - 1))
    } else {
        DEFAULT_CHUNK_SIZE
    };
    if expected_size <= 0 || req.chunk_size != expected_size {
        return Err(bad_request(format!(
            "Invalid chunk size: expected {}, got {}",
            expected_size, req.chunk_size
        )));
    }

    let final_path = resolve_transfer_path(transfer_id, &filename, local_path.as_deref())
        .map_err(bad_request)?;
    let final_path_db = path_to_db(&final_path);
    let part_path = part_file_path(&final_path);
    // Confinement for the .part sibling (same parent under storage root)
    if !is_strictly_under(&storage_root(), &part_path) {
        return Err(bad_request("Transfer path escapes storage root"));
    }
    let expected_offset = DEFAULT_CHUNK_SIZE * req.chunk_index as i64;

    if session_action == UploadSessionAction::VerifyCompletedRetry {
        verify_chunk_bytes(&final_path, &decoded, expected_offset).await?;
        let stored = fs::metadata(&final_path).await.map_err(storage_err)?.len() as i64;
        if stored != file_size {
            return Err(bad_request(format!(
                "Completed file size mismatch: expected {}, got {}",
                file_size, stored
            )));
        }
        txn.commit().await.map_err(db_err)?;
        return Ok(json!({
            "success": true,
            "transfer_id": transfer_id,
            "chunk_index": req.chunk_index,
            "chunks_completed": chunks_completed,
            "chunks_total": chunks_total,
            "status": "completed",
            "bytes_transferred": file_size,
            "progress": 100.0
        }));
    }

    // Same cancellation-safe I/O lock as inbound chunks (see run_transfer_file_work).
    let chunk_file_state = {
        let (part_path, final_path) = (part_path.clone(), final_path.clone());
        run_transfer_file_work(db, transfer_id, async move {
            prepare_chunk_file(
                &part_path,
                &final_path,
                &decoded,
                expected_offset,
                is_last_chunk,
            )
            .await
        })
        .await
        .map_err(storage_err)??
    };

    let (persisted_chunks, persisted_status, should_fanout) =
        if session_action == UploadSessionAction::ResumeFinalization {
            // Release the advisory transaction lock before hashing a potentially
            // multi-gigabyte file. `finalizing` is itself the durable ownership
            // fence, and finalize_uploaded_transfer takes a short lock again for
            // the final database transition.
            txn.commit().await.map_err(db_err)?;
            let became_completed = finalize_uploaded_transfer(
                db,
                transfer_id,
                &part_path,
                &final_path,
                &final_path_db,
                file_size,
                checksum.as_deref(),
            )
            .await?;
            (chunks_completed, "completed".to_string(), became_completed)
        } else {
            let new_chunks = chunks_completed + 1;
            let target_status = if new_chunks >= chunks_total {
                "finalizing"
            } else {
                "in-progress"
            };

            if target_status == "finalizing" {
                // A complete last chunk must be durable before the database enters
                // finalizing. Full-file hashing happens after this transaction.
                let completed_path = match chunk_file_state {
                    ChunkFileState::Part => &part_path,
                    ChunkFileState::Final => &final_path,
                };
                let stored = fs::metadata(completed_path)
                    .await
                    .map_err(storage_err)?
                    .len() as i64;
                if stored != file_size {
                    txn.execute_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        "UPDATE federation_file_transfers SET status = 'failed' \
                     WHERE transfer_id = $1 AND status IN ('pending', 'in-progress')",
                        [transfer_id.into()],
                    ))
                    .await
                    .map_err(db_err)?;
                    txn.commit().await.map_err(db_err)?;
                    return Err(bad_request(format!(
                        "Completed file size mismatch: expected {}, got {}",
                        file_size, stored
                    )));
                }
            }

            let updated = txn
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"UPDATE federation_file_transfers
                   SET chunks_completed = $3,
                       status = $4,
                       local_path = $5,
                       completed_at = NULL
                   WHERE transfer_id = $1
                     AND chunks_completed = $2
                     AND status IN ('pending', 'in-progress')
                   RETURNING chunks_completed, status"#,
                    [
                        transfer_id.into(),
                        chunks_completed.into(),
                        new_chunks.into(),
                        target_status.into(),
                        final_path_db.clone().into(),
                    ],
                ))
                .await
                .map_err(db_err)?
                .ok_or_else(|| {
                    (
                        StatusCode::CONFLICT,
                        Json(AppError::public_json(
                            "Transfer progress changed while uploading chunk",
                        )),
                    )
                })?;

            let persisted_chunks: i32 = updated
                .try_get("", "chunks_completed")
                .unwrap_or(new_chunks);
            let mut persisted_status: String = updated
                .try_get("", "status")
                .unwrap_or_else(|_| target_status.to_string());

            // Filesystem is durable before this UPDATE. Same-index retry CONFLICTs
            // (VerifyCompletedRetry / ResumeFinalization are the other session_action arms).
            txn.commit().await.map_err(db_err)?;

            let should_fanout = if target_status == "finalizing" {
                let became_completed = finalize_uploaded_transfer(
                    db,
                    transfer_id,
                    &part_path,
                    &final_path,
                    &final_path_db,
                    file_size,
                    checksum.as_deref(),
                )
                .await?;
                persisted_status = "completed".to_string();
                became_completed
            } else {
                true
            };
            (persisted_chunks, persisted_status, should_fanout)
        };

    // Fan-out chunk: channel → single remote peer; room → all remote members
    if should_fanout {
        let base_url = get_base_url().await;
        let local_actor = actor_url(&base_url, username);
        if let Some(ref rid) = room_id {
            let activity_id = generate_activity_id(&base_url);
            let chunk_activity = json!({
                "@context": build_context(),
                "type": "myriad:FileTransfer",
                "id": &activity_id,
                "actor": &local_actor,
                "object": {
                    "type": "myriad:FileChunk",
                    "transferId": transfer_id,
                    "roomId": rid,
                    "chunkIndex": req.chunk_index,
                    "chunkSize": req.chunk_size,
                    "chunkData": &req.chunk_data,
                    "isLast": is_last_chunk
                }
            });
            crate::federation::room::fanout_to_remote_members_required(
                db,
                user_id,
                rid,
                &activity_id,
                &chunk_activity,
                "FileTransfer",
                "FileChunk",
            )
            .await
            .map_err(db_err)?;
        } else {
            let inbox = remote_inbox.filter(|i| !i.is_empty()).ok_or_else(|| {
                (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "Cannot send file chunk: remote inbox is missing",
                        "code": "remote_inbox_missing",
                    })),
                )
            })?;
            let activity_id = generate_activity_id(&base_url);
            let chunk_activity = json!({
                "@context": build_context(),
                "type": "myriad:FileTransfer",
                "id": &activity_id,
                "actor": &local_actor,
                "to": [&remote_actor_url],
                "object": {
                    "type": "myriad:FileChunk",
                    "transferId": transfer_id,
                    "channelId": channel_id,
                    "chunkIndex": req.chunk_index,
                    "chunkSize": req.chunk_size,
                    "chunkData": &req.chunk_data,
                    "isLast": is_last_chunk
                }
            });
            insert_and_enqueue_delivery(
                db,
                user_id,
                &activity_id,
                "FileTransfer",
                Some("FileChunk"),
                chunk_activity,
                &inbox,
            )
            .await
            .map_err(db_err)?;
        }
    }

    let progress = (persisted_chunks as f64 / chunks_total as f64) * 100.0;

    Ok(json!({
        "success": true,
        "transfer_id": transfer_id,
        "chunk_index": req.chunk_index,
        "chunks_completed": persisted_chunks,
        "chunks_total": chunks_total,
        "status": persisted_status,
        "bytes_transferred": stored_bytes(&final_path_db).await,
        "progress": progress
    }))
}

/// Open a completed transfer for download.
///
/// - Channel: only the local channel owner
/// - Room: any local room member
///
/// Bytes live under `local_path` after chunk upload finishes; the message
/// payload only carries `transfer_id` and is not self-contained.
pub async fn open_transfer_file(
    transfer_id: &str,
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
) -> Result<TransferFileContent, (StatusCode, Json<serde_json::Value>)> {
    // never open a path derived from an unvalidated transferId / DB local_path
    if !is_valid_transfer_id(transfer_id) {
        return Err(bad_request(
            "Invalid transferId: must be 1-128 chars of [A-Za-z0-9_-] only",
        ));
    }

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ft.transfer_id, ft.filename, ft.mime_type, ft.file_size,
                      ft.status, ft.local_path, ft.room_id, ft.owner_user_id,
                      c.user_id AS channel_user_id
               FROM federation_file_transfers ft
               LEFT JOIN federation_channels c
                 ON c.channel_id = ft.channel_id AND ft.channel_id <> ''
               WHERE ft.transfer_id = $1"#,
            [transfer_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Transfer not found")),
            )
        })?;

    let room_id: Option<String> = row
        .try_get::<Option<String>>("", "room_id")
        .unwrap_or(None)
        .filter(|s| !s.is_empty());
    if let Some(ref rid) = room_id {
        let base_url = get_base_url().await;
        let local_actor = actor_url(&base_url, username);
        let member = crate::federation::room::get_member_role(db, rid, &local_actor)
            .await
            .map_err(db_err)?;
        if member.is_none() {
            return Err((
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("Not a room member")),
            ));
        }
    } else {
        let channel_user: i32 = row
            .try_get::<Option<i32>>("", "channel_user_id")
            .unwrap_or(None)
            .unwrap_or(0);
        if channel_user != user_id {
            return Err((
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("Not your transfer")),
            ));
        }
    }

    let status: String = row.try_get("", "status").unwrap_or_default();
    if status != "completed" {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!("Transfer is not ready for download (status={})", status),
                "status": status,
            })),
        ));
    }

    let filename: String = row
        .try_get("", "filename")
        .unwrap_or_else(|_| "file".into());
    let mime_type: String = row
        .try_get::<Option<String>>("", "mime_type")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "application/octet-stream".into());
    let declared_size: i64 = row.try_get("", "file_size").unwrap_or(0);
    let local_path: Option<String> = row
        .try_get::<Option<String>>("", "local_path")
        .unwrap_or(None);

    let path = resolve_transfer_path(transfer_id, &filename, local_path.as_deref())
        .map_err(bad_request)?;

    let file = fs::File::open(&path).await.map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Transfer file missing on disk",
                "transfer_id": transfer_id,
            })),
        )
    })?;
    let meta = file.metadata().await.map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Transfer file missing on disk")),
        )
    })?;
    if !meta.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Transfer path is not a file")),
        ));
    }

    let file_size = meta.len();
    if declared_size > 0 && file_size == 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(AppError::public_json("Transfer file is empty")),
        ));
    }

    Ok(TransferFileContent {
        filename: safe_filename(&filename),
        mime_type,
        file_size,
        file,
    })
}

/// 获取传输进度
pub async fn get_transfer(
    transfer_id: &str,
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
) -> Result<TransferDetail, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ft.transfer_id, ft.channel_id, ft.room_id, ft.filename, ft.file_size,
                      ft.mime_type, ft.checksum_sha256, ft.status, ft.direction,
                      ft.chunks_total, ft.chunks_completed,
                      ft.local_path, ft.created_at, ft.completed_at,
                      c.user_id AS channel_user_id
               FROM federation_file_transfers ft
               LEFT JOIN federation_channels c
                 ON c.channel_id = ft.channel_id AND ft.channel_id <> ''
               WHERE ft.transfer_id = $1"#,
            [transfer_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Transfer not found")),
            )
        })?;

    let room_id: Option<String> = row
        .try_get::<Option<String>>("", "room_id")
        .unwrap_or(None)
        .filter(|s| !s.is_empty());
    if let Some(ref rid) = room_id {
        let base_url = get_base_url().await;
        let local_actor = actor_url(&base_url, username);
        if crate::federation::room::get_member_role(db, rid, &local_actor)
            .await
            .map_err(db_err)?
            .is_none()
        {
            return Err((
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("Not a room member")),
            ));
        }
    } else {
        let channel_user: i32 = row
            .try_get::<Option<i32>>("", "channel_user_id")
            .unwrap_or(None)
            .unwrap_or(0);
        if channel_user != user_id {
            return Err((
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("Not your transfer")),
            ));
        }
    }

    let chunks_total: i32 = row.try_get("", "chunks_total").unwrap_or(1);
    let chunks_completed: i32 = row.try_get("", "chunks_completed").unwrap_or(0);
    let local_path: Option<String> = row
        .try_get::<Option<String>>("", "local_path")
        .unwrap_or(None);
    let progress = if chunks_total > 0 {
        (chunks_completed as f64 / chunks_total as f64) * 100.0
    } else {
        0.0
    };

    Ok(TransferDetail {
        transfer_id: row.try_get("", "transfer_id").unwrap_or_default(),
        channel_id: row.try_get("", "channel_id").unwrap_or_default(),
        room_id,
        filename: row.try_get("", "filename").unwrap_or_default(),
        file_size: row.try_get("", "file_size").unwrap_or(0),
        mime_type: row
            .try_get::<Option<String>>("", "mime_type")
            .unwrap_or(None),
        checksum: row
            .try_get::<Option<String>>("", "checksum_sha256")
            .unwrap_or(None),
        status: row.try_get("", "status").unwrap_or_default(),
        direction: row.try_get("", "direction").unwrap_or_default(),
        chunks_total,
        chunks_received: chunks_completed,
        bytes_transferred: stored_bytes(local_path.as_deref().unwrap_or("")).await,
        progress,
        created_at: row
            .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
            .map(|t| t.to_rfc3339())
            .unwrap_or_default(),
        completed_at: row
            .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "completed_at")
            .ok()
            .flatten()
            .map(|t| t.to_rfc3339()),
    })
}

/// 列出 Channel 上的所有文件传输
pub async fn list_transfers(
    channel_id: &str,
    user_id: i32,
    db: &DatabaseConnection,
) -> Result<Vec<TransferSummary>, (StatusCode, Json<serde_json::Value>)> {
    // 验证用户对该 Channel 的所有权
    let ch_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM federation_channels WHERE channel_id = $1",
            [channel_id.into()],
        ))
        .await
        .map_err(db_err)?;

    match ch_row {
        Some(r) => {
            let channel_user: i32 = r.try_get("", "user_id").unwrap_or(0);
            if channel_user != user_id {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(AppError::public_json("Not your channel")),
                ));
            }
        }
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Channel not found")),
            ));
        }
    }

    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT transfer_id, channel_id, filename, file_size, mime_type,
                      status, direction, chunks_total, chunks_completed, created_at
               FROM federation_file_transfers
               WHERE channel_id = $1
               ORDER BY created_at DESC"#,
            [channel_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let transfers = rows
        .iter()
        .map(|r| {
            let ct: i32 = r.try_get("", "chunks_total").unwrap_or(1);
            let cr: i32 = r.try_get("", "chunks_completed").unwrap_or(0);
            let progress = if ct > 0 {
                (cr as f64 / ct as f64) * 100.0
            } else {
                0.0
            };
            TransferSummary {
                transfer_id: r.try_get("", "transfer_id").unwrap_or_default(),
                channel_id: r.try_get("", "channel_id").unwrap_or_default(),
                room_id: None,
                filename: r.try_get("", "filename").unwrap_or_default(),
                file_size: r.try_get("", "file_size").unwrap_or(0),
                mime_type: r.try_get::<Option<String>>("", "mime_type").unwrap_or(None),
                status: r.try_get("", "status").unwrap_or_default(),
                direction: r.try_get("", "direction").unwrap_or_default(),
                progress,
                created_at: r
                    .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
            }
        })
        .collect();

    Ok(transfers)
}

/// 列出 Room 上的文件传输（成员可见）
pub async fn list_room_transfers(
    room_id: &str,
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
) -> Result<Vec<TransferSummary>, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    if crate::federation::room::get_member_role(db, room_id, &local_actor)
        .await
        .map_err(db_err)?
        .is_none()
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Not a room member")),
        ));
    }
    let _ = user_id; // membership is the gate

    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT transfer_id, channel_id, room_id, filename, file_size, mime_type,
                      status, direction, chunks_total, chunks_completed, created_at
               FROM federation_file_transfers
               WHERE room_id = $1
               ORDER BY created_at DESC"#,
            [room_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let transfers = rows
        .iter()
        .map(|r| {
            let ct: i32 = r.try_get("", "chunks_total").unwrap_or(1);
            let cr: i32 = r.try_get("", "chunks_completed").unwrap_or(0);
            let progress = if ct > 0 {
                (cr as f64 / ct as f64) * 100.0
            } else {
                0.0
            };
            TransferSummary {
                transfer_id: r.try_get("", "transfer_id").unwrap_or_default(),
                channel_id: r.try_get("", "channel_id").unwrap_or_default(),
                room_id: r.try_get::<Option<String>>("", "room_id").unwrap_or(None),
                filename: r.try_get("", "filename").unwrap_or_default(),
                file_size: r.try_get("", "file_size").unwrap_or(0),
                mime_type: r.try_get::<Option<String>>("", "mime_type").unwrap_or(None),
                status: r.try_get("", "status").unwrap_or_default(),
                direction: r.try_get("", "direction").unwrap_or_default(),
                progress,
                created_at: r
                    .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
            }
        })
        .collect();

    Ok(transfers)
}

/// 取消文件传输（本机 + 联邦通知对端）
pub async fn cancel_transfer(
    user_id: i32,
    username: &str,
    transfer_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let txn = db.begin().await.map_err(db_err)?;
    lock_transfer_session(&txn, transfer_id)
        .await
        .map_err(db_err)?;

    // 验证所有权（channel owner 或 room transfer owner）
    let row = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ft.status, ft.owner_user_id, ft.room_id, ft.channel_id,
                      c.user_id AS channel_user_id, ra.actor_url, ra.inbox_url
               FROM federation_file_transfers ft
               LEFT JOIN federation_channels c
                 ON c.channel_id = ft.channel_id AND ft.channel_id <> ''
               LEFT JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE ft.transfer_id = $1"#,
            [transfer_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Transfer not found")),
            )
        })?;

    let room_id: Option<String> = row
        .try_get::<Option<String>>("", "room_id")
        .unwrap_or(None)
        .filter(|s| !s.is_empty());
    let channel_id: String = row.try_get("", "channel_id").unwrap_or_default();
    let allowed = if room_id.is_some() {
        row.try_get::<Option<i32>>("", "owner_user_id")
            .unwrap_or(None)
            == Some(user_id)
    } else {
        row.try_get::<Option<i32>>("", "channel_user_id")
            .unwrap_or(None)
            == Some(user_id)
    };
    if !allowed {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Not your transfer")),
        ));
    }

    let status: String = row.try_get("", "status").unwrap_or_default();
    if !["pending", "in-progress"].contains(&status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Transfer is not ready")),
        ));
    }

    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_file_transfers SET status = 'cancelled' \
         WHERE transfer_id = $1 AND status IN ('pending', 'in-progress')",
        [transfer_id.into()],
    ))
    .await
    .map_err(db_err)?;

    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    let activity_id = generate_activity_id(&base_url);
    let cancel_object = json!({
        "type": "myriad:FileCancel",
        "transferId": transfer_id,
        "channelId": if channel_id.is_empty() { serde_json::Value::Null } else { json!(channel_id) },
        "roomId": room_id.clone(),
        "status": "cancelled"
    });

    if let Some(ref rid) = room_id {
        let cancel_activity = json!({
            "@context": build_context(),
            "type": "myriad:FileTransfer",
            "id": &activity_id,
            "actor": &local_actor,
            "object": cancel_object
        });
        crate::federation::room::fanout_to_remote_members_required(
            &txn,
            user_id,
            rid,
            &activity_id,
            &cancel_activity,
            "FileTransfer",
            "FileCancel",
        )
        .await
        .map_err(db_err)?;
    } else if !channel_id.is_empty() {
        let remote_inbox = row
            .try_get::<Option<String>>("", "inbox_url")
            .unwrap_or(None)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "Cannot cancel transfer: remote inbox is missing",
                        "code": "remote_inbox_missing",
                    })),
                )
            })?;
        let remote_actor: String = row.try_get("", "actor_url").unwrap_or_default();
        let cancel_activity = json!({
            "@context": build_context(),
            "type": "myriad:FileTransfer",
            "id": &activity_id,
            "actor": &local_actor,
            "to": [&remote_actor],
            "object": cancel_object
        });
        insert_and_enqueue_delivery(
            &txn,
            user_id,
            &activity_id,
            "FileTransfer",
            Some("FileCancel"),
            cancel_activity,
            &remote_inbox,
        )
        .await
        .map_err(db_err)?;
    }
    txn.commit().await.map_err(db_err)?;

    if let Some(ref rid) = room_id {
        crate::federation::ws_gateway::broadcast_to_room(
            rid,
            &json!({
                "type": "transfer_cancelled",
                "room_id": rid,
                "transfer_id": transfer_id
            }),
        )
        .await;
    } else if !channel_id.is_empty() {
        crate::federation::ws_gateway::broadcast_to_channel(
            &channel_id,
            &json!({
                "type": "transfer_cancelled",
                "channel_id": channel_id,
                "transfer_id": transfer_id
            }),
        )
        .await;
    }

    Ok(json!({
        "success": true,
        "transfer_id": transfer_id,
        "status": "cancelled"
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn initiate_persists_transfer_with_outbound_intent() {
        let src = include_str!("http.rs");
        assert!(src.contains("insert_local_activity"));
        assert!(src.contains("enqueue_delivery"));
        assert!(src.contains("fanout_to_remote_members_required"));
        // Built at runtime so this assertion's own literal is not the needle it
        // forbids (include_str! would otherwise always match it).
        let ignored =
            ["let _ = crate::federation::room::", "fanout_to_remote_members"].concat();
        assert!(
            !src.contains(&ignored),
            "room transfer fanout must not be ignored after pending insert"
        );
        let chunk = src
            .split("if should_fanout")
            .nth(1)
            .expect("chunk fanout");
        assert!(chunk.contains("fanout_to_remote_members_required"));
        assert!(chunk.contains("insert_and_enqueue_delivery"));
        assert!(!chunk.contains("if let Ok(Some(act_row))"));
        let cancel = src
            .split("pub async fn cancel_transfer")
            .nth(1)
            .and_then(|rest| rest.split("#[cfg(test)]").next())
            .expect("cancel_transfer");
        assert!(cancel.contains("fanout_to_remote_members_required"));
        assert!(cancel.contains("insert_and_enqueue_delivery"));
        assert!(cancel.contains("txn.commit()"));
        assert!(!cancel.contains("if let Ok(Some(act_row))"));
    }
}
