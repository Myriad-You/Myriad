//! Whether her one mind is on the server now: the site's side of the lease.
//!
//! Every place the server would think as her (answer the owner, speak up
//! unasked, answer a group, her own time, her nightly settling, Work she
//! starts herself) asks [`think`] first and does nothing when it answers
//! `None`. That is the whole switch: while another host core (a Mac) holds
//! the lease she is there, and the server only keeps what arrives (the
//! owner's words, a group's messages, lent to her mind there), never decides
//! anything as her.
//!
//! The server thinks when `myriad_persona_log::lease::here` says so for
//! [`NODE`] and, after a handover to it, once it holds everything the
//! previous holder wrote up to the handover (`lease::behind`): she catches
//! up first, then thinks, unless nothing of it arrives for
//! [`CATCH_UP_WAIT_MICROS`]; then she thinks with what came. The grants
//! are always there (her moves house sign theirs too), so this holds with
//! her log off as well; only the catching up needs her log on and the owner
//! syncing it ([`in_play`]): without it there is nothing to catch up on.
//! While the site's copy of her is not the live one (she lives on a device,
//! or is not here) the site is not her at all, and the pen stays here:
//! `persona_sync::lease` keeps her out.
//!
//! A handover away from the server waits for what is being thought here to
//! end ([`Thought`]): from the moment it is asked nothing new starts, and the
//! grant goes out once the last thought is done, so she never thinks in two
//! places at once. A thought that outlasts [`HANDOVER_WAIT`] does not hold
//! her here: the grant goes out, and what it still writes as her is refused
//! by her log (`store::write_hers` checks the server holds the lease).
//!
//! The state is kept in the process; every process reads it again from the
//! grants when one is kept (`persona_sync::lease`'s listener).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{LazyLock, RwLock};
use std::time::Duration;

use myriad_persona_log::{Grant, How, Through, lease as rules};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use tokio::sync::Notify;

use super::{Error, NODE, Settings, lease, store};

/// The longest a handover away from the server waits for thoughts in hand.
pub const HANDOVER_WAIT: Duration = Duration::from_secs(120);

/// How long the server waits, with nothing of it arriving, for what the
/// holder that gave her back said it had written: past it she thinks here
/// with what came. Each row of it that lands starts the wait again, so a
/// slow upload is never cut short; one that will not come (its end gone,
/// asleep, or never sending those rows) no longer leaves her nowhere. What
/// comes later merges as any late row of that end, and she stays here: a
/// wait that once ran out stays run out. Rows of that end past what it
/// promised do not count, so its owner talking on there does not hold her.
pub const CATCH_UP_WAIT_MICROS: i64 = 2 * 60 * 1_000_000;

/// Where her mind is, as the server sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pen {
    /// On the server: she thinks here.
    Here,
    /// Handed to the server, which still lacks some of what the previous
    /// holder wrote: `behind` is what it waits for, per audience.
    CatchingUp {
        epoch: u64,
        from: String,
        through: Through,
        behind: Through,
    },
    /// Given away, or being given: nothing new starts here.
    Away {
        /// The core it is on (or going to).
        holder: String,
        epoch: u64,
    },
}

impl Pen {
    pub fn here(&self) -> bool {
        matches!(self, Self::Here)
    }
}

static PEN: LazyLock<RwLock<Pen>> = LazyLock::new(|| RwLock::new(Pen::Here));
/// Thoughts in hand here.
static THINKING: AtomicUsize = AtomicUsize::new(0);
/// Told when the last thought in hand ends.
static IDLE: LazyLock<Notify> = LazyLock::new(Notify::new);
/// A handover away from the server is being waited out: nothing reads the
/// pen back to here until it went out or did not.
static HANDING: AtomicBool = AtomicBool::new(false);

