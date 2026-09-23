//! Inbound `myriad:FileTransfer` activity handling (FileMeta, FileChunk, FileCancel).

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;
use tokio::fs;

use super::storage::{
    ChunkFileState, DEFAULT_CHUNK_SIZE, MAX_FILE_SIZE, admit_chunk_bytes_str,
    admit_new_transfer_str, final_file_path, finalize_part_file, http_err_to_string,
    is_strictly_under, is_valid_transfer_id, lock_transfer_admission, lock_transfer_session,
    part_file_path, path_to_db, prepare_chunk_file, resolve_transfer_path,
    run_transfer_file_work, sha256_file, storage_root,
};

// Inbox 处理

/// 处理收到的文件传输 Activity（从远程实例）：FileMeta 与 FileCancel。
///
/// `myriad:FileChunk` has its own classified entry point, [`handle_file_chunk`].
/// The returned notice is broadcast by the caller after its transaction commits.
pub async fn handle_file_transfer(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<Option<TransferNotice>, String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let object_type = object.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if object_type == "myriad:FileChunk" {
        return Err("FileChunk must be dispatched to handle_file_chunk".into());
    }
    if object_type == "myriad:FileCancel" {
        return handle_file_cancel(db, actor_url_str, object).await;
    }

    let transfer_id = object
        .get("transferId")
        .and_then(|v| v.as_str())
        .ok_or("Missing transferId")?;
    // remote transferId must pass strict validation before any path use
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

    // Same admission lock as outbound initiate. Inbox receipt already holds a
    // transaction, so the xact lock covers check + insert until commit.
    lock_transfer_admission(db)
        .await
        .map_err(|error| error.to_string())?;
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

    Ok(None)
}

/// Live-UI event for local clients of a channel/room. Built by the inbound
/// handlers and broadcast by the caller only after the receipt transaction has
/// committed, so clients never see progress that a rollback later undoes.
#[derive(Debug, Clone)]
pub struct TransferNotice {
    room_id: Option<String>,
    channel_id: Option<String>,
    event: serde_json::Value,
}

impl TransferNotice {
    fn new(room_id: Option<&str>, channel_id: Option<&str>, event: serde_json::Value) -> Option<Self> {
        let room_id = room_id.filter(|s| !s.is_empty()).map(str::to_string);
        let channel_id = channel_id.filter(|s| !s.is_empty()).map(str::to_string);
        (room_id.is_some() || channel_id.is_some()).then_some(Self {
            room_id,
            channel_id,
            event,
        })
    }

    pub async fn broadcast(self) {
        let mut event = self.event;
        if let Some(rid) = self.room_id {
            event["room_id"] = json!(rid);
            crate::federation::ws_gateway::broadcast_to_room(&rid, &event).await;
        } else if let Some(cid) = self.channel_id {
            event["channel_id"] = json!(cid);
            crate::federation::ws_gateway::broadcast_to_channel(&cid, &event).await;
        }
    }
}

/// Classified inbound `myriad:FileChunk` failure. The HTTP status decides
/// whether the receipt is durably rejected (4xx) or rolled back for a retry
/// (5xx); the message is for logs only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundChunkError {
    /// Malformed or out-of-contract chunk. Permanent.
    Invalid(String),
    /// Sender is not the channel peer / a room member. Permanent.
    Forbidden(String),
    /// Transfer already cancelled or failed. Permanent.
    Closed(String),
    /// Transfer metadata or an earlier chunk has not arrived yet. Retryable.
    NotReady(String),
    /// In-flight chunk byte budget exhausted. Retryable.
    Busy(String),
    /// Database or filesystem failure. Retryable; nothing is committed.
    Internal(String),
}

