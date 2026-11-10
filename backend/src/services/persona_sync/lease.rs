//! Where she lives: the one end she lives on, read from three records.
//!
//! She lives in one place, her host; every other end is a child. Which end
//! holds her is her lease (`agent_lease_grants`, `her_log::lease`), and only
//! that: `holder` and `epoch` are the grant that stands, and the epoch only
//! goes up. The rest is the site's copy of her (`agent_her_copy`: her
//! `lineage`, her birth, and whether the site's copy is the live one, `live`;
//! let go to the device she lives on, `away`; lost, `vacant`; or exported to
//! be brought in elsewhere, `moved_out`, Q7) and her moves
//! (`agent_her_moves`), each a small state machine: proposed → frozen →
//! transferring → applied → committed → retired. While a move is frozen no
//! end holds her (a freeze grant, `frozen`); before the commit it can be
//! cancelled, which thaws her where she was and gives everything back as it
//! was: freezing her only puts things aside, it destroys nothing. The
//! commit hands her lease to the move's target. The last move that finished
//! is kept (`done`), so a commit asked again is answered. The plumbing is
//! [`rows`].
//!
//! Moves (H3e): to a device (the device agrees, the owner confirms with a
//! passphrase for the copy the site keeps; [`super::moves`]), back from the
//! device she lives on, and a reclaim from a daily backup by another device
//! (`Recover`). Removing the device she lives on leaves the copy `vacant`.
//!
//! Anything that writes to the site's her needs the site owner's logged-in
//! session once: a device token can propose a move, never confirm one.
//!
//! None of it carries a key or a token, and neither does anything made from
//! it. Every change is announced by the database on [`CHANNEL`]; each
//! process keeps the lease it last read ([`current`]) and reads it again on
//! an announcement, and every [`REREAD_EVERY`] in case one was missed. Runs,
//! her own life and her link check that copy.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex, OnceLock, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use myriad_error::AppError;
use myriad_persona_log::lease::Request;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod old_lease;
mod rows;

pub use old_lease::take_old_lease;

/// Where changes to where she lives are announced (`persona_sync.sql`,
/// `persona_log.sql`).
pub const CHANNEL: &str = "myriad_her_host";
/// The holder when she lives on the site.
pub const SITE: &str = "site";
/// How often a process reads the lease again without being told to.
pub const REREAD_EVERY: Duration = Duration::from_secs(30);

/// The site's copy of her.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// The live one: she lives on the site, whichever host core her mind is
    /// on.
    Live,
    /// She lives on the device her lease names: the site let its copy go
    /// when she moved there (H3e), or has none yet while she comes in (H3c).
    Away,
    /// She is not on this site and the site does not know where she is (the
    /// device she lived on was removed, or the site let her go after she
    /// moved out).
    Vacant,
    /// Exported to be brought in elsewhere (「导出并搬走」, Q7): she is still
    /// here but stopped, until the owner lets this copy go or undoes it.
    MovedOut,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Live => "live",
            State::Away => "away",
            State::Vacant => "vacant",
            State::MovedOut => "moved_out",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "live" => Some(State::Live),
            "away" => Some(State::Away),
            "vacant" => Some(State::Vacant),
            "moved_out" => Some(State::MovedOut),
            _ => None,
        }
    }
}

/// Where a move stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Asked for, waiting for the owner to confirm on the site. She is not
    /// touched yet.
    Proposed,
    /// She is frozen where she was; the manifest of what moves is made.
    Frozen,
    /// What moves is being read or written, page by page.
    Transferring,
    /// The target has all of it, checked, waiting to be committed.
    Applied,
    /// Her lease went to the target: she lives there.
    Committed,
    /// The old host let its copy go.
    Retired,
}

impl Phase {
    /// The step, from 1, as both ends show it ("停在第 N 步").
    pub fn step(self) -> u8 {
        match self {
            Phase::Proposed => 1,
            Phase::Frozen => 2,
            Phase::Transferring => 3,
            Phase::Applied => 4,
            Phase::Committed => 5,
            Phase::Retired => 6,
        }
    }

    /// The step after this one, the only one a move may take next.
    pub fn next(self) -> Option<Phase> {
        match self {
            Phase::Proposed => Some(Phase::Frozen),
            Phase::Frozen => Some(Phase::Transferring),
            Phase::Transferring => Some(Phase::Applied),
            Phase::Applied => Some(Phase::Committed),
            Phase::Committed => Some(Phase::Retired),
            Phase::Retired => None,
        }
    }

    /// Before the commit: the old host still holds her, so cancelling gives
    /// everything back.
    pub fn cancellable(self) -> bool {
        self.step() < Phase::Committed.step()
    }

    /// She is frozen at this step: no end has her.
    pub fn freezes(self) -> bool {
        matches!(self, Phase::Frozen | Phase::Transferring | Phase::Applied)
    }
}

/// What a move does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// She goes to live on another end (`to`).
    Move,
    /// The her on a device (`from`) is merged into the site's, the first
    /// time it connects with a her of its own.
    Merge,
    /// She is taken back onto the site from the daily backup the device she
    /// lives on (`from`) pushed, by another device (`via`) that opened it.
    Recover,
}

impl Kind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "move" => Some(Kind::Move),
            "merge" => Some(Kind::Merge),
            "recover" => Some(Kind::Recover),
            _ => None,
        }
    }
}

/// Who asked for a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Starter {
    /// The owner, logged in on the site: that is the confirmation too.
    Owner,
    /// A device with its token: the owner still confirms on the site.
    Device,
}

/// The move in progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    pub id: String,
    pub kind: Kind,
    /// `site` or a device id, and the same for `to`.
    pub from: String,
    pub to: String,
    pub phase: Phase,
    pub by: Starter,
    /// The digest of the manifest of what moves, once it is made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<DateTime<Utc>>,
    /// A move to a device: the device agreed (it said so with its token, the
    /// owner there having said yes). The owner's confirmation needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agreed: Option<bool>,
    /// The import that carries her here (a move back from a device, or a
    /// reclaim), once there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<String>,
    /// A reclaim: the device that opened the backup and sends her.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// Where she stood before the move began, given back if it is called off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<State>,
}

impl Move {
    /// The device a move is with, whichever way it goes: for a reclaim, the
    /// device sending her.
    pub fn device(&self) -> &str {
        if let Some(via) = &self.via {
            via
        } else if self.from == SITE {
            &self.to
        } else {
            &self.from
        }
    }

    /// She goes from the site to the device `to`.
    pub fn to_device(&self) -> bool {
        self.kind == Kind::Move && self.from == SITE
    }

    /// She comes back to the site from the device she lives on.
    pub fn back_to_site(&self) -> bool {
        self.kind == Kind::Move && self.from != SITE && self.to == SITE
    }
}

/// The last move that finished: its id, what it was and the epoch it left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub id: String,
    pub kind: Kind,
    pub epoch: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lease {
    /// The end the grant that stands names (`site` or a device id); while
    /// she is frozen, the end she was frozen from.
    pub holder: String,
    pub lineage: String,
    /// The grant that stands.
    pub epoch: i64,
    /// The epoch at which `holder` got her: freezes and thaws since then
    /// leave her with it.
    pub since: i64,
    /// The site's copy of her.
    pub state: State,
    /// A freeze grant stands: she is moving, no end holds her.
    pub frozen: bool,
    pub moving: Option<Move>,
    pub updated_at: DateTime<Utc>,
    pub done: Option<Done>,
}

/// Where she is, as this site sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// On the site (or not born yet: nothing to keep from anyone).
    Here,
    /// Moving: on no end.
    Moving,
    /// On another end, the device `holder`.
    Elsewhere(String),
    /// Not here, and the site does not know where.
    Vacant,
    /// Exported to be brought in elsewhere; stopped here until the owner
    /// lets this copy go or undoes it.
    MovedOut,
    /// She lives here, but her one mind is on another host core now: the
    /// one that holds her mind lease (`her_log::pen`), `holder`, or the
    /// site itself while it catches up on what that core wrote before it
    /// gave her back. Nothing thinks as her here; what arrives is kept for
    /// her or lent to her mind there.
    MindOn(String),
}

impl Where {
    pub fn here(&self) -> bool {
        *self == Where::Here
    }
}

impl Lease {
    pub fn place(&self) -> Where {
        if self.frozen {
            return Where::Moving;
        }
        match self.state {
            State::Vacant => Where::Vacant,
            State::MovedOut => Where::MovedOut,
            State::Away => Where::Elsewhere(self.holder.clone()),
            State::Live if self.holder == SITE => Where::Here,
            State::Live => Where::MindOn(self.holder.clone()),
        }
    }

    /// Whether she lives on `device`, the site having let its copy go (or
    /// having none yet): that device is her host, moving or not.
    pub fn lives_on(&self, device: &str) -> bool {
        self.state == State::Away && self.holder == device
    }

