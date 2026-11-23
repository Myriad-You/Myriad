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

#[cfg(test)]
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified;
pub use myriad_merope::strangers::Stranger;
#[cfg(test)]
use myriad_merope::strangers::evidence_of;
#[cfg(test)]
use myriad_merope::strangers::talks_key;
use myriad_merope::strangers::{
    EXCHANGE_CHARS, Exchange, NOTE_SCHEMA, Noted, evidence_marker, evidence_with, facts_of,
    note_schema, note_system, parse_note, their_own,
};
#[cfg(test)]
use serde_json::Value;

pub const SOURCE: &str = "stranger";
/// Exchanges before she starts keeping a note on someone.
const REGULAR_AFTER: i64 = 3;
/// A note nobody has touched this long fades.
const FADE_AFTER: chrono::Duration = chrono::Duration::days(60);
const NOTE_TIMEOUT: Duration = Duration::from_secs(60);
/// Runtime-registry namespace of the exchange counts.
pub const TALKS_NAMESPACE: &str = "merope_stranger_talks";

/// One more exchange with this person in this group; how many so far.
#[cfg(test)]
async fn count_exchange(db: &DatabaseConnection, venue: &str, who: &str) -> i64 {
    let keep_until = (crate::clock::now() + FADE_AFTER).timestamp();
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
    // With something asked of her, they are held to it first, in their own
    // time (no one who pairs waits on it).
    let held = super::asked::anything(db, user_id).await;
    for key in keys {
        let marker = evidence_marker(&format!("{}:{}", platform.slug(), key));
        let taken = if held {
            super::store::memory_jobs::enqueue(
                db,
                &super::memory_jobs::key(&["adopt", &user_id.to_string(), &marker]),
                user_id,
                super::memory_jobs::Payload::Adopt { marker },
                None,
            )
            .await
        } else {
            take_over(db, user_id, &marker).await
        };
        if let Err(error) = taken {
            tracing::warn!(%error, "[Merope] could not take over notes from groups");
        }
    }
}

async fn take_over(db: &DatabaseConnection, user_id: i32, marker: &str) -> anyhow::Result<()> {
    let taken = unified::adopt_unowned(db, SOURCE, marker, user_id, "chat").await?;
    if taken > 0 {
        tracing::info!(user_id, taken, "[Merope] notes from groups taken over");
    }
    Ok(())
}

/// What she noted in groups of `user_id`, whom she knows now (`marker`),
/// theirs ([`adopt`]): not what they asked her not to keep. An `Err`: not all
/// of it could be told now, and none of it is theirs yet; with `gives_up`,
/// it stays as it is.
pub(super) async fn adopt_held(
    db: &DatabaseConnection,
    user_id: i32,
    marker: &str,
    until: std::time::Instant,
    gives_up: bool,
) -> Result<(), super::memory_jobs::Failure> {
    use super::memory_jobs::Failure;
    let storage = |_| Failure::Storage;
    let notes = unified::unowned_about(db, SOURCE, marker)
        .await
        .map_err(storage)?;
    let texts: Vec<String> = notes.iter().map(|note| note.content.clone()).collect();
    let told = super::asked::told_of(db, user_id, None, &texts, until).await;
    for index in &told.out {
        let note = &notes[*index];
        unified::retire_unowned_in_favor_of(db, &note.venue, &note.id, "retracted", None)
            .await
            .map_err(storage)?;
    }
    if !told.untold.is_empty() {
        return told.let_be(gives_up);
    }
    take_over(db, user_id, marker)
        .await
        .map_err(|_| Failure::Storage)
}

