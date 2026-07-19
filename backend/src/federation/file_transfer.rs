//! 联邦文件传输模块（Phase 5 补全 — Layer 3 增强）
//!
//! 基于 federation_file_transfers 表实现：
//! 1. 文件元数据发送与接收
//! 2. 分块传输与进度追踪
//! 3. 基于 Channel 的文件传输 Activity

#![allow(dead_code)]

use axum::{http::StatusCode, Json};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::federation::types::*;
use crate::services::data_paths::paths;

// ==================== 请求/响应类型 ====================

/// 发起文件传输请求
#[derive(Debug, Deserialize)]
pub struct InitTransferRequest {
    /// 文件名
    pub filename: String,
    /// 文件大小（字节）
    pub file_size: i64,
    /// MIME 类型
    pub mime_type: Option<String>,
    /// 校验和 (SHA-256)
    pub checksum: Option<String>,
}

/// 上传文件分块请求
#[derive(Debug, Deserialize)]
pub struct UploadChunkRequest {
    /// 分块序号 (0-based)
    pub chunk_index: i32,
    /// 分块数据 (Base64 编码)
    pub chunk_data: String,
    /// 分块大小
    pub chunk_size: i64,
}

/// 文件传输摘要
#[derive(Debug, Serialize)]
pub struct TransferSummary {
    pub transfer_id: String,
    pub channel_id: String,
    pub filename: String,
    pub file_size: i64,
    pub mime_type: Option<String>,
    pub status: String,
    pub direction: String,
    pub progress: f64,
    pub created_at: String,
}

/// 文件传输详情
#[derive(Debug, Serialize)]
pub struct TransferDetail {
    pub transfer_id: String,
    pub channel_id: String,
    pub filename: String,
    pub file_size: i64,
    pub mime_type: Option<String>,
    pub checksum: Option<String>,
    pub status: String,
    pub direction: String,
    pub chunks_total: i32,
    pub chunks_received: i32,
    pub bytes_transferred: i64,
    pub progress: f64,
    pub created_at: String,
    pub completed_at: Option<String>,
}

/// 默认块大小: 1 MiB raw（base64 后约 1.37 MiB，远低于联邦 inbox 40 MiB 上限）
const DEFAULT_CHUNK_SIZE: i64 = 1024 * 1024;

/// 最大文件大小: 5GB
const MAX_FILE_SIZE: i64 = 5_368_709_120;

// ==================== 存储辅助 ====================

fn storage_root() -> PathBuf {
    paths().root.join("federation").join("transfers")
}

fn safe_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches([' ', '.']);
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "file".to_string()
    } else {
        trimmed.chars().take(180).collect()
    }
}

fn final_file_path(transfer_id: &str, filename: &str) -> PathBuf {
    storage_root()
        .join(transfer_id)
        .join(safe_filename(filename))
}

fn part_file_path(final_path: &Path) -> PathBuf {
    let mut part = final_path.as_os_str().to_os_string();
    part.push(".part");
    PathBuf::from(part)
}

fn path_to_db(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn storage_err(e: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!("[FileTransfer] storage error: {}", e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "File storage error"})),
    )
}

fn bad_request(message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": message.into()})),
    )
}

fn http_err_to_string(err: (StatusCode, Json<serde_json::Value>)) -> String {
    format!("{}: {}", err.0, err.1 .0)
}

async fn stored_bytes(path: &str) -> i64 {
    if path.is_empty() {
        return 0;
    }

    let final_path = PathBuf::from(path);
    if let Ok(meta) = fs::metadata(&final_path).await {
        return meta.len() as i64;
    }
    let part_path = part_file_path(&final_path);
    fs::metadata(&part_path)
        .await
        .map(|m| m.len() as i64)
        .unwrap_or(0)
}

async fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

async fn write_chunk_to_part(
    part_path: &Path,
    decoded: &[u8],
    expected_offset: i64,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if let Some(parent) = part_path.parent() {
        fs::create_dir_all(parent).await.map_err(storage_err)?;
    }

    let current_len = match fs::metadata(part_path).await {
        Ok(meta) => meta.len() as i64,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => return Err(storage_err(e)),
    };
    let decoded_len = decoded.len() as i64;

    if current_len == expected_offset + decoded_len {
        // Idempotent retry after DB update failure or client retry. Do not append twice.
        return Ok(());
    }

    if current_len != expected_offset {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Chunk offset mismatch",
                "expected_offset": expected_offset,
                "stored_bytes": current_len
            })),
        ));
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(part_path)
        .await
        .map_err(storage_err)?;
    file.write_all(decoded).await.map_err(storage_err)?;
    file.flush().await.map_err(storage_err)?;
    Ok(())
}