    /// Her state as the owner's page and a device read it, the same words
    /// as before her lease was split off: `frozen` while she moves, `live`
    /// wherever she lives.
    fn shown_state(&self) -> &'static str {
        match self.state {
            _ if self.frozen => "frozen",
            State::Away => State::Live.as_str(),
            state => state.as_str(),
        }
    }

    /// Where she lives as the owner's page and a device read it: the site
    /// while its copy of her is live, wherever her mind is (that is
    /// `/agent/mind/where`'s), so a device her mind is on is not taken for
    /// one she lives on; otherwise the end her lease names.
    fn shown_holder(&self) -> &str {
        match self.state {
            State::Live => SITE,
            _ => &self.holder,
        }
    }

    /// As the site owner's page and a device read it: where she lives, her
    /// lineage and epoch, and the move in progress. `name` gives a device id
    /// the name the owner gave it. Nothing secret is in it.
    pub fn json(&self, name: impl Fn(&str) -> Option<String>) -> Value {
        let named = |id: &str| (id != SITE).then(|| name(id)).flatten();
        let moving = self.moving.as_ref().map(|moving| {
            json!({
                "id": moving.id,
                "kind": moving.kind,
                "from": moving.from,
                "fromName": named(&moving.from),
                "to": moving.to,
                "toName": named(&moving.to),
                "phase": moving.phase,
                "step": moving.phase.step(),
                "by": moving.by,
                "startedAt": moving.started_at.to_rfc3339(),
                "confirmedAt": moving.confirmed_at.map(|at| at.to_rfc3339()),
                "agreed": moving.agreed,
                "via": moving.via,
                "viaName": moving.via.as_deref().and_then(named),
                "import": moving.import,
                "digest": moving.digest,
            })
        });
        json!({
            "holder": self.shown_holder(),
            "holderName": named(self.shown_holder()),
            "lineage": self.lineage,
            "epoch": self.epoch,
            "state": self.shown_state(),
            "move": moving,
            "done": self.done,
            "updatedAt": self.updated_at.to_rfc3339(),
        })
    }
}

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

/// Where she lives, as stored. A site that has her and no copy of her yet
/// (she was just born) makes one, live, with a new lineage, and her lease
/// comes to the site if another end held it. None while the site has no
/// her.
pub async fn read<C: ConnectionTrait>(db: &C) -> Result<Option<Lease>, DbErr> {
    if let Some(copy) = rows::copy(db, false).await? {
        return rows::assemble(db, copy).await.map(Some);
    }
    let Some(copy) = rows::born(db).await? else {
        // Born by someone else meanwhile, or no her.
        return match rows::copy(db, false).await? {
            Some(copy) => rows::assemble(db, copy).await.map(Some),
            None => Ok(None),
        };
    };
    // A new birth is the site's: an end that held the her before it (one
    // the site lost) gives way.
    if rows::standing(db)
        .await?
        .is_some_and(|grant| !grant.held_by(SITE) && grant.holder.is_some())
    {
        rows::to_site(db, None).await?;
    }
    tracing::info!(lineage = %copy.lineage, "[host] she was born here: the site holds her");
    rows::assemble(db, copy).await.map(Some)
}

/// The state of the site's copy of her; None while there is none.
pub async fn copy_state<C: ConnectionTrait>(db: &C) -> Result<Option<State>, DbErr> {
    Ok(rows::copy(db, false).await?.map(|copy| copy.state))
}

/// Her persona on the site was deleted while the site held her: the next one
/// is another birth, with another lineage. A copy let go to the device she
/// lives on is kept (she lives there).
pub async fn forget_birth<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    let Some(lease) = read(db).await? else {
        return Ok(());
    };
    if lease.holder != SITE || lease.state == State::Away {
        return Ok(());
    }
    if lease.frozen {
        rows::hold_still(db, false, None).await?;
    }
    rows::forget_copy(db).await
}

/// Why a move could not be started or taken a step.
#[derive(Debug)]
pub enum MoveError {
    /// The site has no her to move.
    NoHer,
    /// Another move is under way.
    InProgress,
    /// She does not live on the site now, so the site cannot start a move.
    NotHere,
    /// No such move now.
    Unknown,
    /// The move is not at a step this can be done at.
    Phase(Phase),
    /// A move to a device that the device has not agreed to yet.
    NotAgreed,
    /// What arrived is not what was sealed (a different digest).
    Mismatch,
    /// Her moving to a device opens with H4 (Q8): only the acceptance entry.
    Later,
    /// The device has not pushed its first daily backup: the site would have
    /// no copy to take her back from.
    BackupMissing,
    /// Something needed could not be written (a file); the message names
    /// what, never a value.
    Failed(String),
    Db(DbErr),
}

impl From<DbErr> for MoveError {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

impl MoveError {
    /// The code and status a request is refused with.
    pub fn refusal(&self) -> (axum::http::StatusCode, &'static str, &'static str) {
        use axum::http::StatusCode;
        match self {
            MoveError::NoHer => (
                StatusCode::CONFLICT,
                "move_no_her",
                "The site has no her to move",
            ),
            MoveError::InProgress => (
                StatusCode::CONFLICT,
                "move_in_progress",
                "Another move is under way",
            ),
            MoveError::NotHere => (
                StatusCode::CONFLICT,
                "host_moved",
                "She does not live on this site now",
            ),
            MoveError::Unknown => (StatusCode::NOT_FOUND, "move_unknown", "No such move now"),
            MoveError::Phase(_) => (
                StatusCode::CONFLICT,
                "move_phase",
                "The move is not at a step this can be done at",
            ),
            MoveError::NotAgreed => (
                StatusCode::CONFLICT,
                "move_not_agreed",
                "The device has not agreed to her living on it yet",
            ),
            MoveError::Mismatch => (
                StatusCode::CONFLICT,
                "move_mismatch",
                "What arrived is not what was sealed",
            ),
            MoveError::BackupMissing => (
                StatusCode::CONFLICT,
                "move_backup_missing",
                "The device has not kept its first backup of her here yet",
            ),
            MoveError::Later => (
                StatusCode::CONFLICT,
                "device_host_later",
                "Her living on a device opens later",
            ),
            MoveError::Failed(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "move_failed",
                "The move could not go on",
            ),
            MoveError::Db(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Could not change her host lease",
            ),
        }
    }
}

async fn locked(txn: &sea_orm::DatabaseTransaction) -> Result<Option<Lease>, DbErr> {
    // Her lease first (every grant takes it), then her copy, made first if
    // she has none yet.
    rows::lock(txn).await?;
    read(txn).await?;
    match rows::copy(txn, true).await? {
        Some(copy) => rows::assemble(txn, copy).await.map(Some),
        None => Ok(None),
    }
}

/// Writes what a step of a move leaves: the site's copy as `state`, the move
/// as it stands (none: called off or over) and whether she is frozen (no
/// end holds her). A freeze is signed when she stops, a thaw when she goes
/// on as she was.
async fn write(
    txn: &sea_orm::DatabaseTransaction,
    state: State,
    frozen: bool,
    moving: Option<&Move>,
) -> Result<Lease, DbErr> {
    rows::set_copy(txn, state).await?;
    match moving {
        Some(moving) => rows::put_move(txn, moving).await?,
        None => rows::drop_move(txn).await?,
    }
    rows::hold_still(txn, frozen, moving.map(|moving| moving.id.as_str())).await?;
    let copy = rows::copy(txn, false)
        .await?
        .ok_or(DbErr::RecordNotUpdated)?;
    rows::assemble(txn, copy).await
}

/// Starts a move of the site's her with the device `device`: `Move` takes her
/// to it, `Merge` brings its her into the site's. A merge the owner asks for
/// on the site freezes her at once (that is the confirmation too); one a
/// device asks for waits for the owner. A move to a device waits either way
/// for both: the device agreeing (asked by it, or later with [`agree`]) and
/// the owner confirming on the site with the passphrase for the copy the
/// site keeps (H3e-2): only then is she frozen.
pub async fn start(
    db: &DatabaseConnection,
    kind: Kind,
    device: &str,
    by: Starter,
) -> Result<Lease, MoveError> {
    if !matches!(kind, Kind::Move | Kind::Merge) {
        return Err(MoveError::Phase(Phase::Proposed));
    }
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::NoHer)?;
    if lease.moving.is_some() {
        return Err(MoveError::InProgress);
    }
    if lease.place() != Where::Here {
        return Err(MoveError::NotHere);
    }
    let (from, to) = match kind {
        Kind::Merge => (device.to_string(), SITE.to_string()),
        _ => (SITE.to_string(), device.to_string()),
    };
    let now = crate::clock::now();
    let confirmed = by == Starter::Owner && kind == Kind::Merge;
    let moving = Move {
        id: format!("mv_{}", uuid::Uuid::new_v4().simple()),
        kind,
        from,
        to,
        phase: if confirmed {
            Phase::Frozen
        } else {
            Phase::Proposed
        },
        by,
        digest: None,
        started_at: now,
        confirmed_at: confirmed.then_some(now),
        agreed: (kind == Kind::Move).then_some(by == Starter::Device),
        import: None,
        via: None,
        was: Some(lease.state),
    };
    let lease = write(&txn, lease.state, moving.phase.freezes(), Some(&moving)).await?;
    txn.commit().await?;
    tracing::info!(
        move_id = %moving.id,
        kind = ?kind,
        device,
        by = ?by,
        phase = ?moving.phase,
        epoch = lease.epoch,
        "[host] a move of her was started"
    );
    Ok(lease)
}