// Tests run side by side in one process, each on its own thread: each has
// its own pen, so one test's her on a Mac stops nothing of another's.
#[cfg(test)]
thread_local! {
    static TEST_PEN: std::cell::RefCell<Pen> = const { std::cell::RefCell::new(Pen::Here) };
    static TEST_THINKING: AtomicUsize = const { AtomicUsize::new(0) };
    static TEST_HANDING: AtomicBool = const { AtomicBool::new(false) };
}

fn with_thinking<T>(act: impl FnOnce(&AtomicUsize) -> T) -> T {
    #[cfg(test)]
    {
        let _ = &THINKING;
        TEST_THINKING.with(act)
    }
    #[cfg(not(test))]
    act(&THINKING)
}

fn with_handing<T>(act: impl FnOnce(&AtomicBool) -> T) -> T {
    #[cfg(test)]
    {
        let _ = &HANDING;
        TEST_HANDING.with(act)
    }
    #[cfg(not(test))]
    act(&HANDING)
}

/// Where her mind is now.
pub fn current() -> Pen {
    #[cfg(test)]
    {
        let _ = &PEN;
        TEST_PEN.with(|pen| pen.borrow().clone())
    }
    #[cfg(not(test))]
    PEN.read().map(|pen| pen.clone()).unwrap_or(Pen::Here)
}

fn set(pen: Pen) {
    let before = current();
    if before == pen {
        return;
    }
    tracing::info!(pen = ?pen, "[her-log] her mind moved");
    #[cfg(test)]
    TEST_PEN.with(|held| *held.borrow_mut() = pen);
    #[cfg(not(test))]
    if let Ok(mut held) = PEN.write() {
        *held = pen;
    }
    crate::services::persona_sync::lease::mind_moved();
}

/// Whether she thinks here now.
pub fn here() -> bool {
    current().here()
}

/// One thought of hers in hand on the server. While any is held a handover
/// away waits; dropping the last one lets it go.
#[derive(Debug)]
pub struct Thought(());

impl Drop for Thought {
    fn drop(&mut self) {
        if with_thinking(|thinking| thinking.fetch_sub(1, Ordering::SeqCst)) == 1 {
            IDLE.notify_waiters();
        }
    }
}

/// Starts a thought of hers here, or `None` when her mind is not here (on
/// another core, being handed over, or catching up): then the caller does
/// not think, speak or write as her.
pub fn think() -> Option<Thought> {
    // Counted before the pen is read, so a handover that moved the pen
    // first and then found nothing in hand cannot miss this one.
    with_thinking(|thinking| thinking.fetch_add(1, Ordering::SeqCst));
    let thought = Thought(());
    here().then_some(thought)
}

/// How many thoughts are in hand here.
pub fn thinking() -> usize {
    with_thinking(|thinking| thinking.load(Ordering::SeqCst))
}

/// Stops anything new from starting here, for a handover to `holder`, and
/// waits, up to `wait`, for what is in hand to end. Whether it ended in
/// time. [`handed`] ends it, whether the grant went out or not.
pub async fn quiet(holder: &str, epoch: u64, wait: Duration) -> bool {
    with_handing(|handing| handing.store(true, Ordering::SeqCst));
    set(Pen::Away {
        holder: holder.to_string(),
        epoch,
    });
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let idle = IDLE.notified();
        tokio::pin!(idle);
        idle.as_mut().enable();
        if thinking() == 0 {
            return true;
        }
        if tokio::time::timeout_at(deadline, idle).await.is_err() {
            return thinking() == 0;
        }
    }
}

/// The handover [`quiet`] waited for went out or did not: the pen is read
/// from the grants again.
pub async fn handed(
    db: &DatabaseConnection,
    settings: &Settings,
    owner: i32,
) -> Result<Pen, Error> {
    with_handing(|handing| handing.store(false, Ordering::SeqCst));
    settle_for(db, settings, Some(owner)).await
}