// ==================== 文件传输功能 ====================

/// 在 Channel 上发起文件传输
///
/// 创建传输记录 + 通过 ChannelMessage 通知远程方
pub async fn initiate_transfer(
    user_id: i32,
    username: &str,
    channel_id: &str,
    db: &DatabaseConnection,
    req: &InitTransferRequest,
) -> Result<TransferDetail, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // 验证 Channel 存在且支持 file-transfer
    let ch_row = db
        .query_one(Statement::from_sql_and_values(
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
                Json(json!({"error": "Channel not found"})),
            )
        })?;

    let status: String = ch_row.try_get("", "status").unwrap_or_default();
    if !["active", "accepted"].contains(&status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Channel is {}, cannot transfer files", status)})),
        ));
    }

    // 验证文件大小
    if req.file_size <= 0 || req.file_size > MAX_FILE_SIZE {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error": format!("File size must be between 1 byte and {} bytes", MAX_FILE_SIZE)}),
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
    let final_path = final_file_path(&transfer_id, &req.filename);
    let local_path = path_to_db(&final_path);

    db.execute(Statement::from_sql_and_values(
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

    // 发送 FileTransfer Activity 通知远程方
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

    // 投递到远程
    if let Some(inbox) = remote_inbox {
        let domain = extract_domain(&inbox).unwrap_or_default();
        let act_row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'FileTransfer', 'FileMeta', $3, true, NOW())
                   RETURNING id"#,
                [
                    activity_id.clone().into(),
                    user_id.into(),
                    file_activity.into(),
                ],
            ))
            .await
            .map_err(db_err)?;

        if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_delivery_queue
                       (activity_id, target_inbox, target_domain, status, created_at)
                       VALUES ($1, $2, $3, 'pending', NOW())"#,
                    [act_id.into(), inbox.into(), domain.into()],
                ))
                .await;
        }
    }

    Ok(TransferDetail {
        transfer_id,
        channel_id: channel_id.to_string(),
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
    // 验证传输存在且状态正确
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ft.status, ft.chunks_total, ft.chunks_completed,
                      ft.file_size, ft.channel_id, ft.filename, ft.checksum_sha256,
                      ft.local_path, c.user_id, ra.actor_url, ra.inbox_url
               FROM federation_file_transfers ft
               JOIN federation_channels c ON ft.channel_id = c.channel_id
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE ft.transfer_id = $1"#,
            [transfer_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Transfer not found"})),
            )
        })?;

    let channel_user: i32 = row.try_get("", "user_id").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read channel ownership"})),
        )
    })?;
    if channel_user != user_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Not your transfer"})),
        ));
    }

    let status: String = row.try_get("", "status").unwrap_or_default();
    if !["pending", "in-progress"].contains(&status.as_str()) {
        return Err(bad_request(format!("Transfer is {}", status)));
    }

    let chunks_total: i32 = row.try_get("", "chunks_total").unwrap_or(1);
    let chunks_completed: i32 = row.try_get("", "chunks_completed").unwrap_or(0);
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
    let remote_actor_url: String = row.try_get("", "actor_url").unwrap_or_default();
    let remote_inbox: Option<String> = row
        .try_get::<Option<String>>("", "inbox_url")
        .unwrap_or(None);

    if chunks_total <= 0 {
        return Err(bad_request("Invalid transfer chunk count"));
    }
    if req.chunk_index < 0 || req.chunk_index >= chunks_total {
        return Err(bad_request("Chunk index out of range"));
    }
    if req.chunk_index != chunks_completed {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Chunks must be uploaded in order",
                "expected_chunk_index": chunks_completed,
                "received_chunk_index": req.chunk_index
            })),
        ));
    }

    let decoded = BASE64
        .decode(req.chunk_data.as_bytes())
        .map_err(|_| bad_request("Invalid base64 chunk_data"))?;
    if decoded.len() as i64 != req.chunk_size {
        return Err(bad_request("chunk_size does not match decoded data length"));
    }
    if req.chunk_size <= 0 || req.chunk_size > DEFAULT_CHUNK_SIZE {
        return Err(bad_request(format!(
            "chunk_size must be between 1 and {} bytes",
            DEFAULT_CHUNK_SIZE
        )));
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

    let final_path = local_path
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| final_file_path(transfer_id, &filename));
    let final_path_db = path_to_db(&final_path);
    let part_path = part_file_path(&final_path);
    let expected_offset = DEFAULT_CHUNK_SIZE * chunks_completed as i64;

    write_chunk_to_part(&part_path, &decoded, expected_offset).await?;

    let new_chunks = chunks_completed + 1;
    let mut new_status = if new_chunks >= chunks_total {
        "completed"
    } else {
        "in-progress"
    };

    if new_status == "completed" {
        let stored = fs::metadata(&part_path).await.map_err(storage_err)?.len() as i64;
        if stored != file_size {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE federation_file_transfers SET status = 'failed' WHERE transfer_id = $1",
                [transfer_id.into()],
            ))
            .await
            .map_err(db_err)?;
            return Err(bad_request(format!(
                "Completed file size mismatch: expected {}, got {}",
                file_size, stored
            )));
        }

        if let Some(expected_checksum) = checksum.as_deref().filter(|s| !s.is_empty()) {
            let actual = sha256_file(&part_path).await.map_err(storage_err)?;
            if !actual.eq_ignore_ascii_case(expected_checksum) {
                db.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE federation_file_transfers SET status = 'failed' WHERE transfer_id = $1",
                    [transfer_id.into()],
                ))
                .await
                .map_err(db_err)?;
                return Err(bad_request("SHA-256 checksum mismatch"));
            }
        }

        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await.map_err(storage_err)?;
        }
        fs::rename(&part_path, &final_path)
            .await
            .map_err(storage_err)?;
    }

    let updated = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_file_transfers
               SET chunks_completed = $3,
                   status = $4,
                   local_path = $5,
                   completed_at = CASE WHEN $4 = 'completed' THEN NOW() ELSE NULL END
               WHERE transfer_id = $1 AND chunks_completed = $2
               RETURNING chunks_completed, status"#,
            [
                transfer_id.into(),
                chunks_completed.into(),
                new_chunks.into(),
                new_status.into(),
                final_path_db.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                Json(json!({"error": "Transfer progress changed while uploading chunk"})),
            )
        })?;

    let persisted_chunks: i32 = updated
        .try_get("", "chunks_completed")
        .unwrap_or(new_chunks);
    let persisted_status: String = updated
        .try_get("", "status")
        .unwrap_or_else(|_| new_status.to_string());
    new_status = &persisted_status;

    if let Some(inbox) = remote_inbox.filter(|i| !i.is_empty()) {
        let base_url = get_base_url().await;
        let local_actor = actor_url(&base_url, username);
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

        let domain = extract_domain(&inbox).unwrap_or_default();
        if let Ok(Some(act_row)) = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'FileTransfer', 'FileChunk', $3, true, NOW())
                   RETURNING id"#,
                [activity_id.into(), user_id.into(), chunk_activity.into()],
            ))
            .await
        {
            if let Ok(act_id) = act_row.try_get::<i32>("", "id") {
                let _ = db
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"INSERT INTO federation_delivery_queue
                           (activity_id, target_inbox, target_domain, status, created_at)
                           VALUES ($1, $2, $3, 'pending', NOW())"#,
                        [act_id.into(), inbox.into(), domain.into()],
                    ))
                    .await;
            }
        }
    }

    let progress = (persisted_chunks as f64 / chunks_total as f64) * 100.0;

    Ok(json!({
        "success": true,
        "transfer_id": transfer_id,
        "chunk_index": req.chunk_index,
        "chunks_completed": persisted_chunks,
        "chunks_total": chunks_total,
        "status": new_status,
        "bytes_transferred": stored_bytes(&final_path_db).await,
        "progress": progress
    }))
}

