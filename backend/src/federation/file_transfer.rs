//! 联邦文件传输模块（Phase 5 补全 — Layer 3 增强）
//!
//! 基于 federation_file_transfers 表实现：
//! 1. 文件元数据发送与接收
//! 2. 分块传输与进度追踪
//! 3. 基于 Channel 的文件传输 Activity

#![allow(dead_code)]

use axum::{http::StatusCode, Json};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

use crate::federation::types::*;
use crate::services::data_paths::paths;
use std::sync::atomic::{AtomicUsize, Ordering};

// 请求/响应类型

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
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

/// 默认块大小: 4 MiB raw（base64 后约 5.3 MiB；路由层见 TRANSFER_CHUNK_BODY_LIMIT）
use crate::federation::limits::TRANSFER_CHUNK_SIZE as DEFAULT_CHUNK_SIZE;

/// 单个文件传输总大小上限（产品决策，见 limits 注释）
use crate::federation::limits::MAX_FILE_SIZE;
use crate::federation::limits::{
    max_in_flight_chunk_bytes, MAX_CONCURRENT_TRANSFERS, MAX_CONCURRENT_TRANSFERS_PER_USER,
    MAX_CONCURRENT_TRANSFER_BYTES, MAX_IN_FLIGHT_CHUNK_BYTES,
};

// ── MYR-008: in-flight chunk byte budget ────────────────────────────────────
//
// Caps concurrent decoded chunk payloads across upload + inbound handlers so a
// burst of clients cannot pin unbounded memory while each transfer still obeys
// MAX_FILE_SIZE.

/// Process-wide sum of reserved decoded chunk bytes currently held.
static IN_FLIGHT_CHUNK_BYTES: AtomicUsize = AtomicUsize::new(0);

/// RAII reservation against [`IN_FLIGHT_CHUNK_BYTES`].
struct InFlightChunkGuard {
    bytes: usize,
}

impl InFlightChunkGuard {
    /// Try to reserve `bytes` of in-flight budget. Returns `None` if full.
    fn try_acquire(bytes: usize) -> Option<Self> {
        if bytes == 0 {
            return Some(Self { bytes: 0 });
        }
        let limit = max_in_flight_chunk_bytes();
        loop {
            let cur = IN_FLIGHT_CHUNK_BYTES.load(Ordering::Relaxed);
            if cur.saturating_add(bytes) > limit {
                return None;
            }
            match IN_FLIGHT_CHUNK_BYTES.compare_exchange_weak(
                cur,
                cur + bytes,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(Self { bytes }),
                Err(_) => continue,
            }
        }
    }
}

impl Drop for InFlightChunkGuard {
    fn drop(&mut self) {
        if self.bytes > 0 {
            IN_FLIGHT_CHUNK_BYTES.fetch_sub(self.bytes, Ordering::AcqRel);
        }
    }
}

/// Open-transfer statuses that consume concurrent amplification budget.
const OPEN_TRANSFER_STATUSES: &str = "('pending', 'in-progress', 'finalizing')";

/// Snapshot of open transfer load for admission decisions.
#[derive(Debug, Clone, Copy)]
struct TransferLoad {
    count: i64,
    total_bytes: i64,
}

async fn open_transfer_load_global(
    db: &impl ConnectionTrait,
) -> Result<TransferLoad, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                r#"SELECT COUNT(*)::bigint AS cnt,
                          COALESCE(SUM(file_size), 0)::bigint AS total_bytes
                   FROM federation_file_transfers
                   WHERE status IN {OPEN_TRANSFER_STATUSES}"#
            ),
            [],
        ))
        .await
        .map_err(db_err)?;
    Ok(TransferLoad {
        count: row
            .as_ref()
            .and_then(|r| r.try_get::<i64>("", "cnt").ok())
            .unwrap_or(0),
        total_bytes: row
            .as_ref()
            .and_then(|r| r.try_get::<i64>("", "total_bytes").ok())
            .unwrap_or(0),
    })
}

/// Open transfers attributed to a local user (channel owner or room owner_user_id).
async fn open_transfer_load_for_user(
    db: &impl ConnectionTrait,
    user_id: i32,
) -> Result<TransferLoad, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                r#"SELECT COUNT(*)::bigint AS cnt,
                          COALESCE(SUM(ft.file_size), 0)::bigint AS total_bytes
                   FROM federation_file_transfers ft
                   LEFT JOIN federation_channels c
                     ON c.channel_id = ft.channel_id AND ft.channel_id <> ''
                   WHERE ft.status IN {OPEN_TRANSFER_STATUSES}
                     AND (
                       ft.owner_user_id = $1
                       OR c.user_id = $1
                     )"#
            ),
            [user_id.into()],
        ))
        .await
        .map_err(db_err)?;
    Ok(TransferLoad {
        count: row
            .as_ref()
            .and_then(|r| r.try_get::<i64>("", "cnt").ok())
            .unwrap_or(0),
        total_bytes: row
            .as_ref()
            .and_then(|r| r.try_get::<i64>("", "total_bytes").ok())
            .unwrap_or(0),
    })
}