/// The device she lives on, or the owner on the site, asks to bring her
/// back to the site (H3e-8). Asked by the owner it is confirmed at once and
/// she is frozen there (no end has her); asked by the device it waits for
/// the owner. The device then reports what it froze ([`super::moves`]) and
/// sends her as an import.
pub async fn start_back(
    db: &DatabaseConnection,
    device: &str,
    by: Starter,
) -> Result<Lease, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::NoHer)?;
    if lease.moving.is_some() {
        return Err(MoveError::InProgress);
    }
    if lease.place() != Where::Elsewhere(device.to_string()) {
        return Err(MoveError::NotHere);
    }
    let now = crate::clock::now();
    let confirmed = by == Starter::Owner;
    let moving = Move {
        id: format!("mv_{}", uuid::Uuid::new_v4().simple()),
        kind: Kind::Move,
        from: device.to_string(),
        to: SITE.to_string(),
        phase: if confirmed {
            Phase::Frozen
        } else {
            Phase::Proposed
        },
        by,
        digest: None,
        started_at: now,
        confirmed_at: confirmed.then_some(now),
        agreed: None,
        import: None,
        via: None,
        was: Some(lease.state),
    };
    let lease = write(&txn, lease.state, confirmed, Some(&moving)).await?;
    txn.commit().await?;
    tracing::info!(move_id = %moving.id, device, by = ?by, "[host] she is asked back to the site");
    Ok(lease)
}

/// The device a move to it is with agreed (the owner said yes there).
pub async fn agree(db: &DatabaseConnection, id: &str, device: &str) -> Result<Lease, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::Unknown)?;
    let mut moving = lease
        .moving
        .filter(|moving| moving.id == id && moving.to_device() && moving.to == device)
        .ok_or(MoveError::Unknown)?;
    if moving.phase != Phase::Proposed {
        return Err(MoveError::Phase(moving.phase));
    }
    moving.agreed = Some(true);
    let lease = write(&txn, lease.state, lease.frozen, Some(&moving)).await?;
    txn.commit().await?;
    tracing::info!(move_id = %id, device, "[host] the device agreed to her living on it");
    Ok(lease)
}

/// The owner confirmed on the site the move `id` a device asked for: she is
/// frozen where she is.
pub async fn confirm(db: &DatabaseConnection, id: &str) -> Result<Lease, MoveError> {
    // Her records as they are now go into her log before she is frozen.
    crate::services::her_log::state::sync_now(db).await;
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::Unknown)?;
    let mut moving = lease
        .moving
        .filter(|moving| moving.id == id)
        .ok_or(MoveError::Unknown)?;
    if moving.phase != Phase::Proposed {
        return Err(MoveError::Phase(moving.phase));
    }
    if moving.to_device() && moving.agreed != Some(true) {
        return Err(MoveError::NotAgreed);
    }
    moving.phase = moving.phase.next().ok_or(MoveError::Phase(moving.phase))?;
    moving.confirmed_at = Some(crate::clock::now());
    let lease = write(&txn, lease.state, true, Some(&moving)).await?;
    txn.commit().await?;
    tracing::info!(move_id = %id, epoch = lease.epoch, "[host] the owner confirmed a move: she is frozen");
    Ok(lease)
}

/// The move `id` is called off before its commit: she is where she was, as
/// she was. `device`, when a device asks, must be the one the move is with.
/// A her on her way into a site that had none (an import) is not here yet:
/// calling that off leaves the site with no her and no lease (None).
pub async fn cancel(
    db: &DatabaseConnection,
    id: &str,
    device: Option<&str>,
) -> Result<Option<Lease>, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::Unknown)?;
    let moving = lease
        .moving
        .filter(|moving| moving.id == id)
        .filter(|moving| device.is_none_or(|device| moving.device() == device))
        .ok_or(MoveError::Unknown)?;
    if !moving.phase.cancellable() {
        return Err(MoveError::Phase(moving.phase));
    }
    if super::import::is_import(id) {
        super::import::called_off_on(&txn, id).await?;
        txn.commit().await?;
        super::import::forget_files(id).await;
        super::merge::forget_retired(id).await;
        tracing::info!(move_id = %id, by_device = device.is_some(), "[host] her way into the site was called off: no her here");
        return Ok(None);
    }
    // A move back or a reclaim carries her as an import: it goes too.
    if let Some(import) = &moving.import {
        super::import::drop_session(&txn, import).await?;
    }
    let lease = write(&txn, moving.was.unwrap_or(lease.state), false, None).await?;
    txn.commit().await?;
    if let Some(import) = &moving.import {
        super::import::forget_files(import).await;
    }
    super::moves::forget_files(id).await;
    tracing::info!(
        move_id = %id,
        phase = ?moving.phase,
        by_device = device.is_some(),
        epoch = lease.epoch,
        "[host] a move was cancelled: she is back as she was"
    );
    Ok(Some(lease))
}

/// A her on her way into a site that had none, from the device `device`
/// that holds her at `lineage` and `epoch`: the site writes her copy (none
/// yet: away, on the device) and her lease when the owner confirms, naming
/// that device as where she still is and frozen (no end has her) until she
/// lands here or the import is called off (H3c-1). A site that never issued
/// a grant starts at the device's epoch. Nothing else of the site changes
/// before she lands.
pub(super) async fn arriving(
    txn: &sea_orm::DatabaseTransaction,
    import_id: &str,
    device: &str,
    lineage: &str,
    epoch: i64,
    digest: &str,
) -> Result<Lease, DbErr> {
    rows::lock(txn).await?;
    let now = crate::clock::now();
    let mut moving = Move {
        id: import_id.to_string(),
        kind: Kind::Move,
        from: device.to_string(),
        to: SITE.to_string(),
        phase: Phase::Frozen,
        by: Starter::Device,
        digest: Some(digest.to_string()),
        started_at: now,
        confirmed_at: Some(now),
        agreed: None,
        import: None,
        via: None,
        was: None,
    };
    match rows::copy(txn, true).await? {
        // A site she left (vacant) with her lineage: the copy it kept is
        // hers, and what she left on it here (others' parts, sealed) is
        // hers again.
        Some(copy) if copy.state == State::Vacant && copy.lineage == lineage => {
            moving.was = Some(State::Vacant);
            rows::set_copy(txn, State::Away).await?;
        }
        Some(_) => return Err(DbErr::Custom("the site has a her already".into())),
        None => rows::put_copy(txn, lineage, State::Away).await?,
    }
    rows::begin_with(txn, device, epoch).await?;
    rows::put_move(txn, &moving).await?;
    rows::hold_still(txn, true, Some(import_id)).await?;
    let copy = rows::copy(txn, false)
        .await?
        .ok_or(DbErr::RecordNotInserted)?;
    rows::assemble(txn, copy).await
}

/// The import `import_id` took a step (it is being sent, or all of it is
/// here and checked).
pub(super) async fn import_step(
    txn: &sea_orm::DatabaseTransaction,
    import_id: &str,
    phase: Phase,
) -> Result<(), DbErr> {
    let Some(lease) = locked(txn).await? else {
        return Ok(());
    };
    let Some(mut moving) = lease
        .moving
        .filter(|moving| moving.id == import_id || moving.import.as_deref() == Some(import_id))
    else {
        return Ok(());
    };
    if moving.phase.step() >= phase.step() {
        return Ok(());
    }
    moving.phase = phase;
    // A merge's her lives on while the device sends its own (H3d-1).
    let frozen = moving.kind != Kind::Merge || lease.frozen;
    write(txn, lease.state, frozen, Some(&moving)).await?;
    Ok(())
}

/// A merge (H3d) took a step before she stops: asked by the device
/// (`Proposed`), confirmed by the owner and arriving (`Transferring`). The
/// site's her lives on through both; another move cannot start meanwhile.
pub(super) async fn merge_step(
    txn: &sea_orm::DatabaseTransaction,
    import_id: &str,
    device: &str,
    phase: Phase,
) -> Result<Lease, MoveError> {
    let lease = locked(txn).await?.ok_or(MoveError::NoHer)?;
    if lease.place() != Where::Here {
        return Err(MoveError::NotHere);
    }
    let now = crate::clock::now();
    let moving = match lease.moving {
        Some(moving) if moving.id == import_id => Move {
            phase,
            confirmed_at: (phase != Phase::Proposed).then_some(now),
            ..moving
        },
        Some(_) => return Err(MoveError::InProgress),
        None => Move {
            id: import_id.to_string(),
            kind: Kind::Merge,
            from: device.to_string(),
            to: SITE.to_string(),
            phase,
            by: Starter::Device,
            digest: None,
            started_at: now,
            confirmed_at: (phase != Phase::Proposed).then_some(now),
            agreed: None,
            import: None,
            via: None,
            was: None,
        },
    };
    Ok(write(txn, State::Live, false, Some(&moving)).await?)
}

