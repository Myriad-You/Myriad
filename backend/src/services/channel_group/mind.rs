//! The site as the end that runs her group mind (`myriad_merope::group`):
//! each group's state kept as the core hands it back, every line and timer
//! and answer stepped through it one at a time, and what it says to do done
//! here, against the platforms, the database and her other parts (D2).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use myriad_merope::group::{self as core, At, Event, Group, Restore, Resume, Rolls, Target};

use super::*;

/// A group as this process has it: her mind there, when it was last
/// touched, and the token its lines came with (never in her mind).
pub(super) struct Slot {
    pub(super) group: Group,
    pub(super) touched: Instant,
    pub(super) token: String,
}

pub(super) static GROUPS: LazyLock<Mutex<HashMap<String, Slot>>> = LazyLock::new(Default::default);

/// Who waits for her to be done with what they handed in.
pub(super) static WAITERS: LazyLock<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>> =
    LazyLock::new(Default::default);

/// The binding each member speaks to her through, per group, from when she
/// looked them up to when she answers them.
pub(super) static BINDINGS: LazyLock<Mutex<HashMap<(String, i32), ChannelBinding>>> =
    LazyLock::new(Default::default);

/// The Chat session each sender has in each group, so their turns in that
/// group supersede only each other.
pub(super) static SESSIONS: LazyLock<Mutex<HashMap<(String, i32), String>>> =
    LazyLock::new(Default::default);

pub(super) static WAITER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Who sits at a game in which group (`<platform>:<id>` to its venue): what
/// they whisper to her comes to that group's mind first.
pub(super) static SEATED: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(Default::default);

pub(super) fn seat(venue: &str, who: String) {
    if let Ok(mut seated) = SEATED.lock() {
        seated.insert(who, venue.to_string());
    }
}

pub(super) fn unseat(venue: &str, who: &str) {
    if let Ok(mut seated) = SEATED.lock()
        && seated.get(who).is_some_and(|at| at == venue)
    {
        seated.remove(who);
    }
}

/// When this process first heard a group: lines before it reached the one
/// before, and whatever it was holding for her then went with it.
static UP_SINCE: LazyLock<chrono::DateTime<chrono::Utc>> = LazyLock::new(crate::clock::now);

pub(super) fn up_since() -> chrono::DateTime<chrono::Utc> {
    #[cfg(test)]
    if let Some(at) = g0::up_since() {
        return at;
    }
    *UP_SINCE
}

pub(super) fn heard_since_up() {
    LazyLock::force(&UP_SINCE);
}

/// How she is now, as the group's mind takes it.
pub(super) fn her() -> core::Her {
    use crate::services::agent::merope::group::{doing, timing};
    let now = now();
    core::Her {
        away: away(),
        asleep_for: timing::asleep_now(),
        busy_for: doing::current()
            .map(|doing| (doing.ends - now).num_microseconds().unwrap_or(0) as f64 / 1_000_000.0),
        today: now
            .with_timezone(&crate::services::agent::merope::clock::zone())
            .date_naive()
            .to_string(),
    }
}

pub(super) fn rolls() -> Rolls {
    #[cfg(test)]
    if let Some(roll) = g0::roll() {
        return Rolls::new(0, vec![roll; 64]);
    }
    Rolls::new(rand::random(), Vec::new())
}

/// The group's mind, with the oldest group let go once too many are in mind.
pub(super) fn with_slot<T>(venue: &str, act: impl FnOnce(&mut Slot) -> T) -> Option<T> {
    let mut groups = GROUPS.lock().ok()?;
    if !groups.contains_key(venue) && groups.len() >= MAX_GROUPS {
        if let Some(stalest) = groups
            .iter()
            .filter(|(_, slot)| !slot.group.busy)
            .min_by_key(|(_, slot)| slot.touched)
            .map(|(id, _)| id.clone())
        {
            groups.remove(&stalest);
        }
    }
    let slot = groups.entry(venue.to_string()).or_insert_with(|| Slot {
        group: Group::default(),
        touched: Instant::now(),
        token: String::new(),
    });
    slot.touched = Instant::now();
    Some(act(slot))
}