/// MYR-008: admit a new transfer if concurrent count/bytes stay within budgets.
///
/// `user_id`: when `Some`, also enforce the per-user concurrent count.
/// Inbound remote FileMeta passes `None` (only global budgets apply).
async fn admit_new_transfer(
    db: &impl ConnectionTrait,
    file_size: i64,
    user_id: Option<i32>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let global = open_transfer_load_global(db).await?;
    if global.count >= MAX_CONCURRENT_TRANSFERS {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Too many concurrent file transfers",
                "message": format!(
                    "At most {} open transfers are allowed; try again when one completes",
                    MAX_CONCURRENT_TRANSFERS
                ),
                "limit": MAX_CONCURRENT_TRANSFERS,
                "current": global.count,
            })),
        ));
    }
    if global.total_bytes.saturating_add(file_size) > MAX_CONCURRENT_TRANSFER_BYTES {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Concurrent transfer byte budget exceeded",
                "message": format!(
                    "Open transfers already reserve {} bytes; adding {} would exceed the {} byte budget",
                    global.total_bytes, file_size, MAX_CONCURRENT_TRANSFER_BYTES
                ),
                "limit_bytes": MAX_CONCURRENT_TRANSFER_BYTES,
                "current_bytes": global.total_bytes,
            })),
        ));
    }

    if let Some(uid) = user_id {
        let per_user = open_transfer_load_for_user(db, uid).await?;
        if per_user.count >= MAX_CONCURRENT_TRANSFERS_PER_USER {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "Too many concurrent file transfers for this user",
                    "message": format!(
                        "At most {} open transfers per user; finish or cancel one first",
                        MAX_CONCURRENT_TRANSFERS_PER_USER
                    ),
                    "limit": MAX_CONCURRENT_TRANSFERS_PER_USER,
                    "current": per_user.count,
                })),
            ));
        }
    }

    Ok(())
}

fn admit_chunk_bytes(
    chunk_size: i64,
) -> Result<InFlightChunkGuard, (StatusCode, Json<serde_json::Value>)> {
    if chunk_size <= 0 {
        return Err(bad_request("chunk_size must be positive"));
    }
    let bytes = chunk_size as usize;
    InFlightChunkGuard::try_acquire(bytes).ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Too many in-flight transfer chunks",
                "message": format!(
                    "Decoded chunk budget is {} bytes; retry shortly",
                    max_in_flight_chunk_bytes()
                ),
                "limit_bytes": max_in_flight_chunk_bytes(),
            })),
        )
    })
}

/// String-error variant for inbox handlers.
async fn admit_new_transfer_str(db: &impl ConnectionTrait, file_size: i64) -> Result<(), String> {
    admit_new_transfer(db, file_size, None)
        .await
        .map_err(http_err_to_string)
}

fn admit_chunk_bytes_str(chunk_size: i64) -> Result<InFlightChunkGuard, String> {
    admit_chunk_bytes(chunk_size).map_err(http_err_to_string)
}

// 存储辅助
//
// MYR-001: transferId is a path component under the federation transfers root.
// Never join unvalidated remote/DB strings into filesystem paths.

/// Max length for transfer IDs used as storage directory names.
const MAX_TRANSFER_ID_LEN: usize = 128;

fn storage_root() -> PathBuf {
    paths().root.join("federation").join("transfers")
}

/// Strict transferId validation before any filesystem use (MYR-001).
///
/// Allowlist: ASCII alphanumeric, `_`, `-` only (covers local `ft_{uuid}`).
/// Rejects empty, oversize, absolute paths, `..`, separators, null bytes, Unicode.
fn is_valid_transfer_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_TRANSFER_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
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

/// Lexically normalize a path (resolve `.` / `..` without touching the filesystem).
fn normalize_lexically(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    // Preserve escaping `..` so subsequent confinement checks fail.
                    out.push("..");
                }
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

/// True if `path` is strictly under `root` after lexical normalization (no escape).
fn is_strictly_under(root: &Path, path: &Path) -> bool {
    let root_n = normalize_lexically(root);
    let path_n = normalize_lexically(path);
    path_n.starts_with(&root_n) && path_n != root_n
}

/// Build final on-disk path for a transfer. Validates transferId and confines under storage root.
///
/// Protocol transferId is used as the directory name only after validation; original id stays
/// the DB identity (same string for valid `ft_{uuid}` ids).
fn final_file_path(transfer_id: &str, filename: &str) -> Result<PathBuf, String> {
    if !is_valid_transfer_id(transfer_id) {
        return Err("Invalid transferId: must be 1-128 chars of [A-Za-z0-9_-] only".into());
    }
    let root = storage_root();
    let path = root.join(transfer_id).join(safe_filename(filename));
    if !is_strictly_under(&root, &path) {
        return Err("Transfer path escapes storage root".into());
    }
    Ok(path)
}

