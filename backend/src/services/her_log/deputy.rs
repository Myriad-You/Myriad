//! Her temporary host, the server's side: the deputies it gives.
//!
//! A host core that may go on as her when it loses the server does so under
//! a deputy the server gave it ahead of time ([`Deputy`]): which end, under
//! which epoch, from the server's clock as it gave it until when, and where
//! it stood among those given then. Only the server gives them, and it keeps
//! every one it gave ([`issue`]); her rows are placed against these records
//! alone ([`for_holders`]): a deputy an end shows that is not here counts
//! for nothing.
//!
//! One goes, while her mind is on the site, to an end whose link ([`Watch`])
//! - is a host core's (a computer: it keeps her log),
//! - listed [`CAPABILITY`] in its hello, and
//! - keeps up with her log: of the owner's private talk and of her own, its
//!   last `log.vector` lacks nothing the server has.
//!
//! Once on every link, as soon as its log has caught up, and again every
//! hour after ([`RENEW_EVERY_MICROS`]) or when another epoch stands; each
//! runs a day ([`RUNS_MICROS`]). Where it stands among the others
//! ([`order`]) is who the owner used last, the way a call picks its end
//! (`link::pick`); for now that is only shown.
//!
//! One goes again, the same, until its end's `lease.taken` names it
//! ([`received`]): the newest it was given, while it is for the epoch that
//! stands and has not run out ([`unreceived`]). On a link, once its end
//! keeps up, before any new one; then every [`RESEND_EVERY_MICROS`] its
//! heartbeat finds it still not taken. One a newer has replaced, or run out,
//! does not go again.
//!
//! An end removed takes its deputies with it ([`withdraw`]), and syncing
//! turned off takes them all ([`withdraw_all`]): the reason they were given
//! is gone. That she knows she is a temporary host is the end's (its host
//! core's), not the server's.

use std::collections::{BTreeMap, BTreeSet};

pub use myriad_persona_log::lease::Deputy;
use myriad_persona_sync::Rev;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, TransactionTrait,
};

use super::{Error, NODE, Owner, lease, store};

/// What an end lists in its hello when it can go on as her temporary host.
pub const CAPABILITY: &str = "deputy";

/// The frame a deputy goes to its end in (reliable; counted delivered only
/// by the end's `lease.taken`, never by an ack).
pub const FRAME: &str = "lease.deputy";

/// How long a deputy runs from its issue: a day, the longest the rule
/// allows (`timing.deputyMaxMs`). Past that her memory there is no longer
/// settled at night and her last backup is older than what she wrote there.
pub const RUNS_MICROS: i64 = myriad_persona_log::lease::DEPUTY_MAX_MICROS;

/// How often an end that keeps up is given a new one on the same link.
pub const RENEW_EVERY_MICROS: i64 = 60 * 60 * 1_000_000;

/// How often one not taken yet goes again on the same link: a few
/// heartbeats, long enough for its `lease.taken` to come back.
pub const RESEND_EVERY_MICROS: i64 = 60 * 1_000_000;

/// The id the deputy's frame goes by, made once per deputy and the same if
/// it goes again: its clock, which [`issued_of`] reads back.
pub fn frame_id(deputy: &Deputy) -> String {
    format!("dp_{}_{}", deputy.issued.wall_ms, deputy.issued.counter)
}

/// The clock of the deputy a frame id names ([`frame_id`]): the server's.
pub fn issued_of(id: &str) -> Option<Rev> {
    let (wall_ms, counter) = id.strip_prefix("dp_")?.split_once('_')?;
    Rev::new(wall_ms.parse().ok()?, counter.parse().ok()?, NODE).ok()
}

/// What one link's end has shown of keeping up with her log, and what it
/// was given on that link.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watch {
    /// By audience it must keep up with: how many rows the server has that
    /// its last `log.vector` lacked.
    lacking: BTreeMap<String, u64>,
    /// The server's time (microseconds) it was last given one on this link,
    /// and the epoch that one stands under.
    given: Option<(i64, u64)>,
    /// The one sent on this link its end has not said it took, and when it
    /// last went.
    waiting: Option<(Rev, i64)>,
    /// Whether one not taken from before was looked for on this link.
    looked: bool,
}

impl Watch {
    /// The audiences an end must keep up with: the owner's private talk and
    /// her own.
    fn audiences(owner: &Owner) -> [String; 2] {
        [owner.personal(), Owner::OWN.to_string()]
    }

