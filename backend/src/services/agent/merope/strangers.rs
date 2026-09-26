//! People in a group who are not from the community.
//!
//! She answers them too, lightly: they get no account of hers, no memory of
//! anyone, no moods or state of their own, only the group's talk, what she is
//! doing, what she thinks of what they bring up, and the group's own bits.
//! The site's owner hosts her there, so the owner's budget pays.
//!
//! Someone she keeps running into (a few exchanges) gets a small memory: one
//! short note, in her words, of what she would want to remember next time —
//! what they go by, what they like, what they have told her about themselves.
//! It belongs to no account and is kept in that group only, apart from
//! ordinary memory: it comes up only when that person talks to her there. A
//! note nobody has touched in two months fades.
//!
//! How often she has talked with someone is kept in the runtime registry for
//! as long as a note would last, so restarts and replicas share one count.
//!
//! She writes the note when they stop talking for a little while, or after
//! several exchanges if they keep going, from all of it at once; a restart
//! in between loses only those few exchanges' worth of note.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified;
use myriad_merope::strangers::{
    EXCHANGE_CHARS, Exchange, NOTE_SCHEMA, evidence_marker, evidence_of, note_schema, note_system,
    parse_note, talks_key, without_directives,
};
pub use myriad_merope::strangers::{Stranger, section};
#[cfg(test)]
use serde_json::Value;

pub const SOURCE: &str = "stranger";
/// Exchanges before she starts keeping a note on someone.
const REGULAR_AFTER: i64 = 3;
/// A note nobody has touched this long fades.
const FADE_AFTER: chrono::Duration = chrono::Duration::days(60);
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);
const NOTE_TIMEOUT: Duration = Duration::from_secs(30);
/// She writes her note once they have been quiet this long…
const QUIET: Duration = Duration::from_secs(3 * 60);
/// …or once this many exchanges have piled up.
const WRITE_EVERY: usize = 8;

/// Exchanges since her note on someone was last written, by `talks_key`.
struct Pending {
    stranger: Stranger,
    exchanges: Vec<Exchange>,
    /// Bumped by every exchange; a wait that wakes to a newer one yields.
    round: u64,
}

static PENDING: LazyLock<Mutex<HashMap<String, Pending>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Runtime-registry namespace of the exchange counts.
pub const TALKS_NAMESPACE: &str = "merope_stranger_talks";

/// One more exchange with this person in this group; how many so far.
async fn count_exchange(db: &DatabaseConnection, venue: &str, who: &str) -> i64 {
    let keep_until = (chrono::Utc::now() + FADE_AFTER).timestamp();
    crate::services::runtime_registry::increment(
        db,
        TALKS_NAMESPACE,
        &talks_key(venue, who),
        keep_until,
    )
    .await
    .unwrap_or_else(|error| {
        tracing::warn!(%error, "[Merope] could not count an exchange with a stranger");
        0
    })
}

/// Forget every count, with the persona.
pub async fn forget_counts<C: sea_orm::ConnectionTrait>(db: &C) -> Result<u64, sea_orm::DbErr> {
    crate::services::runtime_registry::delete_matching(db, TALKS_NAMESPACE, None, None, None, None)
        .await
}

/// The stored venue of a group (`telegram:-100123` → `group:telegram:-100123`).
fn group_venue(venue: &str) -> String {
    unified::Audience::group(venue, 0).venue()
}

/// Her note on this person in this group, if she keeps one.
async fn note_on(
    db: &DatabaseConnection,
    venue: &str,
    stranger: &Stranger,
) -> Option<agent_memories::Model> {
    let marker = evidence_marker(&stranger.who);
    unified::unowned_with_evidence(db, &group_venue(venue), SOURCE, &marker)
        .await
        .ok()
        .flatten()
}

