//! What a removed device leaves of her: an import it was sending, a move with
//! it, her living on it, her mind on it, a deputy given to it as it was
//! removed. Removing it takes its token and its deputies back first, on their
//! own ([`devices::revoke`]); this comes after, in one transaction, in the
//! order of [`order`](super::order), and then what an import or a move that
//! ended with it kept on disk goes. Until it goes through, all of that stands
//! as it did, and it is done again: when the device is removed once more or
//! forgotten, and from time to time by the listener ([`settle_all`]). Done
//! twice it changes nothing.

use std::time::Duration;

use chrono::{DateTime, FixedOffset, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, TransactionSession, TransactionTrait};

use super::{Lease, Move, State, rows, statement};
use crate::services::persona_sync::{devices, import, merge, moves};
use myriad_persona_sync::archive::Manifest;

/// How long it waits for each thing another holds (the imports, a row of
/// theirs, her lease, the rows) before it is left for later: a few seconds
/// in all at most, not one. Longer in the tests: in one database they hold each
/// other's locks, and the ones about waiting say how long themselves.
pub(in crate::services::persona_sync) const WAIT: Duration = if cfg!(test) {
    Duration::from_secs(30)
} else {
    Duration::from_secs(1)
};

/// Settles what `device` left of her, if it is a removed one.
pub(in crate::services::persona_sync) async fn settle<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    device: &str,
) -> Result<(), DbErr> {
    settle_within(db, device, WAIT).await
}

pub(in crate::services::persona_sync) async fn settle_within<
    C: ConnectionTrait + TransactionTrait,
>(
    db: &C,
    device: &str,
    wait: Duration,
) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    let ended = match settle_on(&txn, device, wait).await {
        Ok(Some(ended)) => ended,
        Ok(None) => return txn.rollback().await,
        Err(error) => {
            if let Err(undone) = txn.rollback().await {
                tracing::debug!(error = %undone, "[host] settling a removed device was not rolled back here");
            }
            return Err(error);
        }
    };
    txn.commit().await?;
    // On its own: who asked may have gone by now, and the next time there is
    // nothing left to settle.
    let device = device.to_string();
    let seen_to = tokio::spawn(async move {
        ended.told(&device).await;
        ended.forget_files().await;
    });
    if let Err(error) = seen_to.await {
        tracing::error!(%error, "[host] what a removed device kept on disk was not seen to");
    }
    Ok(())
}

/// Within the transaction that settles `device`; None when it is not a
/// removed one.
async fn settle_on<C: ConnectionTrait>(
    txn: &C,
    device: &str,
    wait: Duration,
) -> Result<Option<Ended>, DbErr> {
    txn.execute_unprepared(&format!("SET LOCAL lock_timeout = {}", wait.as_millis()))
        .await?;
    // The imports it was sending go with it, and then her lease moves.
    import::hold_all(txn).await?;
    let Some(removed_at) = txn
        .query_one_raw(statement(
            "SELECT revoked_at FROM agent_sync_devices WHERE id = $1 AND revoked_at IS NOT NULL",
            vec![device.into()],
        ))
        .await?
        .map(|row| row.try_get::<DateTime<FixedOffset>>("", "revoked_at"))
        .transpose()?
    else {
        return Ok(None);
    };
    let ended = device_gone(txn, device, removed_at.with_timezone(&Utc)).await?;
    devices::withdraw_deputies(txn, device).await?;
    Ok(Some(ended))
}

/// The imports and the move that ended with a removed device.
#[derive(Debug, Default)]
pub(super) struct Ended {
    imports: Vec<String>,
    moving: Option<String>,
    back: Vec<Back>,
    /// The import of another device that went with the move it carried.
    carried: Option<String>,
}

impl Ended {
    /// The import the move carried, when another device sent it: one the
    /// removed device sent went already.
    async fn import<C: ConnectionTrait>(&mut self, db: &C, id: &str) -> Result<(), DbErr> {
        if import::drop_session(db, id).await? {
            self.imports.push(id.to_string());
            self.carried = Some(id.to_string());
        }
        Ok(())
    }

