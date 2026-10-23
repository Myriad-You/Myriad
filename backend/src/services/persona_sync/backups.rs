//! The daily backups of her a device she lives on pushes to the site (D4,
//! §5.6): each a sealed `myriad.her-archive` made with a passphrase that
//! never leaves the device (Q5), so the site keeps it and cannot open it.
//!
//! One per device and day, sent in pieces (resumable). The newest
//! [`KEEP`] of her lineage are kept, whichever device pushed them; removing
//! the device keeps them, since they are how she is taken back. Any device
//! of the owner may list and read them, to open one in the app and take her
//! back from it; the owner may download or delete them on the devices page.
//!
//! Only the device she lives on pushes one, or the device she is moving to
//! once it has her (its first backup comes before the commit, so the site
//! has a copy to take her back from from the start).

use std::path::PathBuf;

use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, TransactionTrait,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::lease::{self, Phase, State};

/// The newest backups of her lineage that are kept.
pub const KEEP: i64 = 7;
/// Largest backup, and one piece of it.
pub const FILE_MAX_BYTES: u64 = 1 << 30;
pub const PIECE_MAX_BYTES: usize = 8 * 1024 * 1024;

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

fn backups_dir() -> PathBuf {
    crate::services::data_paths::paths().agent.join("backups")
}

fn safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn file_of(device: &str, day: &str) -> Option<PathBuf> {
    (safe(device) && well_formed_day(day)).then(|| {
        backups_dir()
            .join(device)
            .join(format!("{day}.her-archive"))
    })
}

fn well_formed_day(day: &str) -> bool {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_ok() && day.len() == 10
}

/// Why a backup is refused.
#[derive(Debug)]
pub enum BackupError {
    /// This device is not where she lives (or is moving to with her whole).
    NotHost,
    /// Another lineage than hers.
    Lineage,
    /// A piece that is not where the upload stands, too large, or a day that
    /// does not read.
    Invalid(&'static str),
    Unknown,
    Failed(String),
}

impl From<DbErr> for BackupError {
    fn from(error: DbErr) -> Self {
        Self::Failed(error.to_string())
    }
}

impl BackupError {
    pub fn refusal(&self) -> (axum::http::StatusCode, &'static str, &'static str) {
        use axum::http::StatusCode;
        match self {
            Self::NotHost => (
                StatusCode::CONFLICT,
                "backup_not_host",
                "Only the device she lives on keeps a backup of her here",
            ),
            Self::Lineage => (
                StatusCode::CONFLICT,
                "backup_lineage",
                "A backup of another her",
            ),
            Self::Invalid(_) => (
                StatusCode::BAD_REQUEST,
                "backup_invalid",
                "The backup piece does not read, or is not where the upload stands",
            ),
            Self::Unknown => (StatusCode::NOT_FOUND, "backup_unknown", "No such backup"),
            Self::Failed(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "backup_failed",
                "The backup could not be kept",
            ),
        }
    }
}

/// What a piece says of the backup it is part of.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PieceQuery {
    pub offset: u64,
    pub total: u64,
    pub lineage: String,
    pub epoch: i64,
    pub digest: String,
}

/// May `device` keep a backup of her now: it is where she lives, or the
/// device she is moving to once it has her whole.
fn may_push(lease: &lease::Lease, device: &str) -> bool {
    if lease.holder == device && matches!(lease.state, State::Live | State::Frozen) {
        return true;
    }
    lease.moving.as_ref().is_some_and(|moving| {
        moving.to_device()
            && moving.to == device
            && matches!(moving.phase, Phase::Transferring | Phase::Applied)
    })
}