/// Resolve a path for open/write: prefer DB `local_path` only if confined under storage root.
///
/// Never trust stored paths blindly — re-validate confinement before any FS use (MYR-001).
fn resolve_transfer_path(
    transfer_id: &str,
    filename: &str,
    local_path: Option<&str>,
) -> Result<PathBuf, String> {
    let root = storage_root();
    if let Some(p) = local_path.filter(|s| !s.is_empty()) {
        if p.contains('\0') {
            return Err("Invalid local_path: null byte".into());
        }
        let candidate = PathBuf::from(p);
        if is_strictly_under(&root, &candidate) {
            return Ok(normalize_lexically(&candidate));
        }
        tracing::warn!(
            "[FileTransfer] rejecting unconfined local_path for transfer {}",
            transfer_id
        );
    }
    final_file_path(transfer_id, filename)
}

fn part_file_path(final_path: &Path) -> PathBuf {
    let mut part = final_path.as_os_str().to_os_string();
    part.push(".part");
    PathBuf::from(part)
}

fn transfer_lock_key(transfer_id: &str) -> String {
    format!("federation-file-transfer:{transfer_id}")
}

/// Serialize one transfer's database and filesystem state across backend replicas.
/// The caller must hold an explicit transaction for the duration of the mutation.
async fn lock_transfer_session(
    db: &impl ConnectionTrait,
    transfer_id: &str,
) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        [transfer_lock_key(transfer_id).into()],
    ))
    .await?;
    Ok(())
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
    // Do not stat paths outside the transfers root (defense in depth for DB values).
    if !is_strictly_under(&storage_root(), &final_path) {
        return 0;
    }
    if let Ok(meta) = fs::metadata(&final_path).await {
        return meta.len() as i64;
    }
    let part_path = part_file_path(&final_path);
    if !is_strictly_under(&storage_root(), &part_path) {
        return 0;
    }
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

async fn verify_chunk_bytes(
    path: &Path,
    decoded: &[u8],
    expected_offset: i64,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let mut file = fs::File::open(path).await.map_err(storage_err)?;
    file.seek(std::io::SeekFrom::Start(expected_offset as u64))
        .await
        .map_err(storage_err)?;
    let mut existing = vec![0_u8; decoded.len()];
    file.read_exact(&mut existing).await.map_err(storage_err)?;
    if existing != decoded {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "Chunk retry content does not match stored bytes"})),
        ));
    }
    Ok(())
}

/// Deterministically place one chunk at its protocol offset.
///
/// A process can die after a partial write but before the database progress
/// update. Because the database still owns `expected_offset`, bytes beyond that
/// offset are uncommitted and are truncated before the complete chunk is
/// rewritten. A complete retry is accepted only when the stored bytes match.
async fn write_chunk_to_part(
    part_path: &Path,
    decoded: &[u8],
    expected_offset: i64,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if let Some(parent) = part_path.parent() {
        fs::create_dir_all(parent).await.map_err(storage_err)?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(part_path)
        .await
        .map_err(storage_err)?;
    let current_len = file.metadata().await.map_err(storage_err)?.len() as i64;
    let chunk_end = expected_offset
        .checked_add(decoded.len() as i64)
        .ok_or_else(|| bad_request("Chunk offset overflow"))?;

    if current_len < expected_offset || current_len > chunk_end {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Chunk offset mismatch",
                "expected_offset": expected_offset,
                "stored_bytes": current_len
            })),
        ));
    }

    if current_len == chunk_end {
        drop(file);
        return verify_chunk_bytes(part_path, decoded, expected_offset).await;
    }

    if current_len > expected_offset {
        // Recover an interrupted write for the database-owned current chunk.
        file.set_len(expected_offset as u64)
            .await
            .map_err(storage_err)?;
    }
    file.seek(std::io::SeekFrom::Start(expected_offset as u64))
        .await
        .map_err(storage_err)?;
    file.write_all(decoded).await.map_err(storage_err)?;
    file.sync_all().await.map_err(storage_err)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkFileState {
    Part,
    Final,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UploadSessionAction {
    WriteChunk,
    ResumeFinalization,
    VerifyCompletedRetry,
}

fn upload_session_action(
    status: &str,
    chunks_completed: i32,
    chunks_total: i32,
    chunk_index: i32,
) -> Result<UploadSessionAction, (StatusCode, Json<serde_json::Value>)> {
    let is_final_chunk = chunk_index == chunks_total - 1;
    match status {
        "completed" if chunks_completed == chunks_total && is_final_chunk => {
            Ok(UploadSessionAction::VerifyCompletedRetry)
        }
        "finalizing" if chunks_completed == chunks_total && is_final_chunk => {
            Ok(UploadSessionAction::ResumeFinalization)
        }
        "pending" | "in-progress" if chunk_index == chunks_completed => {
            Ok(UploadSessionAction::WriteChunk)
        }
        "pending" | "in-progress" => Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Chunks must be uploaded in order",
                "expected_chunk_index": chunks_completed,
                "received_chunk_index": chunk_index
            })),
        )),
        "finalizing" | "completed" => Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "Transfer finalization state is inconsistent"})),
        )),
        _ => Err(bad_request(format!("Transfer is {}", status))),
    }
}

