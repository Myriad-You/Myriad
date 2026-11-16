//! Her moving between the site and a device she will live on (H3e), with
//! the site as the registry of where she is ([`super::lease`]).
//!
//! - **To a device** (site → Mac, §5.1): the device agrees (its owner said
//!   yes there) and the owner confirms on the site, logged in, with a
//!   passphrase for the copy the site keeps (D4, used once). Only then is she
//!   frozen (H3e-2). In one snapshot the site seals her twice: the copy it
//!   keeps, into `data/agent/retired/`, and the copy the device takes, with a
//!   key made for this move alone (H3e-4), into `data/agent/moves/<id>/`.
//!   The device reads that file in pieces (resumable), checks it, keeps it
//!   apart (`applied`), pushes its first daily backup, and commits: in one
//!   transaction the lease names the device one epoch on and the site lets
//!   its own copy of her go (Q2: her own parts go, others' parts stay sealed
//!   with her lineage). The device starts her only once the commit answered.
//! - **Back from the device** (Mac → site, §5.2, H3e-8): asked on either end
//!   and confirmed by the owner on the site; she is frozen on the device,
//!   which reports what it froze ([`report_frozen`]) and sends her as an
//!   import ([`super::import`]), checked against that and landed in one
//!   transaction with the lease naming the site one epoch on.
//!
//! The transfer key is in no log, error or row; the device fetches it with
//! its token while the move is under way, and it goes with the move.

use std::path::PathBuf;

use base64::Engine as _;
use rand::Rng as _;
use sea_orm::{DatabaseConnection, TransactionTrait};
use serde::Deserialize;
use serde_json::{Value, json};

use super::import::{self, ImportError};
use super::lease::{self, Lease, MoveError, Phase};
use myriad_persona_sync::archive::Manifest;

/// Largest piece of the file a device reads at once.
pub const PIECE_MAX_BYTES: usize = 8 * 1024 * 1024;
const ARCHIVE: &str = "her.her-archive";
const KEY: &str = "key";
/// How long the site waits for a reply under way to end before it reads her
/// for a move (no new one starts while she is frozen).
const SETTLE: std::time::Duration = std::time::Duration::from_secs(60);

fn moves_dir() -> PathBuf {
    crate::services::data_paths::paths().agent.join("moves")
}

fn well_formed(id: &str) -> bool {
    id.strip_prefix("mv_").is_some_and(|rest| {
        !rest.is_empty() && rest.len() <= 40 && rest.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

fn move_dir(id: &str) -> Option<PathBuf> {
    well_formed(id).then(|| moves_dir().join(id))
}

/// The copy the site keeps of her when she moves to a device.
pub fn retired_name(id: &str) -> String {
    format!("her-moved-{id}.her-archive")
}

/// A move called off: what it kept for the way goes, and the copy the site
/// sealed of her (nothing changed).
pub async fn forget_files(id: &str) {
    if !well_formed(id) {
        return;
    }
    forget_transfer(id).await;
    let retired = super::merge::retired_dir().join(retired_name(id));
    if let Err(error) = tokio::fs::remove_file(&retired).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, move_id = %id, "[move] could not remove the copy sealed for it");
    }
}

/// What the device took: the file and its key go once she lives there.
async fn forget_transfer(id: &str) {
    let Some(dir) = move_dir(id) else {
        return;
    };
    if let Err(error) = tokio::fs::remove_dir_all(&dir).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, move_id = %id, "[move] could not remove what a move kept");
    }
}

async fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<(), MoveError> {
    use tokio::io::AsyncWriteExt;
    let failed = |error: std::io::Error| MoveError::Failed(error.to_string());
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(failed)?;
    }
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temp).await.map_err(failed)?;
    file.write_all(bytes).await.map_err(failed)?;
    file.sync_all().await.map_err(failed)?;
    tokio::fs::rename(&temp, path).await.map_err(failed)
}

fn exported(error: super::export::ExportError) -> MoveError {
    use super::export::ExportError;
    match error {
        ExportError::Passphrase => MoveError::Failed("passphrase".into()),
        ExportError::Empty => MoveError::NoHer,
        ExportError::Elsewhere(_) => MoveError::NotHere,
        ExportError::Failed(what) => MoveError::Failed(what),
    }
}