impl InboundChunkError {
    pub fn status(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;
        match self {
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Closed(_) => StatusCode::GONE,
            Self::NotReady(_) | Self::Busy(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Invalid(m)
            | Self::Forbidden(m)
            | Self::Closed(m)
            | Self::NotReady(m)
            | Self::Busy(m)
            | Self::Internal(m) => m,
        }
    }
}

fn internal(error: impl std::fmt::Display) -> InboundChunkError {
    InboundChunkError::Internal(error.to_string())
}

fn storage_failure(err: (axum::http::StatusCode, axum::Json<serde_json::Value>)) -> InboundChunkError {
    if err.0 == axum::http::StatusCode::BAD_REQUEST {
        InboundChunkError::Invalid(http_err_to_string(err))
    } else {
        InboundChunkError::Internal(http_err_to_string(err))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChunkFileOutcome {
    /// Non-final chunk written (or verified) and durable.
    Progress,
    /// Final chunk durable, size/checksum verified, final file published.
    Completed,
    /// All bytes received but the assembled file does not match the metadata.
    Corrupt(String),
}

/// Filesystem phase of one inbound chunk: the database-owned offset decides
/// where bytes go (see `prepare_chunk_file`), and every branch leaves the data
/// and new directory entries synced before returning.
async fn write_inbound_chunk(
    part_path: std::path::PathBuf,
    final_path: std::path::PathBuf,
    decoded: Vec<u8>,
    expected_offset: i64,
    is_last_chunk: bool,
    file_size: i64,
    checksum: Option<String>,
) -> Result<ChunkFileOutcome, InboundChunkError> {
    let state = prepare_chunk_file(
        &part_path,
        &final_path,
        &decoded,
        expected_offset,
        is_last_chunk,
    )
    .await
    .map_err(storage_failure)?;
    if !is_last_chunk {
        return Ok(ChunkFileOutcome::Progress);
    }

    let completed_path = match state {
        ChunkFileState::Part => &part_path,
        ChunkFileState::Final => &final_path,
    };
    let stored = fs::metadata(completed_path).await.map_err(internal)?.len() as i64;
    if stored != file_size {
        return Ok(ChunkFileOutcome::Corrupt(format!(
            "Completed file size mismatch: expected {file_size}, got {stored}"
        )));
    }
    if let Some(expected) = checksum.as_deref().filter(|s| !s.is_empty()) {
        let actual = sha256_file(completed_path).await.map_err(internal)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Ok(ChunkFileOutcome::Corrupt("SHA-256 checksum mismatch".into()));
        }
    }
    if state == ChunkFileState::Part {
        finalize_part_file(&part_path, &final_path, file_size, checksum.as_deref())
            .await
            .map_err(storage_failure)?;
    }
    Ok(ChunkFileOutcome::Completed)
}

/// Inbound `myriad:FileChunk` from a same-version peer.
///
/// Runs on the caller's receipt transaction: the per-transfer advisory lock
/// serializes it with every other writer of this transfer until commit, the
/// file bytes are made durable first, and only then is progress recorded in
/// the same transaction as the receipt. If that transaction rolls back, the
/// synced chunk stays in place and a retry verifies or rewrites it at the
/// database offset. Returns the live-UI notice for the caller to broadcast
/// after commit.
pub async fn handle_file_chunk(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    object: &serde_json::Value,
) -> Result<Option<TransferNotice>, InboundChunkError> {
    use InboundChunkError::{Closed, Forbidden, Invalid, NotReady};

    let transfer_id = object
        .get("transferId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Invalid("Missing transferId".into()))?;
    // reject path-traversal transferIds before FS write
    if !is_valid_transfer_id(transfer_id) {
        return Err(Invalid(
            "Invalid transferId: must be 1-128 chars of [A-Za-z0-9_-] only".into(),
        ));
    }
    let chunk_index = object
        .get("chunkIndex")
        .and_then(|v| v.as_i64())
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| Invalid("Missing or invalid chunkIndex".into()))?;
    let chunk_size = object
        .get("chunkSize")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Invalid("Missing chunkSize".into()))?;
    let chunk_data = object
        .get("chunkData")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Invalid("Missing chunkData".into()))?;
    if chunk_size <= 0 || chunk_size > DEFAULT_CHUNK_SIZE {
        return Err(Invalid(format!(
            "chunk_size must be between 1 and {DEFAULT_CHUNK_SIZE} bytes"
        )));
    }