/// Recover a retry that arrives after the final rename but before the database
/// commit, otherwise write the current chunk into the session's isolated part.
async fn prepare_chunk_file(
    part_path: &Path,
    final_path: &Path,
    decoded: &[u8],
    expected_offset: i64,
    is_last_chunk: bool,
) -> Result<ChunkFileState, (StatusCode, Json<serde_json::Value>)> {
    match fs::metadata(final_path).await {
        Ok(_) if is_last_chunk => {
            verify_chunk_bytes(final_path, decoded, expected_offset).await?;
            return Ok(ChunkFileState::Final);
        }
        Ok(_) => {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "Final file exists before the last chunk"})),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(storage_err(error)),
    }

    write_chunk_to_part(part_path, decoded, expected_offset).await?;
    Ok(ChunkFileState::Part)
}

#[cfg(unix)]
async fn sync_parent_directory(path: &Path) -> Result<(), std::io::Error> {
    let parent = path.parent().map(Path::to_path_buf).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "file has no parent directory",
        )
    })?;
    tokio::task::spawn_blocking(move || std::fs::File::open(parent)?.sync_all())
        .await
        .map_err(std::io::Error::other)?
}

#[cfg(not(unix))]
async fn sync_parent_directory(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

/// Atomically publish a validated part. If another equivalent retry already
/// completed the rename, the existing final path is the idempotent result.
async fn finalize_part_file(
    part_path: &Path,
    final_path: &Path,
    file_size: i64,
    checksum: Option<&str>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    match fs::rename(part_path, final_path).await {
        Ok(()) => sync_parent_directory(final_path).await.map_err(storage_err),
        Err(rename_error) => match fs::metadata(final_path).await {
            Ok(_) => validate_completed_file(final_path, file_size, checksum).await,
            Err(_) => Err(storage_err(rename_error)),
        },
    }
}

async fn validate_completed_file(
    path: &Path,
    file_size: i64,
    checksum: Option<&str>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let metadata = fs::metadata(path).await.map_err(storage_err)?;
    if !metadata.is_file() {
        return Err(bad_request("Completed transfer path is not a file"));
    }
    let stored = metadata.len() as i64;
    if stored != file_size {
        return Err(bad_request(format!(
            "Completed file size mismatch: expected {}, got {}",
            file_size, stored
        )));
    }
    if let Some(expected_checksum) = checksum.filter(|value| !value.is_empty()) {
        let actual = sha256_file(path).await.map_err(storage_err)?;
        if !actual.eq_ignore_ascii_case(expected_checksum) {
            return Err(bad_request("SHA-256 checksum mismatch"));
        }
    }
    Ok(())
}

async fn mark_transfer_failed(
    db: &impl ConnectionTrait,
    transfer_id: &str,
) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_file_transfers SET status = 'failed' \
         WHERE transfer_id = $1 AND status = 'finalizing'",
        [transfer_id.into()],
    ))
    .await?;
    Ok(())
}