/// All of a merge is here: she stops (no end has her) for the moment the
/// site takes the two of her together (`thaw` false), or goes on as she was
/// when that failed (`thaw` true).
pub(super) async fn merge_hold(
    db: &DatabaseConnection,
    import_id: &str,
    thaw: bool,
) -> Result<Lease, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::NoHer)?;
    let mut moving = lease
        .moving
        .filter(|moving| moving.id == import_id && moving.kind == Kind::Merge)
        .ok_or(MoveError::Unknown)?;
    moving.phase = if thaw {
        Phase::Transferring
    } else {
        Phase::Applied
    };
    let lease = write(&txn, State::Live, !thaw, Some(&moving)).await?;
    txn.commit().await?;
    tracing::info!(merge = %import_id, thaw, epoch = lease.epoch, "[host] a merge holds her still while she is taken together");
    Ok(lease)
}

/// The merge `import_id` landed, in its transaction: she lives on the site
/// again, her lease thawed one epoch on (H3d-18), her lineage the site's.
pub(super) async fn merged(
    txn: &sea_orm::DatabaseTransaction,
    import_id: &str,
) -> Result<Lease, DbErr> {
    let lease = locked(txn).await?.ok_or(DbErr::RecordNotUpdated)?;
    if lease.holder != SITE
        || lease
            .moving
            .as_ref()
            .is_none_or(|moving| moving.id != import_id)
    {
        return Err(DbErr::RecordNotUpdated);
    }
    rows::to_site(txn, None).await?;
    rows::set_copy(txn, State::Live).await?;
    let epoch = rows::standing(txn)
        .await?
        .map_or(1, |grant| grant.epoch as i64);
    rows::finish(txn, epoch).await?;
    let copy = rows::copy(txn, false)
        .await?
        .ok_or(DbErr::RecordNotUpdated)?;
    rows::assemble(txn, copy).await
}

/// A merge was called off: the site's her is where she was, as she was.
pub(super) async fn merge_called_off<C: ConnectionTrait>(
    db: &C,
    import_id: &str,
) -> Result<(), DbErr> {
    rows::lock(db).await?;
    let Some(lease) = read(db).await? else {
        return Ok(());
    };
    if lease.holder != SITE
        || lease
            .moving
            .as_ref()
            .is_none_or(|moving| moving.id != import_id)
    {
        return Ok(());
    }
    rows::drop_move(db).await?;
    rows::set_copy(db, State::Live).await?;
    rows::hold_still(db, false, None).await
}

/// She landed on the site, in the transaction that wrote her: her lease
/// comes to the site, past the epoch she came with (a file's, a device's:
/// never lower than one an end already heard); from a file, the site's
/// copy is written here (H3c-3). Her lineage is the one she came with: a
/// move is not a new birth.
pub(super) async fn landed(
    txn: &sea_orm::DatabaseTransaction,
    lineage: &str,
    came_at: i64,
) -> Result<Lease, DbErr> {
    rows::lock(txn).await?;
    match rows::copy(txn, true).await? {
        Some(copy) if copy.lineage == lineage => {}
        Some(_) => return Err(DbErr::Custom("the site has another her".into())),
        None => rows::put_copy(txn, lineage, State::Live).await?,
    }
    rows::to_site(txn, Some(came_at.max(0) + 1)).await?;
    rows::set_copy(txn, State::Live).await?;
    let epoch = rows::standing(txn)
        .await?
        .map_or(1, |grant| grant.epoch as i64);
    rows::finish(txn, epoch).await?;
    let copy = rows::copy(txn, false)
        .await?
        .ok_or(DbErr::RecordNotInserted)?;
    rows::assemble(txn, copy).await
}

/// The import `import_id` was called off: what it wrote goes, her lease
/// thaws, and the site is as it was: no her, a site she had left (vacant),
/// or, for a move back or a reclaim that carried her as this import, where
/// she stood before it.
pub(super) async fn forget_arrival<C: ConnectionTrait>(
    db: &C,
    import_id: &str,
) -> Result<(), DbErr> {
    rows::lock(db).await?;
    let Some(lease) = read(db).await? else {
        return Ok(());
    };
    let Some(moving) = lease.moving.as_ref() else {
        return Ok(());
    };
    if moving.id == import_id && moving.was.is_none() && lease.state == State::Away {
        rows::hold_still(db, false, None).await?;
        return rows::forget_copy(db).await;
    }
    let was = if moving.id == import_id && moving.was == Some(State::Vacant) {
        State::Vacant
    } else if moving.import.as_deref() == Some(import_id) {
        moving.was.unwrap_or(lease.state)
    } else {
        return Ok(());
    };
    rows::drop_move(db).await?;
    rows::set_copy(db, was).await?;
    rows::hold_still(db, false, None).await
}

// --- Moves to and from a device, reclaims, and her leaving the site (H3e) ---

/// Her moving to a device is built and accepted in H3, and opened to users
/// with H4 (Q8): until then only a debug build with
/// `MYRIAD_ACCEPT_DEVICE_HOST=1` takes it (the acceptance entry, as D17 is
/// on the Mac). Not a platform setting.
pub fn device_host_open() -> bool {
    cfg!(debug_assertions)
        && std::env::var("MYRIAD_ACCEPT_DEVICE_HOST").is_ok_and(|value| value == "1")
}

/// A move to a device, frozen and sealed: the digest of what the device is
/// to take.
pub async fn sealed(db: &DatabaseConnection, id: &str, digest: &str) -> Result<Lease, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::Unknown)?;
    let mut moving = lease
        .moving
        .filter(|moving| moving.id == id && moving.to_device())
        .ok_or(MoveError::Unknown)?;
    if moving.phase != Phase::Frozen {
        return Err(MoveError::Phase(moving.phase));
    }
    moving.digest = Some(digest.to_string());
    let lease = write(&txn, lease.state, true, Some(&moving)).await?;
    txn.commit().await?;
    Ok(lease)
}

/// The device a move to it is with took a step: it is reading her
/// (`Transferring`) or has all of her, checked and kept apart (`Applied`).
pub async fn device_step(
    db: &DatabaseConnection,
    id: &str,
    device: &str,
    phase: Phase,
) -> Result<Lease, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::Unknown)?;
    let mut moving = lease
        .moving
        .filter(|moving| moving.id == id && moving.to_device() && moving.to == device)
        .ok_or(MoveError::Unknown)?;
    if moving.digest.is_none() || !moving.phase.freezes() {
        return Err(MoveError::Phase(moving.phase));
    }
    if moving.phase.step() >= phase.step() {
        txn.commit().await?;
        return Ok(Lease {
            moving: Some(moving),
            ..lease
        });
    }
    moving.phase = phase;
    let lease = write(&txn, lease.state, true, Some(&moving)).await?;
    txn.commit().await?;
    Ok(lease)
}

/// The commit of a move to the device `device`: her lease goes to it. In the
/// transaction that lets the site's copy go she lives there now; with the
/// site's copy kept (`keep`, her log synced) her mind is on it.
pub(super) async fn committed_to_device(
    txn: &sea_orm::DatabaseTransaction,
    id: &str,
    device: &str,
    keep: bool,
) -> Result<Lease, DbErr> {
    let lease = locked(txn).await?.ok_or(DbErr::RecordNotUpdated)?;
    if lease.moving.as_ref().is_none_or(|moving| moving.id != id) {
        return Err(DbErr::RecordNotUpdated);
    }
    let grant = rows::sign(
        txn,
        Request::Commit {
            to: device.to_string(),
        },
        None,
    )
    .await?;
    if !keep {
        rows::set_copy(txn, State::Away).await?;
    }
    rows::finish(txn, grant.epoch as i64).await?;
    let copy = rows::copy(txn, false)
        .await?
        .ok_or(DbErr::RecordNotUpdated)?;
    rows::assemble(txn, copy).await
}

/// The lease, held for the transaction that changes it with what else
/// goes with the change.
pub(super) async fn hold(txn: &sea_orm::DatabaseTransaction) -> Result<Option<Lease>, DbErr> {
    locked(txn).await
}

/// The move back `id` is frozen on the device she lives on, and her import
/// `import` carries her: noted on the move.
pub(super) async fn carried_by(
    txn: &sea_orm::DatabaseTransaction,
    id: &str,
    import: &str,
    digest: &str,
) -> Result<Lease, DbErr> {
    let lease = locked(txn)
        .await?
        .ok_or(DbErr::RecordNotFound("lease".into()))?;
    let mut moving = lease
        .moving
        .filter(|moving| moving.id == id)
        .ok_or(DbErr::RecordNotFound("move".into()))?;
    moving.import = Some(import.to_string());
    moving.digest = Some(digest.to_string());
    write(txn, lease.state, true, Some(&moving)).await
}