    lock_transfer_session(db, transfer_id)
        .await
        .map_err(internal)?;

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ft.direction, ft.status, ft.chunks_total, ft.chunks_completed,
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
        .map_err(internal)?
        .ok_or_else(|| {
            NotReady(format!(
                "Transfer {transfer_id} not yet present; retry after FileTransfer"
            ))
        })?;

    let direction: String = row.try_get("", "direction").map_err(internal)?;
    if direction != "inbound" {
        return Err(Forbidden(format!(
            "Transfer {transfer_id} is not an inbound transfer"
        )));
    }
    let room_id: Option<String> = row
        .try_get::<Option<String>>("", "room_id")
        .map_err(internal)?
        .filter(|s| !s.is_empty());
    let channel_id: Option<String> = row
        .try_get::<Option<String>>("", "channel_id")
        .map_err(internal)?;
    if let Some(ref rid) = room_id {
        let member = crate::federation::room::get_member_role(db, rid, actor_url_str)
            .await
            .map_err(internal)?;
        if member.is_none() {
            return Err(Forbidden(format!(
                "File chunk actor {actor_url_str} is not a member of room {rid}"
            )));
        }
    } else {
        let expected_actor: Option<String> = row
            .try_get::<Option<String>>("", "channel_remote_actor")
            .map_err(internal)?;
        if !expected_actor
            .as_deref()
            .is_some_and(|expected| crate::federation::types::same_actor_url(expected, actor_url_str))
        {
            return Err(Forbidden(format!(
                "File chunk sender mismatch: expected {expected_actor:?}, got {actor_url_str}"
            )));
        }
    }

    let status: String = row.try_get("", "status").map_err(internal)?;
    let chunks_total: i32 = row.try_get("", "chunks_total").map_err(internal)?;
    let chunks_completed: i32 = row.try_get("", "chunks_completed").map_err(internal)?;
    let file_size: i64 = row.try_get("", "file_size").map_err(internal)?;
    let filename: String = row.try_get("", "filename").map_err(internal)?;
    let checksum: Option<String> = row
        .try_get::<Option<String>>("", "checksum_sha256")
        .map_err(internal)?;
    let local_path: Option<String> = row
        .try_get::<Option<String>>("", "local_path")
        .map_err(internal)?;

    if chunk_index < 0 || chunk_index >= chunks_total {
        return Err(Invalid("Chunk index out of range".into()));
    }
    if chunk_index < chunks_completed {
        // Already committed under another activity id: nothing left to do.
        tracing::debug!(
            transfer_id,
            chunk_index,
            chunks_completed,
            "[FileTransfer] ignoring already-committed inbound chunk"
        );
        return Ok(None);
    }
    if !["pending", "in-progress"].contains(&status.as_str()) {
        return Err(Closed(format!("Transfer {transfer_id} is closed ({status})")));
    }
    if chunk_index > chunks_completed {
        return Err(NotReady(format!(
            "Chunk {chunk_index} not yet present in order; expected {chunks_completed}"
        )));
    }

    // Reserve decoded chunk budget; it travels with the bytes into the file task.
    let chunk_budget = admit_chunk_bytes_str(chunk_size).map_err(InboundChunkError::Busy)?;
    let decoded = BASE64
        .decode(chunk_data.as_bytes())
        .map_err(|_| Invalid("Invalid base64 chunkData".into()))?;
    if decoded.len() as i64 != chunk_size {
        return Err(Invalid("chunkSize does not match decoded data length".into()));
    }

    let is_last_chunk = chunk_index == chunks_total - 1;
    let expected_size = if is_last_chunk {
        file_size - (DEFAULT_CHUNK_SIZE * (chunks_total as i64 - 1))
    } else {
        DEFAULT_CHUNK_SIZE
    };
    if expected_size <= 0 || chunk_size != expected_size {
        return Err(Invalid(format!(
            "Invalid chunk size: expected {expected_size}, got {chunk_size}"
        )));
    }

