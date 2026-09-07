//! Inbound `myriad:FileTransfer` activity handling (FileMeta, FileChunk, FileCancel).

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;
use tokio::fs;

use super::storage::{
    admit_chunk_bytes_str, admit_new_transfer_str, final_file_path, finalize_part_file,
    http_err_to_string, is_strictly_under, is_valid_transfer_id, lock_transfer_session,
    part_file_path, path_to_db, prepare_chunk_file, resolve_transfer_path, sha256_file,
    storage_root, ChunkFileState, DEFAULT_CHUNK_SIZE, MAX_FILE_SIZE,
};

// Inbox 处理

/// 处理收到的文件传输 Activity（从远程实例）
pub async fn handle_file_transfer(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let object_type = object.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if object_type == "myriad:FileChunk" {
        return handle_file_chunk(db, actor_url_str, object).await;
    }
    if object_type == "myriad:FileCancel" {
        return handle_file_cancel(db, actor_url_str, object).await;
    }

    let transfer_id = object
        .get("transferId")
        .and_then(|v| v.as_str())
        .ok_or("Missing transferId")?;
    // MYR-001: remote transferId must pass strict validation before any path use
    if !is_valid_transfer_id(transfer_id) {
        return Err("Invalid transferId: must be 1-128 chars of [A-Za-z0-9_-] only".into());
    }
    let channel_id = object.get("channelId").and_then(|v| v.as_str());
    let room_id = object.get("roomId").and_then(|v| v.as_str());
    if channel_id.is_none() && room_id.is_none() {
        return Err("Missing channelId or roomId".into());
    }
    let filename = object
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let file_size: i64 = object.get("fileSize").and_then(|v| v.as_i64()).unwrap_or(0);
    let mime_type = object.get("mimeType").and_then(|v| v.as_str());
    let checksum = object.get("checksum").and_then(|v| v.as_str());
    let chunks_total: i32 = object
        .get("chunksTotal")
        .and_then(|v| v.as_i64())
        .unwrap_or(1) as i32;
    if file_size <= 0 || file_size > MAX_FILE_SIZE {
        return Err(format!(
            "Invalid file size {}, max {}",
            file_size, MAX_FILE_SIZE
        ));
    }
    if chunks_total <= 0 {
        return Err("Invalid chunksTotal".to_string());
    }

    // MYR-008: global concurrent transfer admission for inbound FileMeta
    admit_new_transfer_str(db, file_size).await?;

    if let Some(cid) = channel_id {
        let channel_actor = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT ra.actor_url
                   FROM federation_channels c
                   JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
                   WHERE c.channel_id = $1"#,
                [cid.into()],
            ))
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Channel {} not found", cid))?;
        let expected_actor: String = channel_actor.try_get("", "actor_url").unwrap_or_default();
        if !crate::federation::types::same_actor_url(&expected_actor, actor_url_str) {
            return Err(format!(
                "File transfer sender mismatch: expected {}, got {}",
                expected_actor, actor_url_str
            ));
        }
    } else if let Some(rid) = room_id {
        // Sender must be a known room member (remote or local)
        let member = crate::federation::room::get_member_role(db, rid, actor_url_str)
            .await
            .map_err(|e| e.to_string())?;
        if member.is_none() {
            return Err(format!(
                "File transfer actor {} is not a member of room {}",
                actor_url_str, rid
            ));
        }
    }

    let local_path = path_to_db(&final_file_path(transfer_id, filename)?);

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_file_transfers
           (transfer_id, channel_id, room_id, filename, file_size, mime_type,
            checksum_sha256, status, direction, chunks_total, chunks_completed, local_path, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', 'inbound', $8, 0, $9, NOW())
           ON CONFLICT (transfer_id) DO NOTHING"#,
        [
            transfer_id.into(),
            channel_id.unwrap_or("").into(),
            room_id.into(),
            filename.into(),
            file_size.into(),
            mime_type.into(),
            checksum.into(),
            chunks_total.into(),
            local_path.into(),
        ],
    ))
    .await
    .map_err(|e| e.to_string())?;

    tracing::info!(
        "[FileTransfer] Inbound transfer {} from {} — {} ({} bytes, {} chunks)",
        transfer_id,
        actor_url_str,
        filename,
        file_size,
        chunks_total
    );

    Ok(())
}

