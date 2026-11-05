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
//! up first, then thinks. With her log on the server off, or the owner not
//! syncing, there is no lease in play and she thinks here as she always did.
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
use sea_orm::{ConnectionTrait, DatabaseConnection};
use tokio::sync::Notify;

use super::{Error, NODE, Settings, lease, store};

/// The longest a handover away from the server waits for thoughts in hand.
pub const HANDOVER_WAIT: Duration = Duration::from_secs(120);

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
        .map(|earlier| earlier.holder.clone())
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

/// What the pen is under `grant`, the standing one of `history`.
async fn of_grant<C: ConnectionTrait>(
    db: &C,
    owner: i32,
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
            holder: grant.holder.clone(),
            epoch: grant.epoch,
        });
    }
    let (Some(through), How::Handover) = (&grant.through, grant.how) else {
        return Ok(Pen::Here);
    };
    let Some(from) = from_of(history, grant) else {
        return Ok(Pen::Here);
    };
    let behind = rules::behind(Some(through), &from, &held(db, owner, through).await?);
    Ok(if behind.is_empty() {
        Pen::Here
    } else {
        Pen::CatchingUp {
            epoch: grant.epoch,
            from,
            through: through.clone(),
            behind,
        }
    })
}

/// Whether the lease is in play at all: her log on the server on, and the
/// site owner (`owner`) syncing it.
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
    let pen = match owner {
        Some(owner) if in_play(db, settings, Some(owner)).await? => {
            let history = lease::history(db).await?;
            match history.last() {
                None => Pen::Here,
                Some(grant) => of_grant(db, owner, &history, grant, settings, super::now()).await?,
            }
        }
        _ => Pen::Here,
    };
    if !with_handing(|handing| handing.load(Ordering::SeqCst)) {
        set(pen.clone());
    }
    Ok(pen)
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    with_handing(|handing| handing.store(false, Ordering::SeqCst));
    set(Pen::Here);
}