/// Her reply to someone from outside the community, in a group: the same
/// voice, far less context. `None` if the model gave nothing.
pub async fn reply(
    db: &DatabaseConnection,
    owner: i32,
    venue: &str,
    stranger: &Stranger,
    transcript: &[crate::services::agent::ConversationMessage],
    words: &str,
) -> Option<String> {
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let note = note_on(db, venue, stranger).await.map(|row| row.content);
    let mut sections = vec![
        super::group_speaking_section(&stranger.name),
        section(&stranger.name, note.as_deref()),
        super::speaking_prompts::format_now_section(chrono::Local::now()),
    ];
    let now =
        super::doing::current().map(|doing| super::doing::now_line(&doing, chrono::Utc::now()));
    if let Some(block) = super::format_doing_section(now.as_deref(), &[]) {
        sections.push(block);
    }
    if let Some(block) =
        super::format_bits_section(&super::bits::in_group(db, venue, 3).await, true)
    {
        sections.push(block);
    }
    if let Some(block) = super::format_views_section(&super::views::touched(db, words, 2).await) {
        sections.push(block);
    }
    // The group's turtle soup: anyone may ask, and so may they.
    let table = super::soup::Table::Group(venue.to_string());
    let game = match super::soup::this_turn_at(&table, Some(&stranger.name), words, owner).await {
        Some(section) => section,
        None => super::soup::GROUP_OFFER.to_string(),
    };
    sections.push(game);
    let prompt = crate::services::agent::chat_prompt::build_group_chat_prompt(
        &soul,
        &sections.join("\n\n"),
        transcript,
        words,
    );
    let model = super::call::Ask::new(super::call::Voice::Hers, owner, "group_stranger")
        .within(REPLY_TIMEOUT)
        .model()
        .await?;
    // A reply the provider dropped halfway reached no one: ask once more.
    let mut raw = model.say(&prompt).await;
    if raw.as_ref().is_err_and(super::call::was_cut) {
        raw = model.say(&prompt).await;
    }
    let raw = raw.ok()?;
    let (said, started) = super::soup::split_start(&raw);
    let mut text = without_directives(&said);
    // She said she would think one up: the puzzle follows her words.
    if started {
        let opening = super::soup::start_at(&table, words, owner).await;
        text = format!("{text}\n\n{opening}").trim().to_string();
    }
    // A game this line ended is over, and the group remembers it.
    super::soup::after_turn_at(db, &table).await;
    (!text.is_empty()).then_some(text)
}

/// After she answered someone from outside: count it, and when they pause
/// (or after several exchanges), let her note on them catch up with all of
/// it, once they are someone she keeps running into.
pub fn spawn_after(
    db: DatabaseConnection,
    owner: i32,
    venue: String,
    stranger: Stranger,
    words: String,
    reply: String,
) {
    tokio::spawn(async move {
        let count = count_exchange(&db, &venue, &stranger.who).await;
        let key = talks_key(&venue, &stranger.who);
        let Some((round, full)) = queue(&key, stranger, words, reply) else {
            return;
        };
        if !full {
            tokio::time::sleep(QUIET).await;
        }
        // They said more since: that wait writes it.
        let Some(Pending {
            stranger,
            exchanges,
            ..
        }) = take(&key, round, full)
        else {
            return;
        };
        write_note(&db, owner, &venue, &stranger, &exchanges, count).await;
    });
}

/// Adds an exchange to what her note has yet to take in: its round, and
/// whether enough has piled up to write now.
fn queue(key: &str, stranger: Stranger, words: String, reply: String) -> Option<(u64, bool)> {
    let clip = |text: String| text.chars().take(EXCHANGE_CHARS).collect::<String>();
    let mut pending = PENDING.lock().ok()?;
    let entry = pending.entry(key.to_string()).or_insert_with(|| Pending {
        stranger: stranger.clone(),
        exchanges: Vec::new(),
        round: 0,
    });
    // The name they show now.
    entry.stranger = stranger;
    entry.exchanges.push(Exchange {
        they: clip(words),
        you: clip(reply),
    });
    entry.round += 1;
    Some((entry.round, entry.exchanges.len() >= WRITE_EVERY))
}

/// What her note has yet to take in, if this wait is the one to write it:
/// nothing has been said since, or enough has piled up.
fn take(key: &str, round: u64, full: bool) -> Option<Pending> {
    let mut pending = PENDING.lock().ok()?;
    let current = pending
        .get(key)
        .is_some_and(|entry| full || entry.round == round);
    current.then(|| pending.remove(key)).flatten()
}