/// The previous holder of `grant`: whom a handover to the server came from.
/// The grant names it; one kept from before grants did is read from the
/// grant before it.
fn from_of(history: &[Grant], grant: &Grant) -> Option<String> {
    if let Some(from) = &grant.from {
        return Some(from.clone());
    }
    history
        .iter()
        .rev()
        .find(|earlier| earlier.epoch < grant.epoch)
        .and_then(|earlier| earlier.holder.clone())
}

/// What the server holds of `owner`'s log in each audience of `through`.
async fn held<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    through: &Through,
) -> Result<BTreeMap<String, myriad_persona_log::Vector>, Error> {
    let mut held = BTreeMap::new();
    for audience in through.keys() {
        held.insert(
            audience.clone(),
            store::holdings(db, owner, audience).await?.vector,
        );
    }
    Ok(held)
}

/// What the pen is under `grant`, the standing one of `history`. `owner`
/// when her log is in play, so she catches up on what it holds.
async fn of_grant<C: ConnectionTrait>(
    db: &C,
    owner: Option<i32>,
    history: &[Grant],
    grant: &Grant,
    settings: &Settings,
    now: i64,
) -> Result<Pen, Error> {
    let state = rules::here(rules::HereInput {
        me: NODE.to_string(),
        lease: Some(grant.clone()),
        now,
        disconnected_since: None,
        owner_here_at: None,
        stop_after: Some(settings.stop_after_micros),
        stopped_at: None,
    })?;
    if !matches!(state, myriad_persona_log::Here::Thinking) {
        return Ok(Pen::Away {
            holder: grant.holder.clone().unwrap_or_default(),
            epoch: grant.epoch,
        });
    }
    let (Some(owner), Some(through), How::Handover) = (owner, &grant.through, grant.how) else {
        return Ok(Pen::Here);
    };
    let Some(from) = from_of(history, grant) else {
        return Ok(Pen::Here);
    };
    let held = held(db, owner, through).await?;
    let behind = rules::behind(Some(through), &from, &held);
    if behind.is_empty() {
        return Ok(Pen::Here);
    }
    let wait = wait(db, owner, &from, through, grant).await?;
    if wait.longest >= CATCH_UP_WAIT_MICROS {
        return Ok(Pen::Here);
    }
    if now - wait.last >= CATCH_UP_WAIT_MICROS {
        // Said once, by the process that was waiting: counts and audiences
        // only.
        if matches!(current(), Pen::CatchingUp { epoch, .. } if epoch == grant.epoch) {
            let short: Vec<String> = behind
                .iter()
                .map(|(audience, want)| {
                    let have = held
                        .get(audience)
                        .and_then(|vector| vector.get(&from))
                        .copied()
                        .unwrap_or(0);
                    format!("{audience} {}", want.saturating_sub(have))
                })
                .collect();
            tracing::warn!(
                device = %from,
                epoch = grant.epoch,
                short = ?short,
                waited_secs = (now - wait.last) / 1_000_000,
                "[her-log] what her last holder wrote did not all come: she thinks here without it"
            );
        }
        return Ok(Pen::Here);
    }
    Ok(Pen::CatchingUp {
        epoch: grant.epoch,
        from,
        through: through.clone(),
        behind,
    })
}

/// How the catch-up under `grant` has gone, on the server's clock in
/// microseconds: when it last went forward (`last`: the last row the server
/// took from `from` within what it promised, in any audience it promised
/// rows of, or the grant itself) and the longest it ever stood still
/// (`longest`). Every audience of `through` counts, not only those still
/// short: a device sending one audience after another is going forward
/// while the next waits. Read from the log alone, so a restart and every
/// process read it the same.
struct Wait {
    last: i64,
    longest: i64,
}

