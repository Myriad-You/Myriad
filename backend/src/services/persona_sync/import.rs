//! Bringing her into a site that has none (H3c): from a device that holds
//! her, or from an export file the owner uploads.
//!
//! - **From a device** (Mac → empty site): the device asks with what it
//!   holds of her (her lineage and epoch there, her name, the manifest of
//!   every kind). The owner confirms on the site, logged in (Q4): only then
//!   does the site write its first lease, naming that device and frozen,
//!   because she is on her way and no end has her (H3c-1). The device sends
//!   her page by page and file by file, resumable; the commit checks what
//!   arrived against the manifest the owner was shown, so a device cannot
//!   swap her after the confirmation, and lands her in one transaction with
//!   the lease now naming the site, one epoch on (H3c-3). Called off, the
//!   site is as it was: no her, no lease.
//! - **From a file**: the owner uploads the sealed file in pieces (resumable,
//!   H3c-14), then gives the passphrase once to open it; it is used for the
//!   key and kept nowhere. The file goes as soon as she landed or the
//!   import was called off.
//!
//! Only into an empty site (H3c-7): no persona, none of her memories, diary
//! or affect, and no lease. Anything else is refused with what is there;
//! bringing two of her together is the merge, not this.
//!
//! Her pictures go into the media library (one copy per picture, by its
//! sha256) and every address that named the old place names the new one;
//! rigs are kept as packages. Vectors come along when the site's model is
//! the one they were made with (by name and length, H3c-13); otherwise she
//! fills them in as she remembers. Grants and settings never come: they are
//! each end's own, and her switch is not touched (H3c-15).

use std::collections::BTreeMap;
use std::path::PathBuf;

use base64::Engine as _;
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use myriad_persona_sync::{
    AssetRef, AssetType, EntityVersion, Envelope, Kind, State,
    archive::{self, Manifest, RIG_ATLAS_FILE, RIG_MANIFEST_FILE},
    canonical::sha256_hex,
    entity::{
        AffectAnchorBody, AffectEventBody, ConversationBody, DiaryBody, Embedding, EmbeddingModel,
        FieldHistoryBody, IntentionBody, MemoryBody, MemoryVenue, OutfitBody, OwnerStateBody,
        PaceBody, Picture, PresentBody, ProactiveLineBody, StickerBody, WearingBody,
    },
    hlc,
    persona::{FieldRev, PersonaBody, VISUAL_PREFIX},
};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DatabaseTransaction, DbErr, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{assets, lease};
use crate::services::agent::merope::api::sync as her;
use crate::services::media::{MediaActor, MediaContext, MediaExposure, MediaSource, NewMediaBytes};

/// Every import's id starts so; a move with such an id is an import.
pub const ID_PREFIX: &str = "imp_";
/// Largest export file the site takes, and one piece of it.
pub const FILE_MAX_BYTES: u64 = 1 << 30;
pub const PIECE_MAX_BYTES: usize = 8 * 1024 * 1024;
/// Largest page of her a device sends at once.
pub const PAGE_MAX_BYTES: usize = 8 * 1024 * 1024;
const FILE_NAME: &str = "her.her-archive";
const IMAGE_FILE: &str = "image";

/// Where an import is.
pub mod phase {
    /// A device asked; the owner has not confirmed.
    pub const PROPOSED: &str = "proposed";
    /// The owner confirmed; nothing has arrived yet.
    pub const CONFIRMED: &str = "confirmed";
    /// Pages and files are arriving.
    pub const TRANSFERRING: &str = "transferring";
    /// A file's pieces are arriving.
    pub const UPLOADING: &str = "uploading";
    /// She landed.
    pub const COMMITTED: &str = "committed";
}

pub fn is_import(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

pub(super) fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

fn import_dir(id: &str) -> PathBuf {
    crate::services::data_paths::paths()
        .agent
        .join("import")
        .join(id)
}

/// What a device says it brings, shown to the owner before they confirm.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Proposal {
    pub lineage: String,
    pub epoch: i64,
    #[serde(default)]
    pub name: Option<String>,
    pub manifest: Manifest,
    /// `merge`: its her is brought into the site's (H3d); otherwise into a
    /// site with none.
    #[serde(default)]
    pub kind: Option<String>,
    /// Who she is when her names differ: `site` or `device`.
    #[serde(default)]
    pub subject: Option<String>,
    /// Its persona and wardrobe, for the owner to see before confirming a
    /// merge (checked against the manifest when she lands).
    #[serde(default)]
    pub preview: Vec<EntityVersion>,
}

/// What an import is.
pub mod kind {
    /// Her into a site that has none (H3c).
    pub const IMPORT: &str = "import";
    /// A device's her into the site's, the first time they meet (H3d).
    pub const MERGE: &str = "merge";
    /// Her back from the device she lives on (H3e-8).
    pub const RETURN: &str = "return";
    /// Her taken back from a daily backup of the device she lived on, by
    /// another device that opened it (H3e-13).
    pub const RECOVER: &str = "recover";
}

/// One import as stored.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub source: String,
    pub device_id: Option<String>,
    pub lineage: Option<String>,
    pub epoch: Option<i64>,
    pub name: Option<String>,
    pub manifest: Option<Manifest>,
    pub phase: String,
    pub received: i64,
    pub total: Option<i64>,
    pub created_at: DateTime<FixedOffset>,
    pub confirmed_at: Option<DateTime<FixedOffset>>,
    pub kind: String,
    pub subject: Option<String>,
    pub preview: Vec<EntityVersion>,
    /// The file the site sealed of its own her before a merge.
    pub retired: Option<String>,
}

impl Session {
    pub fn is_merge(&self) -> bool {
        self.kind == kind::MERGE
    }

    /// She comes to a site that has her lease (back from her device, or
    /// taken back from a backup).
    pub fn is_carried(&self) -> bool {
        self.kind == kind::RETURN || self.kind == kind::RECOVER
    }

    fn from_row(row: &sea_orm::QueryResult) -> Result<Self, DbErr> {
        Ok(Self {
            id: row.try_get("", "id")?,
            source: row.try_get("", "source")?,
            device_id: row.try_get("", "device_id")?,
            lineage: row.try_get("", "lineage")?,
            epoch: row.try_get("", "epoch")?,
            name: row.try_get("", "name")?,
            manifest: row
                .try_get::<Option<Value>>("", "manifest")?
                .and_then(|value| serde_json::from_value(value).ok()),
            phase: row.try_get("", "phase")?,
            received: row.try_get("", "received")?,
            total: row.try_get("", "total")?,
            created_at: row.try_get("", "created_at")?,
            confirmed_at: row.try_get("", "confirmed_at")?,
            kind: row.try_get("", "kind")?,
            subject: row.try_get("", "subject")?,
            preview: row
                .try_get::<Option<Value>>("", "preview")?
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default(),
            retired: row.try_get("", "retired")?,
        })
    }

    /// As the devices page and the device read it: what comes, never a
    /// token or passphrase (there is none here).
    pub fn json(&self, device_name: Option<String>) -> Value {
        let counts: BTreeMap<&str, u64> = self
            .manifest
            .iter()
            .flat_map(|manifest| manifest.kinds.iter())
            .map(|(kind, tally)| (kind.as_str(), tally.count))
            .collect();
        json!({
            "id": self.id,
            "source": self.source,
            "deviceId": self.device_id,
            "deviceName": device_name,
            "name": self.name,
            "lineage": self.lineage,
            "epoch": self.epoch,
            "phase": self.phase,
            "counts": counts,
            "embeddings": self.manifest.as_ref().map(|manifest| manifest.embeddings.count),
            "files": self.manifest.as_ref().map(|manifest| manifest.assets.count),
            "digest": self.manifest.as_ref().map(|manifest| manifest.digest.clone()),
            "received": self.received,
            "total": self.total,
            "createdAt": self.created_at.to_rfc3339(),
            "confirmedAt": self.confirmed_at.map(|at| at.to_rfc3339()),
            "kind": self.kind,
            "subject": self.subject,
            "retired": self.retired,
        })
    }
}

const COLUMNS: &str = "id, source, device_id, lineage, epoch, name, manifest, phase, received, \
                       total, created_at, confirmed_at, kind, subject, preview, retired";

/// Why an import cannot go on.
#[derive(Debug)]
pub enum ImportError {
    /// The site has her already (her name, when she has one).
    NotEmpty(Option<String>),
    /// No such import now, or not this device's.
    Unknown,
    /// The import is not at a step this can be done at.
    Phase(String),
    /// Another import is under way.
    Busy,
    /// What arrived is not what was shown to the owner, or not whole.
    Mismatch(String),
    /// The file does not open with that passphrase, or is not whole.
    Unreadable,
    /// The file or a piece is too large, or a piece is not where the file
    /// stands.
    Size,
    /// A request that does not read.
    Invalid(&'static str),
    /// A merge into a site with no her.
    NoHer,
    /// A merge while she does not live on the site (or is moving).
    NotHere,
    /// Her names on the two sides are now the same where they differed when
    /// the owner confirmed, or the other way round.
    Changed,
    /// The site she left keeps what was left of others with another her's
    /// lineage (Q2): bringing this one in lets that go, so the owner says so.
    ReplacesSealed,
    Failed(String),
}

impl From<DbErr> for ImportError {
    fn from(error: DbErr) -> Self {
        Self::Failed(error.to_string())
    }
}

impl ImportError {
    fn failed(what: impl std::fmt::Display) -> Self {
        Self::Failed(what.to_string())
    }

