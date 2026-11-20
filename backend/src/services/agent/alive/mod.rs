//! Whether she is living now, in one place: who her host is, when each
//! driver of her own life last ran or was passed over, how her memory's
//! line is doing, which ends are linked and how far behind her log each
//! is, and what the ends asked of each other in the last day.
//!
//! Only times, counts, names and versions are kept here: never a word
//! anyone said, a memory, or a key. Kept in memory, written once a minute
//! as one row of the runtime registry so a restart does not forget, and
//! read by `GET /api/agent/alive`. [`watch`] is the one alarm on top of it.

pub(crate) mod watch;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod troubles_tests;

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::services::persona_sync::lease::{self, SITE, Where};

pub const NAMESPACE: &str = "her_alive";
const RECORD: &str = "state";
const KEEP_DAYS: i64 = 400;
/// How often what is kept here is written down, in looks of the watch.
const SAVED_EVERY_LOOKS: u64 = 4;
/// How often the watch looks.
pub const LOOK_SECS: u64 = 15;
/// What an end may declare is its own words: shown, so bounded.
const DECLARED_MAX: usize = 64;
const DECLARED_CHARS: usize = 48;
const CALL_HOURS_KEPT: i64 = 24;

/// One driver of her own life.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Beat {
    /// Seconds between its ticks; none for one that runs when something
    /// happens.
    #[serde(skip)]
    pub every: Option<u64>,
    /// When it was last let through, and how many times.
    pub ran_at: Option<i64>,
    pub runs: u64,
    /// When a tick let through last ended, with or without anything to do.
    pub ended_at: Option<i64>,
    /// When a tick last broke off (a panic), and how many did.
    pub failed_at: Option<i64>,
    pub failures: u64,
    /// When it was last passed over, why, and how many times since it ran.
    pub skipped_at: Option<i64>,
    pub skipped_why: Option<String>,
    pub skips: u64,
}

/// Her memory's line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Memory {
    /// The last stretch of talk gone over once it was over, and how it came
    /// out (the queue's own word for it).
    pub settled_at: Option<i64>,
    pub settled_outcome: Option<String>,
    /// Calls to the model for how their life stands now that failed.
    pub standing_failed: u64,
    pub standing_failed_at: Option<i64>,
}

/// Whether her model answers her: her calls through `merope::call`, an
/// answer that cannot be read counted as an answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Answers {
    /// When it last answered.
    pub answered_at: Option<i64>,
    /// Calls in a row with no answer since, and since when.
    pub unanswered_in_a_row: u64,
    pub unanswered_since: Option<i64>,
}

/// One end that said hello.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct End {
    pub name: String,
    pub protocol: u32,
    /// The app there and its version, when its hello said.
    pub app: Option<(String, String)>,
    /// What its hello listed, as it listed it.
    pub declared: Vec<String>,
    /// What the site took of that: what it may use on this link.
    pub granted: Vec<String>,
    pub linked_at: i64,
    /// How many rows of her log it is behind, by audience, and when it
    /// last said how far it is. Not written down: the audiences are names
    /// of who she talks with.
    #[serde(skip)]
    pub behind: BTreeMap<String, u64>,
    #[serde(skip)]
    pub heard_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub drivers: BTreeMap<String, Beat>,
    pub memory: Memory,
    pub ends: BTreeMap<String, End>,
    /// Calls between ends: kind, then the hour it fell in, then how it
    /// came out.
    pub calls: BTreeMap<String, BTreeMap<i64, BTreeMap<String, u64>>>,
    pub answers: Answers,
    pub watch: watch::Watch,
    /// When this process started keeping this.
    #[serde(skip)]
    pub started_at: Option<i64>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);

// Tests run side by side in one process, each on its own thread.
#[cfg(test)]
thread_local! {
    static TEST_STATE: std::cell::RefCell<State> = Default::default();
}

fn with<T>(act: impl FnOnce(&mut State) -> T) -> T {
    #[cfg(test)]
    {
        let _ = &STATE;
        TEST_STATE.with(|state| act(&mut state.borrow_mut()))
    }
    #[cfg(not(test))]
    match STATE.lock() {
        Ok(mut state) => act(&mut state),
        Err(poisoned) => act(&mut poisoned.into_inner()),
    }
}