    /// Its last `log.vector` of `audience` showed `theirs`, where the server
    /// has `ours`. Only the audiences it must keep up with are kept.
    pub fn heard(
        &mut self,
        owner: &Owner,
        audience: &str,
        theirs: &BTreeMap<String, u64>,
        ours: &BTreeMap<String, u64>,
    ) {
        if !Self::audiences(owner).iter().any(|kept| kept == audience) {
            return;
        }
        let lacking = ours
            .iter()
            .map(|(origin, have)| have.saturating_sub(theirs.get(origin).copied().unwrap_or(0)))
            .sum();
        self.lacking.insert(audience.to_string(), lacking);
    }

    /// Whether, by what it last showed, it has all the server has of both
    /// (only those two are kept: [`Self::heard`]).
    pub fn keeps_up(&self) -> bool {
        self.lacking.len() == 2 && self.lacking.values().all(|lacking| *lacking == 0)
    }

    /// Whether one goes to it now, to stand under `under`, at the server's
    /// time `now`: it keeps up, and on this link it has none yet, or one
    /// under another epoch (a grant was issued since), or one given an hour
    /// ago or more.
    pub fn due(&self, under: u64, now: i64) -> bool {
        self.keeps_up()
            && self.given.is_none_or(|(at, given_under)| {
                given_under != under || now.saturating_sub(at) >= RENEW_EVERY_MICROS
            })
    }

    /// Whether the one not taken yet goes again now, at the server's time
    /// `now`: it keeps up, and on this link none was looked for yet, or the
    /// one sent waits a [`RESEND_EVERY_MICROS`] or more.
    pub fn again_due(&self, now: i64) -> bool {
        self.keeps_up()
            && (!self.looked
                || self
                    .waiting
                    .as_ref()
                    .is_some_and(|(_, at)| now.saturating_sub(*at) >= RESEND_EVERY_MICROS))
    }

    /// Its end said it took the one issued at `issued`.
    pub fn taken(&mut self, issued: &Rev) {
        if self
            .waiting
            .as_ref()
            .is_some_and(|(waiting, _)| waiting == issued)
        {
            self.waiting = None;
        }
    }

    fn gave(&mut self, deputy: &Deputy, now: i64) {
        self.given = Some((now, deputy.under));
        self.sent(Some(deputy), now);
    }

    fn sent(&mut self, deputy: Option<&Deputy>, now: i64) {
        self.waiting = deputy.map(|deputy| (deputy.issued.clone(), now));
        self.looked = true;
    }
}

fn deputy_of(row: QueryResult) -> Result<Deputy, Error> {
    let issued: String = row.try_get("", "issued")?;
    Ok(Deputy {
        holder: row.try_get("", "holder")?,
        under: row.try_get::<i64>("", "under")? as u64,
        issued: issued
            .parse()
            .map_err(|_| Error::Refused("a kept deputy has no clock".into()))?,
        until: row.try_get("", "until_micros")?,
        order: row.try_get::<i32>("", "ord")? as u32,
    })
}

const COLUMNS: &str = "holder, issued, under, until_micros, ord";

/// A deputy issued now, at the server's time `now` (microseconds), for
/// `holder` under the epoch `under`, standing `order`th: kept, then given
/// back to be sent. In the caller's transaction.
pub async fn issue<C: ConnectionTrait>(
    db: &C,
    holder: &str,
    under: u64,
    order: u32,
) -> Result<Deputy, Error> {
    if holder.is_empty() || under == 0 || order == 0 {
        return Err(Error::Refused("a deputy names an end, an epoch and an order".into()));
    }
    let issued = store::next_rev(db).await?;
    let from = i64::try_from(issued.wall_ms)
        .unwrap_or(i64::MAX)
        .saturating_mul(1000);
    let deputy = Deputy {
        holder: holder.to_string(),
        under,
        until: from.saturating_add(RUNS_MICROS),
        issued,
        order,
    };
    deputy.check()?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_lease_deputies \
         (holder, issued, under, until_micros, ord, issued_at) \
         VALUES ($1, $2, $3, $4, $5, $6)",
        [
            deputy.holder.clone().into(),
            deputy.issued.to_string().into(),
            (deputy.under as i64).into(),
            deputy.until.into(),
            (deputy.order as i32).into(),
            crate::clock::now().into(),
        ],
    ))
    .await?;
    Ok(deputy)
}