async fn handle_file_chunk(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    object: &serde_json::Value,
) -> Result<(), String> {
    let transfer_id = object
        .get("transferId")
        .and_then(|v| v.as_str())
        .ok_or("Missing transferId")?;
    // MYR-001: reject path-traversal transferIds before FS write
    if !is_valid_transfer_id(transfer_id) {
        return Err("Invalid transferId: must be 1-128 chars of [A-Za-z0-9_-] only".into());
    }
    // Inbox dispatch supplies the enclosing receipt transaction, so this lock
    // covers the filesystem mutation until the receipt/database commit.
    lock_transfer_session(db, transfer_id)
        .await
        .map_err(|error| error.to_string())?;
    let chunk_index = object
        .get("chunkIndex")
        .and_then(|v| v.as_i64())
        .ok_or("Missing chunkIndex")? as i32;
    let chunk_size = object
        .get("chunkSize")
        .and_then(|v| v.as_i64())
        .ok_or("Missing chunkSize")?;
    let chunk_data = object
        .get("chunkData")
        .and_then(|v| v.as_str())
        .ok_or("Missing chunkData")?;

    if chunk_size <= 0 || chunk_size > DEFAULT_CHUNK_SIZE {
        return Err(format!(
            "chunk_size must be between 1 and {} bytes",
            DEFAULT_CHUNK_SIZE
        ));
    }
    // MYR-008: reserve decoded chunk budget for inbound FileChunk
    let _chunk_budget = admit_chunk_bytes_str(chunk_size)?;

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ft.status, ft.chunks_total, ft.chunks_completed,
                      ft.file_size, ft.filename, ft.checksum_sha256, ft.local_path,
                      ft.room_id, ft.channel_id,
                      ra.actor_url AS channel_remote_actor
               FROM federation_file_transfers ft
               LEFT JOIN federation_channels c
                 ON c.channel_id = ft.channel_id AND ft.channel_id <> ''
               LEFT JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE ft.transfer_id = $1"#,
            [transfer_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Transfer {} not found", transfer_id))?;

    let room_id: Option<String> = row
        .try_get::<Option<String>>("", "room_id")
        .unwrap_or(None)
        .filter(|s| !s.is_empty());
    if let Some(ref rid) = room_id {
        let member = crate::federation::room::get_member_role(db, rid, actor_url_str)
            .await
            .map_err(|e| e.to_string())?;
        if member.is_none() {
            return Err(format!(
                "File chunk actor {} is not a member of room {}",
                actor_url_str, rid
            ));
        }
    } else {
        let expected_actor: String = row
            .try_get::<Option<String>>("", "channel_remote_actor")
            .unwrap_or(None)
            .unwrap_or_default();
        if !crate::federation::types::same_actor_url(&expected_actor, actor_url_str) {
            return Err(format!(
                "File chunk sender mismatch: expected {}, got {}",
                expected_actor, actor_url_str
            ));
        }
    }

    let status: String = row.try_get("", "status").unwrap_or_default();
    if !["pending", "in-progress"].contains(&status.as_str()) {
        return Err(format!("Transfer is {}", status));
    }

    let chunks_total: i32 = row.try_get("", "chunks_total").unwrap_or(1);
    let chunks_completed: i32 = row.try_get("", "chunks_completed").unwrap_or(0);
    let file_size: i64 = row.try_get("", "file_size").unwrap_or(0);
    let filename: String = row
        .try_get("", "filename")
        .unwrap_or_else(|_| "file".to_string());
    let checksum: Option<String> = row
        .try_get::<Option<String>>("", "checksum_sha256")
        .unwrap_or(None);
    let local_path: Option<String> = row
        .try_get::<Option<String>>("", "local_path")
        .unwrap_or(None);

    if chunk_index < 0 || chunk_index >= chunks_total {
        return Err("Chunk index out of range".to_string());
    }
    if chunk_index != chunks_completed {
        return Err(format!(
            "Chunks must be received in order: expected {}, got {}",
            chunks_completed, chunk_index
        ));
    }

    let decoded = BASE64
        .decode(chunk_data.as_bytes())
        .map_err(|_| "Invalid base64 chunkData".to_string())?;
    if decoded.len() as i64 != chunk_size {
        return Err("chunkSize does not match decoded data length".to_string());
    }

    let is_last_chunk = chunk_index == chunks_total - 1;
    let expected_size = if is_last_chunk {
        file_size - (DEFAULT_CHUNK_SIZE * (chunks_total as i64 - 1))
    } else {
        DEFAULT_CHUNK_SIZE
    };
    if expected_size <= 0 || chunk_size != expected_size {
        return Err(format!(
            "Invalid chunk size: expected {}, got {}",
            expected_size, chunk_size
        ));
    }

    let final_path = resolve_transfer_path(transfer_id, &filename, local_path.as_deref())?;
    let final_path_db = path_to_db(&final_path);
    let part_path = part_file_path(&final_path);
    if !is_strictly_under(&storage_root(), &part_path) {
        return Err("Transfer path escapes storage root".into());
    }
    let expected_offset = DEFAULT_CHUNK_SIZE * chunks_completed as i64;

    let chunk_file_state = prepare_chunk_file(
        &part_path,
        &final_path,
        &decoded,
        expected_offset,
        is_last_chunk,
    )
    .await
    .map_err(http_err_to_string)?;

    let new_chunks = chunks_completed + 1;
    let new_status = if new_chunks >= chunks_total {
        "completed"
    } else {
        "in-progress"
    };

    if new_status == "completed" {
        // sha256 / rename only on paths already confined by resolve_transfer_path + part check
        debug_assert!(
            is_strictly_under(&storage_root(), &part_path)
                && is_strictly_under(&storage_root(), &final_path)
        );
        let completed_path = match chunk_file_state {
            ChunkFileState::Part => &part_path,
            ChunkFileState::Final => &final_path,
        };
        let stored = fs::metadata(completed_path)
            .await
            .map_err(|e| e.to_string())?
            .len() as i64;
        if stored != file_size {
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE federation_file_transfers SET status = 'failed' WHERE transfer_id = $1",
                [transfer_id.into()],
            ))
            .await
            .map_err(|e| e.to_string())?;
            return Err(format!(
                "Completed file size mismatch: expected {}, got {}",
                file_size, stored
            ));
        }

        if let Some(expected_checksum) = checksum.as_deref().filter(|s| !s.is_empty()) {
            let actual = sha256_file(completed_path)
                .await
                .map_err(|e| e.to_string())?;
            if !actual.eq_ignore_ascii_case(expected_checksum) {
                db.execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE federation_file_transfers SET status = 'failed' WHERE transfer_id = $1",
                    [transfer_id.into()],
                ))
                .await
                .map_err(|e| e.to_string())?;
                return Err("SHA-256 checksum mismatch".to_string());
            }
        }

        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
        }
        if chunk_file_state == ChunkFileState::Part {
            finalize_part_file(&part_path, &final_path, file_size, checksum.as_deref())
                .await
                .map_err(http_err_to_string)?;
        }
    }

    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_file_transfers
               SET chunks_completed = $2,
                   status = $3,
                   local_path = $4,
                   completed_at = CASE WHEN $3 = 'completed' THEN NOW() ELSE NULL END
               WHERE transfer_id = $1
                 AND chunks_completed = $5
                 AND status IN ('pending', 'in-progress')"#,
            [
                transfer_id.into(),
                new_chunks.into(),
                new_status.into(),
                final_path_db.into(),
                chunks_completed.into(),
            ],
        ))
        .await
        .map_err(|e| e.to_string())?;

    // CAS miss: do not broadcast success; another writer won the race.
    if result.rows_affected() == 0 {
        tracing::warn!(
            "[FileTransfer] CAS miss receiving chunk for {} (progress changed)",
            transfer_id
        );
        // Never delete on a *completed* CAS miss — the other writer may already
        // own the final file / still need the .part. Protocol is sequential, so
        // non-complete CAS races are rare; only best-effort clean orphan .part.
        if new_status != "completed" {
            let _ = fs::remove_file(&part_path).await;
        }
        return Err("Transfer progress changed while receiving chunk".into());
    }

    tracing::info!(
        "[FileTransfer] Received chunk {}/{} for {} from {}",
        new_chunks,
        chunks_total,
        transfer_id,
        actor_url_str
    );

    // Live UI progress for open Aro clients
    let ch_id: String = row.try_get("", "channel_id").unwrap_or_default();
    let progress = if chunks_total > 0 {
        (new_chunks as f64 / chunks_total as f64) * 100.0
    } else {
        0.0
    };
    let evt = json!({
        "type": if new_status == "completed" { "transfer_completed" } else { "transfer_progress" },
        "transfer_id": transfer_id,
        "chunks_completed": new_chunks,
        "chunks_total": chunks_total,
        "progress": progress,
        "status": new_status
    });
    if let Some(ref rid) = room_id {
        let mut e = evt.clone();
        e["room_id"] = json!(rid);
        crate::federation::ws_gateway::broadcast_to_room(rid, &e).await;
    } else if !ch_id.is_empty() {
        let mut e = evt;
        e["channel_id"] = json!(ch_id);
        crate::federation::ws_gateway::broadcast_to_channel(&ch_id, &e).await;
    }

    Ok(())
}