    let final_path =
        resolve_transfer_path(transfer_id, &filename, local_path.as_deref()).map_err(internal)?;
    let final_path_db = path_to_db(&final_path);
    let part_path = part_file_path(&final_path);
    if !is_strictly_under(&storage_root(), &part_path)
        || !is_strictly_under(&storage_root(), &final_path)
    {
        return Err(internal("Transfer path escapes storage root"));
    }
    let expected_offset = DEFAULT_CHUNK_SIZE * chunks_completed as i64;

    let outcome = run_transfer_file_work(transfer_id, async move {
        let _chunk_budget = chunk_budget;
        write_inbound_chunk(
            part_path,
            final_path,
            decoded,
            expected_offset,
            is_last_chunk,
            file_size,
            checksum,
        )
        .await
    })
    .await
    .map_err(internal)??;

    let new_chunks = chunks_completed + 1;
    let (new_status, event_type) = match &outcome {
        ChunkFileOutcome::Progress => ("in-progress", "transfer_progress"),
        ChunkFileOutcome::Completed => ("completed", "transfer_completed"),
        ChunkFileOutcome::Corrupt(reason) => {
            tracing::warn!(
                transfer_id,
                reason = %reason,
                "[FileTransfer] inbound transfer failed verification"
            );
            ("failed", "transfer_failed")
        }
    };
    let recorded_chunks = if new_status == "failed" {
        chunks_completed
    } else {
        new_chunks
    };

    // Compare-and-set on the progress the advisory lock observed; commits with
    // the receipt. A miss leaves the synced file for a replay to verify.
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
                recorded_chunks.into(),
                new_status.into(),
                final_path_db.into(),
                chunks_completed.into(),
            ],
        ))
        .await
        .map_err(internal)?;
    if result.rows_affected() == 0 {
        return Err(internal(format!(
            "Transfer {transfer_id} progress changed while receiving chunk"
        )));
    }

    tracing::info!(
        "[FileTransfer] Received chunk {}/{} for {} from {} ({})",
        new_chunks,
        chunks_total,
        transfer_id,
        actor_url_str,
        new_status
    );

    let progress = (recorded_chunks as f64 / chunks_total as f64) * 100.0;
    Ok(TransferNotice::new(
        room_id.as_deref(),
        channel_id.as_deref(),
        json!({
            "type": event_type,
            "transfer_id": transfer_id,
            "chunks_completed": recorded_chunks,
            "chunks_total": chunks_total,
            "progress": progress,
            "status": new_status
        }),
    ))
}