/// Every deputy issued to any of `holders`, oldest first: what her rows
/// from them are placed against.
pub async fn for_holders<C: ConnectionTrait>(
    db: &C,
    holders: &[&str],
) -> Result<Vec<Deputy>, Error> {
    if holders.is_empty() {
        return Ok(Vec::new());
    }
    let holders: Vec<String> = holders.iter().map(|holder| holder.to_string()).collect();
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        format!(
            "SELECT {COLUMNS} FROM agent_lease_deputies WHERE holder = ANY($1) \
             ORDER BY issued_at, holder"
        ),
        [holders.into()],
    ))
    .await?
    .into_iter()
    .map(deputy_of)
    .collect()
}

/// The ends holding a deputy that has not run out at `now` (microseconds).
async fn running<C: ConnectionTrait>(db: &C, now: i64) -> Result<Vec<String>, Error> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT DISTINCT holder FROM agent_lease_deputies WHERE until_micros > $1 \
         ORDER BY holder",
        [now.into()],
    ))
    .await?
    .into_iter()
    .map(|row| Ok(row.try_get("", "holder")?))
    .collect()
}

/// Where `device` stands, from 1, among it and the other ends whose deputy
/// still runs at `now`: by who the owner used last (`link::pick`).
pub async fn order<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    device: &str,
    now: i64,
) -> Result<u32, Error> {
    let mut ends: BTreeSet<String> = running(db, now).await?.into_iter().collect();
    ends.insert(device.to_string());
    let ends: Vec<String> = ends.into_iter().collect();
    let ranked = crate::services::agent::link::ranked_devices(owner, &ends);
    let at = ranked
        .iter()
        .position(|end| end.as_str() == device)
        .unwrap_or(ranked.len());
    Ok(u32::try_from(at + 1).unwrap_or(u32::MAX))
}

/// A deputy for `device`, when one is due on its link (`watch`) at the
/// server's time `now` and her mind is on the site: issued and kept, and
/// given back to be sent. None otherwise.
pub async fn give(
    db: &DatabaseConnection,
    owner: &Owner,
    device: &str,
    watch: &mut Watch,
    now: i64,
) -> Result<Option<Deputy>, Error> {
    if !watch.keeps_up() {
        return Ok(None);
    }
    let txn = db.begin().await?;
    // With her lease: no grant moves while one is given.
    lease::lock(&txn).await?;
    let standing = match lease::standing(&txn).await? {
        Some(standing) if standing.held_by(NODE) && watch.due(standing.epoch, now) => {
            standing
        }
        _ => {
            txn.commit().await?;
            return Ok(None);
        }
    };
    let order = order(&txn, owner.user, device, now).await?;
    let deputy = issue(&txn, device, standing.epoch, order).await?;
    txn.commit().await?;
    watch.gave(&deputy, now);
    Ok(Some(deputy))
}

/// The newest deputy issued to `holder`, when its end has not said it took
/// it, it is for the epoch that stands while her mind is on the site, and
/// it runs at `now` (microseconds): the one that goes again. None when a
/// newer replaced it, or it was taken, or it ran out.
pub async fn unreceived<C: ConnectionTrait>(
    db: &C,
    holder: &str,
    now: i64,
) -> Result<Option<Deputy>, Error> {
    let Some(standing) = lease::standing(db).await? else {
        return Ok(None);
    };
    if !standing.held_by(NODE) {
        return Ok(None);
    }
    let Some(row) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT {COLUMNS}, received_at IS NOT NULL AS taken FROM agent_lease_deputies \
                 WHERE holder = $1 ORDER BY issued_at DESC, issued DESC LIMIT 1"
            ),
            [holder.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let taken: bool = row.try_get("", "taken")?;
    let deputy = deputy_of(row)?;
    Ok((!taken && deputy.under == standing.epoch && deputy.until > now).then_some(deputy))
}

/// The deputy `device` was given and has not said it took, when it goes
/// again on its link (`watch`) at the server's time `now`
/// ([`Watch::again_due`], [`unreceived`]): the same, by the same id. None
/// otherwise.
pub async fn again(
    db: &DatabaseConnection,
    device: &str,
    watch: &mut Watch,
    now: i64,
) -> Result<Option<Deputy>, Error> {
    if !watch.again_due(now) {
        return Ok(None);
    }
    let deputy = unreceived(db, device, now).await?;
    watch.sent(deputy.as_ref(), now);
    Ok(deputy)
}