/// Finish the recoverable `finalizing` state without holding a database
/// connection while hashing a potentially multi-gigabyte file.
///
/// The part is already fsynced before `status = finalizing` commits. The final
/// rename happens before the short completion transaction, so a crash can only
/// leave `finalizing + .part` or `finalizing + final`, both safe to retry.
async fn finalize_uploaded_transfer(
    db: &DatabaseConnection,
    transfer_id: &str,
    part_path: &Path,
    final_path: &Path,
    final_path_db: &str,
    file_size: i64,
    checksum: Option<&str>,
) -> Result<bool, (StatusCode, Json<serde_json::Value>)> {
    let completed_path = match fs::metadata(final_path).await {
        Ok(_) => final_path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => part_path,
        Err(error) => return Err(storage_err(error)),
    };

    if let Err(error) = validate_completed_file(completed_path, file_size, checksum).await {
        if error.0 == StatusCode::BAD_REQUEST {
            let txn = db.begin().await.map_err(db_err)?;
            lock_transfer_session(&txn, transfer_id)
                .await
                .map_err(db_err)?;
            mark_transfer_failed(&txn, transfer_id)
                .await
                .map_err(db_err)?;
            txn.commit().await.map_err(db_err)?;
        }
        return Err(error);
    }

    if completed_path == part_path {
        finalize_part_file(part_path, final_path, file_size, checksum).await?;
    }

    let txn = db.begin().await.map_err(db_err)?;
    lock_transfer_session(&txn, transfer_id)
        .await
        .map_err(db_err)?;
    let row = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT status, chunks_total, chunks_completed \
             FROM federation_file_transfers WHERE transfer_id = $1",
            [transfer_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Transfer not found while finalizing"})),
            )
        })?;
    let status: String = row.try_get("", "status").unwrap_or_default();
    let chunks_total: i32 = row.try_get("", "chunks_total").unwrap_or_default();
    let chunks_completed: i32 = row.try_get("", "chunks_completed").unwrap_or_default();
    if status == "completed" {
        txn.commit().await.map_err(db_err)?;
        return Ok(false);
    }
    if status != "finalizing" || chunks_completed != chunks_total {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "Transfer state changed while finalizing"})),
        ));
    }

    let updated = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_file_transfers
               SET status = 'completed', local_path = $2, completed_at = NOW()
               WHERE transfer_id = $1
                 AND status = 'finalizing'
                 AND chunks_completed = chunks_total"#,
            [transfer_id.into(), final_path_db.into()],
        ))
        .await
        .map_err(db_err)?;
    if updated.rows_affected() != 1 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "Transfer state changed while finalizing"})),
        ));
    }
    txn.commit().await.map_err(db_err)?;
    Ok(true)
}