    /// The status and code a request is refused with; never a value.
    pub fn refusal(&self) -> (axum::http::StatusCode, &'static str, &'static str) {
        use axum::http::StatusCode;
        match self {
            Self::NotEmpty(_) => (
                StatusCode::CONFLICT,
                "import_not_empty",
                "The site has her already; bringing two of her together is a merge",
            ),
            Self::Unknown => (
                StatusCode::NOT_FOUND,
                "import_unknown",
                "No such import now",
            ),
            Self::Phase(_) => (
                StatusCode::CONFLICT,
                "import_phase",
                "The import is not at a step this can be done at",
            ),
            Self::Busy => (
                StatusCode::CONFLICT,
                "import_busy",
                "Another import is under way",
            ),
            Self::Mismatch(_) => (
                StatusCode::CONFLICT,
                "import_mismatch",
                "What arrived is not what was confirmed",
            ),
            Self::Unreadable => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "archive_unreadable",
                "The file does not open with this passphrase, or it is not whole",
            ),
            Self::Size => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "import_size",
                "Too large, or not where the upload stands",
            ),
            Self::Invalid(_) => (
                StatusCode::BAD_REQUEST,
                "import_invalid",
                "The request does not read",
            ),
            Self::NoHer => (
                StatusCode::CONFLICT,
                "merge_no_her",
                "The site has no her to merge into",
            ),
            Self::NotHere => (
                StatusCode::CONFLICT,
                "host_moved",
                "She does not live on this site now",
            ),
            Self::Changed => (
                StatusCode::CONFLICT,
                "merge_changed",
                "Her names changed since the owner confirmed; confirm the merge again",
            ),
            Self::ReplacesSealed => (
                StatusCode::CONFLICT,
                "import_replaces_sealed",
                "The site keeps what was left of another her; bringing this one in lets it go",
            ),
            Self::Failed(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "import_failed",
                "She could not be brought in",
            ),
        }
    }
}

/// Why the site is not empty, None when it is: her persona or anything of
/// hers, or a lease (she lives somewhere, or is on her way).
pub async fn not_empty<C: ConnectionTrait>(
    db: &C,
    owner: i32,
) -> Result<Option<ImportError>, DbErr> {
    let holdings = super::holdings(db, owner).await?;
    if !myriad_persona_sync::is_empty(&holdings) {
        return Ok(Some(ImportError::NotEmpty(
            super::persona_name(db)
                .await?
                .filter(|name| !name.is_empty()),
        )));
    }
    // A lease that is vacant is a site she left: she may come in again
    // (her lineage takes up what she left; another lets it go, H3e-15).
    let leased = db
        .query_one_raw(statement(
            "SELECT 1 AS one FROM agent_host_lease WHERE id = 'her' AND state <> 'vacant'",
            vec![],
        ))
        .await?
        .is_some();
    Ok(leased.then_some(ImportError::NotEmpty(None)))
}

/// A site she left (vacant) that keeps another lineage than `lineage`:
/// what is left there of others would go with her coming in.
async fn sealed_other<C: ConnectionTrait>(db: &C, lineage: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one_raw(statement(
            "SELECT 1 AS one FROM agent_host_lease WHERE id = 'her' AND state = 'vacant' \
             AND lineage <> $1",
            vec![lineage.into()],
        ))
        .await?
        .is_some())
}

/// What was left of another her on a site she left goes, with its lease,
/// so a her of another lineage comes in (the owner said so).
async fn let_sealed_go(txn: &DatabaseTransaction) -> Result<(), ImportError> {
    crate::services::agent::merope::clear_sealed_on(txn)
        .await
        .map_err(ImportError::failed)?;
    lease::forget_vacant_on(txn).await?;
    tracing::info!("[import] what was left of another her here goes, with her lease");
    Ok(())
}

/// The approval-layer record of her own Work on this end goes when she
/// arrives here (H3e-17): whatever the owner let a her here do before, she
/// starts with it off and the owner turns it on again here. Tool
/// permissions and roles are this end's own and stay. Not for a merge: the
/// her that was here is still here.
pub(super) async fn void_her_own_work<C: ConnectionTrait>(db: &C, owner: i32) -> Result<(), DbErr> {
    let gone = db
        .execute_raw(statement(
            "DELETE FROM agent_autonomy_grants WHERE user_id = $1",
            vec![owner.into()],
        ))
        .await?;
    if gone.rows_affected() > 0 {
        tracing::info!(
            "[import] she arrived: her own Work is off here until the owner turns it on"
        );
    }
    Ok(())
}

