//! What a removed device leaves of her: an import it was sending, a move with
//! it, her living on it, her mind on it, a deputy given to it as it was
//! removed. Removing it takes its token and its deputies back first, on their
//! own ([`devices::revoke`]); this comes after, in one transaction, in the
//! order of [`order`](super::order). Until it goes
//! through, all of that stands as it did, and it is done again: when the
//! device is removed once more or forgotten, and from time to time by the
//! listener ([`settle_all`]). Done twice it changes nothing.

use std::time::Duration;

use chrono::{DateTime, FixedOffset, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, TransactionSession, TransactionTrait};

use super::{Lease, Move, State, rows, statement};
use crate::services::persona_sync::devices;

/// How long it waits for what another holds (an import landing) before it is
/// left for later.
const WAIT: Duration = Duration::from_secs(1);

/// Settles what `device` left of her, if it is a removed one.
pub(in crate::services::persona_sync) async fn settle<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    device: &str,
) -> Result<(), DbErr> {
    settle_within(db, device, WAIT).await
}

pub(super) async fn settle_within<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    device: &str,
    wait: Duration,
) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    txn.execute_unprepared(&format!("SET LOCAL lock_timeout = {}", wait.as_millis()))
        .await?;
    // The imports it was sending go with it, and then her lease moves.
    crate::services::persona_sync::import::hold_all(&txn).await?;
    let Some(removed_at) = txn
        .query_one_raw(statement(
            "SELECT revoked_at FROM agent_sync_devices WHERE id = $1 AND revoked_at IS NOT NULL",
            vec![device.into()],
        ))
        .await?
        .map(|row| row.try_get::<DateTime<FixedOffset>>("", "revoked_at"))
        .transpose()?
    else {
        return Ok(());
    };
    device_gone(&txn, device, removed_at.with_timezone(&Utc)).await?;
    devices::withdraw_deputies(&txn, device).await?;
    txn.commit().await
}

/// Settles every removed device that still holds something of her. One that
/// cannot be now is left for the next time.
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
        let arriving = with_it.is_some_and(|moving| {
            crate::services::persona_sync::import::is_import(&moving.id) && moving.was.is_none()
        });
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
/// move with it is called off, the device she lived on leaves the site not
/// knowing where she is (vacant), her mind on it comes back to the site
/// while she lives here; nothing of her comes back to life here otherwise.
/// In the caller's transaction, which holds the imports already.
pub(super) async fn device_gone<C: ConnectionTrait>(
    db: &C,
    device: &str,
    removed_at: DateTime<Utc>,
) -> Result<(), DbErr> {
    rows::lock(db).await?;
    db.execute_raw(statement(
        "DELETE FROM agent_host_import WHERE device_id = $1 AND phase <> 'committed'",
        vec![device.into()],
    ))
    .await?;
    let Some(lease) = super::read(db).await? else {
        return Ok(());
    };
    let state = match What::left(&lease, device, removed_at) {
        None => return Ok(()),
        // Called off, the site as it was (no her).
        Some(What::Arriving) => {
            rows::hold_still(db, false, None).await?;
            return rows::forget_copy(db).await;
        }
        Some(What::Mind) => return rows::to_site(db, None).await,
        Some(What::Home) => State::Vacant,
        Some(What::Moving(moving)) => moving.was.unwrap_or(lease.state),
    };
    if lease.moving.is_some() {
        rows::drop_move(db).await?;
    }
    rows::set_copy(db, state).await?;
    rows::hold_still(db, false, None).await
}