/// Inbound cancel: mark transfer cancelled and notify local clients.
async fn handle_file_cancel(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    object: &serde_json::Value,
) -> Result<Option<TransferNotice>, String> {
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
        return Ok(None);
    }

    tracing::info!(
        "[FileTransfer] Transfer {} cancelled by remote {}",
        transfer_id,
        actor_url_str
    );
    Ok(TransferNotice::new(
        room_id,
        channel_id,
        json!({
            "type": "transfer_cancelled",
            "transfer_id": transfer_id,
            "from": actor_url_str
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::TransactionTrait;
    use sha2::{Digest, Sha256};

    const PEER: &str = "https://peer.example/users/bob";

    fn chunk(transfer_id: &str, index: i32, bytes: &[u8]) -> serde_json::Value {
        json!({
            "type": "myriad:FileChunk",
            "transferId": transfer_id,
            "channelId": "ch-inbound",
            "chunkIndex": index,
            "chunkSize": bytes.len(),
            "chunkData": BASE64.encode(bytes),
        })
    }

    async fn progress(db: &impl ConnectionTrait, transfer_id: &str) -> (i32, String) {
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT chunks_completed, status FROM federation_file_transfers WHERE transfer_id = $1",
                [transfer_id.into()],
            ))
            .await
            .unwrap()
            .unwrap();
        (
            row.try_get("", "chunks_completed").unwrap(),
            row.try_get("", "status").unwrap(),
        )
    }

    /// Two-chunk inbound transfer against a real schema: ordering, sender
    /// binding, file-ahead-of-DB recovery after a rolled-back receipt, replay
    /// of a committed chunk, and final publish with checksum.
    #[tokio::test]
    async fn inbound_chunks_commit_with_the_receipt_and_recover_after_rollback() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new().await else {
            return;
        };
        let db = &fixture.db;
        let transfer_id = format!("ft_test_{}", uuid::Uuid::new_v4().simple());
        let first = vec![0x11_u8; DEFAULT_CHUNK_SIZE as usize];
        let last = b"tail-bytes".to_vec();
        let mut whole = first.clone();
        whole.extend_from_slice(&last);
        let checksum = hex::encode(Sha256::digest(&whole));
        db.execute_unprepared(&format!(
            r#"
            INSERT INTO users (id, username) VALUES (1, 'alice');
            INSERT INTO federation_remote_actors (id, actor_url, domain, inbox_url)
                VALUES (10, '{PEER}', 'peer.example', '{PEER}/inbox');
            INSERT INTO federation_channels
                (channel_id, user_id, remote_actor_id, channel_type, status, initiated_by)
                VALUES ('ch-inbound', 1, 10, 'dm', 'active', 'remote');
            INSERT INTO federation_file_transfers
                (transfer_id, channel_id, filename, file_size, checksum_sha256,
                 status, direction, chunks_total, chunks_completed, created_at)
                VALUES ('{transfer_id}', 'ch-inbound', 'f.bin', {size}, '{checksum}',
                        'pending', 'inbound', 2, 0, NOW());
            "#,
            size = whole.len()
        ))
        .await
        .unwrap();

        // Out of order: retryable, nothing written.
        let txn = db.begin().await.unwrap();
        let err = handle_file_chunk(&txn, PEER, &chunk(&transfer_id, 1, &last))
            .await
            .unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
        txn.rollback().await.unwrap();

        // Wrong sender: permanent.
        let txn = db.begin().await.unwrap();
        let err = handle_file_chunk(&txn, "https://evil.example/users/x", &chunk(&transfer_id, 0, &first))
            .await
            .unwrap_err();
        assert_eq!(err.status(), axum::http::StatusCode::FORBIDDEN);
        txn.rollback().await.unwrap();

        // Chunk 0 written and synced, but the receipt transaction rolls back.
        let txn = db.begin().await.unwrap();
        let notice = handle_file_chunk(&txn, PEER, &chunk(&transfer_id, 0, &first))
            .await
            .unwrap();
        assert!(notice.is_some());
        txn.rollback().await.unwrap();
        assert_eq!(progress(db, &transfer_id).await, (0, "pending".into()));

        // Retry verifies the durable bytes instead of appending, then commits.
        let txn = db.begin().await.unwrap();
        handle_file_chunk(&txn, PEER, &chunk(&transfer_id, 0, &first))
            .await
            .unwrap();
        txn.commit().await.unwrap();
        assert_eq!(progress(db, &transfer_id).await, (1, "in-progress".into()));

        // Replay of a committed chunk under another activity id: no effect.
        let txn = db.begin().await.unwrap();
        assert!(
            handle_file_chunk(&txn, PEER, &chunk(&transfer_id, 0, &first))
                .await
                .unwrap()
                .is_none()
        );
        txn.commit().await.unwrap();
        assert_eq!(progress(db, &transfer_id).await, (1, "in-progress".into()));

        let txn = db.begin().await.unwrap();
        handle_file_chunk(&txn, PEER, &chunk(&transfer_id, 1, &last))
            .await
            .unwrap();
        txn.commit().await.unwrap();
        assert_eq!(progress(db, &transfer_id).await, (2, "completed".into()));

        let final_path = final_file_path(&transfer_id, "f.bin").unwrap();
        assert_eq!(tokio::fs::read(&final_path).await.unwrap(), whole);
        assert!(!part_file_path(&final_path).exists());

        tokio::fs::remove_dir_all(storage_root().join(&transfer_id))
            .await
            .unwrap();
        fixture.close().await;
    }
}