/// The import that carries her back from the device she lives on: it
/// starts confirmed (the owner confirmed the move it carries).
pub(super) async fn begin_carrying(
    txn: &DatabaseTransaction,
    kind_of: &str,
    device: &str,
    lineage: &str,
    epoch: i64,
    name: Option<String>,
    manifest: &Manifest,
) -> Result<Session, ImportError> {
    check_lineage(lineage)?;
    make_room(txn, Some(device)).await?;
    let id = format!("{ID_PREFIX}{}", uuid::Uuid::new_v4().simple());
    let name = name
        .map(|name| name.trim().chars().take(80).collect::<String>())
        .filter(|name| !name.is_empty());
    let row = txn
        .query_one_raw(statement(
            &format!(
                "INSERT INTO agent_host_import (id, source, device_id, lineage, epoch, name, \
                 manifest, phase, created_at, updated_at, confirmed_at, kind) \
                 VALUES ($1, 'device', $2, $3, $4, $5, $6, $7, $9::timestamptz, $9::timestamptz, $9::timestamptz, $8) \
                 RETURNING {COLUMNS}"
            ),
            vec![
                id.into(),
                device.into(),
                lineage.into(),
                epoch.into(),
                name.into(),
                serde_json::to_value(manifest)
                    .map_err(ImportError::failed)?
                    .into(),
                phase::CONFIRMED.into(),
                kind_of.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?
        .ok_or_else(|| ImportError::failed("no row"))?;
    Ok(Session::from_row(&row)?)
}

/// An import that carried her and is called off with its move: its session
/// goes (what was staged with it).
pub(super) async fn drop_session<C: ConnectionTrait>(db: &C, id: &str) -> Result<(), DbErr> {
    db.execute_raw(statement(
        "DELETE FROM agent_host_import WHERE id = $1 AND phase <> 'committed'",
        vec![id.into()],
    ))
    .await?;
    Ok(())
}

/// The import now, if any (the last committed one too, until another).
pub async fn current<C: ConnectionTrait>(db: &C) -> Result<Option<Session>, DbErr> {
    db.query_one_raw(statement(
        &format!("SELECT {COLUMNS} FROM agent_host_import ORDER BY created_at DESC LIMIT 1"),
        vec![],
    ))
    .await?
    .as_ref()
    .map(Session::from_row)
    .transpose()
}

/// The import under way (not committed), as the devices page shows it; a
/// merge with what the owner is shown before confirming it.
pub async fn shown(db: &DatabaseConnection, owner: i32) -> Result<Value, DbErr> {
    let Some(session) = current(db)
        .await?
        .filter(|session| session.phase != phase::COMMITTED)
    else {
        return Ok(Value::Null);
    };
    let name = match &session.device_id {
        Some(device) => lease::device_name(db, device).await?,
        None => None,
    };
    let mut shown = session.json(name.clone());
    if session.is_merge() {
        shown["merge"] = super::merge::preview(db, owner, &session, name).await?;
    }
    Ok(shown)
}

/// The import this device asked for, as it reads it back (committed too, so
/// a device that missed the answer learns she landed).
pub async fn shown_for_device<C: ConnectionTrait>(db: &C, device: &str) -> Result<Value, DbErr> {
    Ok(current(db)
        .await?
        .filter(|session| session.device_id.as_deref() == Some(device))
        .map_or(Value::Null, |session| session.json(None)))
}

pub(super) async fn one_at_a_time(txn: &DatabaseTransaction) -> Result<(), DbErr> {
    txn.execute_raw(statement(
        "SELECT pg_advisory_xact_lock(hashtext('agent_host_import'))",
        vec![],
    ))
    .await?;
    Ok(())
}

pub(super) async fn locked(txn: &DatabaseTransaction, id: &str) -> Result<Option<Session>, DbErr> {
    txn.query_one_raw(statement(
        &format!("SELECT {COLUMNS} FROM agent_host_import WHERE id = $1 FOR UPDATE"),
        vec![id.into()],
    ))
    .await?
    .as_ref()
    .map(Session::from_row)
    .transpose()
}

/// Drops imports that are over (committed) or were never confirmed, before
/// a new one; one under way refuses the new one.
async fn make_room(txn: &DatabaseTransaction, device: Option<&str>) -> Result<(), ImportError> {
    let rows = txn
        .query_all_raw(statement(
            &format!("SELECT {COLUMNS} FROM agent_host_import FOR UPDATE"),
            vec![],
        ))
        .await?;
    let mut gone = Vec::new();
    for row in &rows {
        let session = Session::from_row(row)?;
        let stale = session.phase == phase::COMMITTED
            || (session.phase == phase::PROPOSED
                && device.is_some()
                && session.device_id.as_deref() == device);
        if !stale {
            return Err(ImportError::Busy);
        }
        gone.push(session.id);
    }
    for id in gone {
        txn.execute_raw(statement(
            "DELETE FROM agent_host_import WHERE id = $1",
            vec![id.clone().into()],
        ))
        .await?;
        forget_files(&id).await;
    }
    Ok(())
}

/// A move of the lease refused, as an import is.
pub(super) fn merge_refused(error: lease::MoveError) -> ImportError {
    match error {
        lease::MoveError::NoHer => ImportError::NoHer,
        lease::MoveError::NotHere => ImportError::NotHere,
        lease::MoveError::InProgress => ImportError::Busy,
        lease::MoveError::Unknown => ImportError::Unknown,
        lease::MoveError::Phase(phase) => ImportError::Phase(format!("{phase:?}").to_lowercase()),
        lease::MoveError::Db(error) => ImportError::Failed(error.to_string()),
        lease::MoveError::Mismatch => ImportError::Mismatch("not what was sealed".into()),
        lease::MoveError::NotAgreed | lease::MoveError::Later | lease::MoveError::BackupMissing => {
            ImportError::Phase("proposed".into())
        }
        lease::MoveError::Failed(what) => ImportError::Failed(what),
    }
}

fn check_lineage(lineage: &str) -> Result<(), ImportError> {
    let ok = !lineage.is_empty()
        && lineage.len() <= 36
        && lineage
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    if ok {
        Ok(())
    } else {
        Err(ImportError::Invalid("lineage"))
    }
}

/// A device asks to bring the her it holds into this empty site. Nothing
/// happens to the site until the owner confirms.
pub async fn propose(
    db: &DatabaseConnection,
    owner: i32,
    device: &str,
    proposal: Proposal,
) -> Result<Session, ImportError> {
    check_lineage(&proposal.lineage)?;
    if proposal.epoch < 1 {
        return Err(ImportError::Invalid("epoch"));
    }
    let merging = match proposal.kind.as_deref() {
        None | Some(kind::IMPORT) => false,
        Some(kind::MERGE) => true,
        Some(kind::RECOVER) => return propose_reclaim(db, device, proposal).await,
        Some(_) => return Err(ImportError::Invalid("kind")),
    };
    let subject = match proposal.subject.as_deref() {
        None => None,
        Some(side @ ("site" | "device")) if merging => Some(side.to_string()),
        Some(_) => return Err(ImportError::Invalid("subject")),
    };
    let preview: Vec<EntityVersion> = proposal
        .preview
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Persona | Kind::Outfit | Kind::Wearing))
        .cloned()
        // Only shown: its digest is the site's own reading of it.
        .map(|mut entity| {
            entity.digest.clear();
            myriad_persona_sync::canonicalize(entity)
        })
        .collect::<Result<_, _>>()
        .map_err(|_| ImportError::Invalid("preview"))?;
    let txn = db.begin().await?;
    one_at_a_time(&txn).await?;
    if merging {
        if not_empty(&txn, owner).await?.is_none() {
            return Err(ImportError::NoHer);
        }
    } else if let Some(refusal) = not_empty(&txn, owner).await? {
        return Err(refusal);
    }
    make_room(&txn, Some(device)).await?;
    let id = format!("{ID_PREFIX}{}", uuid::Uuid::new_v4().simple());
    let manifest = serde_json::to_value(&proposal.manifest).map_err(ImportError::failed)?;
    let name = proposal
        .name
        .map(|name| name.trim().chars().take(80).collect::<String>())
        .filter(|name| !name.is_empty());
    let row = txn
        .query_one_raw(statement(
            &format!(
                "INSERT INTO agent_host_import (id, source, device_id, lineage, epoch, name, \
                 manifest, phase, created_at, updated_at, kind, subject, preview) \
                 VALUES ($1, 'device', $2, $3, $4, $5, $6, $7, $11::timestamptz, $11::timestamptz, $8, $9, $10) \
                 RETURNING {COLUMNS}"
            ),
            vec![
                id.clone().into(),
                device.into(),
                proposal.lineage.into(),
                proposal.epoch.into(),
                name.into(),
                manifest.into(),
                phase::PROPOSED.into(),
                (if merging { kind::MERGE } else { kind::IMPORT }).into(),
                subject.into(),
                merging
                    .then(|| serde_json::to_value(&preview))
                    .transpose()
                    .map_err(ImportError::failed)?
                    .into(),
                crate::clock::now().into(),
            ],
        ))
        .await?
        .ok_or_else(|| ImportError::failed("no row"))?;
    let session = Session::from_row(&row)?;
    if merging {
        lease::merge_step(&txn, &id, device, lease::Phase::Proposed)
            .await
            .map_err(merge_refused)?;
    }
    txn.commit().await?;
    if merging {
        lease::reload(db).await;
    }
    tracing::info!(import = %session.id, device, "[import] a device asked to bring her here; the owner confirms on the site");
    Ok(session)
}

/// Another device asks to take her back onto the site from a daily backup
/// of the device she lived on (H3e-13), with what the backup holds. Her
/// lineage must be the lease's; nothing happens to her until the owner
/// confirms on the site.
async fn propose_reclaim(
    db: &DatabaseConnection,
    device: &str,
    proposal: Proposal,
) -> Result<Session, ImportError> {
    let txn = db.begin().await?;
    one_at_a_time(&txn).await?;
    let held = lease::hold(&txn).await?.ok_or(ImportError::NoHer)?;
    if held.lineage != proposal.lineage {
        return Err(ImportError::Mismatch("another lineage".into()));
    }
    let session = begin_carrying(
        &txn,
        kind::RECOVER,
        device,
        &proposal.lineage,
        proposal.epoch,
        proposal.name,
        &proposal.manifest,
    )
    .await?;
    // Asked, not confirmed: the owner confirms it on the site.
    txn.execute_raw(statement(
        "UPDATE agent_host_import SET phase = $2, confirmed_at = NULL WHERE id = $1",
        vec![session.id.clone().into(), phase::PROPOSED.into()],
    ))
    .await?;
    lease::reclaiming(&txn, &session.id, device)
        .await
        .map_err(merge_refused)?;
    let session = locked(&txn, &session.id)
        .await?
        .ok_or(ImportError::Unknown)?;
    txn.commit().await?;
    lease::reload(db).await;
    tracing::info!(import = %session.id, device, "[import] a device asks to take her back from a backup; the owner confirms on the site");
    Ok(session)
}

/// The owner, logged in, confirms a device's import: the site's first lease
/// names that device, frozen, until she lands or it is called off. On a
/// site she left that keeps another her's lineage, `replace` says the owner
/// lets what was left of that her go (H3e-15).
pub async fn confirm(
    db: &DatabaseConnection,
    owner: i32,
    id: &str,
    passphrase: Option<String>,
    replace: bool,
) -> Result<lease::Lease, ImportError> {
    if current(db)
        .await?
        .is_some_and(|session| session.id == id && session.is_merge())
    {
        return super::merge::confirm(db, owner, id, passphrase.unwrap_or_default()).await;
    }
    let txn = db.begin().await?;
    one_at_a_time(&txn).await?;
    let session = locked(&txn, id).await?.ok_or(ImportError::Unknown)?;
    if session.phase != phase::PROPOSED {
        return Err(ImportError::Phase(session.phase));
    }
    if session.kind == kind::RECOVER {
        let written = lease::import_confirmed(&txn, id)
            .await
            .map_err(merge_refused)?;
        txn.execute_raw(statement(
            "UPDATE agent_host_import SET phase = $2, confirmed_at = $3::timestamptz, updated_at = $3::timestamptz \
             WHERE id = $1",
            vec![id.into(), phase::CONFIRMED.into(), crate::clock::now().into()],
        ))
        .await?;
        txn.commit().await?;
        tracing::info!(import = %id, "[import] the owner confirmed taking her back from a backup: she is frozen everywhere");
        lease::reload(db).await;
        return Ok(written);
    }
    if let Some(refusal) = not_empty(&txn, owner).await? {
        return Err(refusal);
    }
    let (Some(device), Some(lineage), Some(epoch), Some(manifest)) = (
        session.device_id.as_deref(),
        session.lineage.as_deref(),
        session.epoch,
        session.manifest.as_ref(),
    ) else {
        return Err(ImportError::Phase(session.phase));
    };
    if sealed_other(&txn, lineage).await? {
        if !replace {
            return Err(ImportError::ReplacesSealed);
        }
        let_sealed_go(&txn).await?;
    }
    let written = lease::arriving(&txn, id, device, lineage, epoch, &manifest.digest).await?;
    txn.execute_raw(statement(
        "UPDATE agent_host_import SET phase = $2, confirmed_at = $3::timestamptz, updated_at = $3::timestamptz \
         WHERE id = $1",
        vec![id.into(), phase::CONFIRMED.into(), crate::clock::now().into()],
    ))
    .await?;
    txn.commit().await?;
    tracing::info!(import = %id, device, epoch, "[import] the owner confirmed: she is on her way here");
    lease::reload(db).await;
    Ok(written)
}

/// Inside the transaction that calls an import off: its session and the
/// lease it wrote go. The files go once it commits ([`forget_files`]).
pub async fn called_off_on<C: ConnectionTrait>(db: &C, id: &str) -> Result<(), DbErr> {
    db.execute_raw(statement(
        "DELETE FROM agent_host_import WHERE id = $1",
        vec![id.into()],
    ))
    .await?;
    lease::forget_arrival(db, id).await?;
    // A merge leaves the site's her where she lives, as she was.
    lease::merge_called_off(db, id).await
}

/// The files an import kept while it was under way.
pub async fn forget_files(id: &str) {
    if !is_import(id) || id.contains(['/', '\\', '.']) {
        return;
    }
    let dir = import_dir(id);
    if let Err(error) = tokio::fs::remove_dir_all(&dir).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, import = %id, "[import] could not remove an import's files");
    }
}

/// Calls an import off before she landed: the owner (`device` None) or the
/// device it is from. The site is as it was.
pub async fn cancel(
    db: &DatabaseConnection,
    id: &str,
    device: Option<&str>,
) -> Result<(), ImportError> {
    let txn = db.begin().await?;
    one_at_a_time(&txn).await?;
    let session = locked(&txn, id).await?.ok_or(ImportError::Unknown)?;
    if device.is_some() && session.device_id.as_deref() != device {
        return Err(ImportError::Unknown);
    }
    if session.phase == phase::COMMITTED {
        return Err(ImportError::Phase(session.phase));
    }
    called_off_on(&txn, id).await?;
    txn.commit().await?;
    forget_files(id).await;
    super::merge::forget_retired(id).await;
    lease::reload(db).await;
    tracing::info!(import = %id, by_device = device.is_some(), "[import] called off: the site is as it was");
    Ok(())
}

async fn device_session(
    txn: &DatabaseTransaction,
    id: &str,
    device: &str,
) -> Result<Session, ImportError> {
    let session = locked(txn, id).await?.ok_or(ImportError::Unknown)?;
    if session.device_id.as_deref() != Some(device) {
        return Err(ImportError::Unknown);
    }
    Ok(session)
}