/// The owner confirms on the site, logged in, the move of her to a device
/// that agreed: she is frozen, and sealed for the copy the site keeps (with
/// `passphrase`) and for the device (with a key of this move's own).
/// Anything failing calls the move off: she is as she was.
pub async fn confirm_to_device(
    db: &DatabaseConnection,
    owner: i32,
    id: &str,
    passphrase: String,
) -> Result<Lease, MoveError> {
    if passphrase.is_empty() {
        return Err(MoveError::Failed("passphrase".into()));
    }
    let moving = lease::read(db)
        .await?
        .and_then(|lease| lease.moving)
        .filter(|moving| moving.id == id && moving.to_device())
        .ok_or(MoveError::Unknown)?;
    let Some(dir) = move_dir(&moving.id) else {
        return Err(MoveError::Unknown);
    };
    lease::confirm(db, id).await?;
    lease::reload(db).await;
    let sealed = async {
        // A reply under way ends first, so what she just said goes with her.
        let waited = std::time::Instant::now();
        while crate::services::agent::run_hub::user_has_executing_run(owner).await
            && waited.elapsed() < SETTLE
        {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        let her = super::export::read(db, owner).await.map_err(exported)?;
        let kept = super::export::seal(&her, passphrase)
            .await
            .map_err(exported)?;
        write_private(&super::merge::retired_dir().join(retired_name(id)), &kept).await?;
        let key = {
            let mut bytes = [0u8; 32];
            rand::rng().fill_bytes(&mut bytes);
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
        };
        let taken = super::export::seal(&her, key.clone())
            .await
            .map_err(exported)?;
        write_private(&dir.join(ARCHIVE), &taken).await?;
        write_private(&dir.join(KEY), key.as_bytes()).await?;
        lease::sealed(db, id, her.digest()).await
    }
    .await;
    match sealed {
        Ok(lease) => {
            tracing::info!(move_id = %id, device = %moving.to, "[move] the owner confirmed: she is frozen and sealed for the device");
            Ok(lease)
        }
        Err(error) => {
            if let MoveError::Failed(what) = &error {
                tracing::error!(what, move_id = %id, "[move] could not seal her for the device; called off");
            }
            let _ = lease::cancel(db, id, None).await;
            lease::reload(db).await;
            Err(error)
        }
    }
}

/// The move to this device, as it reads it: where it stands, the file to
/// take (its size and digest) and, while it is under way, the key to open it.
pub async fn shown_to_device(
    db: &DatabaseConnection,
    id: &str,
    device: &str,
) -> Result<Value, MoveError> {
    let lease = lease::read(db).await?.ok_or(MoveError::Unknown)?;
    let host = lease::shown(db).await?;
    // Committed already: she lives on it (a commit answer it missed).
    if lease.done.as_ref().is_some_and(|done| done.id == id) && lease.holder == device {
        return Ok(json!({ "host": host, "committed": true }));
    }
    let moving = lease
        .moving
        .filter(|moving| moving.id == id && moving.to_device() && moving.to == device)
        .ok_or(MoveError::Unknown)?;
    let mut answer = json!({ "host": host, "committed": false, "phase": moving.phase });
    if let (Some(digest), Some(dir)) = (&moving.digest, move_dir(id)) {
        let bytes = tokio::fs::metadata(dir.join(ARCHIVE))
            .await
            .map(|meta| meta.len())
            .map_err(|error| MoveError::Failed(error.to_string()))?;
        let key = tokio::fs::read_to_string(dir.join(KEY))
            .await
            .map_err(|error| MoveError::Failed(error.to_string()))?;
        answer["archive"] = json!({ "bytes": bytes, "digest": digest, "key": key });
    }
    Ok(answer)
}

/// A piece of the file of her this device takes, from `offset`.
pub async fn piece(
    db: &DatabaseConnection,
    id: &str,
    device: &str,
    offset: u64,
    length: usize,
) -> Result<Vec<u8>, MoveError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    lease::device_step(db, id, device, Phase::Transferring).await?;
    let dir = move_dir(id).ok_or(MoveError::Unknown)?;
    let failed = |error: std::io::Error| MoveError::Failed(error.to_string());
    let mut file = tokio::fs::File::open(dir.join(ARCHIVE))
        .await
        .map_err(failed)?;
    let size = file.metadata().await.map_err(failed)?.len();
    if offset > size {
        return Err(MoveError::Phase(Phase::Transferring));
    }
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

/// The device has all of her, checked against `digest` and kept apart.
pub async fn applied(
    db: &DatabaseConnection,
    id: &str,
    device: &str,
    digest: &str,
) -> Result<Lease, MoveError> {
    let moving = lease::read(db)
        .await?
        .and_then(|lease| lease.moving)
        .filter(|moving| moving.id == id && moving.to_device() && moving.to == device)
        .ok_or(MoveError::Unknown)?;
    if moving.digest.as_deref() != Some(digest) {
        return Err(MoveError::Mismatch);
    }
    lease::device_step(db, id, device, Phase::Applied).await
}

/// The device commits: her lease goes to it, one epoch on. Asked again after
/// it went through, it answers the lease. Only once the device's first
/// daily backup is here, so the site has a copy to take her back from.
///
/// With her log synced (C2-a) the site keeps its copy of her: her mind is
/// on the device ([`lease::Where::MindOn`]), the device is one of her host
/// cores from now, and the server takes her mind back if it goes out of
/// reach ([`crate::services::her_log::lease::watch`]). What the log does not
/// carry yet stays as it was here when she left. Without it the site lets
/// its copy go in the same transaction (Q2) and takes her back only when
/// the owner confirms it.
pub async fn commit_to_device(
    db: &DatabaseConnection,
    owner: i32,
    id: &str,
    device: &str,
    digest: &str,
) -> Result<Lease, MoveError> {
    commit_to_device_with(
        db,
        crate::services::her_log::settings(),
        owner,
        id,
        device,
        digest,
    )
    .await
}

pub(crate) async fn commit_to_device_with(
    db: &DatabaseConnection,
    settings: &crate::services::her_log::Settings,
    owner: i32,
    id: &str,
    device: &str,
    digest: &str,
) -> Result<Lease, MoveError> {
    let keep = crate::services::her_log::pen::in_play(db, settings, Some(owner))
        .await
        .map_err(|error| MoveError::Failed(error.to_string()))?;
    let txn = db.begin().await?;
    let lease = lease::hold(&txn).await?.ok_or(MoveError::Unknown)?;
    if lease.done.as_ref().is_some_and(|done| done.id == id) && lease.holder == device {
        txn.commit().await?;
        return Ok(lease);
    }
    let moving = lease
        .moving
        .clone()
        .filter(|moving| moving.id == id && moving.to_device() && moving.to == device)
        .ok_or(MoveError::Unknown)?;
    if moving.phase != Phase::Applied {
        return Err(MoveError::Phase(moving.phase));
    }
    if moving.digest.as_deref() != Some(digest) {
        return Err(MoveError::Mismatch);
    }
    if !super::backups::has_one(&txn, device, &lease.lineage).await? {
        return Err(MoveError::BackupMissing);
    }
    if keep {
        let written = lease::committed_to_device(&txn, id, device, true).await?;
        crate::services::her_log::lease::heard(
            &txn,
            device,
            Some(crate::services::her_log::lease::Core::Host),
            crate::services::her_log::now(),
        )
        .await
        .map_err(|error| MoveError::Failed(error.to_string()))?;
        txn.commit().await?;
        forget_transfer(id).await;
        lease::reload(db).await;
        tracing::info!(move_id = %id, device, epoch = written.epoch, "[move] her mind is on the device now; the site keeps its copy (her log is synced)");
        return Ok(written);
    }
    crate::services::agent::merope::api::store::lock_persona_on(&txn)
        .await
        .map_err(|error| MoveError::Failed(error.to_string()))?;
    crate::services::agent::merope::retire_her_on(&txn, owner)
        .await
        .map_err(|error| MoveError::Failed(error.to_string()))?;
    let cleared = crate::services::merope_rig::persist_active_asset(&txn, None)
        .await
        .map_err(|error| MoveError::Failed(error.to_string()))?;
    crate::services::media::bind_persona(
        &txn,
        None,
        None,
        None,
        &crate::services::media::configured_origins().await,
    )
    .await
    .map_err(|error| MoveError::Failed(error.to_string()))?;
    let written = lease::committed_to_device(&txn, id, device, false).await?;
    txn.commit().await?;
    crate::services::merope_rig::mirror_active_asset(cleared).await;
    crate::services::agent::merope::forget_in_memory();
    forget_transfer(id).await;
    lease::reload(db).await;
    tracing::info!(move_id = %id, device, epoch = written.epoch, "[move] she lives on the device now; the site let its copy go");
    Ok(written)
}

/// What the device she lives on froze for a move back to the site: her
/// lineage, the epoch there, her name and the manifest of every kind.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Frozen {
    pub lineage: String,
    pub epoch: i64,
    #[serde(default)]
    pub name: Option<String>,
    pub manifest: Manifest,
}