fn now() -> i64 {
    crate::clock::now().timestamp()
}

/// Why a driver was passed over, as a word: never which device.
fn why(place: &Where) -> &'static str {
    match place {
        Where::Here => "here",
        Where::Moving => "moving",
        Where::Elsewhere(_) => "lives_elsewhere",
        Where::Vacant => "vacant",
        Where::MovedOut => "moved_out",
        Where::MindOn(holder) if holder == SITE => "catching_up",
        Where::MindOn(_) => "mind_elsewhere",
    }
}

/// A driver ticks every `every`: said once as it is started.
pub fn ticks_every(driver: &'static str, every: std::time::Duration) {
    with(|state| {
        state.drivers.entry(driver.into()).or_default().every = Some(every.as_secs());
    });
}

/// A driver asked whether it may run (`lease::paused`) and was let through.
pub fn ran(driver: &'static str) {
    let now = now();
    with(|state| {
        let beat = state.drivers.entry(driver.into()).or_default();
        beat.ran_at = Some(now);
        beat.runs += 1;
        beat.skips = 0;
    });
}

/// A driver asked and was passed over: she is at `place`, not here.
pub fn skipped(driver: &'static str, place: &Where) {
    let now = now();
    with(|state| {
        let beat = state.drivers.entry(driver.into()).or_default();
        beat.skipped_at = Some(now);
        beat.skipped_why = Some(why(place).into());
        beat.skips += 1;
    });
}

/// A tick that was let through ended.
pub fn ended(driver: &'static str) {
    let now = now();
    with(|state| {
        state.drivers.entry(driver.into()).or_default().ended_at = Some(now);
    });
}

/// A tick that was let through broke off. Written down at once: the
/// process ends on it (`persona::drivers`), and what comes up next should
/// know.
pub async fn failed(driver: &'static str) {
    let now = now();
    with(|state| {
        let beat = state.drivers.entry(driver.into()).or_default();
        beat.failed_at = Some(now);
        beat.failures += 1;
    });
    if let Ok(db) = crate::services::process_db::database() {
        keep(&db).await;
    }
}

/// A stretch of talk was gone over; `outcome` is the queue's word for how.
pub fn settled(outcome: &str) {
    let now = now();
    // Only the queue's own words: anything else is a failure, unnamed.
    let outcome = match outcome {
        "applied" | "no_change" | "stale" | "storage_failed" => outcome,
        model if model.starts_with("model_") && model.len() <= 32 => model,
        _ => "failed",
    };
    with(|state| {
        state.memory.settled_at = Some(now);
        state.memory.settled_outcome = Some(outcome.into());
    });
}

/// A call to the model failed; counted when it was for how their life
/// stands now (`memory_nodes::standing`).
pub fn model_failed(operation: &str) {
    if !operation.starts_with("memory_standing") {
        return;
    }
    let now = now();
    with(|state| {
        state.memory.standing_failed += 1;
        state.memory.standing_failed_at = Some(now);
    });
}

/// Her model answered a call of hers (readable or not).
pub fn model_answered() {
    let now = now();
    with(|state| {
        state.answers.answered_at = Some(now);
        state.answers.unanswered_in_a_row = 0;
        state.answers.unanswered_since = None;
    });
}

/// A call of hers got no answer at all: none to be had, it timed out, or
/// the model's side refused or failed.
pub fn model_unanswered() {
    let now = now();
    with(|state| {
        state.answers.unanswered_in_a_row += 1;
        state.answers.unanswered_since.get_or_insert(now);
    });
}

/// Her calls in a row with no answer, as counted now.
#[cfg(test)]
pub(crate) fn unanswered_in_a_row() -> u64 {
    with(|state| state.answers.unanswered_in_a_row)
}

/// Since when her model has not answered her, once that is a trouble
/// (`watch::UNANSWERED_IN_A_ROW` in a row, for `watch::QUIET_SECS`).
fn unanswered_from(answers: &Answers, now: i64) -> Option<i64> {
    answers.unanswered_since.filter(|since| {
        answers.unanswered_in_a_row >= watch::UNANSWERED_IN_A_ROW
            && now - since >= watch::QUIET_SECS
    })
}