fn receiving(session: &Session) -> Result<(), ImportError> {
    if session.phase == phase::CONFIRMED || session.phase == phase::TRANSFERRING {
        Ok(())
    } else {
        Err(ImportError::Phase(session.phase.clone()))
    }
}

/// One page of her from the device, kept until she lands. Sending a page
/// again is harmless.
pub async fn take_page(
    db: &DatabaseConnection,
    id: &str,
    device: &str,
    envelope: Envelope,
) -> Result<u64, ImportError> {
    let envelope = myriad_persona_sync::canonicalize_envelope(envelope)
        .map_err(|error| ImportError::Mismatch(error.to_string()))?;
    let txn = db.begin().await?;
    let session = device_session(&txn, id, device).await?;
    receiving(&session)?;
    if envelope.lineage != session.lineage {
        return Err(ImportError::Mismatch("a page of another lineage".into()));
    }
    let mut rows: Vec<(String, String, Value)> = Vec::new();
    for entity in &envelope.entities {
        rows.push((
            entity.kind.as_str().to_string(),
            entity.id.clone(),
            serde_json::to_value(entity).map_err(ImportError::failed)?,
        ));
    }
    for embedding in &envelope.embeddings {
        rows.push((
            "embedding".into(),
            embedding.memory_id.clone(),
            serde_json::to_value(embedding).map_err(ImportError::failed)?,
        ));
    }
    for asset in &envelope.assets {
        rows.push((
            "asset".into(),
            asset.id.clone(),
            serde_json::to_value(asset).map_err(ImportError::failed)?,
        ));
    }
    if let Some(model) = &envelope.embedding_model {
        rows.push((
            "embedding_model".into(),
            "model".into(),
            serde_json::to_value(model).map_err(ImportError::failed)?,
        ));
    }
    rows.push((
        "origin".into(),
        "origin".into(),
        Value::String(envelope.origin.clone()),
    ));
    for (kind, entity_id, canonical) in rows {
        txn.execute_raw(statement(
            "INSERT INTO agent_host_import_staging (import_id, kind, entity_id, canonical) \
             VALUES ($1, $2, $3, $4) ON CONFLICT (import_id, kind, entity_id) \
             DO UPDATE SET canonical = EXCLUDED.canonical",
            vec![id.into(), kind.into(), entity_id.into(), canonical.into()],
        ))
        .await?;
    }
    if session.phase == phase::CONFIRMED {
        txn.execute_raw(statement(
            "UPDATE agent_host_import SET phase = $2, updated_at = $3::timestamptz WHERE id = $1",
            vec![
                id.into(),
                phase::TRANSFERRING.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
        lease::import_step(&txn, id, lease::Phase::Transferring).await?;
    }
    let staged = staged_count(&txn, id).await?;
    txn.commit().await?;
    Ok(staged)
}

async fn staged_count<C: ConnectionTrait>(db: &C, id: &str) -> Result<u64, DbErr> {
    Ok(db
        .query_one_raw(statement(
            "SELECT COUNT(*)::BIGINT AS n FROM agent_host_import_staging \
             WHERE import_id = $1 AND kind NOT IN ('embedding', 'asset', 'embedding_model', 'origin')",
            vec![id.into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0)
        .max(0) as u64)
}

fn asset_file(id: &str, asset: &str, file: Option<&str>) -> Result<PathBuf, ImportError> {
    let asset = myriad_persona_sync::canonical::hex(
        &(0..asset.len() / 2)
            .map(|at| u8::from_str_radix(asset.get(at * 2..at * 2 + 2).unwrap_or("zz"), 16))
            .collect::<Result<Vec<u8>, _>>()
            .map_err(|_| ImportError::Invalid("asset id"))?,
    );
    if asset.len() != 64 {
        return Err(ImportError::Invalid("asset id"));
    }
    let name = match file {
        None => IMAGE_FILE,
        Some(RIG_MANIFEST_FILE) => RIG_MANIFEST_FILE,
        Some(RIG_ATLAS_FILE) => RIG_ATLAS_FILE,
        Some(_) => return Err(ImportError::Invalid("file")),
    };
    Ok(import_dir(id).join("assets").join(asset).join(name))
}

/// One file of her from the device: a picture (checked against its id) or
/// one of a rig's two files (checked as a package when she lands).
pub async fn take_asset(
    db: &DatabaseConnection,
    id: &str,
    device: &str,
    asset: &str,
    file: Option<&str>,
    bytes: Vec<u8>,
) -> Result<(), ImportError> {
    if bytes.len() as u64 > super::ASSET_MAX_BYTES {
        return Err(ImportError::Size);
    }
    let path = asset_file(id, asset, file)?;
    {
        let txn = db.begin().await?;
        let session = device_session(&txn, id, device).await?;
        receiving(&session)?;
        txn.commit().await?;
    }
    if file.is_none() && sha256_hex(&bytes) != asset.to_ascii_lowercase() {
        return Err(ImportError::Mismatch(
            "a picture is not the bytes its id names".into(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| ImportError::failed("no folder"))?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(ImportError::failed)?;
    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    tokio::fs::write(&temp, &bytes)
        .await
        .map_err(ImportError::failed)?;
    tokio::fs::rename(&temp, &path)
        .await
        .map_err(ImportError::failed)?;
    Ok(())
}

/// What has arrived of a device's import, so an interrupted one goes on
/// where it stopped: the entities kept, and the files whole.
pub async fn received(
    db: &DatabaseConnection,
    id: &str,
    device: &str,
) -> Result<Value, ImportError> {
    let session = current(db)
        .await?
        .filter(|session| session.id == id && session.device_id.as_deref() == Some(device))
        .ok_or(ImportError::Unknown)?;
    let staged = staged_count(db, id).await?;
    let mut files = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(import_dir(id).join("assets")).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let asset = entry.file_name().to_string_lossy().to_string();
            for name in [IMAGE_FILE, RIG_MANIFEST_FILE, RIG_ATLAS_FILE] {
                if tokio::fs::try_exists(entry.path().join(name))
                    .await
                    .unwrap_or(false)
                {
                    files.push(if name == IMAGE_FILE {
                        asset.clone()
                    } else {
                        format!("{asset}/{name}")
                    });
                }
            }
        }
    }
    files.sort();
    Ok(json!({ "import": session.json(None), "staged": staged, "files": files }))
}

/// Her, as one side holds her whole: entities, vectors, the files by asset
/// id (`image` for a picture, the rig's two files), and where she came from.
pub(super) struct Whole {
    pub(super) origin: String,
    pub(super) lineage: Option<String>,
    pub(super) epoch: Option<i64>,
    pub(super) model: Option<EmbeddingModel>,
    pub(super) entities: Vec<EntityVersion>,
    pub(super) embeddings: Vec<Embedding>,
    pub(super) files: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
}

/// The device says all of her is sent: she is checked against what the
/// owner confirmed and lands in one transaction. A commit asked again after
/// she landed answers the lease.
pub async fn commit(
    db: &DatabaseConnection,
    owner: i32,
    id: &str,
    device: &str,
    digest: &str,
) -> Result<Landed, ImportError> {
    let session = current(db)
        .await?
        .filter(|session| session.id == id && session.device_id.as_deref() == Some(device))
        .ok_or(ImportError::Unknown)?;
    if session.phase == phase::COMMITTED {
        return Ok(Landed {
            counts: BTreeMap::new(),
            vectors: 0,
            her_on: her_on().await,
        });
    }
    receiving(&session)?;
    let confirmed = session
        .manifest
        .clone()
        .ok_or_else(|| ImportError::Phase(session.phase.clone()))?;
    if confirmed.digest != digest {
        return Err(ImportError::Mismatch(
            "not the manifest that was confirmed".into(),
        ));
    }
    let rows = db
        .query_all_raw(statement(
            "SELECT kind, entity_id, canonical FROM agent_host_import_staging \
             WHERE import_id = $1 ORDER BY kind, entity_id",
            vec![id.into()],
        ))
        .await?;
    let mut whole = Whole {
        origin: hlc::SITE.to_string(),
        lineage: session.lineage.clone(),
        epoch: session.epoch,
        model: None,
        entities: Vec::new(),
        embeddings: Vec::new(),
        files: BTreeMap::new(),
    };
    let mut refs: Vec<AssetRef> = Vec::new();
    for row in &rows {
        let kind: String = row.try_get("", "kind")?;
        let canonical: Value = row.try_get("", "canonical")?;
        let bad = |error: serde_json::Error| ImportError::Mismatch(error.to_string());
        match kind.as_str() {
            "embedding" => whole
                .embeddings
                .push(serde_json::from_value(canonical).map_err(bad)?),
            "asset" => refs.push(serde_json::from_value(canonical).map_err(bad)?),
            "embedding_model" => {
                whole.model = Some(serde_json::from_value(canonical).map_err(bad)?)
            }
            "origin" => {
                whole.origin = canonical.as_str().unwrap_or(hlc::SITE).to_string();
            }
            _ => whole
                .entities
                .push(serde_json::from_value(canonical).map_err(bad)?),
        }
    }
    let manifest = Manifest::of(&whole.entities, &whole.embeddings, &refs)
        .map_err(|error| ImportError::Mismatch(error.to_string()))?;
    if manifest != confirmed {
        return Err(ImportError::Mismatch(
            "what arrived is not what the owner confirmed".into(),
        ));
    }
    for asset in &refs {
        let mut files = BTreeMap::new();
        let names: &[&str] = match asset.kind {
            AssetType::Image => &[IMAGE_FILE],
            AssetType::Rig => &[RIG_MANIFEST_FILE, RIG_ATLAS_FILE],
        };
        for name in names {
            let path = import_dir(id).join("assets").join(&asset.id).join(name);
            let bytes = tokio::fs::read(&path).await.map_err(|_| {
                ImportError::Mismatch(format!("the file {} has not arrived", asset.id))
            })?;
            files.insert(
                if *name == IMAGE_FILE {
                    String::new()
                } else {
                    name.to_string()
                },
                bytes,
            );
        }
        whole.files.insert(asset.id.clone(), files);
    }
    check_files(&whole.files, &refs)?;
    if session.is_carried() {
        // The move it carries is still the lease's.
        let carried = lease::read(db)
            .await?
            .and_then(|lease| lease.moving)
            .is_some_and(|moving| moving.import.as_deref() == Some(id) && moving.phase.freezes());
        if !carried {
            return Err(ImportError::Unknown);
        }
    }
    let landed = if session.is_merge() {
        super::merge::commit(db, owner, &session, whole).await?
    } else {
        land(db, owner, Some(id), whole, false).await?
    };
    forget_files(id).await;
    Ok(landed)
}

/// Every file the pages refer to is here and is what its id names.
fn check_files(
    files: &BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    refs: &[AssetRef],
) -> Result<(), ImportError> {
    for asset in refs {
        let Some(found) = files.get(&asset.id) else {
            return Err(ImportError::Mismatch(format!(
                "the file {} has not arrived",
                asset.id
            )));
        };
        match asset.kind {
            AssetType::Image => {
                if found.get("").map(|bytes| sha256_hex(bytes)) != Some(asset.id.clone()) {
                    return Err(ImportError::Mismatch(
                        "a picture is not what its id names".into(),
                    ));
                }
            }
            AssetType::Rig => {
                let (Some(manifest), Some(atlas)) =
                    (found.get(RIG_MANIFEST_FILE), found.get(RIG_ATLAS_FILE))
                else {
                    return Err(ImportError::Mismatch("a rig is its two files".into()));
                };
                let manifest: myriad_merope::RigManifest = serde_json::from_slice(manifest)
                    .map_err(|error| ImportError::Mismatch(error.to_string()))?;
                if myriad_persona_sync::rig_package_id(atlas, &manifest)
                    .ok()
                    .as_deref()
                    != Some(asset.id.as_str())
                {
                    return Err(ImportError::Mismatch(
                        "a rig is not the package its id names".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

// --- From a file -----------------------------------------------------------

/// The owner starts bringing her in from an export file of `total` bytes.
pub async fn begin_file(
    db: &DatabaseConnection,
    owner: i32,
    total: u64,
) -> Result<Session, ImportError> {
    if total == 0 || total > FILE_MAX_BYTES {
        return Err(ImportError::Size);
    }
    let txn = db.begin().await?;
    one_at_a_time(&txn).await?;
    if let Some(refusal) = not_empty(&txn, owner).await? {
        return Err(refusal);
    }
    // A file the owner started and left is theirs to start again.
    let rows = txn
        .query_all_raw(statement(
            "SELECT id FROM agent_host_import WHERE source = 'file' OR phase IN ($1, $2)",
            vec![phase::COMMITTED.into(), phase::PROPOSED.into()],
        ))
        .await?;
    for row in &rows {
        let old: String = row.try_get("", "id")?;
        called_off_on(&txn, &old).await?;
        forget_files(&old).await;
    }
    if current(&txn).await?.is_some() {
        return Err(ImportError::Busy);
    }
    let id = format!("{ID_PREFIX}{}", uuid::Uuid::new_v4().simple());
    let row = txn
        .query_one_raw(statement(
            &format!(
                "INSERT INTO agent_host_import (id, source, phase, total, created_at, updated_at) \
                 VALUES ($1, 'file', $2, $3, $4::timestamptz, $4::timestamptz) RETURNING {COLUMNS}"
            ),
            vec![
                id.clone().into(),
                phase::UPLOADING.into(),
                (total as i64).into(),
                crate::clock::now().into(),
            ],
        ))
        .await?
        .ok_or_else(|| ImportError::failed("no row"))?;
    let session = Session::from_row(&row)?;
    txn.commit().await?;
    tokio::fs::create_dir_all(import_dir(&id))
        .await
        .map_err(ImportError::failed)?;
    Ok(session)
}

async fn file_session(db: &DatabaseConnection, id: &str) -> Result<Session, ImportError> {
    current(db)
        .await?
        .filter(|session| session.id == id && session.source == "file")
        .ok_or(ImportError::Unknown)
}

/// One piece of the file, at `offset` (which must be where the upload
/// stands, so a piece sent twice or out of turn is refused, not doubled).
pub async fn take_piece(
    db: &DatabaseConnection,
    id: &str,
    offset: u64,
    bytes: Vec<u8>,
) -> Result<Session, ImportError> {
    use tokio::io::AsyncWriteExt as _;
    if bytes.is_empty() || bytes.len() > PIECE_MAX_BYTES {
        return Err(ImportError::Size);
    }
    let txn = db.begin().await?;
    let session = locked(&txn, id).await?.ok_or(ImportError::Unknown)?;
    if session.source != "file" || session.phase != phase::UPLOADING {
        return Err(ImportError::Phase(session.phase));
    }
    let total = session.total.unwrap_or(0).max(0) as u64;
    if offset != session.received.max(0) as u64 || offset + bytes.len() as u64 > total {
        return Err(ImportError::Size);
    }
    let path = import_dir(id).join(FILE_NAME);
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .await
        .map_err(ImportError::failed)?;
    file.set_len(offset).await.map_err(ImportError::failed)?;
    use tokio::io::AsyncSeekExt as _;
    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(ImportError::failed)?;
    file.write_all(&bytes).await.map_err(ImportError::failed)?;
    file.flush().await.map_err(ImportError::failed)?;
    let row = txn
        .query_one_raw(statement(
            &format!(
                "UPDATE agent_host_import SET received = $2, updated_at = $3::timestamptz WHERE id = $1 \
                 RETURNING {COLUMNS}"
            ),
            vec![
                id.into(),
                ((offset + bytes.len() as u64) as i64).into(),
                crate::clock::now().into(),
            ],
        ))
        .await?
        .ok_or(ImportError::Unknown)?;
    let session = Session::from_row(&row)?;
    txn.commit().await?;
    Ok(session)
}

/// Where a file's upload stands.
pub async fn file_status(db: &DatabaseConnection, id: &str) -> Result<Session, ImportError> {
    file_session(db, id).await
}

/// The whole file is here: it is opened with the passphrase (used once,
/// for the key, and kept nowhere), checked, and she lands. The file goes
/// either way once she landed; a wrong passphrase leaves it to try again.
pub async fn open_file(
    db: &DatabaseConnection,
    owner: i32,
    id: &str,
    passphrase: String,
    replace: bool,
) -> Result<Landed, ImportError> {
    if passphrase.is_empty() {
        return Err(ImportError::Invalid("passphrase"));
    }
    let session = file_session(db, id).await?;
    if session.phase != phase::UPLOADING || Some(session.received) != session.total {
        return Err(ImportError::Phase(session.phase));
    }
    if let Some(refusal) = not_empty(db, owner).await? {
        return Err(refusal);
    }
    let bytes = tokio::fs::read(import_dir(id).join(FILE_NAME))
        .await
        .map_err(ImportError::failed)?;
    // argon2id and the cipher take a moment: off the async threads.
    let opened = tokio::task::spawn_blocking(move || archive::open(&passphrase, &bytes))
        .await
        .map_err(ImportError::failed)?
        .map_err(|_| ImportError::Unreadable)?;
    let mut whole = Whole {
        origin: opened.header.origin.clone(),
        lineage: opened.header.lineage.clone(),
        epoch: opened
            .header
            .epoch
            .map(|epoch| epoch.min(i64::MAX as u64) as i64),
        model: opened
            .envelopes
            .iter()
            .find_map(|envelope| envelope.embedding_model.clone()),
        entities: Vec::new(),
        embeddings: Vec::new(),
        files: BTreeMap::new(),
    };
    for envelope in opened.envelopes {
        whole.entities.extend(envelope.entities);
        whole.embeddings.extend(envelope.embeddings);
    }
    for (asset, files) in opened.assets {
        whole.files.insert(
            asset,
            files
                .into_iter()
                .map(|(name, bytes)| {
                    (
                        if name == IMAGE_FILE {
                            String::new()
                        } else {
                            name
                        },
                        bytes,
                    )
                })
                .collect(),
        );
    }
    let landed = land(db, owner, Some(id), whole, replace).await?;
    forget_files(id).await;
    Ok(landed)
}

// --- Landing ---------------------------------------------------------------

/// What landed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Landed {
    /// Entities by kind.
    pub counts: BTreeMap<String, u64>,
    /// Vectors that came along (none when the site's model differs).
    pub vectors: u64,
    /// Her switch on the site, which the import does not touch.
    pub her_on: bool,
}

pub(super) async fn her_on() -> bool {
    crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved()
}

/// What a picture's bytes are, by their first bytes.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else {
        None
    }
}

pub(super) fn at(micros: i64) -> DateTime<FixedOffset> {
    Utc.timestamp_micros(micros)
        .single()
        .unwrap_or_else(crate::clock::now)
        .fixed_offset()
}

pub(super) fn utc(micros: i64) -> DateTime<Utc> {
    Utc.timestamp_micros(micros)
        .single()
        .unwrap_or_else(crate::clock::now)
}

/// Her end on the side she came from: the site's, or a Mac on its own.
fn own_end(origin: &str) -> &'static str {
    if origin == hlc::SITE { "site" } else { "local" }
}

/// How long the site keeps each of her own records it keeps in the
/// registry (as `merope::pace`, `vitals`, `serial`, `soup`, `doing` do).
pub(super) const KEEP_DAYS: i64 = 400;
pub(super) const KEEP_PRESENT_DAYS: i64 = 2;

/// Writes her into the empty site in one transaction: everything of her,
/// the lease naming the site, and the import committed.
async fn land(
    db: &DatabaseConnection,
    owner: i32,
    import: Option<&str>,
    whole: Whole,
    replace: bool,
) -> Result<Landed, ImportError> {
    let lineage = match whole.lineage.clone() {
        Some(lineage) => {
            check_lineage(&lineage)?;
            lineage
        }
        // A file from before her lineage was kept on its side: she gets one
        // now, once (H3c-2).
        None => uuid::Uuid::new_v4().to_string(),
    };
    let mut by_kind: BTreeMap<Kind, Vec<&EntityVersion>> = BTreeMap::new();
    for entity in &whole.entities {
        by_kind.entry(entity.kind).or_default().push(entity);
    }
    let of = |kind: Kind| by_kind.get(&kind).cloned().unwrap_or_default();
    if of(Kind::Persona).is_empty() {
        return Err(ImportError::Mismatch("there is no persona in it".into()));
    }

    // Files first, outside the transaction: a picture is kept once by its
    // sha256 (a retry finds it), a rig as its package.
    let urls = keep_files(db, owner, &whole.files).await?;
    let picture_url = |picture: Option<&Picture>| -> Option<String> {
        picture
            .and_then(Picture::asset)
            .and_then(|asset| urls.get(&asset.id).cloned())
    };

    let txn = db.begin().await?;
    if let Some(import) = import {
        one_at_a_time(&txn).await?;
        let session = locked(&txn, import).await?.ok_or(ImportError::Unknown)?;
        if session.phase == phase::COMMITTED || session.phase == phase::PROPOSED {
            return Err(ImportError::Phase(session.phase));
        }
    }
    crate::services::agent::merope::api::store::lock_persona_on(&txn)
        .await
        .map_err(ImportError::failed)?;
    let holdings = super::holdings(&txn, owner).await?;
    if !myriad_persona_sync::is_empty(&holdings) {
        return Err(ImportError::NotEmpty(super::persona_name(&txn).await?));
    }
    // A site she left with another her's lineage (a file brought in by the
    // owner): what was left of that her goes only when the owner said so.
    if sealed_other(&txn, &lineage).await? {
        if !replace {
            return Err(ImportError::ReplacesSealed);
        }
        let_sealed_go(&txn).await?;
    }
    // Her own Work starts off on the end she arrives at (H3e-17).
    void_her_own_work(&txn, owner).await?;
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut count = |kind: Kind| *counts.entry(kind.as_str().to_string()).or_default() += 1;

    // Her persona, her wardrobe and which set her ends wear.
    let persona: PersonaBody = of(Kind::Persona)[0]
        .body_as()
        .map_err(|error| ImportError::Mismatch(error.to_string()))?;
    let wearing: WearingBody = of(Kind::Wearing)
        .first()
        .and_then(|entity| entity.body_as().ok())
        .unwrap_or_default();
    let mut name = String::new();
    let mut personality = String::new();
    let mut zone = String::new();
    let mut persona_json = serde_json::Map::new();
    let mut visual = serde_json::Map::new();
    let mut revs = serde_json::Map::new();
    let mut avatar: Option<String> = None;
    for (field, value) in &persona.fields {
        if field == "stickerAvatar" {
            avatar = serde_json::from_value::<Picture>(value.value.clone())
                .ok()
                .and_then(|picture| picture_url(Some(&picture)));
            continue;
        }
        revs.insert(
            field.clone(),
            serde_json::to_value(FieldRev {
                rev: value.rev.clone(),
                origin: value.origin.clone(),
            })
            .map_err(ImportError::failed)?,
        );
        match field.as_str() {
            "name" => name = value.value.as_str().unwrap_or_default().to_string(),
            "personality" => personality = value.value.as_str().unwrap_or_default().to_string(),
            "zone" => zone = value.value.as_str().unwrap_or_default().to_string(),
            _ if value.value.is_null() => {}
            _ => match field.strip_prefix(VISUAL_PREFIX) {
                Some(key) => {
                    visual.insert(key.to_string(), value.value.clone());
                }
                None => {
                    persona_json.insert(field.clone(), value.value.clone());
                }
            },
        }
    }
    count(Kind::Persona);
    let mut outfits = of(Kind::Outfit);
    outfits.retain(|entity| entity.state == State::Live);
    outfits.sort_by_key(|entity| {
        (
            entity.id != myriad_merope::DEFAULT_WARDROBE_ID,
            entity.id.clone(),
        )
    });
    let mut wardrobe = Vec::new();
    let mut default_portrait: Option<String> = None;
    let mut rigs: BTreeMap<String, String> = BTreeMap::new();
    for entity in &outfits {
        let outfit: OutfitBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        let mut item = serde_json::Map::new();
        item.insert("id".into(), json!(entity.id));
        if let Some(name) = &outfit.name {
            item.insert("name".into(), json!(name));
        }
        item.insert("clothingStyle".into(), json!(outfit.clothing_style));
        item.insert("outfit".into(), outfit.outfit.clone());
        for (key, value) in [
            ("visualIdentity", outfit.visual_identity.clone()),
            ("profile", outfit.profile.clone().map(Value::String)),
            (
                "generationFingerprint",
                outfit.generation_fingerprint.clone().map(Value::String),
            ),
            (
                "referenceOutfitId",
                outfit.reference_outfit_id.clone().map(Value::String),
            ),
        ] {
            if let Some(value) = value {
                item.insert(key.into(), value);
            }
        }
        if let Some(url) = picture_url(outfit.portrait.as_ref()) {
            if entity.id == myriad_merope::DEFAULT_WARDROBE_ID {
                default_portrait = Some(url.clone());
            }
            item.insert("portraitAssetId".into(), json!(url));
        }
        if let Some(rig) = &outfit.rig
            && whole.files.contains_key(&rig.id)
        {
            item.insert("rigAssetId".into(), json!(rig.id));
            rigs.insert(entity.id.clone(), rig.id.clone());
        }
        wardrobe.push(Value::Object(item));
        count(Kind::Outfit);
    }
    // The site's own end wears what she wore where she came from (H3c-12);
    // the other ends' sets stay as they were.
    let own = wearing
        .ends
        .get(own_end(&whole.origin))
        .cloned()
        .unwrap_or_default();
    let has = |id: &Option<String>| {
        id.as_ref()
            .filter(|id| outfits.iter().any(|entity| &entity.id == *id))
            .cloned()
    };
    let active = has(&own.outfit).or_else(|| {
        outfits
            .iter()
            .any(|entity| entity.id == myriad_merope::DEFAULT_WARDROBE_ID)
            .then(|| myriad_merope::DEFAULT_WARDROBE_ID.to_string())
    });
    if !wardrobe.is_empty() {
        visual.insert("wardrobe".into(), Value::Array(wardrobe));
    }
    if let Some(active) = &active {
        visual.insert("activeOutfitId".into(), json!(active));
    }
    if let Some(full) = has(&own.full_body) {
        visual.insert("activeFullBodyOutfitId".into(), json!(full));
    }
    let by_end: serde_json::Map<String, Value> = wearing
        .ends
        .iter()
        .filter(|(end, _)| end.starts_with("app:"))
        .filter_map(|(end, worn)| Some((end.clone(), json!(has(&worn.outfit)?))))
        .collect();
    if !by_end.is_empty() {
        visual.insert("activeOutfitByEnd".into(), Value::Object(by_end));
    }
    if !of(Kind::Wearing).is_empty() {
        count(Kind::Wearing);
    }
    let visual_profile = (!visual.is_empty()).then_some(Value::Object(visual));
    {
        use sea_orm::{ActiveModelTrait, ActiveValue::Set};
        crate::models::entities::agent_persona::ActiveModel {
            id: Set(crate::services::agent::merope::api::store::PERSONA_ROW_ID.to_string()),
            name: Set(name),
            personality: Set(personality),
            persona_json: Set((!persona_json.is_empty()).then_some(Value::Object(persona_json))),
            visual_profile: Set(visual_profile.clone()),
            portrait_asset_id: Set(default_portrait.clone()),
            portrait_generation: Set(None),
            avatar_asset_id: Set(avatar.clone()),
            avatar_generation: Set(None),
            updated_by: Set(Some(owner)),
            updated_at: Set(crate::clock::now().fixed_offset()),
            zone: Set(zone.clone()),
            field_revs: Set(Value::Object(revs)),
        }
        .insert(&txn)
        .await?;
    }
    for entity in of(Kind::FieldHistory) {
        let past: FieldHistoryBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        txn.execute_raw(statement(
            "INSERT INTO agent_persona_field_history \
             (field, value, rev, origin, replaced_at, replaced_by_rev, reason) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            vec![
                past.field.into(),
                past.value.into(),
                past.rev.to_string().into(),
                past.origin.into(),
                at(past.replaced_at).into(),
                past.replaced_by_rev.to_string().into(),
                past.reason.into(),
            ],
        ))
        .await?;
        count(Kind::FieldHistory);
    }

    // Her memories, and their vectors when the site's model made them.
    let mut contents: BTreeMap<&str, String> = BTreeMap::new();
    for entity in of(Kind::Memory) {
        let memory: MemoryBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        let (venue, audience) = match memory.audience.venue {
            MemoryVenue::Own => ("own", json!([])),
            MemoryVenue::Private => ("private", json!([owner])),
        };
        let updated = at((entity.rev.wall_ms as i64).saturating_mul(1000));
        txn.execute_raw(statement(
            "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, \
             venue, audience, concepts, importance, valid_from, invalid_at, invalid_reason, \
             created_at, updated_at, superseded_by, origin) VALUES \
             ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)",
            vec![
                entity.id.clone().into(),
                owner.into(),
                memory.kind.into(),
                memory.content.clone().into(),
                memory.evidence.into(),
                memory.speaker.into(),
                memory.source.into(),
                venue.into(),
                audience.into(),
                serde_json::to_value(&memory.concepts)
                    .map_err(ImportError::failed)?
                    .into(),
                memory.importance.into(),
                at(memory.valid_from).into(),
                memory.invalid_at.map(at).into(),
                memory.invalid_reason.into(),
                at(memory.created_at).into(),
                updated.into(),
                memory.superseded_by.into(),
                entity.origin.clone().into(),
            ],
        ))
        .await?;
        if entity.state == State::Live {
            contents.insert(entity.id.as_str(), memory.content);
        }
        count(Kind::Memory);
    }
    let mut vectors = 0u64;
    if let Some(model) = vectors_fit(db, whole.model.as_ref()).await? {
        for embedding in &whole.embeddings {
            let Some(content) = contents.get(embedding.memory_id.as_str()) else {
                continue;
            };
            let Ok(vector) = base64::engine::general_purpose::STANDARD.decode(&embedding.vector)
            else {
                continue;
            };
            if embedding.digest != crate::services::agent::memory::meaning::digest(content)
                || vector.len() != model.dims as usize * 4
            {
                continue;
            }
            txn.execute_raw(statement(
                "INSERT INTO agent_memory_embeddings (memory_id, model, digest, vector, created_at) \
                 VALUES ($1, $2, $3, $4, $5::timestamptz)",
                vec![
                    embedding.memory_id.clone().into(),
                    model.model.clone().into(),
                    embedding.digest.clone().into(),
                    vector.into(),
                    crate::clock::now().into(),
                ],
            ))
            .await?;
            vectors += 1;
        }
    }

    for entity in of(Kind::Diary) {
        let note: DiaryBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        txn.execute_raw(statement(
            "INSERT INTO agent_diary (id, user_id, content, source, created_at) \
             VALUES ($1, $2, $3, $4, $5)",
            vec![
                entity.id.clone().into(),
                owner.into(),
                note.content.into(),
                note.source.into(),
                at(note.created_at).into(),
            ],
        ))
        .await?;
        count(Kind::Diary);
    }

    for entity in of(Kind::Sticker) {
        let sticker: StickerBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        let Some(url) = picture_url(Some(&sticker.image)) else {
            tracing::warn!(sticker = %entity.id, "[import] a sticker came without its picture; left out");
            continue;
        };
        txn.execute_raw(statement(
            "INSERT INTO merope_stickers (id, picture, meaning, venue, asset_id, identity, sent, \
             last_sent_at, created_at) VALUES ($1, $2, $3, NULL, $4, $5, $6, $7, $8)",
            vec![
                entity.id.clone().into(),
                sticker.picture.into(),
                sticker.meaning.into(),
                url.into(),
                sticker.identity.into(),
                (sticker.sent.unwrap_or(0).min(i32::MAX as u32) as i32).into(),
                sticker.last_sent_at.map(at).into(),
                at(sticker.created_at).into(),
            ],
        ))
        .await?;
        count(Kind::Sticker);
    }

    // Her affect toward the owner: the anchor, each change, and where it
    // stands now (right after the last change, as the site keeps it).
    let mut standing: Option<(myriad_persona_sync::entity::Affect, i64)> = None;
    if let Some(entity) = of(Kind::AffectAnchor).first() {
        let anchor: AffectAnchorBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        txn.execute_raw(statement(
            "INSERT INTO agent_affect_anchor (user_id, mood, arousal, emotion, emotion_arousal, \
             at, through_event, rev) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (user_id) DO UPDATE SET mood = EXCLUDED.mood, arousal = EXCLUDED.arousal, \
             emotion = EXCLUDED.emotion, emotion_arousal = EXCLUDED.emotion_arousal, at = EXCLUDED.at, \
             through_event = EXCLUDED.through_event, rev = EXCLUDED.rev",
            vec![
                owner.into(),
                anchor.affect.mood.into(),
                anchor.affect.arousal.into(),
                anchor.affect.emotion.into(),
                anchor.affect.emotion_arousal.into(),
                at(anchor.at).into(),
                anchor.through_event.into(),
                entity.rev.to_string().into(),
            ],
        ))
        .await?;
        standing = Some((anchor.affect, anchor.at));
        count(Kind::AffectAnchor);
    }
    let mut changes: Vec<(&EntityVersion, AffectEventBody)> = Vec::new();
    for entity in of(Kind::AffectEvent) {
        changes.push((
            entity,
            entity
                .body_as()
                .map_err(|error| ImportError::Mismatch(error.to_string()))?,
        ));
    }
    changes.sort_by(|a, b| (a.1.at, &a.0.id).cmp(&(b.1.at, &b.0.id)));
    for (entity, change) in &changes {
        txn.execute_raw(statement(
            "INSERT INTO agent_affect_events (id, user_id, at, cause, impulse, baseline_mood, \
             baseline_arousal, before, after, late, origin, rev, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13::timestamptz)",
            vec![
                entity.id.clone().into(),
                owner.into(),
                at(change.at).into(),
                change.cause.clone().into(),
                serde_json::to_value(&change.impulse)
                    .map_err(ImportError::failed)?
                    .into(),
                change.baseline.mood.into(),
                change.baseline.arousal.into(),
                serde_json::to_value(change.before)
                    .map_err(ImportError::failed)?
                    .into(),
                serde_json::to_value(change.after)
                    .map_err(ImportError::failed)?
                    .into(),
                change.late.into(),
                entity.origin.clone().into(),
                entity.rev.to_string().into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
        if standing.is_none_or(|(_, since)| change.at >= since) {
            standing = Some((change.after, change.at));
        }
        count(Kind::AffectEvent);
    }
    let owner_state: Option<OwnerStateBody> = of(Kind::OwnerState)
        .first()
        .and_then(|entity| entity.body_as().ok());
    if standing.is_some() || owner_state.is_some() {
        let state = owner_state.clone().unwrap_or_default();
        let (affect, settled) = match standing {
            Some((affect, since)) => (Some(affect), at(since)),
            None => (None, crate::clock::now().fixed_offset()),
        };
        let activity: String = state
            .activity
            .clone()
            .map(|activity| activity.chars().take(16).collect())
            .unwrap_or_else(|| "idle".to_string());
        txn.execute_raw(statement(
            "INSERT INTO agent_addressee_state (user_id, mood, arousal, emotion, emotion_arousal, \
             activity, activity_updated_at, do_not_disturb, dnd_start_minute, dnd_end_minute, \
             last_user_message_at, last_proactive_at, music_mood_credited_at, mood_settled_at, \
             emotion_settled_at, updated_at) VALUES \
             ($1, COALESCE($2, 70), COALESCE($3, 50), COALESCE($4, 50), COALESCE($5, 50), $6, \
              $7, $8, $9, $10, $11, $12, $13, $14, $14, $15::timestamptz) \
             ON CONFLICT (user_id) DO UPDATE SET mood = EXCLUDED.mood, arousal = EXCLUDED.arousal, \
             emotion = EXCLUDED.emotion, emotion_arousal = EXCLUDED.emotion_arousal, \
             activity = EXCLUDED.activity, activity_updated_at = EXCLUDED.activity_updated_at, \
             do_not_disturb = EXCLUDED.do_not_disturb, dnd_start_minute = EXCLUDED.dnd_start_minute, \
             dnd_end_minute = EXCLUDED.dnd_end_minute, \
             last_user_message_at = EXCLUDED.last_user_message_at, \
             last_proactive_at = EXCLUDED.last_proactive_at, \
             music_mood_credited_at = EXCLUDED.music_mood_credited_at, \
             mood_settled_at = EXCLUDED.mood_settled_at, \
             emotion_settled_at = EXCLUDED.emotion_settled_at, updated_at = $15::timestamptz",
            vec![
                owner.into(),
                affect.map(|affect| affect.mood).into(),
                affect.map(|affect| affect.arousal).into(),
                affect.map(|affect| affect.emotion).into(),
                affect.map(|affect| affect.emotion_arousal).into(),
                activity.into(),
                state
                    .activity_updated_at
                    .map(at)
                    .unwrap_or_else(|| crate::clock::now().fixed_offset())
                    .into(),
                state.do_not_disturb.into(),
                state.dnd_start_minute.into(),
                state.dnd_end_minute.into(),
                state.last_user_message_at.map(at).into(),
                state.last_proactive_at.map(at).into(),
                state.music_mood_credited_at.map(at).into(),
                settled.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
        if owner_state.is_some() {
            count(Kind::OwnerState);
        }
    }

    // Her own records the site keeps in its registry.
    let keep = |days: i64| (crate::clock::now() + chrono::Duration::days(days)).timestamp();
    let mut registry: Vec<(&str, String, Value, i64)> = Vec::new();
    for entity in of(Kind::VitalsDay) {
        let day = entity.id.split('@').next().unwrap_or_default().to_string();
        registry.push((
            her::VITALS_NAMESPACE,
            format!("day:{day}"),
            entity.body.clone(),
            keep(KEEP_DAYS),
        ));
        count(Kind::VitalsDay);
    }
    let following: Vec<Value> = of(Kind::SerialFollowing)
        .iter()
        .map(|entity| entity.body.clone())
        .collect();
    if !following.is_empty() {
        for _ in &following {
            count(Kind::SerialFollowing);
        }
        registry.push((
            her::SERIAL_NAMESPACE,
            "reading".into(),
            Value::Array(following),
            keep(KEEP_DAYS),
        ));
    }
    for (kind, record) in [(Kind::SerialPast, "past"), (Kind::SerialShelf, "shelf")] {
        if let Some(entity) = of(kind).first() {
            registry.push((
                her::SERIAL_NAMESPACE,
                record.into(),
                entity.body.clone(),
                keep(KEEP_DAYS),
            ));
            count(kind);
        }
    }
    for entity in of(Kind::SerialWork) {
        registry.push((
            her::SERIAL_NAMESPACE,
            format!("work:{}", entity.id),
            entity.body.clone(),
            keep(KEEP_DAYS),
        ));
        count(Kind::SerialWork);
    }
    if let Some(entity) = of(Kind::SoupGame).first() {
        registry.push((
            her::SOUP_NAMESPACE,
            format!("p:{owner}"),
            entity.body.clone(),
            keep(KEEP_DAYS),
        ));
        count(Kind::SoupGame);
    }
    if let Some(entity) = of(Kind::Pace).first() {
        let pace: PaceBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        if let Some(usual) = &pace.pace {
            registry.push((
                her::PACE_NAMESPACE,
                "pace".into(),
                serde_json::to_value(usual).map_err(ImportError::failed)?,
                keep(KEEP_DAYS),
            ));
        }
        for (day, minutes) in &pace.lazed {
            registry.push((
                her::PACE_NAMESPACE,
                format!("lazed:{day}"),
                json!(minutes),
                keep(KEEP_DAYS),
            ));
        }
        count(Kind::Pace);
    }
    if let Some(entity) = of(Kind::Present).first() {
        let present: PresentBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        let kept = json!({
            "doing": present.doing.map(|doing| json!({
                "thing": doing.thing,
                "started": utc(doing.started),
                "ends": utc(doing.ends),
                "why": doing.why,
                "had_before": doing.had_before,
            })),
            "lazing": present.lazing.map(|lazing| json!({
                "kind": lazing.kind,
                "started": utc(lazing.started),
                "ends": utc(lazing.ends),
            })),
        });
        registry.push((
            her::PRESENT_NAMESPACE,
            "now".into(),
            kept,
            keep(KEEP_PRESENT_DAYS),
        ));
        count(Kind::Present);
    }
    for (namespace, record, payload, expires) in registry {
        txn.execute_raw(statement(
            "INSERT INTO runtime_registry (namespace, record_id, payload, expires_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5::timestamptz) ON CONFLICT (namespace, record_id) \
             DO UPDATE SET payload = EXCLUDED.payload, expires_at = EXCLUDED.expires_at, \
             updated_at = $5::timestamptz",
            vec![
                namespace.into(),
                record.into(),
                payload.into(),
                expires.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
    }

    // Her private conversations with the owner, every line, with the
    // pictures in them at their new addresses.
    for entity in of(Kind::Conversation) {
        let conversation: ConversationBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        txn.execute_raw(statement(
            "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
             created_at, last_active_at) VALUES ($1, $2, $3, $4, $5, false, $6, $7)",
            vec![
                entity.id.clone().into(),
                owner.into(),
                conversation
                    .title
                    .clone()
                    .map(|title| title.chars().take(255).collect::<String>())
                    .into(),
                json!({ "mode": "chat" }).into(),
                (conversation.lines.len() as i32).into(),
                at(conversation.created_at).into(),
                at(conversation.last_active_at).into(),
            ],
        ))
        .await?;
        for line in &conversation.lines {
            let mut content = line.content.clone();
            for picture in &line.pictures {
                if let Some(url) = picture_url(Some(&picture.image)) {
                    content = content.replace(&picture.url, &url);
                }
            }
            let mut metadata = serde_json::Map::new();
            if let Some(cut) = &line.cut_off {
                metadata.insert("cutOff".into(), json!(cut));
            }
            if let Some(heard) = line.heard_chars {
                metadata.insert("heardChars".into(), json!(heard));
            }
            txn.execute_raw(statement(
                "INSERT INTO agent_messages (session_id, role, content, metadata, created_at) \
                 VALUES ($1, $2, $3, $4, $5)",
                vec![
                    entity.id.clone().into(),
                    line.role.clone().into(),
                    content.into(),
                    (!metadata.is_empty())
                        .then_some(Value::Object(metadata))
                        .into(),
                    at(line.at).into(),
                ],
            ))
            .await?;
        }
        count(Kind::Conversation);
    }
    for entity in of(Kind::ProactiveLine) {
        let line: ProactiveLineBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        let mut metadata = serde_json::Map::new();
        if let Some(cut) = &line.cut_off {
            metadata.insert("cutOff".into(), json!(cut));
        }
        if let Some(heard) = line.heard_chars {
            metadata.insert("heardChars".into(), json!(heard));
        }
        txn.execute_raw(statement(
            "INSERT INTO agent_proactive_messages (user_id, role, content, event_key, notified, \
             created_at, say_id, metadata) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            vec![
                owner.into(),
                line.role.into(),
                line.content.into(),
                line.event_key.into(),
                line.notified.into(),
                at(line.at).into(),
                line.say_id.into(),
                (!metadata.is_empty())
                    .then_some(Value::Object(metadata))
                    .into(),
            ],
        ))
        .await?;
        count(Kind::ProactiveLine);
    }
    for entity in of(Kind::Intention) {
        let intention: IntentionBody = entity
            .body_as()
            .map_err(|error| ImportError::Mismatch(error.to_string()))?;
        // A proposal for a Mac stays as it was; accepting it where that
        // Mac is not connected says so (H3c-19).
        txn.execute_raw(statement(
            "INSERT INTO agent_intentions (id, user_id, source_event_id, summary, reason_code, \
             status, proposal, result_summary, expires_at, accept_source, created_at, \
             updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            vec![
                entity.id.clone().into(),
                owner.into(),
                intention
                    .source_event_id
                    .unwrap_or_else(|| format!("import:{}", entity.id))
                    .into(),
                intention.summary.into(),
                intention.reason_code.into(),
                intention.status.into(),
                intention.proposal.unwrap_or_else(|| json!({})).into(),
                intention.result_summary.into(),
                intention.expires_at.map(at).into(),
                intention
                    .accept_source
                    .filter(|source| source.len() <= 16)
                    .unwrap_or_else(|| "user".to_string())
                    .into(),
                at(intention.created_at).into(),
                at(intention.updated_at).into(),
            ],
        ))
        .await?;
        count(Kind::Intention);
    }

    // Her rig: the one of the set she wears.
    let rig = active.as_ref().and_then(|active| rigs.get(active)).cloned();
    if rig.is_some() {
        crate::services::merope_rig::persist_active_asset(&txn, rig.as_deref())
            .await
            .map_err(ImportError::failed)?;
    }
    crate::services::media::bind_persona(
        &txn,
        default_portrait.as_deref(),
        avatar.as_deref(),
        visual_profile.as_ref(),
        &crate::services::media::configured_origins().await,
    )
    .await
    .map_err(ImportError::failed)?;
    let written = lease::landed(&txn, &lineage, whole.epoch.unwrap_or(0)).await?;
    if let Some(import) = import {
        txn.execute_raw(statement(
            "UPDATE agent_host_import SET phase = $2, updated_at = $3::timestamptz WHERE id = $1",
            vec![
                import.into(),
                phase::COMMITTED.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
        txn.execute_raw(statement(
            "DELETE FROM agent_host_import_staging WHERE import_id = $1",
            vec![import.into()],
        ))
        .await?;
    }
    txn.commit().await?;
    if rig.is_some() {
        crate::services::merope_rig::mirror_active_asset(rig).await;
    }
    crate::services::agent::merope::clock::note_zone(Some(&zone));
    lease::reload(db).await;
    tracing::info!(
        epoch = written.epoch,
        kinds = counts.len(),
        vectors,
        from = %whole.origin,
        "[import] she landed on the site"
    );
    Ok(Landed {
        counts,
        vectors,
        her_on: her_on().await,
    })
}

/// Her pictures into the media library (each once, by its sha256) and her
/// rigs as packages: the address each picture is at now, by asset id.
pub(super) async fn keep_files(
    db: &DatabaseConnection,
    owner: i32,
    files: &BTreeMap<String, BTreeMap<String, Vec<u8>>>,
) -> Result<BTreeMap<String, String>, ImportError> {
    let pictures = assets::Pictures::new(db, owner);
    let actor = MediaActor::admin(owner).map_err(ImportError::failed)?;
    let mut urls: BTreeMap<String, String> = BTreeMap::new();
    for (asset, files) in files {
        if let Some(bytes) = files.get("") {
            let media_type = sniff(bytes).ok_or_else(|| {
                ImportError::Mismatch(format!("the picture {asset} is not an image"))
            })?;
            let ctx = MediaContext::site(actor.clone(), MediaSource::Import)
                .with_producer_key(format!("persona-import:{asset}"));
            let (kept, _) = pictures
                .media
                .persist_ready_bytes(
                    db,
                    ctx,
                    NewMediaBytes {
                        bytes: bytes.clone().into(),
                        claimed_mime: media_type.to_string(),
                        filename: format!(
                            "{}.{}",
                            &asset[..16],
                            media_type.trim_start_matches("image/")
                        ),
                        max_bytes: super::ASSET_MAX_BYTES as usize,
                        derived_from_id: None,
                        exposure: MediaExposure::Public,
                    },
                )
                .await
                .map_err(ImportError::failed)?;
            urls.insert(asset.clone(), kept.url);
        } else if let (Some(manifest), Some(atlas)) =
            (files.get(RIG_MANIFEST_FILE), files.get(RIG_ATLAS_FILE))
        {
            let mut manifest: myriad_merope::RigManifest =
                serde_json::from_slice(manifest).map_err(ImportError::failed)?;
            let url = crate::services::merope_rig::public_atlas_url(asset);
            for texture in &mut manifest.textures {
                texture.url = url.clone();
            }
            let json = serde_json::to_string(&manifest).map_err(ImportError::failed)?;
            crate::services::merope_rig::persist_package(asset, atlas, &json)
                .await
                .map_err(ImportError::failed)?;
        }
    }
    Ok(urls)
}

/// The site's model, when the vectors that came were made with it (H3c-13):
/// the same name, and the same length as the site's own vectors, or any
/// length while the site has none yet.
pub(super) async fn vectors_fit<C: ConnectionTrait>(
    db: &C,
    theirs: Option<&EmbeddingModel>,
) -> Result<Option<EmbeddingModel>, DbErr> {
    let Some(theirs) = theirs else {
        return Ok(None);
    };
    let configured = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .aux_embedding_model
        .trim()
        .to_string();
    if configured.is_empty() || configured != theirs.model.trim() || theirs.dims == 0 {
        return Ok(None);
    }
    Ok(match super::embedding_model(db).await? {
        Some(ours) => (ours.dims == theirs.dims).then_some(ours),
        None => Some(EmbeddingModel {
            model: configured,
            dims: theirs.dims,
        }),
    })
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