/// `holder` said it took the deputy issued at `issued`. False when there is
/// no such deputy of its (one the server never gave, or gone since).
pub async fn received<C: ConnectionTrait>(
    db: &C,
    holder: &str,
    issued: &Rev,
) -> Result<bool, Error> {
    let done = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_lease_deputies SET received_at = COALESCE(received_at, $3) \
             WHERE holder = $1 AND issued = $2",
            [
                holder.into(),
                issued.to_string().into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
    Ok(done.rows_affected() > 0)
}

/// Every deputy of `holder` goes: the end was removed. How many went.
pub async fn withdraw<C: ConnectionTrait>(db: &C, holder: &str) -> Result<u64, Error> {
    let done = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM agent_lease_deputies WHERE holder = $1",
            [holder.into()],
        ))
        .await?;
    Ok(done.rows_affected())
}

/// Every deputy goes: syncing her was turned off. How many went.
pub async fn withdraw_all<C: ConnectionTrait>(db: &C) -> Result<u64, Error> {
    let done = db
        .execute_unprepared("DELETE FROM agent_lease_deputies")
        .await?;
    Ok(done.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> Owner {
        Owner {
            user: 1,
            partner: super::super::test_partner::PARTNER.to_string(),
        }
    }

    fn vector(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
        pairs
            .iter()
            .map(|(origin, count)| (origin.to_string(), *count))
            .collect()
    }

    #[test]
    fn an_end_keeps_up_only_once_both_its_audiences_lack_nothing() {
        let owner = owner();
        let mut watch = Watch::default();
        let ours = vector(&[("site", 4), ("dev_a", 2)]);
        watch.heard(&owner, &owner.personal(), &ours, &ours);
        // Her own not heard yet.
        assert!(!watch.keeps_up());
        watch.heard(&owner, Owner::OWN, &vector(&[("site", 3)]), &ours);
        assert!(!watch.keeps_up());
        // A group's says nothing of it.
        watch.heard(&owner, "group:qq:1", &BTreeMap::new(), &ours);
        watch.heard(&owner, Owner::OWN, &vector(&[("site", 4), ("dev_a", 2)]), &ours);
        assert!(watch.keeps_up());
        // Past the server is not behind.
        watch.heard(&owner, Owner::OWN, &vector(&[("site", 9), ("dev_a", 2)]), &ours);
        assert!(watch.keeps_up());
        watch.heard(&owner, &owner.personal(), &vector(&[("site", 4)]), &ours);
        assert!(!watch.keeps_up());
    }

    #[test]
    fn a_frame_id_names_the_deputys_clock() {
        let deputy = Deputy {
            holder: "dev_a".into(),
            under: 3,
            issued: Rev::new(1_763_500_000_123, 7, NODE).expect("a clock"),
            until: 1_763_500_000_123_000 + RUNS_MICROS,
            order: 1,
        };
        let id = frame_id(&deputy);
        assert_eq!(id, "dp_1763500000123_7");
        assert!(myriad_her_link::valid_id(&id));
        assert_eq!(issued_of(&id), Some(deputy.issued));
        assert_eq!(issued_of("dp_1"), None);
        assert_eq!(issued_of("rows_1_2"), None);
    }

    #[test]
    fn one_is_due_once_a_link_and_again_each_hour_or_for_another_epoch() {
        let owner = owner();
        let mut watch = Watch::default();
        let ours = vector(&[("site", 1)]);
        assert!(!watch.due(3, 0));
        watch.heard(&owner, &owner.personal(), &ours, &ours);
        watch.heard(&owner, Owner::OWN, &ours, &ours);
        assert!(watch.due(3, 0));
        let deputy = Deputy {
            holder: "dev_a".into(),
            under: 3,
            issued: Rev::new(1, 0, NODE).expect("a clock"),
            until: RUNS_MICROS,
            order: 1,
        };
        watch.gave(&deputy, 10);
        assert!(!watch.due(3, 10 + RENEW_EVERY_MICROS - 1));
        assert!(watch.due(3, 10 + RENEW_EVERY_MICROS));
        assert!(watch.due(4, 11));
        // Behind again: none, whatever the hour.
        watch.heard(&owner, Owner::OWN, &BTreeMap::new(), &ours);
        assert!(!watch.due(3, 10 + 2 * RENEW_EVERY_MICROS));
    }
}