// 文件传输功能

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
                Json(json!({"error": "Channel not found"})),
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
                json!({"error": format!("File size must be between 1 byte and {} bytes", MAX_FILE_SIZE)}),
            ),
        ));
    }

    // MYR-008: concurrent transfer admission (count + reserved bytes)
    admit_new_transfer(db, req.file_size, Some(user_id)).await?;

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

    db.execute_raw(Statement::from_sql_and_values(
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
            .query_one_raw(Statement::from_sql_and_values(
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
                Json(json!({"error": "Not a room member"})),
            )
        })?;
    if role == "observer" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Observers cannot upload files"})),
        ));
    }

    if req.file_size <= 0 || req.file_size > MAX_FILE_SIZE {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error": format!("File size must be between 1 byte and {} bytes", MAX_FILE_SIZE)}),
            ),
        ));
    }

    // MYR-008: concurrent transfer admission (count + reserved bytes)
    admit_new_transfer(db, req.file_size, Some(user_id)).await?;

    let chunks_total =
        ((req.file_size + DEFAULT_CHUNK_SIZE - 1) / DEFAULT_CHUNK_SIZE).max(1) as i32;
    let transfer_id = generate_transfer_id();
    let final_path = final_file_path(&transfer_id, &req.filename).map_err(bad_request)?;
    let local_path = path_to_db(&final_path);

    db.execute_raw(Statement::from_sql_and_values(
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
    let _ = crate::federation::room::fanout_to_remote_members(
        db,
        user_id,
        room_id,
        &activity_id,
        &file_activity,
        "FileTransfer",
        "FileMeta",
    )
    .await;

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
    // MYR-001: reject unsafe transferId before any path construction / DB-driven FS write
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
                Json(json!({"error": "Transfer not found"})),
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
            Json(json!({"error": "Not your transfer"})),
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

    // MYR-008: reserve decoded chunk budget before base64 decode / disk write
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

    let chunk_file_state = prepare_chunk_file(
        &part_path,
        &final_path,
        &decoded,
        expected_offset,
        is_last_chunk,
    )
    .await?;

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
                        Json(json!({"error": "Transfer progress changed while uploading chunk"})),
                    )
                })?;

            let persisted_chunks: i32 = updated
                .try_get("", "chunks_completed")
                .unwrap_or(new_chunks);
            let mut persisted_status: String = updated
                .try_get("", "status")
                .unwrap_or_else(|_| target_status.to_string());

            // Filesystem state is durable before database progress is committed.
            // A retry either verifies this chunk or resumes finalization.
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
            let _ = crate::federation::room::fanout_to_remote_members(
                db,
                user_id,
                rid,
                &activity_id,
                &chunk_activity,
                "FileTransfer",
                "FileChunk",
            )
            .await;
        } else if let Some(inbox) = remote_inbox.filter(|i| !i.is_empty()) {
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
                .query_one_raw(Statement::from_sql_and_values(
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

/// Resolved on-disk file for a completed transfer the user is allowed to read.
#[derive(Debug)]
pub struct TransferFileContent {
    pub transfer_id: String,
    pub filename: String,
    pub mime_type: String,
    pub file_size: u64,
    pub path: PathBuf,
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
    // MYR-001: never open a path derived from an unvalidated transferId / DB local_path
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
                Json(json!({"error": "Transfer not found"})),
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
                Json(json!({"error": "Not a room member"})),
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
                Json(json!({"error": "Not your transfer"})),
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

    let meta = fs::metadata(&path).await.map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Transfer file missing on disk",
                "transfer_id": transfer_id,
            })),
        )
    })?;
    if !meta.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Transfer path is not a file"})),
        ));
    }

    let file_size = meta.len();
    if declared_size > 0 && file_size == 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "Transfer file is empty"})),
        ));
    }

    Ok(TransferFileContent {
        transfer_id: row.try_get("", "transfer_id").unwrap_or_default(),
        filename: safe_filename(&filename),
        mime_type,
        file_size,
        path,
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
                Json(json!({"error": "Transfer not found"})),
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
                Json(json!({"error": "Not a room member"})),
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
                Json(json!({"error": "Not your transfer"})),
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
            Json(json!({"error": "Not a room member"})),
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
                Json(json!({"error": "Transfer not found"})),
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
            Json(json!({"error": "Not your transfer"})),
        ));
    }

    let status: String = row.try_get("", "status").unwrap_or_default();
    if !["pending", "in-progress"].contains(&status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Transfer is not ready"})),
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
    txn.commit().await.map_err(db_err)?;

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
        let _ = crate::federation::room::fanout_to_remote_members(
            db,
            user_id,
            rid,
            &activity_id,
            &cancel_activity,
            "FileTransfer",
            "FileCancel",
        )
        .await;
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
        let remote_inbox: Option<String> = row
            .try_get::<Option<String>>("", "inbox_url")
            .unwrap_or(None);
        let remote_actor: String = row.try_get("", "actor_url").unwrap_or_default();
        if let Some(inbox) = remote_inbox.filter(|s| !s.is_empty()) {
            let cancel_activity = json!({
                "@context": build_context(),
                "type": "myriad:FileTransfer",
                "id": &activity_id,
                "actor": &local_actor,
                "to": [&remote_actor],
                "object": cancel_object
            });
            let domain = extract_domain(&inbox).unwrap_or_default();
            if let Ok(Some(act_row)) = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_activities
                       (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                       VALUES ($1, $2, 'FileTransfer', 'FileCancel', $3, true, NOW())
                       RETURNING id"#,
                    [activity_id.into(), user_id.into(), cancel_activity.into()],
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

// ── MYR-001 path safety + MYR-008 admission tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_FT_UUID: &str = "ft_550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn in_flight_chunk_budget_admits_then_rejects_when_full() {
        // Drain any leftover from parallel tests in this binary (best-effort).
        let _ = InFlightChunkGuard::try_acquire(0);
        let half = MAX_IN_FLIGHT_CHUNK_BYTES / 2;
        let g1 = InFlightChunkGuard::try_acquire(half).expect("first half");
        let g2 = InFlightChunkGuard::try_acquire(half).expect("second half");
        assert!(
            InFlightChunkGuard::try_acquire(1).is_none(),
            "must reject when budget is exhausted"
        );
        drop(g1);
        let g3 = InFlightChunkGuard::try_acquire(half).expect("after release");
        drop(g2);
        drop(g3);
        assert_eq!(IN_FLIGHT_CHUNK_BYTES.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn admit_chunk_bytes_rejects_non_positive() {
        assert!(admit_chunk_bytes(0).is_err());
        assert!(admit_chunk_bytes(-1).is_err());
    }

    #[test]
    fn amplification_limits_are_generous_product_values() {
        // Document the shipped numbers so a silent shrink is a test failure.
        assert_eq!(MAX_FILE_SIZE, 20 * 1024 * 1024 * 1024);
        assert_eq!(MAX_CONCURRENT_TRANSFERS, 64);
        assert_eq!(MAX_CONCURRENT_TRANSFERS_PER_USER, 16);
        assert_eq!(MAX_CONCURRENT_TRANSFER_BYTES, 64 * 1024 * 1024 * 1024);
        assert_eq!(MAX_IN_FLIGHT_CHUNK_BYTES, 128 * 1024 * 1024);
        // compile-time relations also asserted in limits.rs; keep runtime docs here.
        const {
            assert!(MAX_CONCURRENT_TRANSFER_BYTES >= MAX_FILE_SIZE * 3);
            assert!(MAX_IN_FLIGHT_CHUNK_BYTES >= DEFAULT_CHUNK_SIZE as usize * 16);
        }
    }

    #[test]
    fn transfer_id_accepts_ft_uuid() {
        assert!(is_valid_transfer_id(VALID_FT_UUID));
        assert!(is_valid_transfer_id("ft_abc-DEF_0123456789"));
        assert!(is_valid_transfer_id("a"));
        assert!(is_valid_transfer_id(&"x".repeat(MAX_TRANSFER_ID_LEN)));
    }

    #[test]
    fn transfer_id_rejects_path_traversal_and_unsafe() {
        // Absolute / parent / separators
        assert!(!is_valid_transfer_id(""));
        assert!(!is_valid_transfer_id("/etc/passwd"));
        assert!(!is_valid_transfer_id("C:\\Windows\\System32"));
        assert!(!is_valid_transfer_id("../../agent/mcp_servers"));
        assert!(!is_valid_transfer_id(".."));
        assert!(!is_valid_transfer_id("."));
        assert!(!is_valid_transfer_id("foo/bar"));
        assert!(!is_valid_transfer_id("foo\\bar"));
        assert!(!is_valid_transfer_id("ft_.."));
        assert!(!is_valid_transfer_id("ft_/evil"));
        // Mixed separators (would be invalid charset)
        assert!(!is_valid_transfer_id("ft_..\\..\\agent"));
        assert!(!is_valid_transfer_id("ft_..%2f..%2fagent"));
        // Oversize
        assert!(!is_valid_transfer_id(&"a".repeat(MAX_TRANSFER_ID_LEN + 1)));
        // Spaces / Unicode / null / control
        assert!(!is_valid_transfer_id("id with spaces"));
        assert!(!is_valid_transfer_id("ft_\u{2024}evil")); // one-dot leader
        assert!(!is_valid_transfer_id("ft_\u{2215}evil")); // division slash
        assert!(!is_valid_transfer_id("ft_\u{ff0f}evil")); // fullwidth solidus
        assert!(!is_valid_transfer_id("ft_\0null"));
        assert!(!is_valid_transfer_id("ft_\nevil"));
    }

    #[test]
    fn final_file_path_never_escapes_root_for_valid_id() {
        let root = storage_root();
        let p = final_file_path(VALID_FT_UUID, "doc.pdf").expect("valid id");
        assert!(
            is_strictly_under(&root, &p),
            "path {:?} must be under {:?}",
            p,
            root
        );
        assert!(p.starts_with(root.join(VALID_FT_UUID)));
        // Filename is sanitized (separators → `_`, leading dots trimmed) but path stays under root
        let p2 = final_file_path(VALID_FT_UUID, "../../etc/passwd").expect("safe filename");
        assert!(is_strictly_under(&root, &p2));
        let fname = p2.file_name().and_then(|s| s.to_str()).unwrap_or("");
        assert!(!fname.contains('/'));
        assert!(!fname.contains('\\'));
        assert_ne!(fname, "..");
        assert_eq!(p2.parent(), Some(root.join(VALID_FT_UUID).as_path()));
    }

    #[test]
    fn final_file_path_rejects_malicious_transfer_id() {
        assert!(final_file_path("../../agent/mcp_servers", "x.json").is_err());
        assert!(final_file_path("/etc/passwd", "x").is_err());
        assert!(final_file_path("a/b", "x").is_err());
        assert!(final_file_path("..\\..\\agent", "x").is_err());
        assert!(final_file_path("", "x").is_err());
        assert!(final_file_path(&"z".repeat(200), "x").is_err());
        assert!(final_file_path("ft_\u{2024}x", "x").is_err());
    }

    #[test]
    fn resolve_transfer_path_rejects_db_local_path_escape() {
        let root = storage_root();

        // Absolute path outside root → rebuild under root via transfer_id
        let p = resolve_transfer_path(VALID_FT_UUID, "f.txt", Some("/etc/passwd"))
            .expect("rebuild under root");
        assert!(is_strictly_under(&root, &p));
        assert!(!p.starts_with(Path::new("/etc")));

        // Relative traversal outside root
        let p = resolve_transfer_path(VALID_FT_UUID, "f.txt", Some("../../agent/mcp_servers.json"))
            .expect("rebuild");
        assert!(is_strictly_under(&root, &p));

        // Mixed-separator style path under root's parent
        let escape = root
            .join("..")
            .join("agent")
            .join("mcp_servers.json")
            .to_string_lossy()
            .into_owned();
        let p = resolve_transfer_path(VALID_FT_UUID, "f.txt", Some(&escape)).expect("rebuild");
        assert!(is_strictly_under(&root, &p));
        // Escaped path must not be used as-is
        assert_ne!(
            normalize_lexically(&p),
            normalize_lexically(Path::new(&escape))
        );

        // Valid confined local_path is accepted
        let good = root.join(VALID_FT_UUID).join("file.bin");
        let p = resolve_transfer_path(VALID_FT_UUID, "f.txt", Some(good.to_str().unwrap()))
            .expect("accept confined");
        assert_eq!(normalize_lexically(&p), normalize_lexically(&good));

        // Null byte rejected
        assert!(resolve_transfer_path(VALID_FT_UUID, "f.txt", Some("evil\0path")).is_err());

        // Malicious transferId with no usable local_path
        assert!(resolve_transfer_path("../../agent", "x", None).is_err());
        assert!(resolve_transfer_path("../../agent/mcp_servers", "x", Some("/tmp/out")).is_err());
    }

    #[test]
    fn is_strictly_under_blocks_parent_and_sibling_escapes() {
        let root = PathBuf::from("data/federation/transfers");
        assert!(is_strictly_under(
            &root,
            &root.join("ft_abc").join("file.txt")
        ));
        assert!(!is_strictly_under(&root, &root));
        assert!(!is_strictly_under(
            &root,
            &root.join("..").join("agent").join("mcp_servers.json")
        ));
        assert!(!is_strictly_under(&root, Path::new("/tmp/evil")));
        assert!(!is_strictly_under(
            &root,
            &root.join("..").join("..").join("etc").join("passwd")
        ));
        // Sibling via `..` then back should not count as under when normalized leaves root
        assert!(!is_strictly_under(
            &root,
            Path::new("data/federation/transfers/../../agent/mcp_servers")
        ));
    }

    #[test]
    fn generate_transfer_id_is_always_valid() {
        for _ in 0..20 {
            let id = generate_transfer_id();
            assert!(
                is_valid_transfer_id(&id),
                "generated id must pass validation: {}",
                id
            );
            assert!(final_file_path(&id, "a.bin").is_ok());
        }
    }

    fn chunk_test_paths(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "myriad-file-transfer-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let final_path = root.join("payload.bin");
        let part_path = part_file_path(&final_path);
        (root, part_path, final_path)
    }

    #[test]
    fn upload_session_state_machine_has_recoverable_retry_edges() {
        assert_eq!(
            upload_session_action("pending", 0, 2, 0).unwrap(),
            UploadSessionAction::WriteChunk
        );
        assert_eq!(
            upload_session_action("in-progress", 1, 2, 1).unwrap(),
            UploadSessionAction::WriteChunk
        );
        assert_eq!(
            upload_session_action("finalizing", 2, 2, 1).unwrap(),
            UploadSessionAction::ResumeFinalization
        );
        assert_eq!(
            upload_session_action("completed", 2, 2, 1).unwrap(),
            UploadSessionAction::VerifyCompletedRetry
        );

        assert_eq!(
            upload_session_action("in-progress", 1, 3, 2).unwrap_err().0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            upload_session_action("finalizing", 1, 2, 1).unwrap_err().0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            upload_session_action("cancelled", 1, 2, 1).unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn concurrent_duplicate_chunk_writes_do_not_append_twice() {
        let (root, part_path, _) = chunk_test_paths("duplicate");
        let payload = vec![0x5a; 64 * 1024];
        let (left, right) = tokio::join!(
            write_chunk_to_part(&part_path, &payload, 0),
            write_chunk_to_part(&part_path, &payload, 0)
        );
        left.expect("first duplicate write");
        right.expect("second duplicate write");
        assert_eq!(tokio::fs::read(&part_path).await.unwrap(), payload);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn retry_rejects_different_bytes_and_out_of_order_offsets() {
        let (root, part_path, _) = chunk_test_paths("retry-content");
        write_chunk_to_part(&part_path, b"original", 0)
            .await
            .unwrap();
        let before = tokio::fs::read(&part_path).await.unwrap();
        let mismatch = write_chunk_to_part(&part_path, b"changed!", 0)
            .await
            .unwrap_err();
        assert_eq!(mismatch.0, StatusCode::CONFLICT);
        let out_of_order = write_chunk_to_part(&part_path, b"next", 32)
            .await
            .unwrap_err();
        assert_eq!(out_of_order.0, StatusCode::CONFLICT);
        assert_eq!(tokio::fs::read(&part_path).await.unwrap(), before);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn interrupted_partial_chunk_is_rewritten_from_database_offset() {
        let (root, part_path, _) = chunk_test_paths("partial");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(&part_path, b"half").await.unwrap();
        write_chunk_to_part(&part_path, b"complete", 0)
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(&part_path).await.unwrap(), b"complete");
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn final_rename_and_post_rename_retry_are_idempotent() {
        let (root, part_path, final_path) = chunk_test_paths("finalize");
        let payload = b"last-chunk";
        assert_eq!(
            prepare_chunk_file(&part_path, &final_path, payload, 0, true)
                .await
                .unwrap(),
            ChunkFileState::Part
        );

        let (left, right) = tokio::join!(
            finalize_part_file(&part_path, &final_path, payload.len() as i64, None),
            finalize_part_file(&part_path, &final_path, payload.len() as i64, None)
        );
        left.expect("first finalize");
        right.expect("racing finalize retry");

        assert_eq!(
            prepare_chunk_file(&part_path, &final_path, payload, 0, true)
                .await
                .unwrap(),
            ChunkFileState::Final
        );
        assert_eq!(tokio::fs::read(&final_path).await.unwrap(), payload);
        assert!(!part_path.exists());
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn transfer_lock_key_is_namespaced_and_stable() {
        assert_eq!(
            transfer_lock_key(VALID_FT_UUID),
            format!("federation-file-transfer:{VALID_FT_UUID}")
        );
    }
}