/// 获取传输进度
pub async fn get_transfer(
    transfer_id: &str,
    user_id: i32,
    db: &DatabaseConnection,
) -> Result<TransferDetail, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ft.transfer_id, ft.channel_id, ft.filename, ft.file_size,
                      ft.mime_type, ft.checksum_sha256, ft.status, ft.direction,
                      ft.chunks_total, ft.chunks_completed,
                      ft.local_path, ft.created_at, ft.completed_at, c.user_id
               FROM federation_file_transfers ft
               JOIN federation_channels c ON ft.channel_id = c.channel_id
               WHERE ft.transfer_id = $1"#,
            [transfer_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Transfer not found"})),
            )
        })?;

    let channel_user: i32 = row.try_get("", "user_id").unwrap_or(0);
    if channel_user != user_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Not your transfer"})),
        ));
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
        .query_one(Statement::from_sql_and_values(
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
                    Json(json!({"error": "Not your channel"})),
                ));
            }
        }
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Channel not found"})),
            ));
        }
    }

    let rows = db
        .query_all(Statement::from_sql_and_values(
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

/// 取消文件传输
pub async fn cancel_transfer(
    user_id: i32,
    transfer_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    // 验证所有权
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ft.status, c.user_id
               FROM federation_file_transfers ft
               JOIN federation_channels c ON ft.channel_id = c.channel_id
               WHERE ft.transfer_id = $1"#,
            [transfer_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Transfer not found"})),
            )
        })?;

    let channel_user: i32 = row.try_get("", "user_id").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read channel ownership"})),
        )
    })?;
    if channel_user != user_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Not your transfer"})),
        ));
    }

    let status: String = row.try_get("", "status").unwrap_or_default();
    if status == "completed" || status == "cancelled" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Transfer is already {}", status)})),
        ));
    }

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_file_transfers SET status = 'cancelled' WHERE transfer_id = $1",
        [transfer_id.into()],
    ))
    .await
    .map_err(db_err)?;

    Ok(json!({
        "success": true,
        "transfer_id": transfer_id,
        "status": "cancelled"
    }))
}