/// Another device asks to take her back onto the site from a daily backup
/// of the device she lived on (H3e-13): nothing happens to her until the
/// owner confirms; then she is frozen everywhere (the device she lived on
/// stops too, if it hears) until she lands here or it is called off.
pub(super) async fn reclaiming(
    txn: &sea_orm::DatabaseTransaction,
    import: &str,
    via: &str,
) -> Result<Lease, MoveError> {
    let lease = locked(txn).await?.ok_or(MoveError::NoHer)?;
    if lease.moving.is_some() {
        return Err(MoveError::InProgress);
    }
    match lease.place() {
        Where::Elsewhere(holder) if holder == via => return Err(MoveError::NotHere),
        Where::Elsewhere(_) | Where::Vacant => {}
        _ => return Err(MoveError::NotHere),
    }
    let moving = Move {
        id: format!("mv_{}", uuid::Uuid::new_v4().simple()),
        kind: Kind::Recover,
        from: lease.holder.clone(),
        to: SITE.to_string(),
        phase: Phase::Proposed,
        by: Starter::Device,
        digest: None,
        started_at: crate::clock::now(),
        confirmed_at: None,
        agreed: None,
        import: Some(import.to_string()),
        via: Some(via.to_string()),
        was: Some(lease.state),
    };
    Ok(write(txn, lease.state, lease.frozen, Some(&moving)).await?)
}

/// The owner confirmed the move that carries her as the import `import`:
/// she is frozen until she lands or it is called off.
pub(super) async fn import_confirmed(
    txn: &sea_orm::DatabaseTransaction,
    import: &str,
) -> Result<Lease, MoveError> {
    let lease = locked(txn).await?.ok_or(MoveError::Unknown)?;
    let mut moving = lease
        .moving
        .filter(|moving| moving.import.as_deref() == Some(import))
        .ok_or(MoveError::Unknown)?;
    if moving.phase != Phase::Proposed {
        return Err(MoveError::Phase(moving.phase));
    }
    moving.phase = Phase::Frozen;
    moving.confirmed_at = Some(crate::clock::now());
    Ok(write(txn, lease.state, true, Some(&moving)).await?)
}

/// The device she lives on was removed (or its owner is not the site's any
/// more): the site does not know where she is now (vacant). A move with that
/// device is called off; nothing of her comes back to life here. Her mind
/// on it, while she lives here, comes back to the site.
pub(super) async fn device_gone<C: ConnectionTrait>(db: &C, device: &str) -> Result<(), DbErr> {
    rows::lock(db).await?;
    db.execute_raw(statement(
        "DELETE FROM agent_host_import WHERE device_id = $1 AND phase <> 'committed'",
        vec![device.into()],
    ))
    .await?;
    let Some(lease) = read(db).await? else {
        return Ok(());
    };
    let with_it = lease.moving.as_ref().filter(|moving| {
        moving.to == device || moving.via.as_deref() == Some(device) || moving.from == device
    });
    // Her way into a site that had none, from it: called off, the site as
    // it was (no her).
    if let Some(moving) = with_it
        && super::import::is_import(&moving.id)
        && moving.was.is_none()
        && lease.lives_on(device)
    {
        rows::hold_still(db, false, None).await?;
        return rows::forget_copy(db).await;
    }
    let state = if lease.lives_on(device) {
        // She lived on it: where she is now is not known here.
        Some(State::Vacant)
    } else {
        // A move to it, a merge from it, or a reclaim it was sending:
        // called off, she is where she was.
        with_it.map(|moving| moving.was.unwrap_or(lease.state))
    };
    if let Some(state) = state {
        if lease.moving.is_some() {
            rows::drop_move(db).await?;
        }
        rows::set_copy(db, state).await?;
        rows::hold_still(db, false, None).await?;
    } else if lease.state == State::Live && lease.holder == device && !lease.frozen {
        rows::to_site(db, None).await?;
    }
    Ok(())
}

/// She lives on a device the site's owner no longer has (removed, or
/// another owner's): vacant, what [`device_gone`] does when the device is
/// removed, for an owner change found later. Read from time to time.
pub async fn settle_orphan(db: &DatabaseConnection) -> Result<(), DbErr> {
    let Some(lease) = read(db).await? else {
        return Ok(());
    };
    if lease.state != State::Away || lease.moving.is_some() || lease.holder == SITE {
        return Ok(());
    }
    let owned = db
        .query_one_raw(statement(
            &format!(
                "SELECT 1 AS one FROM agent_sync_devices d WHERE d.id = $1 \
                 AND d.revoked_at IS NULL AND d.user_id = ({})",
                crate::services::principal::SITE_OWNER_ID_SQL
            ),
            vec![lease.holder.clone().into()],
        ))
        .await?
        .is_some();
    if !owned {
        rows::set_copy(db, State::Vacant).await?;
        tracing::info!("[host] the device she lived on is not the site owner's any more: vacant");
    }
    Ok(())
}

/// She is exported to be brought in elsewhere (「导出并搬走」 on the site, Q7):
/// stopped here until the owner lets this copy go or undoes it.
pub async fn move_out(db: &DatabaseConnection) -> Result<Lease, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::NoHer)?;
    if lease.moving.is_some() {
        return Err(MoveError::InProgress);
    }
    if lease.place() != Where::Here {
        return Err(MoveError::NotHere);
    }
    let lease = write(&txn, State::MovedOut, false, None).await?;
    txn.commit().await?;
    tracing::info!(
        epoch = lease.epoch,
        "[host] she moved out of the site: stopped here"
    );
    Ok(lease)
}

/// The owner undid her moving out: she lives here again, as she was.
pub async fn undo_move_out(db: &DatabaseConnection) -> Result<Lease, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::NoHer)?;
    if lease.state != State::MovedOut {
        return Err(MoveError::Unknown);
    }
    let lease = write(&txn, State::Live, false, None).await?;
    txn.commit().await?;
    tracing::info!(
        epoch = lease.epoch,
        "[host] her moving out was undone: she is here again"
    );
    Ok(lease)
}

/// She moved out and the owner lets this copy go, in the transaction that
/// does: the site does not have her now (vacant), her lineage kept for what
/// is left of others here (Q2).
pub async fn let_go_on(txn: &sea_orm::DatabaseTransaction) -> Result<Lease, MoveError> {
    let lease = locked(txn).await?.ok_or(MoveError::NoHer)?;
    if lease.state != State::MovedOut {
        return Err(MoveError::Unknown);
    }
    Ok(write(txn, State::Vacant, false, None).await?)
}

/// Her copy goes with what was left of her here, when the owner starts
/// over on a site she left (vacant): the next her is another birth.
pub async fn forget_vacant_on<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    if copy_state(db).await? == Some(State::Vacant) {
        rows::forget_copy(db).await?;
    }
    Ok(())
}

/// The lease this process last read; None before the first read or while
/// the site has no her.
static CURRENT: LazyLock<RwLock<Option<Lease>>> = LazyLock::new(|| RwLock::new(None));
/// Her own drivers told they are paused, so each says so once.
static PAUSED: LazyLock<Mutex<HashSet<&'static str>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

pub fn current() -> Option<Lease> {
    CURRENT.read().ok().and_then(|lease| lease.clone())
}

/// Where she is, by the lease this process last read and, while she lives
/// here, where her mind is (`her_log::pen`: it moves before the grant is
/// read back, so it is the one asked).
pub fn place() -> Where {
    use crate::services::her_log::pen::{self, Pen};
    match current().map_or(Where::Here, |lease| lease.place()) {
        Where::Here | Where::MindOn(_) => match pen::current() {
            Pen::Here => Where::Here,
            Pen::Away { holder, .. } => Where::MindOn(holder),
            Pen::CatchingUp { .. } => Where::MindOn(SITE.into()),
        },
        elsewhere => elsewhere,
    }
}

/// What a run is refused with while she is not on the site; Ok while she is.
pub fn refuse_while_away() -> Result<(), AppError> {
    match place() {
        Where::Here => Ok(()),
        Where::Moving => Err(AppError::conflict(
            "She is moving between her ends; nothing reaches her until the move is done or cancelled",
        )
        .with_code("host_frozen")),
        Where::Elsewhere(_) | Where::Vacant | Where::MovedOut => {
            Err(AppError::conflict("She does not live on this site now").with_code("host_moved"))
        }
        Where::MindOn(_) => Err(
            AppError::conflict("She is on another of your devices now").with_code("mind_elsewhere")
        ),
    }
}