/// Let the group go from mind, as a restart would (and who sits at its
/// table with it).
#[cfg(test)]
pub(super) fn forget(venue: &str) {
    if let Ok(mut groups) = GROUPS.lock() {
        groups.remove(venue);
    }
    if let Ok(mut seated) = SEATED.lock() {
        seated.retain(|_, at| at != venue);
    }
}

pub(super) fn with_group<T>(venue: &str, act: impl FnOnce(&Group) -> T) -> Option<T> {
    with_slot(venue, |slot| act(&slot.group))
}

/// One step of the group's mind.
pub(super) fn step(venue: &str, event: Event) -> core::Outcome {
    let mut at = At {
        now: now().timestamp_micros(),
        her: her(),
        rolls: rolls(),
    };
    with_slot(venue, |slot| core::step(&mut slot.group, &mut at, event)).unwrap_or_default()
}

pub(super) fn note_token(venue: &str, token: &str) {
    with_slot(venue, |slot| slot.token = token.to_string());
}

pub(super) fn token(venue: &str) -> String {
    with_slot(venue, |slot| slot.token.clone()).unwrap_or_default()
}

pub(super) fn platform_of(slug: &str) -> Option<ChannelPlatform> {
    ChannelPlatform::ALL
        .into_iter()
        .find(|platform| platform.slug() == slug)
}

/// How she shows in the group `venue`, as its latest line told.
pub(crate) fn shown_as(venue: &str) -> Option<String> {
    with_group(venue, |group| group.shown_as.clone()).flatten()
}

/// A line to send to: where `to` is, with nothing of who said it.
pub(super) fn line_to(to: &Target) -> Option<GroupLine> {
    Some(GroupLine {
        platform: platform_of(&to.platform)?,
        chat: to.chat.clone(),
        message_id: to.message_id.clone(),
        thread: to.thread,
        from: String::new(),
        display_name: String::new(),
        text: String::new(),
        addressed: false,
        reply_to: None,
        images: Vec::new(),
        shown_as: None,
    })
}

/// What was kept of the group before a restart, if it is not in mind yet
/// and the kept lines could be read (not read is not nothing kept).
pub(super) async fn kept_before(venue: &str) -> Option<Restore> {
    if with_group(venue, |group| group.restored).unwrap_or(true) {
        return None;
    }
    let db = db().ok()?;
    let lines = match kept::<StoredLines>(&db, LINES_NAMESPACE, venue).await {
        Ok(stored) => stored.map(|stored| stored.lines),
        Err(error) => {
            warn!(%error, %venue, "[Group] could not read back the lines kept before a restart");
            return None;
        }
    };
    let spoke_up = spoke_up_lately(&db, venue, core::SPOKE_UP_KEPT as u64)
        .await
        .into_iter()
        .map(|(at, taken)| core::SpokeUp {
            at: at.timestamp_micros(),
            taken,
        })
        .collect();
    // The game with seats that was on at the table, if one was.
    let table = kept::<serde_json::Value>(&db, TABLE_NAMESPACE, venue)
        .await
        .ok()
        .flatten();
    Some(Restore {
        lines,
        spoke_up,
        table,
    })
}

/// Bring back what was kept before a restart, once.
pub(super) async fn restore(venue: &str) {
    if let Some(Restore {
        lines,
        spoke_up,
        table,
    }) = kept_before(venue).await
    {
        let out = step(
            venue,
            Event::Restored {
                lines,
                spoke_up,
                table,
            },
        );
        execute(venue.to_string(), out.effects).await;
    }
}

// ---------------------------------------------------------------- lines in

/// Keep a group line in mind, whoever wrote it. False when it was heard
/// already (delivered again after a restart): nothing more to do with it.
pub async fn record(message: &GroupLine) -> bool {
    heard_since_up();
    let venue = message.venue();
    let restore = kept_before(&venue).await;
    let out = step(
        &venue,
        Event::Heard {
            message: message.to_message(),
            restore,
        },
    );
    let fresh = out.fresh.unwrap_or(false);
    execute(venue, out.effects).await;
    fresh
}