// ==================== Inbox 处理 ====================

/// 处理收到的文件传输 Activity（从远程实例）
pub async fn handle_file_transfer(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let object_type = object.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if object_type == "myriad:FileChunk" {
        return handle_file_chunk(db, actor_url_str, object).await;
    }

    let transfer_id = object
        .get("transferId")
        .and_then(|v| v.as_str())
        .ok_or("Missing transferId")?;
    let channel_id = object
        .get("channelId")
        .and_then(|v| v.as_str())
        .ok_or("Missing channelId")?;
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

    let channel_actor = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ra.actor_url
               FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.channel_id = $1"#,
            [channel_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Channel {} not found", channel_id))?;
    let expected_actor: String = channel_actor.try_get("", "actor_url").unwrap_or_default();
    if !crate::federation::types::same_actor_url(&expected_actor, actor_url_str) {
        return Err(format!(
            "File transfer sender mismatch: expected {}, got {}",
            expected_actor, actor_url_str
        ));
    }

    let local_path = path_to_db(&final_file_path(transfer_id, filename));

    // 创建入站传输记录
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_file_transfers
           (transfer_id, channel_id, filename, file_size, mime_type,
            checksum_sha256, status, direction, chunks_total, chunks_completed, local_path, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, 'pending', 'inbound', $7, 0, $8, NOW())
           ON CONFLICT (transfer_id) DO NOTHING"#,
        [
            transfer_id.into(),
            channel_id.into(),
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
    db: &DatabaseConnection,
    actor_url_str: &str,
    object: &serde_json::Value,
) -> Result<(), String> {
    let transfer_id = object
        .get("transferId")
        .and_then(|v| v.as_str())
        .ok_or("Missing transferId")?;
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

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ft.status, ft.chunks_total, ft.chunks_completed,
                      ft.file_size, ft.filename, ft.checksum_sha256, ft.local_path,
                      ra.actor_url
               FROM federation_file_transfers ft
               JOIN federation_channels c ON ft.channel_id = c.channel_id
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE ft.transfer_id = $1"#,
            [transfer_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Transfer {} not found", transfer_id))?;

    let expected_actor: String = row.try_get("", "actor_url").unwrap_or_default();
    if !crate::federation::types::same_actor_url(&expected_actor, actor_url_str) {
        return Err(format!(
            "File chunk sender mismatch: expected {}, got {}",
            expected_actor, actor_url_str
        ));
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

    let final_path = local_path
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| final_file_path(transfer_id, &filename));
    let final_path_db = path_to_db(&final_path);
    let part_path = part_file_path(&final_path);
    let expected_offset = DEFAULT_CHUNK_SIZE * chunks_completed as i64;

    write_chunk_to_part(&part_path, &decoded, expected_offset)
        .await
        .map_err(http_err_to_string)?;

    let new_chunks = chunks_completed + 1;
    let new_status = if new_chunks >= chunks_total {
        "completed"
    } else {
        "in-progress"
    };

    if new_status == "completed" {
        let stored = fs::metadata(&part_path)
            .await
            .map_err(|e| e.to_string())?
            .len() as i64;
        if stored != file_size {
            db.execute(Statement::from_sql_and_values(
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
            let actual = sha256_file(&part_path).await.map_err(|e| e.to_string())?;
            if !actual.eq_ignore_ascii_case(expected_checksum) {
                db.execute(Statement::from_sql_and_values(
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
        fs::rename(&part_path, &final_path)
            .await
            .map_err(|e| e.to_string())?;
    }

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_file_transfers
           SET chunks_completed = $2,
               status = $3,
               local_path = $4,
               completed_at = CASE WHEN $3 = 'completed' THEN NOW() ELSE NULL END
           WHERE transfer_id = $1 AND chunks_completed = $5"#,
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

    tracing::info!(
        "[FileTransfer] Received chunk {}/{} for {} from {}",
        new_chunks,
        chunks_total,
        transfer_id,
        actor_url_str
    );

    Ok(())
}
