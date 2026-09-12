//! Path confinement, concurrent admission, and chunk I/O.

use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

use crate::federation::types::db_err;
use crate::services::data_paths::paths;

/// 默认块大小: 4 MiB raw（base64 后约 5.3 MiB；路由层见 TRANSFER_CHUNK_BODY_LIMIT）
pub(super) use crate::federation::limits::TRANSFER_CHUNK_SIZE as DEFAULT_CHUNK_SIZE;

/// 单个文件传输总大小上限（产品决策，见 limits 注释）
pub(super) use crate::federation::limits::MAX_FILE_SIZE;
use crate::federation::limits::{
    max_in_flight_chunk_bytes, MAX_CONCURRENT_TRANSFERS, MAX_CONCURRENT_TRANSFERS_PER_USER,
    MAX_CONCURRENT_TRANSFER_BYTES, MAX_IN_FLIGHT_CHUNK_BYTES,
};

// ── in-flight chunk byte budget ────────────────────────────────────
//
// Caps concurrent decoded chunk payloads across upload + inbound handlers so a
// burst of clients cannot pin unbounded memory while each transfer still obeys
// MAX_FILE_SIZE.

/// Process-wide sum of reserved decoded chunk bytes currently held.
static IN_FLIGHT_CHUNK_BYTES: AtomicUsize = AtomicUsize::new(0);

/// RAII reservation against [`IN_FLIGHT_CHUNK_BYTES`].
pub(super) struct InFlightChunkGuard {
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

/// admit a new transfer if concurrent count/bytes stay within budgets.
///
/// `user_id`: when `Some`, also enforce the per-user concurrent count.
/// Inbound remote FileMeta passes `None` (only global budgets apply).
pub(super) async fn admit_new_transfer(
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

pub(super) fn admit_chunk_bytes(
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
pub(super) async fn admit_new_transfer_str(
    db: &impl ConnectionTrait,
    file_size: i64,
) -> Result<(), String> {
    admit_new_transfer(db, file_size, None)
        .await
        .map_err(http_err_to_string)
}

pub(super) fn admit_chunk_bytes_str(chunk_size: i64) -> Result<InFlightChunkGuard, String> {
    admit_chunk_bytes(chunk_size).map_err(http_err_to_string)
}

// 存储辅助
//
// transferId is a path component under the federation transfers root.
// Never join unvalidated remote/DB strings into filesystem paths.

/// Max length for transfer IDs used as storage directory names.
const MAX_TRANSFER_ID_LEN: usize = 128;

pub(super) fn storage_root() -> PathBuf {
    paths().root.join("federation").join("transfers")
}

/// Strict transferId validation before any filesystem use.
///
/// Allowlist: ASCII alphanumeric, `_`, `-` only (covers local `ft_{uuid}`).
/// Rejects empty, oversize, absolute paths, `..`, separators, null bytes, Unicode.
pub(super) fn is_valid_transfer_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_TRANSFER_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(super) fn safe_filename(name: &str) -> String {
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
pub(super) fn is_strictly_under(root: &Path, path: &Path) -> bool {
    let root_n = normalize_lexically(root);
    let path_n = normalize_lexically(path);
    path_n.starts_with(&root_n) && path_n != root_n
}

/// Build final on-disk path for a transfer. Validates transferId and confines under storage root.
///
/// Protocol transferId is used as the directory name only after validation; original id stays
/// the DB identity (same string for valid `ft_{uuid}` ids).
pub(super) fn final_file_path(transfer_id: &str, filename: &str) -> Result<PathBuf, String> {
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
/// Never trust stored paths blindly — re-validate confinement before any FS use.
pub(super) fn resolve_transfer_path(
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

pub(super) fn part_file_path(final_path: &Path) -> PathBuf {
    let mut part = final_path.as_os_str().to_os_string();
    part.push(".part");
    PathBuf::from(part)
}

fn transfer_lock_key(transfer_id: &str) -> String {
    format!("federation-file-transfer:{transfer_id}")
}

const TRANSFER_ADMISSION_LOCK_KEY: &str = "myriad:federation:transfer_admission";

/// Serialize concurrent transfer admission (count + insert) across connections.
/// The caller must hold an explicit transaction for the duration of check+insert.
pub(super) async fn lock_transfer_admission(
    db: &impl ConnectionTrait,
) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        [TRANSFER_ADMISSION_LOCK_KEY.into()],
    ))
    .await?;
    Ok(())
}

/// Serialize one transfer's database and filesystem state across backend replicas.
/// The caller must hold an explicit transaction for the duration of the mutation.
pub(super) async fn lock_transfer_session(
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

pub(super) fn path_to_db(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

pub(super) fn storage_err(e: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!("[FileTransfer] storage error: {}", e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AppError::public_json("File storage error")),
    )
}

pub(super) fn bad_request(message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(AppError::public_json(message)),
    )
}

pub(super) fn http_err_to_string(err: (StatusCode, Json<serde_json::Value>)) -> String {
    format!("{}: {}", err.0, err.1 .0)
}

pub(super) async fn stored_bytes(path: &str) -> i64 {
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

pub(super) async fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
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

pub(super) async fn verify_chunk_bytes(
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
            Json(AppError::public_json(
                "Chunk retry content does not match stored bytes",
            )),
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
pub(super) enum ChunkFileState {
    Part,
    Final,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UploadSessionAction {
    WriteChunk,
    ResumeFinalization,
    VerifyCompletedRetry,
}

pub(super) fn upload_session_action(
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
            Json(AppError::public_json(
                "Transfer finalization state is inconsistent",
            )),
        )),
        _ => Err(bad_request(format!("Transfer is {}", status))),
    }
}

/// Recover a retry that arrives after the final rename but before the database
/// commit, otherwise write the current chunk into the session's isolated part.
pub(super) async fn prepare_chunk_file(
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
                Json(AppError::public_json(
                    "Final file exists before the last chunk",
                )),
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
pub(super) async fn finalize_part_file(
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
pub(super) async fn finalize_uploaded_transfer(
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
                Json(AppError::public_json("Transfer not found while finalizing")),
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
            Json(AppError::public_json(
                "Transfer state changed while finalizing",
            )),
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
            Json(AppError::public_json(
                "Transfer state changed while finalizing",
            )),
        ));
    }
    txn.commit().await.map_err(db_err)?;
    Ok(true)
}

// ── path safety + admission tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::federation::types::generate_transfer_id;

    const VALID_FT_UUID: &str = "ft_550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn in_flight_chunk_budget_admits_then_rejects_when_full() {
        // `try_acquire(0)` is a no-op（不占/不释放 in-flight 预算）。
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
    fn transfer_admission_lock_key_is_stable() {
        assert_eq!(
            TRANSFER_ADMISSION_LOCK_KEY,
            "myriad:federation:transfer_admission"
        );
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

        // Lexical `..` parent escape via `PathBuf::join`
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
use myriad_error::AppError;