/// What an end's hello had, read into what is shown only: nothing here
/// decides whether or how it is welcomed.
pub struct Hello<'a> {
    pub device_id: &'a str,
    pub name: &'a str,
    pub protocol: u32,
    pub app: Option<(&'a str, &'a str)>,
    pub declared: &'a [String],
    pub granted: &'a [String],
}

fn shown(word: &str) -> String {
    word.chars()
        .filter(|c| !c.is_control())
        .take(DECLARED_CHARS)
        .collect()
}

/// An end was welcomed.
pub fn linked(hello: Hello<'_>) {
    let now = now();
    with(|state| {
        let behind = state.ends.remove(hello.device_id);
        state.ends.insert(
            hello.device_id.into(),
            End {
                name: hello.name.into(),
                protocol: hello.protocol,
                app: hello
                    .app
                    .map(|(name, version)| (shown(name), shown(version))),
                declared: hello
                    .declared
                    .iter()
                    .take(DECLARED_MAX)
                    .map(|word| shown(word))
                    .collect(),
                granted: hello.granted.to_vec(),
                linked_at: now,
                behind: behind
                    .as_ref()
                    .map(|end| end.behind.clone())
                    .unwrap_or_default(),
                heard_at: behind.and_then(|end| end.heard_at),
            },
        );
    });
}

/// An end said how far it has `audience` of her log (`theirs`), where the
/// site has `ours`: how many rows it is behind, by origin, added up.
pub fn log_heard(
    device_id: &str,
    audience: &str,
    theirs: &BTreeMap<String, u64>,
    ours: &BTreeMap<String, u64>,
) {
    let behind: u64 = ours
        .iter()
        .map(|(origin, have)| have.saturating_sub(theirs.get(origin).copied().unwrap_or(0)))
        .sum();
    let now = now();
    with(|state| {
        if let Some(end) = state.ends.get_mut(device_id) {
            end.behind.insert(audience.into(), behind);
            end.heard_at = Some(now);
        }
    });
}

/// One end asked something of another, or that came out some way: `kind`
/// is the call (`act.request` is the one there is), `outcome` its word.
pub fn called(kind: &'static str, outcome: &str) {
    let now = now();
    let hour = now - now.rem_euclid(3600);
    let outcome = shown(outcome);
    with(|state| {
        let hours = state.calls.entry(kind.into()).or_default();
        hours.retain(|at, _| now - *at < CALL_HOURS_KEPT * 3600);
        *hours.entry(hour).or_default().entry(outcome).or_default() += 1;
    });
}

/// When one of her drivers that tick on a clock was last let through.
fn lived_at(state: &State) -> Option<i64> {
    state
        .drivers
        .values()
        .filter(|beat| beat.every.is_some())
        .filter_map(|beat| beat.ran_at)
        .max()
}

/// Whether the site is her host: her copy here is the live one and she is
/// not moving. Where her mind is does not come into it: lent away while
/// the site is her host is what the watch is for.
fn host_is_here(lease: Option<&lease::Lease>) -> bool {
    lease.is_none_or(|lease| lease.state == lease::State::Live && !lease.frozen)
}

async fn persona_on() -> bool {
    crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved()
}

fn identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

/// Takes back what was written down before this process: what ran since
/// it started stays as it is.
pub async fn recall(db: &DatabaseConnection) {
    let kept = match crate::services::runtime_registry::get::<State>(db, NAMESPACE, RECORD).await {
        Ok(kept) => kept.unwrap_or_default(),
        Err(error) => {
            tracing::warn!(%error, "[alive] what was kept could not be read");
            State::default()
        }
    };
    let now = now();
    with(|state| merge(state, kept, now));
}