/// One piece of the backup of `day` from `device`: the first (`offset` 0)
/// starts it again; the last makes it whole, and the oldest beyond the
/// newest [`KEEP`] of her lineage go.
pub async fn take_piece(
    db: &DatabaseConnection,
    device: &str,
    day: &str,
    query: PieceQuery,
    bytes: Vec<u8>,
) -> Result<Value, BackupError> {
    use tokio::io::AsyncWriteExt;
    let path = file_of(device, day).ok_or(BackupError::Invalid("day"))?;
    if bytes.is_empty()
        || bytes.len() > PIECE_MAX_BYTES
        || query.total > FILE_MAX_BYTES
        || query.offset + bytes.len() as u64 > query.total
        || query.digest.len() != 64
        || !query.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(BackupError::Invalid("piece"));
    }
    let held = lease::read(db).await?.ok_or(BackupError::NotHost)?;
    if !may_push(&held, device) {
        return Err(BackupError::NotHost);
    }
    if held.lineage != query.lineage {
        return Err(BackupError::Lineage);
    }
    let part = path.with_extension("her-archive.part");
    let failed = |error: std::io::Error| BackupError::Failed(error.to_string());
    let txn = db.begin().await?;
    let now = crate::clock::now();
    let row = txn
        .query_one_raw(statement(
            "SELECT id, received, bytes FROM agent_host_backups WHERE device_id = $1 AND day = $2 \
             FOR UPDATE",
            vec![device.into(), day.into()],
        ))
        .await?;
    let id = if query.offset == 0 {
        let id = match &row {
            Some(row) => row.try_get::<String>("", "id")?,
            None => format!("bk_{}", uuid::Uuid::new_v4().simple()),
        };
        txn.execute_raw(statement(
            "INSERT INTO agent_host_backups (id, device_id, day, lineage, epoch, digest, bytes, \
             received, complete, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, 0, false, $8::timestamptz, $8::timestamptz) \
             ON CONFLICT (device_id, day) DO UPDATE SET lineage = EXCLUDED.lineage, \
             epoch = EXCLUDED.epoch, digest = EXCLUDED.digest, bytes = EXCLUDED.bytes, \
             received = 0, complete = false, updated_at = $8::timestamptz",
            vec![
                id.clone().into(),
                device.into(),
                day.into(),
                query.lineage.clone().into(),
                query.epoch.into(),
                query.digest.clone().into(),
                (query.total as i64).into(),
                now.into(),
            ],
        ))
        .await?;
        if let Some(parent) = part.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(failed)?;
        }
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        options.mode(0o600);
        options.open(&part).await.map_err(failed)?;
        id
    } else {
        let row = row.ok_or(BackupError::Invalid("piece"))?;
        let received: i64 = row.try_get("", "received")?;
        let total: i64 = row.try_get("", "bytes")?;
        if received as u64 != query.offset || total as u64 != query.total {
            return Err(BackupError::Invalid("piece"));
        }
        row.try_get::<String>("", "id")?
    };
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(&part)
        .await
        .map_err(failed)?;
    file.write_all(&bytes).await.map_err(failed)?;
    file.sync_all().await.map_err(failed)?;
    let received = query.offset + bytes.len() as u64;
    let complete = received == query.total;
    txn.execute_raw(statement(
        "UPDATE agent_host_backups SET received = $2, complete = $3, updated_at = $4::timestamptz \
         WHERE id = $1",
        vec![
            id.clone().into(),
            (received as i64).into(),
            complete.into(),
            now.into(),
        ],
    ))
    .await?;
    if complete {
        tokio::fs::rename(&part, &path).await.map_err(failed)?;
    }
    txn.commit().await?;
    if complete {
        let gone = rotate(db, &query.lineage).await?;
        tracing::info!(
            device,
            day,
            bytes = received,
            gone,
            "[backup] a daily backup of her is kept"
        );
    }
    Ok(json!({ "id": id, "received": received, "complete": complete }))
}

/// The oldest whole backups of `lineage` beyond the newest [`KEEP`] go.
async fn rotate(db: &DatabaseConnection, lineage: &str) -> Result<usize, BackupError> {
    let rows = db
        .query_all_raw(statement(
            "SELECT id, device_id, day FROM agent_host_backups WHERE lineage = $1 AND complete \
             ORDER BY day DESC, updated_at DESC OFFSET $2",
            vec![lineage.into(), KEEP.into()],
        ))
        .await?;
    let mut gone = 0;
    for row in &rows {
        let id: String = row.try_get("", "id")?;
        let device: String = row.try_get("", "device_id")?;
        let day: String = row.try_get("", "day")?;
        forget_row(db, &id, &device, &day).await?;
        gone += 1;
    }
    Ok(gone)
}

async fn forget_row(
    db: &DatabaseConnection,
    id: &str,
    device: &str,
    day: &str,
) -> Result<(), BackupError> {
    db.execute_raw(statement(
        "DELETE FROM agent_host_backups WHERE id = $1",
        vec![id.into()],
    ))
    .await?;
    if let Some(path) = file_of(device, day) {
        for path in [path.clone(), path.with_extension("her-archive.part")] {
            if let Err(error) = tokio::fs::remove_file(&path).await
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(%error, backup = %id, "[backup] could not remove a backup file");
            }
        }
    }
    Ok(())
}