async fn wait<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    from: &str,
    through: &Through,
    grant: &Grant,
) -> Result<Wait, Error> {
    let (audiences, promised): (Vec<String>, Vec<i64>) = through
        .iter()
        .map(|(audience, upto)| (audience.clone(), i64::try_from(*upto).unwrap_or(i64::MAX)))
        .unzip();
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "WITH start AS (SELECT created_at AS at FROM agent_lease_grants WHERE epoch = $3), \
             came AS ( \
                 SELECT at FROM start \
                 UNION ALL \
                 SELECT r.received_at FROM agent_log_rows r \
                 JOIN unnest($4::text[], $5::bigint[]) AS promised(audience, upto) \
                   ON r.audience = promised.audience AND r.dseq <= promised.upto \
                 WHERE r.user_id = $1 AND r.origin = $2 AND r.received_at > (SELECT at FROM start) \
             ), \
             apart AS (SELECT at, at - LAG(at) OVER (ORDER BY at) AS still FROM came) \
             SELECT (EXTRACT(EPOCH FROM MAX(at)) * 1000000)::BIGINT AS last, \
                    COALESCE((EXTRACT(EPOCH FROM MAX(still)) * 1000000)::BIGINT, 0) AS longest \
             FROM apart",
            [
                owner.into(),
                from.into(),
                (grant.epoch as i64).into(),
                audiences.into(),
                promised.into(),
            ],
        ))
        .await?;
    let issued = grant.issued.wall_ms as i64 * 1000;
    let (last, longest) = match row {
        Some(row) => (
            row.try_get::<Option<i64>>("", "last")?,
            row.try_get::<i64>("", "longest")?,
        ),
        None => (None, 0),
    };
    Ok(Wait {
        last: last.unwrap_or(issued),
        longest,
    })
}

/// Whether her log is in play: her log on the server on, and the site owner
/// (`owner`) syncing it. Only then does she catch up, and only then do the
/// lease's own frames flow on her link (`sync`).
pub async fn in_play<C: ConnectionTrait>(
    db: &C,
    settings: &Settings,
    owner: Option<i32>,
) -> Result<bool, Error> {
    if !settings.enabled || super::REFUSED.load(Ordering::Relaxed) {
        return Ok(false);
    }
    let Some(owner) = owner else {
        return Ok(false);
    };
    Ok(store::account(db, owner)
        .await?
        .is_some_and(|account| account.on()))
}

/// Reads where her mind is from the grants kept: at start, and after the
/// lease moved or rows arrived that a catch-up waits for.
pub async fn settle(db: &DatabaseConnection, settings: &Settings) -> Result<Pen, Error> {
    let owner = crate::services::principal::site_owner_id(db).await?;
    settle_for(db, settings, owner).await
}

/// [`settle`], the site owner known.
pub async fn settle_for(
    db: &DatabaseConnection,
    settings: &Settings,
    owner: Option<i32>,
) -> Result<Pen, Error> {
    settle_for_at(db, settings, owner, super::now()).await
}

/// [`settle_for`] at the server's time `now`.
pub(crate) async fn settle_for_at(
    db: &DatabaseConnection,
    settings: &Settings,
    owner: Option<i32>,
    now: i64,
) -> Result<Pen, Error> {
    let pen = if copy_live(db).await? {
        let catching = match owner {
            Some(owner) if in_play(db, settings, Some(owner)).await? => Some(owner),
            _ => None,
        };
        let history = lease::history(db).await?;
        match history.last() {
            None => Pen::Here,
            Some(grant) => of_grant(db, catching, &history, grant, settings, now).await?,
        }
    } else {
        Pen::Here
    };
    if !with_handing(|handing| handing.load(Ordering::SeqCst)) {
        set(pen.clone());
    }
    Ok(pen)
}

/// Whether the site's copy of her is the live one, or there is none yet.
async fn copy_live<C: ConnectionTrait>(db: &C) -> Result<bool, Error> {
    Ok(crate::services::persona_sync::lease::copy_state(db)
        .await?
        .is_none_or(|state| state == crate::services::persona_sync::lease::State::Live))
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    with_handing(|handing| handing.store(false, Ordering::SeqCst));
    set(Pen::Here);
}