fn merge(state: &mut State, kept: State, now: i64) {
    for (name, was) in kept.drivers {
        let beat = state.drivers.entry(name).or_default();
        let every = beat.every;
        if beat.ran_at.is_none() && beat.skipped_at.is_none() {
            *beat = was;
            beat.every = every;
        }
    }
    if state.memory == Memory::default() {
        state.memory = kept.memory;
    }
    if state.answers == Answers::default() {
        state.answers = kept.answers;
    }
    for (device, end) in kept.ends {
        state.ends.entry(device).or_insert(end);
    }
    for (kind, hours) in kept.calls {
        let ours = state.calls.entry(kind).or_default();
        for (hour, outcomes) in hours {
            if now - hour < CALL_HOURS_KEPT * 3600 {
                ours.entry(hour).or_insert(outcomes);
            }
        }
    }
    // What the watch had said stands; since when she should live is this
    // process's to see again.
    state.watch = watch::Watch {
        should_since: None,
        gap_from: None,
        ..kept.watch
    };
    state.started_at.get_or_insert(now);
}

async fn keep(db: &DatabaseConnection) {
    let state = with(|state| state.clone());
    let until = (crate::clock::now() + chrono::Duration::days(KEEP_DAYS)).timestamp();
    if let Err(error) =
        crate::services::runtime_registry::put(db, NAMESPACE, RECORD, identity(), &state, until)
            .await
    {
        tracing::warn!(%error, "[alive] could not be written down");
    }
}

/// One look of the watch: says what there is to say to the site's owner,
/// and every few looks writes down what is kept.
pub async fn look(db: DatabaseConnection, looks: u64) {
    let should_live = persona_on().await && host_is_here(lease::current().as_ref());
    let now = now();
    let (said, troubles) = with(|state| {
        let started_at = *state.started_at.get_or_insert(now);
        let seen = watch::Seen {
            now,
            started_at,
            should_live,
            lived_at: lived_at(state),
        };
        let said = state.watch.look(seen);
        let (stalled, stalled_from) = watch::stalled(
            state
                .drivers
                .values()
                .filter_map(|beat| beat.every.map(|every| (every, beat.ran_at))),
            state.watch.watching_from(started_at),
            now,
        );
        let unanswered_from = unanswered_from(&state.answers, now);
        let troubles = state
            .watch
            .look_troubles(now, stalled_from, unanswered_from);
        (said, (troubles, stalled))
    });
    let (troubles, stalled) = troubles;
    for trouble in &troubles {
        match trouble {
            watch::Trouble::Stalled(watch::Turn::Began { .. })
            | watch::Trouble::Unanswered(watch::Turn::Began { .. }) => {
                tracing::warn!(?trouble, stalled, "[alive] a trouble of hers began");
            }
            _ => tracing::info!(?trouble, "[alive] a trouble of hers ended"),
        }
        say_trouble(&db, *trouble, stalled).await;
    }
    if let Some(said) = said {
        match said {
            watch::Said::Stopped { from } => {
                tracing::warn!(
                    quiet_secs = now - from,
                    "[alive] nothing of her own life ran while she should be living here"
                );
            }
            watch::Said::Back { from, since } => {
                tracing::info!(stopped_secs = since - from, "[alive] she is living again");
            }
        }
        say(&db, said).await;
        keep(&db).await;
    } else if !troubles.is_empty() || looks.is_multiple_of(SAVED_EVERY_LOOKS) {
        keep(&db).await;
    }
}

/// Tells the site's owner, as one notification that is changed in place.
async fn say(db: &DatabaseConnection, said: watch::Said) {
    let owner = match crate::services::principal::site_owner_id(db).await {
        Ok(Some(owner)) => owner,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(%error, "[alive] the site's owner could not be read");
            return;
        }
    };
    let Some(manager) = crate::services::agent::notifications::get_notification_manager() else {
        return;
    };
    manager.notify_her_life(owner, said).await;
}

/// Tells the site's owner of a trouble short of her stopping: one
/// notification a trouble, changed in place when it ends.
/// `stalled`: how many of her drivers are stuck now.
async fn say_trouble(db: &DatabaseConnection, trouble: watch::Trouble, stalled: usize) {
    let Ok(Some(owner)) = crate::services::principal::site_owner_id(db).await else {
        return;
    };
    let Some(manager) = crate::services::agent::notifications::get_notification_manager() else {
        return;
    };
    manager.notify_her_trouble(owner, trouble, stalled).await;
}

fn at(seconds: Option<i64>) -> Value {
    seconds
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|at| json!(at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)))
        .unwrap_or(Value::Null)
}