    async fn told(&self, device: &str) {
        for back in &self.back {
            back.told(device).await;
        }
        if let Some(import) = &self.carried {
            tracing::warn!(import = %import, device, "[host] a device was removed while another was sending her back from its backup: that import is let go, the backup is kept");
        }
    }

    /// What they kept on disk goes, as when they are called off: the sealed
    /// copy of her and its key, what an import sent so far.
    async fn forget_files(self) {
        for import in &self.imports {
            import::forget_files(import).await;
            merge::forget_retired(import).await;
        }
        if let Some(moving) = &self.moving {
            moves::forget_files(moving).await;
        }
    }
}

/// Her on her way back from the device she lived on, which was removed
/// before she landed: what had come of her goes with the import. The device
/// keeps her until she lands, and its daily backups are kept here.
#[derive(Debug)]
struct Back {
    import: String,
    manifest: Option<Manifest>,
    entities: i64,
    vectors: i64,
    assets: i64,
}

impl Back {
    async fn from<C: ConnectionTrait>(db: &C, device: &str) -> Result<Vec<Self>, DbErr> {
        db.query_all_raw(statement(
            "SELECT i.id, i.manifest, \
                    COUNT(*) FILTER (WHERE s.kind NOT IN \
                      ('embedding', 'asset', 'embedding_model', 'origin'))::BIGINT AS entities, \
                    COUNT(*) FILTER (WHERE s.kind = 'embedding')::BIGINT AS vectors, \
                    COUNT(*) FILTER (WHERE s.kind = 'asset')::BIGINT AS assets \
             FROM agent_host_import i \
             LEFT JOIN agent_host_import_staging s ON s.import_id = i.id \
             WHERE i.device_id = $1 AND i.kind = $2 AND i.phase <> 'committed' GROUP BY i.id",
            vec![device.into(), import::kind::RETURN.into()],
        ))
        .await?
        .iter()
        .map(|row| {
            Ok(Self {
                import: row.try_get("", "id")?,
                manifest: row
                    .try_get::<Option<serde_json::Value>>("", "manifest")?
                    .and_then(|manifest| serde_json::from_value(manifest).ok()),
                entities: row.try_get("", "entities")?,
                vectors: row.try_get("", "vectors")?,
                assets: row.try_get("", "assets")?,
            })
        })
        .collect()
    }

    /// Says so, and whether all of her had come, before its files go.
    async fn told(&self, device: &str) {
        let files = import::assets_here(&self.import).await;
        let whole = self.manifest.as_ref().is_some_and(|manifest| {
            let entities: u64 = manifest.kinds.values().map(|kind| kind.count).sum();
            let assets = manifest.assets.count;
            [self.entities, self.vectors, self.assets].map(i64::unsigned_abs)
                == [entities, manifest.embeddings.count, assets]
                && files as u64 == assets
        });
        tracing::warn!(import = %self.import, device, whole, "[host] a device was removed while she was on her way back from it: what had come of her is let go, the device keeps her");
    }
}

/// Settles every removed device that still holds something of her, one after
/// another, each for a few seconds at most ([`WAIT`]). One that cannot be now
/// is left for the next time.
pub(super) async fn settle_all(db: &DatabaseConnection) -> Result<(), DbErr> {
    for device in unsettled(db).await? {
        match settle(db, &device).await {
            Ok(()) => {
                tracing::info!(device = %device, "[host] what a removed device left of her is settled")
            }
            Err(error) => {
                tracing::warn!(%error, device = %device, "[host] what a removed device left of her is not settled yet")
            }
        }
    }
    Ok(())
}