/// What a Work run is refused with while she is not on the site: only
/// while she is moving (no end has her). Living on another end, the site's
/// Work runs as the plain assistant, without her (H3e-9); exported from
/// here and not let go yet, the copy here is still hers, so it waits.
pub fn refuse_work_while_away() -> Result<(), AppError> {
    match place() {
        Where::Here | Where::Elsewhere(_) | Where::Vacant => Ok(()),
        _ => refuse_while_away(),
    }
}

/// Her own settings and records on the site are read only while she is not
/// here (D5): she lives on another end and they are hers there.
pub fn refuse_edit_while_away() -> Result<(), AppError> {
    refuse_while_away()
}

/// One of her own drivers asks before each tick: true while she is not on
/// the site, and the driver skips it. Each says it is paused once.
pub fn paused(driver: &'static str) -> bool {
    let place = place();
    if place.here() {
        return false;
    }
    if PAUSED.lock().is_ok_and(|mut paused| paused.insert(driver)) {
        tracing::info!(driver, place = ?place, "[host] her own life is paused here: skipped");
    }
    true
}

/// Where she was when this process last took a lease or her mind moved.
static TAKEN: LazyLock<Mutex<Where>> = LazyLock::new(|| Mutex::new(Where::Here));

/// Takes a lease just read: what changed for her links and her drivers.
pub fn adopt(lease: Option<Lease>) {
    if let Ok(mut current) = CURRENT.write() {
        *current = lease.clone();
    }
    let after = place();
    let before = match TAKEN.lock() {
        Ok(mut taken) => std::mem::replace(&mut *taken, after.clone()),
        Err(_) => return,
    };
    if before == after {
        return;
    }
    tracing::info!(
        before = ?before,
        after = ?after,
        epoch = lease.as_ref().map(|lease| lease.epoch),
        state = lease.as_ref().map(|lease| lease.state.as_str()),
        "[host] where she lives changed"
    );
    // Every child is told and reconnects to the refusal; the device she
    // lives on is told on its own link what became of her there.
    if let Some(lease) = &lease {
        crate::services::agent::link::lease_changed(lease);
    }
    // Her mind left a device: what was lent to it and not taken is the
    // site's again (groups for the group mind once she is back, private
    // messages held again).
    let given_back = match &before {
        Where::MindOn(device) => crate::services::her_log::lend::let_go(device),
        _ => Vec::new(),
    };
    let mut given_back = Some(given_back).filter(|back| !back.is_empty());
    match after {
        Where::Moving
        | Where::Elsewhere(_)
        | Where::Vacant
        | Where::MovedOut
        | Where::MindOn(_) => {}
        Where::Here => {
            if let Ok(mut paused) = PAUSED.lock() {
                paused.clear();
            }
            tracing::info!("[host] she is here again: her own life goes on");
            // What came by chat app while she was away is read now, in the
            // process that runs the chat apps (D3).
            if crate::runtime_role::PERSONA_RUNTIME_LOCAL.load(std::sync::atomic::Ordering::Acquire)
                && let Ok(db) = crate::services::process_db::database()
                && tokio::runtime::Handle::try_current().is_ok()
            {
                // Back from her mind on another core: only the groups whose
                // lent lines that core never answered are hers to take up
                // here; the rest she answered there.
                if matches!(before, Where::MindOn(_)) {
                    crate::services::channel_group::back_from_lending();
                } else {
                    crate::services::channel_group::back_from_away();
                }
                // What was given back is held first, then read with the rest.
                let back = given_back.take().unwrap_or_default();
                let held = db.clone();
                tokio::spawn(async move {
                    crate::services::her_log::lend::give_back(
                        &held,
                        &crate::services::her_log::lend::Platforms,
                        back,
                    )
                    .await;
                    crate::services::channel_work::held::replay(held).await;
                });
                tokio::spawn(crate::services::agent::merope::replay_kept_for_her(db));
            }
        }
    }
    // She is not back here: what was given back is held until she is.
    if let Some(back) = given_back
        && let Ok(db) = crate::services::process_db::database()
        && tokio::runtime::Handle::try_current().is_ok()
    {
        tokio::spawn(async move {
            crate::services::her_log::lend::give_back(
                &db,
                &crate::services::her_log::lend::Platforms,
                back,
            )
            .await;
        });
    }
}

/// Reads the lease again and takes it. Tests share one process, so there
/// this process's copy stays as it is (nobody's runs are refused by another
/// test's move).
pub async fn reload<C: ConnectionTrait>(db: &C) {
    if cfg!(test) {
        return;
    }
    match read(db).await {
        Ok(lease) => {
            settle_pen().await;
            adopt(lease)
        }
        Err(error) => tracing::warn!(%error, "[host] could not read her host lease"),
    }
}

/// Reads where her mind is again (`her_log::pen`) before the lease is
/// taken, so [`adopt`] sees both change together.
async fn settle_pen() {
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    if let Err(error) =
        crate::services::her_log::pen::settle(&db, crate::services::her_log::settings()).await
    {
        tracing::warn!(%error, "[host] could not read where her mind is");
    }
}

/// Her mind moved (a grant was kept, or the server caught up): this
/// process takes it as it takes a lease change.
pub fn mind_moved() {
    adopt(current());
}

/// [`reload`], first making a lease vacant whose device is not the site
/// owner's any more ([`settle_orphan`]): what the listener does each time.
async fn reload_settled(db: &DatabaseConnection) {
    if let Err(error) = settle_orphan(db).await {
        tracing::warn!(%error, "[host] could not check the device she lives on");
    }
    reload(db).await;
}

/// Keeps this process's copy of the lease: read now, again on each
/// announcement and every [`REREAD_EVERY`]. Once per process.
pub fn keep(db: DatabaseConnection) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    crate::services::jobs::jobs().supervised(
        "host lease listener",
        crate::services::jobs::Backoff::new(Duration::from_secs(2), Duration::from_secs(60)),
        move || listen(db.clone()),
    );
}

async fn listen(db: DatabaseConnection) {
    let pool = db.get_postgres_connection_pool().clone();
    let mut listener = match sea_orm::sqlx::postgres::PgListener::connect_with(&pool).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(%error, "[host] lease listener unavailable");
            reload(&db).await;
            return;
        }
    };
    if listener.listen(CHANNEL).await.is_err() {
        reload(&db).await;
        return;
    }
    // Listening first, then reading: nothing committed in between is missed.
    reload_settled(&db).await;
    let mut every =
        tokio::time::interval_at(tokio::time::Instant::now() + REREAD_EVERY, REREAD_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            heard = listener.try_recv() => match heard {
                Ok(Some(_)) => reload(&db).await,
                // The connection dropped and came back: what was missed is read.
                Ok(None) => reload(&db).await,
                Err(_) => return,
            },
            _ = every.tick() => reload_settled(&db).await,
        }
    }
}

/// Why her link is refused before the upgrade, read from the database (not
/// this process's copy): `(code, body)`; None while she is on the site.
pub async fn link_refusal<C: ConnectionTrait>(
    db: &C,
    device: &str,
) -> Result<Option<(&'static str, Value)>, DbErr> {
    let Some(lease) = read(db).await? else {
        return Ok(None);
    };
    // The device she lives on dials in as her host (H3e-7), moving or not.
    if lease.lives_on(device) {
        return Ok(None);
    }
    Ok(match lease.place() {
        // Her mind on another core: her links stay open (the mind lease is
        // told over them).
        Where::Here | Where::MindOn(_) => None,
        Where::Moving => Some((
            "host_frozen",
            json!({
                "error": "She is moving between her ends; try again once the move is done or cancelled",
                "code": "host_frozen",
            }),
        )),
        Where::Elsewhere(holder) => {
            let name = device_name(db, &holder).await?;
            Some((
                "host_moved",
                json!({
                    "error": "She lives on another end now",
                    "code": "host_moved",
                    "host": name.map(|name| json!({ "kind": "app", "name": name })),
                }),
            ))
        }
        Where::Vacant => Some((
            "host_moved",
            json!({ "error": "She does not live on this site now", "code": "host_moved", "host": Value::Null }),
        )),
        Where::MovedOut => Some((
            "host_moved",
            json!({
                "error": "She moved out of this site",
                "code": "host_moved",
                "host": Value::Null,
                "movedOut": true,
            }),
        )),
    })
}

/// The name the owner gave the device `id`, removed or not.
pub async fn device_name<C: ConnectionTrait>(db: &C, id: &str) -> Result<Option<String>, DbErr> {
    db.query_one_raw(statement(
        "SELECT name FROM agent_sync_devices WHERE id = $1",
        vec![id.into()],
    ))
    .await?
    .map(|row| row.try_get::<String>("", "name"))
    .transpose()
}

/// Device names by id, for [`Lease::json`].
pub async fn names<C: ConnectionTrait>(
    db: &C,
    lease: &Lease,
) -> Result<std::collections::HashMap<String, String>, DbErr> {
    let mut names = std::collections::HashMap::new();
    let mut ids = vec![lease.holder.clone()];
    if let Some(moving) = &lease.moving {
        ids.push(moving.from.clone());
        ids.push(moving.to.clone());
        ids.extend(moving.via.clone());
    }
    for id in ids.into_iter().filter(|id| id != SITE) {
        if let Some(name) = device_name(db, &id).await? {
            names.insert(id, name);
        }
    }
    Ok(names)
}