async fn write_note(
    db: &DatabaseConnection,
    owner: i32,
    venue: &str,
    stranger: &Stranger,
    exchanges: &[Exchange],
    count: i64,
) {
    let kept = note_on(db, venue, stranger).await;
    if kept.is_none() && count < REGULAR_AFTER {
        return;
    }
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let input = json!({
        "name": stranger.name,
        "remembered": kept.as_ref().map(|row| row.content.as_str()),
        "exchanges": exchanges,
    })
    .to_string();
    let raw = super::call::Ask::new(super::call::Voice::HersAtLength, owner, NOTE_SCHEMA)
        .within(NOTE_TIMEOUT)
        .json_raw(&note_system(&soul), &input, NOTE_SCHEMA, &note_schema())
        .await;
    let Some(note) = raw.and_then(|raw| parse_note(&raw)) else {
        return;
    };
    let group = group_venue(venue);
    match (note, kept) {
        // Nothing new: the note stays, and stays fresh.
        (None, Some(row)) => {
            let _ = unified::refresh_unowned(db, &group, &row.id).await;
        }
        (None, None) => {}
        (Some(note), kept) => {
            if let Some(row) = kept {
                if unified::normalize_content(&row.content) == unified::normalize_content(&note) {
                    let _ = unified::refresh_unowned(db, &group, &row.id).await;
                    return;
                }
                let _ = unified::retire_unowned(db, &group, &row.id, "superseded").await;
            }
            let _ =
                unified::remember_in_venue(db, &group, &note, &evidence_of(stranger), SOURCE).await;
        }
    }
}

/// Notes on people she has not run into for a long time fade.
pub async fn let_fade(db: &DatabaseConnection) {
    match unified::fade_source(db, SOURCE, FADE_AFTER).await {
        Ok(faded) if faded > 0 => tracing::info!(faded, "[Merope] notes on strangers faded"),
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not let notes on strangers fade"),
    }
}

#[cfg(test)]
mod pending_tests {
    use super::*;

    fn someone() -> Stranger {
        Stranger {
            who: "telegram:1".into(),
            name: "阿明".into(),
        }
    }

    #[test]
    fn her_note_waits_for_a_pause_or_a_pile() {
        let key = "test|pause";
        let (first, full) = queue(key, someone(), "在吗".into(), "在".into()).unwrap();
        assert!(!full);
        let (second, _) = queue(key, someone(), "练吉他呢".into(), "练多久了".into()).unwrap();
        // The first wait wakes to a newer exchange and yields to it.
        assert!(take(key, first, false).is_none());
        let pending = take(key, second, false).unwrap();
        assert_eq!(pending.exchanges.len(), 2);
        assert_eq!(pending.exchanges[1].they, "练吉他呢");
        assert!(take(key, second, false).is_none(), "written once");

        let key = "test|pile";
        let mut last = (0, false);
        for n in 0..WRITE_EVERY {
            last = queue(key, someone(), format!("第{n}句"), "嗯".into()).unwrap();
        }
        assert!(last.1, "enough piled up to write now");
        assert_eq!(
            take(key, last.0, true).unwrap().exchanges.len(),
            WRITE_EVERY
        );
    }
}

#[cfg(test)]
pub(crate) fn note_probe_contract(soul: &str) -> (String, Value) {
    (note_system(soul), note_schema())
}

#[cfg(test)]
pub(crate) fn note_verdict(raw: &str) -> Option<Option<String>> {
    parse_note(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts outlive a restart: they are in the database, per person and
    /// group, and go with the persona.
    #[tokio::test]
    async fn a_note_is_kept_only_on_someone_she_keeps_running_into() {
        assert_eq!(group_venue("telegram:-9100"), "group:telegram:-9100");
        assert_eq!(
            talks_key("discord:22", "discord:44"),
            "discord:22|discord:44"
        );
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "stranger_talks").await;
        let db = &schema.db;
        let venue = "telegram:-9100";
        assert_eq!(count_exchange(db, venue, "telegram:1").await, 1);
        assert_eq!(count_exchange(db, venue, "telegram:1").await, 2);
        assert_eq!(count_exchange(db, venue, "telegram:2").await, 1);
        assert_eq!(count_exchange(db, "telegram:-9101", "telegram:1").await, 1);
        assert!(count_exchange(db, venue, "telegram:1").await >= REGULAR_AFTER);
        assert_eq!(forget_counts(db).await.unwrap(), 3);
        assert_eq!(count_exchange(db, venue, "telegram:1").await, 1);
        schema.drop().await;
    }
}