/// Inbound cancel: mark transfer cancelled and notify local clients.
async fn handle_file_cancel(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    object: &serde_json::Value,
) -> Result<(), String> {
    let transfer_id = object
        .get("transferId")
        .and_then(|v| v.as_str())
        .ok_or("Missing transferId")?;
    if !is_valid_transfer_id(transfer_id) {
        return Err("Invalid transferId: must be 1-128 chars of [A-Za-z0-9_-] only".into());
    }
    lock_transfer_session(db, transfer_id)
        .await
        .map_err(|error| error.to_string())?;
    let room_id = object.get("roomId").and_then(|v| v.as_str());
    let channel_id = object.get("channelId").and_then(|v| v.as_str());

    if let Some(rid) = room_id {
        let member = crate::federation::room::get_member_role(db, rid, actor_url_str)
            .await
            .map_err(|e| e.to_string())?;
        if member.is_none() {
            return Err(format!(
                "File cancel actor {} is not a member of room {}",
                actor_url_str, rid
            ));
        }
    }

    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_file_transfers
               SET status = 'cancelled'
               WHERE transfer_id = $1
                 AND status IN ('pending', 'in-progress')"#,
            [transfer_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if result.rows_affected() == 0 {
        tracing::debug!(
            "[FileTransfer] cancel no-op for {} (missing or already final)",
            transfer_id
        );
        return Ok(());
    }

    let evt = json!({
        "type": "transfer_cancelled",
        "transfer_id": transfer_id,
        "from": actor_url_str
    });
    if let Some(rid) = room_id {
        let mut e = evt.clone();
        e["room_id"] = json!(rid);
        crate::federation::ws_gateway::broadcast_to_room(rid, &e).await;
    } else if let Some(cid) = channel_id.filter(|s| !s.is_empty()) {
        let mut e = evt;
        e["channel_id"] = json!(cid);
        crate::federation::ws_gateway::broadcast_to_channel(cid, &e).await;
    }

    tracing::info!(
        "[FileTransfer] Transfer {} cancelled by remote {}",
        transfer_id,
        actor_url_str
    );
    Ok(())
}