/// The device she lives on is frozen for the move back `id` and says what
/// it froze: the import that carries her starts, confirmed (the owner
/// confirmed the move), to be checked against this manifest.
pub async fn report_frozen(
    db: &DatabaseConnection,
    id: &str,
    device: &str,
    frozen: Frozen,
) -> Result<import::Session, ImportError> {
    let txn = db.begin().await?;
    import::one_at_a_time(&txn).await?;
    let lease = lease::hold(&txn).await?.ok_or(ImportError::Unknown)?;
    let moving = lease
        .moving
        .clone()
        .filter(|moving| moving.id == id && moving.back_to_site() && moving.from == device)
        .ok_or(ImportError::Unknown)?;
    if !moving.phase.freezes() {
        return Err(ImportError::Phase(
            format!("{:?}", moving.phase).to_lowercase(),
        ));
    }
    if frozen.lineage != lease.lineage {
        return Err(ImportError::Mismatch("another lineage".into()));
    }
    if let Some(carrying) = &moving.import {
        // Said again (the answer was lost): the import it started.
        let session = import::locked(&txn, carrying)
            .await?
            .ok_or(ImportError::Unknown)?;
        if session.manifest.as_ref() != Some(&frozen.manifest) {
            return Err(ImportError::Mismatch("not what it froze".into()));
        }
        txn.commit().await?;
        return Ok(session);
    }
    let session = import::begin_carrying(
        &txn,
        import::kind::RETURN,
        device,
        &frozen.lineage,
        frozen.epoch,
        frozen.name,
        &frozen.manifest,
    )
    .await?;
    lease::carried_by(&txn, id, &session.id, &frozen.manifest.digest).await?;
    txn.commit().await?;
    lease::reload(db).await;
    tracing::info!(move_id = %id, import = %session.id, device, "[move] she is frozen on the device and comes back as an import");
    Ok(session)
}

#[cfg(test)]
#[path = "moves_tests.rs"]
mod tests;