/// The removed devices that still hold something of her.
async fn unsettled(db: &DatabaseConnection) -> Result<Vec<String>, DbErr> {
    let lease = super::read(db).await?;
    let named = lease.as_ref().map(|lease| {
        let moving = lease.moving.as_ref();
        [
            lease.holder.as_str(),
            moving.map_or("", Move::device),
            moving.map_or("", |moving| moving.from.as_str()),
        ]
    });
    let [holder, with, from] = named.unwrap_or_default();
    let rows = db
        .query_all_raw(statement(
            "SELECT id, revoked_at, held FROM ( \
               SELECT d.id, d.revoked_at, \
                      EXISTS (SELECT 1 FROM agent_host_import i \
                              WHERE i.device_id = d.id AND i.phase <> 'committed') \
                      OR EXISTS (SELECT 1 FROM agent_lease_deputies l WHERE l.holder = d.id) \
                      AS held \
               FROM agent_sync_devices d WHERE d.revoked_at IS NOT NULL \
             ) removed WHERE held OR id IN ($1, $2, $3) ORDER BY id",
            vec![holder.into(), with.into(), from.into()],
        ))
        .await?;
    let mut unsettled = Vec::new();
    for row in rows {
        let id: String = row.try_get("", "id")?;
        let removed_at: DateTime<FixedOffset> = row.try_get("", "revoked_at")?;
        let holds = lease
            .as_ref()
            .is_some_and(|lease| What::left(lease, &id, removed_at.with_timezone(&Utc)).is_some());
        if holds || row.try_get("", "held")? {
            unsettled.push(id);
        }
    }
    Ok(unsettled)
}

/// What of her lease a removed device still holds.
enum What<'a> {
    /// Her way into a site that had none, from it.
    Arriving,
    /// She lived on it: where she is now is not known here.
    Home,
    /// A move to it, a merge from it, or a reclaim it was sending or from
    /// it: called off, she is where she was.
    Moving(&'a Move),
    /// Her mind, while she lives here.
    Mind,
}

impl<'a> What<'a> {
    fn left(lease: &'a Lease, device: &str, removed_at: DateTime<Utc>) -> Option<Self> {
        let moving = lease.moving.as_ref();
        // A reclaim from it that began after it was removed is how she is
        // taken back from a device that is gone: it goes on, and what is left
        // of it here is settled if that is called off.
        let with_it = moving.filter(|moving| {
            moving.to == device
                || moving.device() == device
                || (moving.from == device && moving.started_at <= removed_at)
        });
        let arriving =
            with_it.is_some_and(|moving| import::is_import(&moving.id) && moving.was.is_none());
        if lease.lives_on(device) {
            return match (moving, with_it) {
                (Some(_), Some(_)) if arriving => Some(Self::Arriving),
                (None, _) | (_, Some(_)) => Some(Self::Home),
                (Some(_), None) => None,
            };
        }
        if let Some(moving) = with_it {
            return Some(Self::Moving(moving));
        }
        (lease.state == State::Live && lease.holder == device && !lease.frozen)
            .then_some(Self::Mind)
    }
}

/// The device was removed at `removed_at`: the imports it was sending go, a
/// move with it is called off (with the import that carries her, whichever
/// device sends that), the device she lived on leaves the site not
/// knowing where she is (vacant), her mind on it comes back to the site
/// while she lives here; nothing of her comes back to life here otherwise.
/// In the caller's transaction, which holds the imports already.
pub(super) async fn device_gone<C: ConnectionTrait>(
    db: &C,
    device: &str,
    removed_at: DateTime<Utc>,
) -> Result<Ended, DbErr> {
    rows::lock(db).await?;
    let mut ended = Ended {
        back: Back::from(db, device).await?,
        ..Ended::default()
    };
    for row in db
        .query_all_raw(statement(
            "DELETE FROM agent_host_import WHERE device_id = $1 AND phase <> 'committed' \
             RETURNING id",
            vec![device.into()],
        ))
        .await?
    {
        ended.imports.push(row.try_get("", "id")?);
    }
    let Some(lease) = super::read(db).await? else {
        return Ok(ended);
    };
    let state = match What::left(&lease, device, removed_at) {
        None => return Ok(ended),
        // Called off, the site as it was (no her).
        Some(What::Arriving) => {
            rows::hold_still(db, false, None).await?;
            rows::forget_copy(db).await?;
            return Ok(ended);
        }
        Some(What::Mind) => {
            rows::to_site(db, None).await?;
            return Ok(ended);
        }
        Some(What::Home) => State::Vacant,
        Some(What::Moving(moving)) => moving.was.unwrap_or(lease.state),
    };
    if let Some(moving) = &lease.moving {
        if let Some(import) = &moving.import {
            ended.import(db, import).await?;
        }
        ended.moving = Some(moving.id.clone());
        rows::drop_move(db).await?;
    }
    rows::set_copy(db, state).await?;
    rows::hold_still(db, false, None).await?;
    Ok(ended)
}
