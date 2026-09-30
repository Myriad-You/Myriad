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
//! in between resumes the durable batch after its lease expires.

use std::time::Duration;

use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified;
#[cfg(test)]
use myriad_merope::strangers::talks_key;
use myriad_merope::strangers::{
    EXCHANGE_CHARS, Exchange, NOTE_SCHEMA, evidence_marker, evidence_of, note_schema, note_system,
    parse_note, without_directives,
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
/// Runtime-registry namespace of the exchange counts.
pub const TALKS_NAMESPACE: &str = "merope_stranger_talks";

/// One more exchange with this person in this group; how many so far.
#[cfg(test)]
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

/// Someone she knew only from groups has paired `platform` account `keys`
/// with `user_id`: what she noted on them there is theirs now, an ordinary
/// memory of that group. Only accounts proven theirs are taken over; nobody
/// is matched by name.
pub async fn adopt(
    db: &DatabaseConnection,
    platform: crate::services::channel_platform::ChannelPlatform,
    keys: &[String],
    user_id: i32,
) {
    for key in keys {
        let who = format!("{}:{}", platform.slug(), key);
        match unified::adopt_unowned(db, SOURCE, &evidence_marker(&who), user_id, "chat").await {
            Ok(0) => {}
            Ok(taken) => tracing::info!(user_id, taken, "[Merope] notes from groups taken over"),
            Err(error) => tracing::warn!(%error, "[Merope] could not take over notes from groups"),
        }
    }
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
    why: Option<&str>,
    // How the group types, how her lines differ from theirs, and what she
    // made of the talk, as for anyone in the group.
    reading: &[String],
) -> Option<(String, Option<serde_json::Value>)> {
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let note = note_on(db, venue, stranger).await.map(|row| row.content);
    let mut sections = vec![
        super::group_speaking_section(&stranger.name),
        myriad_merope::speaking::chat_app_section().to_string(),
        section(&stranger.name, note.as_deref()),
        super::speaking_prompts::format_now_section(chrono::Local::now()),
    ];
    // Whom she takes them for, if anyone: a guess, nothing more.
    if let Some(block) = super::recognizing::guess_section(db, venue, stranger).await {
        sections.push(block);
    }
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
    if let Some(block) = super::format_group_days_section(&super::bits::days_in(db, venue, 3).await)
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
    // Stickers of her: hers, and this group's own.
    let sticker_key = format!("stranger:{venue}:{}", stranger.who);
    if let Some(stickers) = super::stickers::section(db, &sticker_key, Some(venue), words).await {
        sections.push(stickers);
    }
    sections.extend(reading.iter().cloned());
    // They did not call her: she speaks for a reason of her own.
    if let Some(why) = why {
        sections.push(myriad_merope::joining::speaking_up_section(why));
    }
    let prompt = crate::services::agent::chat_prompt::build_group_chat_prompt(
        &soul,
        &sections.join("\n\n"),
        transcript,
        words,
    );
    let model = super::call::Ask::new(super::call::Voice::Hers, owner, "group_stranger")
        .within(REPLY_TIMEOUT)
        .model()
        .await
        .ok()?;
    // A reply the provider dropped halfway reached no one: ask once more.
    let mut raw = model.say(&prompt).await;
    if raw.as_ref().is_err_and(super::call::was_cut) {
        raw = model.say(&prompt).await;
    }
    let raw = raw.ok()?;
    let (said, started) = super::soup::split_start(&raw);
    let (said, sticker) = myriad_merope::stickers::split_sticker_directive(&said);
    let sticker = match sticker {
        Some(choice) => super::stickers::chosen_in(db, &sticker_key, choice).await,
        None => None,
    };
    let mut text = without_directives(&said);
    // She said she would think one up: the puzzle follows her words.
    if started {
        let opening = super::soup::start_at(&table, words, owner).await;
        text = format!("{text}\n\n{opening}").trim().to_string();
    }
    // A game this line ended is over, and the group remembers it.
    super::soup::after_turn_at(db, &table).await;
    (!text.is_empty() || sticker.is_some()).then_some((text, sticker))
}

/// After she answered someone from outside: count it, and when they pause
/// (or after several exchanges), let her note on them catch up with all of
/// it, once they are someone she keeps running into.
pub async fn enqueue_after(
    db: &DatabaseConnection,
    owner: i32,
    venue: String,
    stranger: Stranger,
    words: String,
    reply: String,
    event_id: &str,
) {
    let clip = |text: String| {
        super::ingest::redact_event_text(&text)
            .chars()
            .take(EXCHANGE_CHARS)
            .collect()
    };
    let id = super::memory_jobs::key(&["stranger", &venue, &stranger.who]);
    let event = super::memory_jobs::key(&["stranger", &venue, &stranger.who, event_id]);
    let data = super::memory_jobs::Payload::Stranger {
        venue,
        stranger,
        exchanges: vec![Exchange {
            they: clip(words),
            you: clip(reply),
        }],
        count: 0,
    };
    if !matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            super::store::memory_jobs::enqueue(db, &id, owner, data, Some(&event))
        )
        .await,
        Ok(Ok(()))
    ) {
        tracing::warn!(
            owner,
            outcome = "enqueue_failed",
            "[Merope] stranger memory"
        );
    }
}