/// The device `device` has a whole backup of `lineage` here.
pub(super) async fn has_one<C: ConnectionTrait>(
    db: &C,
    device: &str,
    lineage: &str,
) -> Result<bool, DbErr> {
    Ok(db
        .query_one_raw(statement(
            "SELECT 1 AS one FROM agent_host_backups WHERE device_id = $1 AND lineage = $2 \
             AND complete",
            vec![device.into(), lineage.into()],
        ))
        .await?
        .is_some())
}

/// The whole backups of her lineage, newest first: id, day, size, epoch,
/// digest, which device pushed it (its name), when. Nothing of what is in
/// them (the site cannot read that).
pub async fn listed(db: &DatabaseConnection) -> Result<Vec<Value>, DbErr> {
    let Some(lineage) = lease::read(db).await?.map(|lease| lease.lineage) else {
        return Ok(Vec::new());
    };
    let rows = db
        .query_all_raw(statement(
            "SELECT b.id, b.device_id, d.name, b.day, b.epoch, b.digest, b.bytes, b.updated_at \
             FROM agent_host_backups b LEFT JOIN agent_sync_devices d ON d.id = b.device_id \
             WHERE b.lineage = $1 AND b.complete ORDER BY b.day DESC, b.updated_at DESC",
            vec![lineage.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(json!({
                "id": row.try_get::<String>("", "id")?,
                "deviceId": row.try_get::<String>("", "device_id")?,
                "deviceName": row.try_get::<Option<String>>("", "name")?,
                "day": row.try_get::<String>("", "day")?,
                "epoch": row.try_get::<i64>("", "epoch")?,
                "digest": row.try_get::<String>("", "digest")?,
                "bytes": row.try_get::<i64>("", "bytes")?,
                "at": row
                    .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "updated_at")?
                    .with_timezone(&Utc)
                    .to_rfc3339(),
            }))
        })
        .collect()
}

/// Where the whole backup `id` is kept, and its size.
pub async fn file(db: &DatabaseConnection, id: &str) -> Result<(PathBuf, u64), BackupError> {
    if !safe(id) {
        return Err(BackupError::Unknown);
    }
    let row = db
        .query_one_raw(statement(
            "SELECT device_id, day, bytes FROM agent_host_backups WHERE id = $1 AND complete",
            vec![id.into()],
        ))
        .await?
        .ok_or(BackupError::Unknown)?;
    let device: String = row.try_get("", "device_id")?;
    let day: String = row.try_get("", "day")?;
    let bytes: i64 = row.try_get("", "bytes")?;
    let path = file_of(&device, &day).ok_or(BackupError::Unknown)?;
    Ok((path, bytes.max(0) as u64))
}

/// A piece of the whole backup `id`, from `offset`.
pub async fn piece(
    db: &DatabaseConnection,
    id: &str,
    offset: u64,
    length: usize,
) -> Result<Vec<u8>, BackupError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let (path, size) = file(db, id).await?;
    if offset > size {
        return Err(BackupError::Invalid("offset"));
    }
    let failed = |error: std::io::Error| BackupError::Failed(error.to_string());
    let mut file = tokio::fs::File::open(&path).await.map_err(failed)?;
    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(failed)?;
    let wanted = length
        .clamp(1, PIECE_MAX_BYTES)
        .min((size - offset) as usize);
    let mut out = vec![0u8; wanted];
    file.read_exact(&mut out).await.map_err(failed)?;
    Ok(out)
}

/// The owner deletes the backup `id`.
pub async fn forget(db: &DatabaseConnection, id: &str) -> Result<(), BackupError> {
    if !safe(id) {
        return Err(BackupError::Unknown);
    }
    let row = db
        .query_one_raw(statement(
            "SELECT device_id, day FROM agent_host_backups WHERE id = $1",
            vec![id.into()],
        ))
        .await?
        .ok_or(BackupError::Unknown)?;
    let device: String = row.try_get("", "device_id")?;
    let day: String = row.try_get("", "day")?;
    forget_row(db, id, &device, &day).await
}