/// The stored venue of a group (`telegram:-100123` → `group:telegram:-100123`).
fn group_venue(venue: &str) -> String {
    unified::Audience::group(venue, 0).venue()
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
    // The note is made of facts resting on their own words; what they were
    // answering is someone else's, and shown as that.
    let held = kept
        .as_ref()
        .map(|row| facts_of(&row.content, row.evidence.as_deref()))
        .unwrap_or_default();
    let (theirs, shown) = shown_exchanges(exchanges);
    let input =
        json!({ "name": stranger.name, "remembered": held, "exchanges": shown }).to_string();
    let raw = super::call::Ask::new(super::call::Voice::Thorough, owner, NOTE_SCHEMA)
        .within(NOTE_TIMEOUT)
        .json_raw(&note_system(&soul), &input, NOTE_SCHEMA, &note_schema())
        .await?;
    let note = parse_note(&raw, &held, &theirs).ok_or(super::call::Failure::InvalidOutput)?;
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

/// The exchanges as her note reads them: their own words, the line they
/// answered, her reply; and their own words alone, which a fact must rest on.
fn shown_exchanges(exchanges: &[Exchange]) -> (Vec<String>, Vec<serde_json::Value>) {
    exchanges
        .iter()
        .map(|exchange| {
            let (own, replying) = their_own(&exchange.they);
            let shown = json!({ "they": own, "replyingTo": replying, "you": exchange.you });
            (own, shown)
        })
        .unzip()
}

/// Called inside the same transaction as queue acknowledgement. Only replace
/// the note read by this attempt; a concurrent fade/change cannot be undone.
pub(super) async fn apply_note(
    db: &impl sea_orm::ConnectionTrait,
    venue: &str,
    stranger: &Stranger,
    previous: Option<&str>,
    note: Option<&Noted>,
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
        (Some(noted), kept) => {
            if let Some(row) = &kept
                && unified::normalize_content(&row.content)
                    == unified::normalize_content(&noted.note)
            {
                unified::refresh_unowned(db, &group, &row.id).await?;
                return Ok(());
            }
            // The new note takes the old one's place.
            let new = unified::remember_in_venue(
                db,
                &group,
                &noted.note,
                &evidence_with(stranger, &noted.facts),
                SOURCE,
            )
            .await?;
            if let Some(row) = kept {
                unified::retire_unowned_in_favor_of(
                    db,
                    &group,
                    &row.id,
                    "superseded",
                    new.as_deref(),
                )
                .await?;
            }
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

/// The note an answer comes to, given what she remembered (an older note)
/// and the exchanges' lines of theirs.
#[cfg(test)]
pub(crate) fn note_verdict(
    raw: &str,
    remembered: Option<&str>,
    they: &[String],
) -> Option<Option<String>> {
    let held = remembered
        .map(|note| facts_of(note, None))
        .unwrap_or_default();
    let theirs: Vec<String> = they.iter().map(|said| their_own(said).0).collect();
    parse_note(raw, &held, &theirs).map(|noted| noted.map(|noted| noted.note))
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
        let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
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
        let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
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

    /// A note on someone met in a group, theirs once they pair: not one saying
    /// what they asked not to keep, and none of them while that is not told.
    #[tokio::test]
    async fn what_was_noted_of_them_before_is_held_to_it_when_it_becomes_theirs() {
        use crate::services::agent::merope::asked::tests::asks;
        use crate::services::channel_group::g0;
        use crate::services::channel_platform::ChannelPlatform;
        use sea_orm::ConnectionTrait;
        const THEM: i32 = 7;
        let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "stranger_asked").await;
        let db = &schema.db;
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (7, 'seven')")
            .await
            .unwrap();
        asks(db, THEM, "不要记和房东吵架的事").await;
        for (group, note) in [("group:qq:42", "跟房东闹过"), ("group:qq:43", "在学做面包")]
        {
            unified::remember_in_venue(
                db,
                group,
                note,
                r#"{"who":"onebot:111","name":"某人"}"#,
                SOURCE,
            )
            .await
            .unwrap()
            .unwrap();
        }
        let theirs = async || {
            let mut kept: Vec<String> = unified::active_in(
                db,
                THEM,
                &unified::Audience::group("qq:42", THEM),
                &[unified::MemoryKind::Fact],
            )
            .await
            .unwrap()
            .into_iter()
            .chain(
                unified::active_in(
                    db,
                    THEM,
                    &unified::Audience::group("qq:43", THEM),
                    &[unified::MemoryKind::Fact],
                )
                .await
                .unwrap(),
            )
            .map(|row| row.content)
            .collect();
            kept.sort();
            kept
        };
        // Paired: theirs only once they are held to it, however long that
        // takes; no one waits on it.
        adopt(db, ChannelPlatform::OneBot, &["111".to_string()], THEM).await;
        assert!(theirs().await.is_empty());
        let becomes = async |answers: &[&str]| {
            let mut world = g0::World::new(0.5);
            world.models.insert(
                "not_to_keep".into(),
                answers.iter().map(|one| Some(one.to_string())).collect(),
            );
            g0::enter(world);
            crate::services::agent::merope::memory_jobs::tick(db.clone()).await;
            g0::leave();
            db.execute_unprepared(
                "UPDATE runtime_registry SET payload = \
                   jsonb_set(jsonb_set(payload, '{ready}', '0'), '{lease}', '0') \
                 WHERE namespace = 'merope_asked_jobs'",
            )
            .await
            .unwrap();
        };
        // However it fails: none is theirs, none is let go, and it is tried
        // again.
        for answers in [
            &[][..],
            &["!rejected"],
            &["!quota"],
            &["!rateLimited"],
            &["嗯"; 9],
            &["!refused"; 3],
        ] {
            becomes(answers).await;
            assert!(theirs().await.is_empty(), "{answers:?}");
            let notes = unified::unowned_about(db, SOURCE, "onebot:111")
                .await
                .unwrap();
            assert_eq!(notes.len(), 2, "{answers:?}");
        }
        // Which comes first is the database's: found by what each says.
        let notes = unified::unowned_about(db, SOURCE, "onebot:111")
            .await
            .unwrap();
        let about = notes
            .iter()
            .position(|note| note.content == "跟房东闹过")
            .unwrap();
        becomes(&[&format!(r#"{{"drop":[{about}]}}"#)]).await;
        assert_eq!(theirs().await, ["在学做面包"]);
        schema.drop().await;
    }
}