/// The lease as JSON with its device names, or null while there is no her.
pub async fn shown<C: ConnectionTrait>(db: &C) -> Result<Value, DbErr> {
    let Some(lease) = read(db).await? else {
        return Ok(Value::Null);
    };
    let names = names(db, &lease).await?;
    Ok(lease.json(|id| names.get(id).cloned()))
}

/// For tests: she lives as `state` with `lineage`, and her lease names
/// `holder` (at `epoch` when the site never issued one, or moved to it at
/// `epoch` or later). Nothing of a move.
#[cfg(test)]
pub(crate) async fn seed<C: ConnectionTrait>(
    db: &C,
    holder: &str,
    lineage: &str,
    epoch: i64,
    state: State,
) -> Result<Lease, DbErr> {
    rows::forget_copy(db).await?;
    rows::put_copy(db, lineage, state).await?;
    rows::lock(db).await?;
    match rows::standing(db).await? {
        None => rows::begin_with(db, holder, epoch).await?,
        Some(grant) if grant.held_by(holder) => {}
        Some(_) => {
            rows::sign(
                db,
                Request::Force {
                    to: holder.to_string(),
                },
                Some(epoch),
            )
            .await?;
        }
    }
    read(db).await?.ok_or(DbErr::RecordNotFound("her".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_goes_one_step_at_a_time_and_can_be_called_off_until_its_commit() {
        let order = [
            Phase::Proposed,
            Phase::Frozen,
            Phase::Transferring,
            Phase::Applied,
            Phase::Committed,
            Phase::Retired,
        ];
        for (index, phase) in order.iter().enumerate() {
            assert_eq!(phase.step() as usize, index + 1);
            assert_eq!(phase.next(), order.get(index + 1).copied());
        }
        assert!(Phase::Applied.cancellable() && !Phase::Committed.cancellable());
        assert!(
            !Phase::Proposed.freezes() && Phase::Frozen.freezes() && !Phase::Committed.freezes()
        );
    }

    fn lease(holder: &str, state: State) -> Lease {
        Lease {
            holder: holder.into(),
            lineage: "6f1c".into(),
            epoch: 3,
            since: 3,
            state,
            frozen: false,
            moving: None,
            updated_at: crate::clock::now(),
            done: None,
        }
    }

    #[test]
    fn where_she_is_and_what_a_run_is_refused_with() {
        assert_eq!(lease(SITE, State::Live).place(), Where::Here);
        let frozen = Lease {
            frozen: true,
            ..lease(SITE, State::Live)
        };
        assert_eq!(frozen.place(), Where::Moving);
        assert_eq!(frozen.json(|_| None)["state"], "frozen", "as before");
        // Her lease on a device: where she lives if the site let its copy
        // go, only where her mind is if not.
        assert_eq!(
            lease("dev_1", State::Away).place(),
            Where::Elsewhere("dev_1".into())
        );
        assert!(lease("dev_1", State::Away).lives_on("dev_1"));
        assert_eq!(lease("dev_1", State::Away).json(|_| None)["state"], "live");
        assert_eq!(
            lease("dev_1", State::Live).place(),
            Where::MindOn("dev_1".into())
        );
        assert!(!lease("dev_1", State::Live).lives_on("dev_1"));
        assert_eq!(lease("dev_1", State::Vacant).place(), Where::Vacant);
        assert_eq!(lease(SITE, State::MovedOut).place(), Where::MovedOut);
        assert_eq!(
            serde_json::to_value(State::MovedOut).unwrap(),
            "moved_out",
            "as the row keeps it"
        );
        assert_eq!(State::parse("moved_out"), Some(State::MovedOut));
    }

    /// Which device a move is with, and which way it goes.
    #[test]
    fn a_move_names_the_device_it_is_with() {
        let mut moving = Move {
            id: "mv_1".into(),
            kind: Kind::Move,
            from: SITE.into(),
            to: "dev_1".into(),
            phase: Phase::Proposed,
            by: Starter::Owner,
            digest: None,
            started_at: crate::clock::now(),
            confirmed_at: None,
            agreed: Some(false),
            import: None,
            via: None,
            was: Some(State::Live),
        };
        assert!(moving.to_device() && !moving.back_to_site());
        assert_eq!(moving.device(), "dev_1");
        moving.from = "dev_1".into();
        moving.to = SITE.into();
        assert!(!moving.to_device() && moving.back_to_site());
        assert_eq!(moving.device(), "dev_1");
        moving.kind = Kind::Recover;
        moving.via = Some("dev_2".into());
        assert!(!moving.back_to_site());
        assert_eq!(
            moving.device(),
            "dev_2",
            "a reclaim is with the device sending her"
        );
        let read: Move = serde_json::from_value(serde_json::to_value(&moving).unwrap()).unwrap();
        assert_eq!(read, moving);
        // A move written before H3e reads with none of the new fields.
        let old: Move = serde_json::from_value(json!({
            "id": "mv_0", "kind": "merge", "from": "dev_1", "to": "site", "phase": "frozen",
            "by": "device", "startedAt": "2026-10-13T00:00:00Z"
        }))
        .unwrap();
        assert_eq!(
            (old.agreed, old.import, old.via, old.was),
            (None, None, None, None)
        );
    }

    /// The lease as it is shown carries where she lives and the move, and
    /// nothing else: no key, token or grant.
    #[test]
    fn the_lease_shown_has_no_secret_in_it() {
        let mut shown = Lease {
            frozen: true,
            ..lease(SITE, State::Live)
        };
        shown.moving = Some(Move {
            id: "mv_1".into(),
            kind: Kind::Merge,
            from: "dev_1".into(),
            to: SITE.into(),
            phase: Phase::Frozen,
            by: Starter::Device,
            digest: None,
            started_at: crate::clock::now(),
            confirmed_at: Some(crate::clock::now()),
            agreed: None,
            import: None,
            via: None,
            was: None,
        });
        let value = shown.json(|id| (id == "dev_1").then(|| "MacBook".to_string()));
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "done",
                "epoch",
                "holder",
                "holderName",
                "lineage",
                "move",
                "state",
                "updatedAt"
            ]
        );
        assert_eq!(value["holderName"], Value::Null);
        assert_eq!(value["move"]["fromName"], "MacBook");
        assert_eq!(value["move"]["step"], 2);
        assert_eq!(value["move"]["kind"], "merge");
        let moving: Vec<&str> = value["move"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            moving,
            [
                "agreed",
                "by",
                "confirmedAt",
                "digest",
                "from",
                "fromName",
                "id",
                "import",
                "kind",
                "phase",
                "startedAt",
                "step",
                "to",
                "toName",
                "via",
                "viaName"
            ]
        );
    }

    /// Issuing, a move asked for by a device and confirmed by the owner
    /// (her lease frozen), cancelling (thawed), the guard on the grants, and
    /// a new birth after her persona goes (an isolated schema; skipped
    /// without a test database).
    #[tokio::test]
    async fn the_lease_on_a_database() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        assert_eq!(read(db).await.unwrap(), None, "no her, no lease");
        assert!(matches!(
            start(db, Kind::Move, "dev_1", Starter::Owner).await,
            Err(MoveError::NoHer)
        ));
        db.execute_unprepared(
            "INSERT INTO agent_persona (id, name, updated_at) VALUES ('site', 'Merope', NOW())",
        )
        .await
        .unwrap();
        let issued = read(db).await.unwrap().unwrap();
        assert_eq!(
            (issued.holder.as_str(), issued.epoch, issued.state),
            (SITE, 1, State::Live)
        );
        assert_eq!(issued.lineage.len(), 36);
        assert_eq!(
            read(db).await.unwrap().unwrap().lineage,
            issued.lineage,
            "once"
        );

        // A device asks: nothing happens to her until the owner confirms.
        let proposed = start(db, Kind::Merge, "dev_1", Starter::Device)
            .await
            .unwrap();
        let moving = proposed.moving.clone().unwrap();
        assert_eq!(
            (
                moving.phase,
                moving.from.as_str(),
                moving.to.as_str(),
                proposed.state
            ),
            (Phase::Proposed, "dev_1", SITE, State::Live)
        );
        assert!(matches!(
            start(db, Kind::Move, "dev_2", Starter::Owner).await,
            Err(MoveError::InProgress)
        ));
        assert!(matches!(
            confirm(db, "mv_other").await,
            Err(MoveError::Unknown)
        ));
        let frozen = confirm(db, &moving.id).await.unwrap();
        assert!(frozen.frozen);
        assert_eq!(frozen.place(), Where::Moving);
        assert_eq!(
            crate::services::her_log::lease::standing(db)
                .await
                .unwrap()
                .map(|grant| (grant.how, grant.holder, grant.moving)),
            Some((
                myriad_persona_log::How::Freeze,
                None,
                Some(moving.id.clone())
            )),
            "no end holds her"
        );
        assert!(frozen.moving.as_ref().unwrap().confirmed_at.is_some());
        assert!(matches!(
            confirm(db, &moving.id).await,
            Err(MoveError::Phase(Phase::Frozen))
        ));
        // Another device cannot call it off; the one it is with can.
        assert!(matches!(
            cancel(db, &moving.id, Some("dev_2")).await,
            Err(MoveError::Unknown)
        ));
        let back = cancel(db, &moving.id, Some("dev_1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                back.place(),
                back.moving.is_none(),
                back.holder.as_str(),
                back.lineage.as_str()
            ),
            (Where::Here, true, SITE, issued.lineage.as_str()),
            "as she was"
        );
        assert_eq!(
            back.epoch,
            frozen.epoch + 1,
            "thawed: the epoch only goes up"
        );
        // A move to a device the owner starts waits for the device to agree
        // (H3e-2): the owner's confirmation needs it, and only then is she
        // frozen.
        let owner = start(db, Kind::Move, "dev_2", Starter::Owner)
            .await
            .unwrap();
        assert_eq!(owner.state, State::Live);
        let moving = owner.moving.unwrap();
        assert_eq!(
            (moving.phase, moving.agreed),
            (Phase::Proposed, Some(false))
        );
        assert!(matches!(
            confirm(db, &moving.id).await,
            Err(MoveError::NotAgreed)
        ));
        assert!(matches!(
            agree(db, &moving.id, "dev_3").await,
            Err(MoveError::Unknown)
        ));
        agree(db, &moving.id, "dev_2").await.unwrap();
        let frozen = confirm(db, &moving.id).await.unwrap();
        assert!(frozen.frozen);
        assert!(
            matches!(
                device_step(db, &moving.id, "dev_2", Phase::Transferring).await,
                Err(MoveError::Phase(Phase::Frozen))
            ),
            "nothing to read before she is sealed"
        );
        sealed(db, &moving.id, "d1").await.unwrap();
        device_step(db, &moving.id, "dev_2", Phase::Transferring)
            .await
            .unwrap();
        let applied = device_step(db, &moving.id, "dev_2", Phase::Applied)
            .await
            .unwrap();
        assert_eq!(applied.moving.unwrap().phase, Phase::Applied);
        cancel(db, &moving.id, None).await.unwrap();
        assert_eq!(
            read(db).await.unwrap().unwrap().place(),
            Where::Here,
            "as she was"
        );

        // She lives on a device: the site starts nothing and refuses her link.
        seed(db, "dev_9", &issued.lineage, 2, State::Away)
            .await
            .unwrap();
        assert!(matches!(
            start(db, Kind::Move, "dev_2", Starter::Owner).await,
            Err(MoveError::NotHere)
        ));
        let (code, body) = link_refusal(db, "dev_2").await.unwrap().unwrap();
        assert_eq!((code, &body["host"]), ("host_moved", &Value::Null));
        // The device she lives on dials in as her host.
        assert_eq!(link_refusal(db, "dev_9").await.unwrap(), None);
        assert!(
            db.execute_unprepared("UPDATE agent_lease_grants SET epoch = 1")
                .await
                .is_err(),
            "a grant is never changed: the epoch never goes down"
        );
        forget_birth(db).await.unwrap();
        assert_eq!(
            read(db).await.unwrap().unwrap().holder,
            "dev_9",
            "kept: she lives there"
        );
        let back = seed(db, SITE, &issued.lineage, 1, State::Live)
            .await
            .unwrap();
        // Her persona deleted on the site: the next her is another birth,
        // and her lease goes on.
        forget_birth(db).await.unwrap();
        let reborn = read(db).await.unwrap().unwrap();
        assert_ne!(reborn.lineage, issued.lineage);
        assert_eq!((reborn.holder.as_str(), reborn.epoch), (SITE, back.epoch));

        fixture.close().await;
    }

    /// H3e on a database (an isolated schema; skipped without one): a move
    /// back asked by the device she lives on and called off, a reclaim
    /// called off, the device she lives on removed (vacant, a move with it
    /// called off), an owner change found later, moving out and undoing it,
    /// and a landing that keeps the epoch going up whatever the file says.
    #[tokio::test]
    async fn moves_back_reclaims_and_leaving_on_a_database() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        db.execute_unprepared(
            "INSERT INTO users (id, username, is_owner) VALUES (41, 'owner41', true); \
             INSERT INTO agent_sync_devices (id, user_id, name, token_hash, token_hint, created_at) \
             VALUES ('dev_a', 41, 'A', 'ha', 'mys_a', NOW()), ('dev_b', 41, 'B', 'hb', 'mys_b', NOW())",
        )
        .await
        .unwrap();
        seed(db, "dev_a", "l-1", 2, State::Away).await.unwrap();
        // The device she lives on asks to bring her back: nothing yet.
        let asked = start_back(db, "dev_a", Starter::Device).await.unwrap();
        assert_eq!(asked.place(), Where::Elsewhere("dev_a".into()));
        assert!(matches!(
            start_back(db, "dev_b", Starter::Device).await,
            Err(MoveError::InProgress)
        ));
        let id = asked.moving.unwrap().id;
        let frozen = confirm(db, &id).await.unwrap();
        assert_eq!(frozen.place(), Where::Moving);
        let back = cancel(db, &id, Some("dev_a")).await.unwrap().unwrap();
        assert_eq!(
            (back.place(), back.since, back.epoch),
            (Where::Elsewhere("dev_a".into()), 2, 4),
            "she lives on the device as she was, frozen and thawed since 2"
        );
        // Only the device she lives on can ask her back.
        assert!(matches!(
            start_back(db, "dev_b", Starter::Device).await,
            Err(MoveError::NotHere)
        ));

        // A reclaim by another device, called off: as she was.
        {
            let txn = db.begin().await.unwrap();
            assert!(
                matches!(
                    reclaiming(&txn, "imp_x", "dev_a").await,
                    Err(MoveError::NotHere)
                ),
                "not by the device she lives on"
            );
            let asked = reclaiming(&txn, "imp_1", "dev_b").await.unwrap();
            let moving = asked.moving.unwrap();
            assert_eq!(
                (moving.kind, moving.from.as_str()),
                (Kind::Recover, "dev_a")
            );
            let frozen = import_confirmed(&txn, "imp_1").await.unwrap();
            assert!(frozen.frozen);
            txn.commit().await.unwrap();
        }
        forget_arrival(db, "imp_1").await.unwrap();
        let after = read(db).await.unwrap().unwrap();
        assert_eq!(
            (after.place(), after.moving.is_none()),
            (Where::Elsewhere("dev_a".into()), true)
        );

        // A move to the site she lives on... the device is removed: vacant,
        // and nothing comes back to life here.
        device_gone(db, "dev_a").await.unwrap();
        let vacant = read(db).await.unwrap().unwrap();
        assert_eq!(
            (vacant.place(), vacant.lineage.as_str()),
            (Where::Vacant, "l-1")
        );
        assert!(refuse_while_away_at(&vacant).is_err());

        // Her lineage lands again (a file at epoch 7): one past the higher.
        {
            let txn = db.begin().await.unwrap();
            let landed = landed(&txn, "l-1", 7).await.unwrap();
            txn.commit().await.unwrap();
            assert_eq!((landed.place(), landed.epoch), (Where::Here, 8));
        }
        // Moving out and undoing it; moving out and letting her go.
        let out = move_out(db).await.unwrap();
        assert_eq!(out.place(), Where::MovedOut);
        assert!(matches!(move_out(db).await, Err(MoveError::NotHere)));
        let undone = undo_move_out(db).await.unwrap();
        assert_eq!((undone.place(), undone.epoch), (Where::Here, 8));
        move_out(db).await.unwrap();
        {
            let txn = db.begin().await.unwrap();
            let gone = let_go_on(&txn).await.unwrap();
            txn.commit().await.unwrap();
            assert_eq!((gone.place(), gone.epoch), (Where::Vacant, 8));
        }
        forget_vacant_on(db).await.unwrap();
        assert_eq!(
            read(db).await.unwrap(),
            None,
            "the next her is another birth"
        );

        // An owner change found later: the device she lives on is another
        // owner's now.
        seed(db, "dev_b", "l-2", 3, State::Away).await.unwrap();
        db.execute_unprepared(
            "UPDATE users SET is_owner = false WHERE id = 41; \
             INSERT INTO users (id, username, is_owner) VALUES (42, 'owner42', true)",
        )
        .await
        .unwrap();
        settle_orphan(db).await.unwrap();
        assert_eq!(read(db).await.unwrap().unwrap().place(), Where::Vacant);
        fixture.close().await;
    }

    fn refuse_while_away_at(lease: &Lease) -> Result<(), ()> {
        if lease.place().here() {
            Ok(())
        } else {
            Err(())
        }
    }
}