/// Answer one group line that spoke to her, once she sees it (see
/// `merope::timing`). A long wait does not hold the caller.
pub async fn handle(message: GroupLine, token: String) {
    let venue = message.venue();
    note_token(&venue, &token);
    let waiter = format!(
        "{venue}#{}",
        WAITER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let (sender, done) = tokio::sync::oneshot::channel();
    if let Ok(mut waiters) = WAITERS.lock() {
        waiters.insert(waiter.clone(), sender);
    }
    let out = step(
        &venue,
        Event::Called {
            message: message.to_message(),
            waiter: Some(waiter),
        },
    );
    execute(venue, out.effects).await;
    let _ = done.await;
}

/// What `from` whispered to her on `platform`, ahead of pairing: true when
/// it was a move of theirs in a game they sit at in a group (taken there,
/// whoever they are, paired or not); else it is not the game's, and goes on
/// as any private line does.
pub async fn whispered(platform: ChannelPlatform, from: &str, text: &str) -> bool {
    whispered_on(platform, from, text, None).await
}

/// Whether `from` on `platform` sits at a game in a group now (from when
/// they are found reachable to its end).
pub fn seated(platform: ChannelPlatform, from: &str) -> bool {
    let who = format!("{}:{from}", platform.slug());
    SEATED
        .lock()
        .ok()
        .is_some_and(|seated| seated.contains_key(&who))
}

/// A button of a game that `from` pressed on `platform` (`data` as it
/// came back): none when it is no game's button; else whether it was taken.
pub async fn pressed(platform: ChannelPlatform, from: &str, data: &str) -> Option<bool> {
    let (wait, choice) = seat_button(data)?;
    Some(whispered_on(platform, from, &choice, wait).await)
}

async fn whispered_on(
    platform: ChannelPlatform,
    from: &str,
    text: &str,
    wait: Option<u64>,
) -> bool {
    let who = format!("{}:{from}", platform.slug());
    let Some(venue) = SEATED
        .lock()
        .ok()
        .and_then(|seated| seated.get(&who).cloned())
    else {
        return false;
    };
    let out = step(
        &venue,
        Event::Whispered {
            who,
            text: text.to_string(),
            wait,
        },
    );
    let taken = out.taken.unwrap_or(false);
    execute(venue, out.effects).await;
    taken
}

/// A line that did not call her by name: she reads the group the way a
/// person does (see `myriad_merope::group`).
pub fn notice(message: GroupLine, token: String) {
    let venue = message.venue();
    note_token(&venue, &token);
    let out = step(
        &venue,
        Event::Noticed {
            message: message.to_message(),
        },
    );
    tokio::spawn(execute(venue, out.effects));
}

/// She was muted in a group, or it was lifted, by `by` (their platform id;
/// empty if not known): she knows it, and says nothing there until it lifts.
pub async fn muted(
    venue: &str,
    by: &str,
    muted: myriad_agent_rules::onebot::decode::Muted,
    everyone: bool,
) {
    use myriad_agent_rules::onebot::decode::Muted;
    let mute = match muted {
        Muted::For(seconds) => core::Mute::For { seconds },
        Muted::UntilLifted => core::Mute::UntilLifted,
        Muted::Lifted => core::Mute::Lifted,
    };
    info!(%venue, ?muted, everyone, "[Group] her mute there changed");
    let restore = kept_before(venue).await;
    let out = step(
        venue,
        Event::Muted {
            by: by.to_string(),
            mute,
            everyone,
            restore,
        },
    );
    execute(venue.to_string(), out.effects).await;
}

/// What a group said while she was not connected, read back as a person
/// scrolls up on opening a chat; the line that called her before a restart
/// and that she never got to is taken up, late.
pub async fn catch_up(venue: &str, past: Vec<(GroupLine, chrono::DateTime<chrono::Utc>, bool)>) {
    heard_since_up();
    restore(venue).await;
    let past = past
        .into_iter()
        .map(|(line, at, hers)| core::Past {
            message: line.to_message(),
            at: at.timestamp_micros(),
            hers,
        })
        .collect();
    let out = step(
        venue,
        Event::Resumed {
            why: Resume::Restart,
            past,
            up_since: Some(up_since().timestamp_micros()),
        },
    );
    execute(venue.to_string(), out.effects).await;
}

/// After a restart, on a platform that cannot read a group back (Telegram,
/// Discord): in each group kept lately, the line that called her before it
/// and that she never got to, taken up with the configured token.
pub async fn take_back_kept(platform: ChannelPlatform) {
    take_back_tables(platform).await;
    heard_since_up();
    let Ok(db) = db() else {
        return;
    };
    let prefix = format!("{}:", platform.slug());
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    for row in all_kept(&db, LINES_NAMESPACE).await {
        let Some(chat) = row.record_id.strip_prefix(&prefix).map(str::to_string) else {
            continue;
        };
        let venue = row.record_id.clone();
        let Some(token) = token_for(platform, &chat, &config) else {
            continue;
        };
        let Ok(stored) = serde_json::from_value::<StoredLines>(row.payload) else {
            continue;
        };
        let thread = kept::<StoredReach>(&db, REACH_NAMESPACE, &venue)
            .await
            .ok()
            .flatten()
            .and_then(|reach| reach.thread);
        let past = core::kept_as_heard(platform.slug(), &chat, thread, stored.lines);
        restore(&venue).await;
        note_token(&venue, &token);
        let out = step(
            &venue,
            Event::Resumed {
                why: Resume::Restart,
                past,
                up_since: Some(up_since().timestamp_micros()),
            },
        );
        execute(venue, out.effects).await;
    }
}

/// After a restart: each game with seats that was on at a table on
/// `platform` goes on (who sits at it seated again, its wait going on for
/// the time it had left), with the configured token.
pub async fn take_back_tables(platform: ChannelPlatform) {
    let Ok(db) = db() else {
        return;
    };
    let prefix = format!("{}:", platform.slug());
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    for row in all_kept(&db, TABLE_NAMESPACE).await {
        let Some(chat) = row.record_id.strip_prefix(&prefix) else {
            continue;
        };
        let Some(token) = token_for(platform, chat, &config) else {
            continue;
        };
        note_token(&row.record_id, &token);
        restore(&row.record_id).await;
        info!(venue = %row.record_id, "[Group] a game at the table goes on after a restart");
    }
}

/// She is back on the site: in each group, the latest line that called her
/// while she was away, taken up as if it had just come in, if it is still
/// recent enough to be talk.
pub fn back_from_away() {
    let venues: Vec<String> = GROUPS
        .lock()
        .map(|groups| groups.keys().cloned().collect())
        .unwrap_or_default();
    for venue in venues {
        let out = step(
            &venue,
            Event::Resumed {
                why: Resume::Back,
                past: Vec::new(),
                up_since: None,
            },
        );
        if !out.effects.is_empty() {
            info!(%venue, "[Group] she is back: the line that called her is taken up");
        }
        tokio::spawn(execute(venue, out.effects));
    }
}

/// Groups of `platform` she has kept lines of lately.
pub async fn groups_lately(platform: ChannelPlatform) -> Vec<String> {
    let Ok(db) = db() else {
        return Vec::new();
    };
    let prefix = format!("{}:", platform.slug());
    all_kept(&db, LINES_NAMESPACE)
        .await
        .into_iter()
        .filter_map(|row| row.record_id.strip_prefix(&prefix).map(str::to_string))
        .collect()
}

/// The group's people lately who are paired site users, each by the name
/// the group knows them by.
pub async fn names_here(db: &DatabaseConnection, venue: &str) -> HashMap<i32, String> {
    let Some(platform) = venue
        .split_once(':')
        .and_then(|(slug, _)| platform_of(slug))
    else {
        return HashMap::new();
    };
    let now = now().timestamp_micros();
    let people = with_group(venue, |group| core::people(group, now)).unwrap_or_default();
    let mut names = HashMap::new();
    for (name, from) in people {
        if let Ok(PairingLookup::Paired { user_id }) =
            crate::services::channel_pairing::lookup_openid(db, PairingChannel::of(platform), &from)
                .await
        {
            names.entry(user_id).or_insert(name);
        }
    }
    names
}