pub(super) async fn prepare_note(
    db: &DatabaseConnection,
    owner: i32,
    venue: &str,
    stranger: &Stranger,
    exchanges: &[Exchange],
    count: i64,
) -> Result<super::memory_jobs::Effect, super::memory_jobs::Failure> {
    use super::memory_jobs::{Effect, Failure};
    let kept = unified::unowned_with_evidence(
        db,
        &group_venue(venue),
        SOURCE,
        &evidence_marker(&stranger.who),
    )
    .await
    .map_err(|_| Failure::Storage)?;
    if kept.is_none() && count < REGULAR_AFTER {
        return Ok(Effect::NoChange);
    }
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let input = json!({ "name": stranger.name, "remembered": kept.as_ref().map(|row| row.content.as_str()), "exchanges": exchanges }).to_string();
    let raw = super::call::Ask::new(super::call::Voice::HersAtLength, owner, NOTE_SCHEMA)
        .within(NOTE_TIMEOUT)
        .json_raw(&note_system(&soul), &input, NOTE_SCHEMA, &note_schema())
        .await?;
    let note = parse_note(&raw).ok_or(super::call::Failure::InvalidOutput)?;
    // Having talked with them a while, whether they might be someone she
    // knows from elsewhere.
    let said: Vec<String> = exchanges
        .iter()
        .map(|exchange| exchange.they.clone())
        .collect();
    super::recognizing::consider(db, owner, venue, stranger, &said).await;
    Ok(Effect::Stranger {
        previous: kept.map(|row| row.id),
        note,
    })
}

/// Called inside the same transaction as queue acknowledgement. Only replace
/// the note read by this attempt; a concurrent fade/change cannot be undone.
pub(super) async fn apply_note(
    db: &impl sea_orm::ConnectionTrait,
    venue: &str,
    stranger: &Stranger,
    previous: Option<&str>,
    note: Option<&str>,
) -> anyhow::Result<()> {
    let group = group_venue(venue);
    let kept =
        unified::unowned_with_evidence(db, &group, SOURCE, &evidence_marker(&stranger.who)).await?;
    if kept.as_ref().map(|row| row.id.as_str()) != previous {
        return Ok(());
    }
    match (note, kept) {
        (None, Some(row)) => {
            unified::refresh_unowned(db, &group, &row.id).await?;
        }
        (None, None) => {}
        (Some(note), kept) => {
            if let Some(row) = kept {
                if unified::normalize_content(&row.content) == unified::normalize_content(note) {
                    unified::refresh_unowned(db, &group, &row.id).await?;
                    return Ok(());
                }
                unified::retire_unowned(db, &group, &row.id, "superseded").await?;
            }
            unified::remember_in_venue(db, &group, note, &evidence_of(stranger), SOURCE).await?;
        }
    }
    Ok(())
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

    /// Someone she knew only as a stranger in a group pairs that account:
    /// her note on them is theirs, in that group only; a note on anyone else
    /// stays as it was.
    #[tokio::test]
    async fn pairing_takes_over_what_she_noted_on_that_account_only() {
        use crate::services::channel_platform::ChannelPlatform;
        use sea_orm::{ConnectionTrait, DatabaseBackend, EntityTrait, Statement};
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "stranger_adopt").await;
        let db = isolated.db.clone();
        let user_id: i32 = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username) VALUES ('adopt-test') RETURNING id",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "id")
            .unwrap();
        let venue = "onebot:1057102407";
        let group = group_venue(venue);
        let them = Stranger {
            who: "onebot:3059342645".into(),
            name: "leaphy".into(),
        };
        let other = Stranger {
            who: "onebot:111".into(),
            name: "别人".into(),
        };
        let mine = unified::remember_in_venue(
            &db,
            &group,
            "leaphy 在做自己的 bot",
            &evidence_of(&them),
            SOURCE,
        )
        .await
        .unwrap()
        .unwrap();
        let theirs =
            unified::remember_in_venue(&db, &group, "别人喜欢拉面", &evidence_of(&other), SOURCE)
                .await
                .unwrap()
                .unwrap();
        adopt(
            &db,
            ChannelPlatform::OneBot,
            &["3059342645".to_string()],
            user_id,
        )
        .await;
        let row = |id: String| {
            let db = db.clone();
            async move {
                agent_memories::Entity::find_by_id(id)
                    .one(&db)
                    .await
                    .unwrap()
                    .unwrap()
            }
        };
        let adopted = row(mine).await;
        assert_eq!(adopted.user_id, Some(user_id));
        assert_eq!(adopted.source, "chat");
        assert_eq!(adopted.venue, group);
        let untouched = row(theirs).await;
        assert_eq!(untouched.user_id, None);
        assert_eq!(untouched.source, SOURCE);
        // It comes up in that group, and never in private.
        let in_group = unified::recall(
            &db,
            user_id,
            &unified::Audience::group(venue, user_id),
            Some("bot"),
            &[],
            8,
        )
        .await
        .unwrap();
        assert!(in_group.iter().any(|memory| memory.content.contains("bot")));
        let in_private = unified::recall(
            &db,
            user_id,
            &unified::Audience::private(user_id),
            Some("bot"),
            &[],
            8,
        )
        .await
        .unwrap();
        assert!(in_private.is_empty());
        isolated.drop().await;
    }
}