/// When the site's holding of her began: the grant at `since`.
async fn host_since<C: ConnectionTrait>(db: &C, lease: &lease::Lease) -> Option<i64> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT created_at FROM agent_lease_grants WHERE epoch = $1",
            [lease.since.into()],
        ))
        .await
        .ok()??;
    row.try_get::<chrono::DateTime<chrono::Utc>>("", "created_at")
        .ok()
        .map(|at| at.timestamp())
}

/// When she last wrote how someone's life stands now.
async fn standing_written_at<C: ConnectionTrait>(db: &C) -> Option<i64> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT MAX(created_at) AS at FROM agent_node_rows WHERE kind = $1 AND author = $2",
            [myriad_nodes::stands::KIND.into(), "her".into()],
        ))
        .await
        .ok()??;
    row.try_get::<Option<chrono::DateTime<chrono::Utc>>>("", "at")
        .ok()
        .flatten()
        .map(|at| at.timestamp())
}

/// What `GET /api/agent/alive` gives the site's owner. The same for any
/// end that is shown it later.
pub async fn report(db: &DatabaseConnection, owner: i32) -> Value {
    let now = now();
    let lease = lease::current();
    let persona_on = persona_on().await;
    let host_here = host_is_here(lease.as_ref());
    let host = match &lease {
        Some(lease) if !host_here => json!({
            "holder": if lease.frozen { Value::Null } else { json!("device") },
            "moving": lease.frozen,
            "since": Value::Null,
            "temporary": false,
        }),
        Some(lease) => json!({
            "holder": "site",
            "moving": false,
            "since": at(if lease.holder == SITE { host_since(db, lease).await } else { None }),
            "temporary": false,
        }),
        // Never moved: she has been here all along.
        None => {
            json!({ "holder": "site", "moving": false, "since": Value::Null, "temporary": false })
        }
    };
    let devices = crate::services::persona_sync::devices::list(db, owner)
        .await
        .unwrap_or_default();
    let state = with(|state| state.clone());
    let started_at = state.started_at.unwrap_or(now);
    let should_live = persona_on && host_here;
    let lived_at = lived_at(&state);
    let seen = watch::Seen {
        now,
        started_at,
        should_live,
        lived_at,
    };
    let quiet_secs = (now - state.watch.quiet_from(&seen)).max(0);
    let (gaps, gap_secs) = state.watch.gaps_of_the_day(now);
    let (host_stops, host_stop_secs) = state.watch.host_stops_of_the_day();
    let drivers: Vec<Value> = state
        .drivers
        .iter()
        .filter(|(_, beat)| beat.ran_at.is_some() || beat.skipped_at.is_some())
        .map(|(name, beat)| {
            // Passed over since it last ran, or never ran.
            let passed_over = beat.skips > 0;
            json!({
                "name": name,
                "everySecs": beat.every,
                "ranAt": at(beat.ran_at),
                "runs": beat.runs,
                "endedAt": at(beat.ended_at),
                // Let through and not ended yet: a tick is going on.
                "running": beat.every.is_some()
                    && beat.ran_at.is_some_and(|ran| beat.ended_at.is_none_or(|ended| ended < ran))
                    && beat.failed_at.is_none_or(|failed| beat.ran_at.is_some_and(|ran| failed < ran)),
                "failedAt": at(beat.failed_at),
                "failures": beat.failures,
                "skippedAt": at(beat.skipped_at),
                "skippedWhy": if passed_over { json!(beat.skipped_why) } else { Value::Null },
                "skips": beat.skips,
                // Three of its own rounds without being let through.
                "overdue": should_live && beat.every.is_some_and(|every| {
                    now - beat.ran_at.unwrap_or(started_at).max(started_at) > 3 * every as i64
                }),
            })
        })
        .collect();
    let ends: Vec<Value> = devices
        .iter()
        .filter(|device| device.revoked_at.is_none())
        .map(|device| {
            let end = state.ends.get(&device.id);
            let connected = crate::services::agent::link::is_open(&device.id);
            json!({
                "id": device.id,
                "name": device.name,
                "connected": connected,
                "linkedAt": at(end.map(|end| end.linked_at)),
                "lastSeenAt": device.last_seen_at.map(|at| at.to_rfc3339()),
                "protocol": end.map(|end| end.protocol),
                "app": end.and_then(|end| end.app.as_ref()).map(|(name, version)| json!({
                    "name": name, "version": version,
                })),
                // Read off what it was granted: the highest `logN`.
                "coreAbi": end.map(|end| crate::services::her_log::relay::peer_abi(&end.granted)),
                // Three layers, kept apart. No end's capabilities are
                // approved by anyone yet: `approved` is null until they are.
                "capabilities": {
                    "declared": end.map(|end| &end.declared),
                    "approved": Value::Null,
                    "granted": end.map(|end| &end.granted),
                },
                "log": end.and_then(|end| end.heard_at.map(|heard| json!({
                    "behindRows": end.behind.values().sum::<u64>(),
                    "audiencesBehind": end.behind.values().filter(|rows| **rows > 0).count(),
                    "heardAt": at(Some(heard)),
                }))),
                // Kinds of frame it told the host it dropped, as the link
                // counts them: links that met each, not frames lost.
                "dropped": crate::services::agent::link::counts::dropped_by(&device.id)
                    .into_iter()
                    .map(|(t, reason, told)| json!({
                        "t": t,
                        "reason": reason,
                        "links": told.count,
                        "lastAt": told.last_at.map(|at| at.to_rfc3339()),
                    }))
                    .collect::<Vec<_>>(),
                // Her reliable frames its ack covered that it then said it
                // could not read: the site took them for delivered.
                "unread": crate::services::agent::link::counts::unread_by(&device.id)
                    .into_iter()
                    .map(|(t, told)| json!({
                        "t": t,
                        "count": told.count,
                        "lastAt": told.last_at.map(|at| at.to_rfc3339()),
                    }))
                    .collect::<Vec<_>>(),
                // Pages of her log on their way to it (`log.pages`), by the
                // kind of audience, with when each went: one sent long ago
                // waits for a name that does not come, and only a new link
                // starts it again.
                "logPagesOpen": crate::services::agent::link::counts::pages_open_by(&device.id)
                    .into_iter()
                    .map(|(kind, at)| json!({ "audience": kind, "sentAt": at.to_rfc3339() }))
                    .collect::<Vec<_>>(),
                // Its log.vector frames that got no page by naming one not
                // on its way or one that brought nothing, and rows it asked
                // again for past what a link keeps, by why.
                "logPagesPassed": crate::services::agent::link::counts::pages_passed_by(&device.id)
                    .into_iter()
                    .map(|(why, passed)| json!({
                        "why": why,
                        "count": passed.count,
                        "lastAt": passed.last_at.map(|at| at.to_rfc3339()),
                    }))
                    .collect::<Vec<_>>(),
                // Rows of her log it said it did not take, by the kind of
                // audience and why: batches told and their rows.
                "logRefused": crate::services::agent::link::counts::log_refused_by(&device.id)
                    .into_iter()
                    .map(|(kind, reason, refused)| json!({
                        "audience": kind,
                        "reason": reason,
                        "batches": refused.batches.count,
                        "rows": refused.rows,
                        "lastAt": refused.batches.last_at.map(|at| at.to_rfc3339()),
                    }))
                    .collect::<Vec<_>>(),
                // Its links closed for a page of its that could not be
                // settled (the database failing on it): in a row since one
                // was named back, and in all. Many in a row is a device
                // reconnecting and sending the page again into a fault.
                "logPagesUnsettled": crate::services::agent::link::counts::pages_unsettled_by(&device.id)
                    .map(|unsettled| json!({
                        "inARow": unsettled.in_a_row,
                        "closes": unsettled.closes.count,
                        "lastAt": unsettled.closes.last_at.map(|at| at.to_rfc3339()),
                    })),
            })
        })
        .collect();
    let calls: Vec<Value> = state
        .calls
        .iter()
        .map(|(kind, hours)| {
            let mut outcomes: BTreeMap<&str, u64> = BTreeMap::new();
            for (_, by_outcome) in hours
                .iter()
                .filter(|(hour, _)| now - **hour < CALL_HOURS_KEPT * 3600)
            {
                for (outcome, count) in by_outcome {
                    *outcomes.entry(outcome).or_default() += count;
                }
            }
            json!({ "kind": kind, "outcomes": outcomes })
        })
        .collect();
    json!({
        "now": at(Some(now)),
        "startedAt": at(Some(started_at)),
        "host": host,
        "life": {
            "personaOn": persona_on,
            "shouldLive": should_live,
            "background": crate::persona::background_status(),
            "livedAt": at(lived_at),
            "quietSecs": if should_live { json!(quiet_secs) } else { Value::Null },
            "alarmedFrom": at(state.watch.alarmed_from),
            "quietAfterSecs": watch::QUIET_SECS,
            "gaps": { "count": gaps, "secs": gap_secs },
            // The host itself asleep or this process down: not hers.
            "hostStops": { "count": host_stops, "secs": host_stop_secs },
            "drivers": drivers,
            // One of her drivers stuck while the others run, as said to the
            // site's owner (`watch::stalled`).
            "stalledFrom": at(state.watch.stalled.from),
            // Whether her model answers her calls; an answer that cannot be
            // read counts as one.
            "answers": {
                "answeredAt": at(state.answers.answered_at),
                "unansweredInARow": state.answers.unanswered_in_a_row,
                "unansweredSince": at(state.answers.unanswered_since),
                "alarmedFrom": at(state.watch.unanswered.from),
                "alarmAfter": watch::UNANSWERED_IN_A_ROW,
            },
        },
        "memory": {
            "settledAt": at(state.memory.settled_at),
            "settledOutcome": state.memory.settled_outcome,
            "standingWrittenAt": at(standing_written_at(db).await),
            // By kind, since the site started (`standing::failures`).
            "standingFailures": crate::services::agent::merope::memory_nodes::standing::failures(),
            "standingFailed": state.memory.standing_failed,
            "standingFailedAt": at(state.memory.standing_failed_at),
            // How going over what was chatted went wrong since the site
            // started, by kind (`settling::failures`).
            "settlingFailures":
                crate::services::agent::merope::store::memory_jobs::settling::failures(),
            // How asking for vectors went unanswered since the site started:
            // ahead of recall, in recall, for lines by meaning, filling rows
            // (`meaning::failures`).
            "meaningFailures": crate::services::agent::memory::meaning::failures(),
            // Her answers cut off at their output limit since the site
            // started, by the call they answered (`call::cut_off`).
            "cutOff": crate::services::agent::merope::answers_cut_off(),
            // Talk her placing could not place, since the site started: spans
            // kept to place later, batches placed later, lines let go after
            // a week (`placing::backlog::failures`).
            "unplaced": crate::services::agent::merope::memory_nodes::placing::backlog::failures(),
            // What rests on memories taken back or forgotten, cleared from her
            // nodes since the site started: memories still waiting for the
            // sweep, sweeps that cleared something, sweeps stopped on an
            // error, versions left out as they were written for resting on
            // what went (`memory_nodes::forget::failures`). Counts only.
            "forgetting": crate::services::agent::merope::memory_nodes::forget::failures(),
            // What they took back, waiting for the second look before what
            // rests on it is forgotten, only let go as a change or neither,
            // and left unsure (`memory_nodes::take_back_check`), over
            // everyone. Counts only; null when they could not be read.
            "takeBackChecks":
                crate::services::agent::merope::memory_nodes::take_back_check::counts(db).await,
            // Lines whose corrections were left undone since the site started, their
            // new fact not holding up (`chat_remember::corrections_unkept`).
            "correctionsUnkept": crate::services::agent::merope::chat_remember::corrections_unkept(),
        },
        "ends": ends,
        // Keys of a look at the screen the site deleted before reading it
        // (not ones the contract lets through), per frame type and key,
        // all devices together: names and counts, never a value.
        "screenKeys": crate::services::agent::link::counts::screen_keys()
            .into_iter()
            .map(|(t, key, deleted)| json!({
                "t": t,
                "key": key,
                "count": deleted.count,
                "lastAt": deleted.last_at.map(|at| at.to_rfc3339()),
            }))
            .collect::<Vec<_>>(),
        "calls": calls,
    })
}
