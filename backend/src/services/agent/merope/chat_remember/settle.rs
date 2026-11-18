//! Going over a stretch of talk once it is over (see the module above): the
//! short notes taken from it, with the whole stretch in view.

use serde_json::json;

use super::super::ingest::compact_summary;
use super::{ChatMemoryUpdate, ChatMemoryUpdates};

/// Lines of a stretch, at most, shown when it is gone over.
pub(in crate::services::agent::merope) const SETTLE_LINES: i64 = 80;
/// What she knows of them for good, at most, shown beside it.
const SETTLE_KNOWN: usize = 40;
/// Short notes gone over at once, at most: a stretch with more is gone over
/// in passes, oldest first (see [`first_pass`]).
const SETTLE_NOTES: usize = 9;
/// How long going over a stretch may take. Nobody waits on it (it runs well
/// after she has answered): generous enough to ride out a slow answer.
pub(in crate::services::agent::merope) const SETTLE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(90);

/// Once a stretch of their talk in one place is over: the short notes taken
/// from it since `from`, gone over with the whole stretch in view. What
/// lasts is kept for good (put together, as it is now), what it shows has
/// changed is replaced, and the notes it puts together are taken back in
/// its favor; the rest stay notes and fade, but only those it leaves to
/// the moment or finds already known: a note it passes over is kept for
/// good as it is. Nothing to go over, no call.
///
/// At most [`SETTLE_NOTES`] notes in one pass, the oldest: the rest is gone
/// over in the next, from where it begins (`Effect::Settle::rest`), once
/// what this pass keeps is written, so the next sees it among what she
/// knows and does not keep the same thing twice. Each pass is shown the
/// stretch's lines from where they begin (`said_from`, when it is gone
/// over in passes; else `from`) up to where its own notes end.
pub(in crate::services::agent::merope) async fn settle(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    from: chrono::DateTime<chrono::FixedOffset>,
    said_from: Option<chrono::DateTime<chrono::FixedOffset>>,
) -> Result<super::super::memory_jobs::Effect, super::super::memory_jobs::Failure> {
    use super::super::memory_jobs::{Effect, Failure};
    use crate::services::agent::memory::unified;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = unified::active_in(db, user_id, present, &unified::MemoryKind::ABOUT_PERSON)
        .await
        .map_err(|_| Failure::Storage)?;
    // Those of this place only: `active_in` is what may be said here.
    let mut notes: Vec<&unified::MemoryRecord> = rows
        .iter()
        .filter(|row| row.source == unified::SHORT_NOTE_SOURCE && row.created_at >= from)
        .collect();
    // A private talk's stretch is placed on her nodes first, notes or none
    // (see `memory_nodes::placing`).
    let nodes = !present.is_group();
    if notes.is_empty() && !nodes {
        return Ok(Effect::NoChange);
    }
    notes.sort_by_key(|row| row.created_at);
    let this_pass = first_pass(&notes.iter().map(|row| row.created_at).collect::<Vec<_>>());
    let rest = notes.get(this_pass).map(|row| row.created_at);
    notes.truncate(this_pass);
    // A pass that keeps nothing still lets the next go on.
    let nothing = || match rest {
        Some(_) => Effect::Settle {
            updates: ChatMemoryUpdates::default(),
            settled: Vec::new(),
            rest,
        },
        None => Effect::NoChange,
    };
    // What she kept from talks here: what another end kept
    // (`unified::SYNC_SOURCE`) is that end's, and going over leaves it be.
    let mut known: Vec<&unified::MemoryRecord> =
        rows.iter().filter(|row| row.source == "chat").collect();
    known.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    let known: Vec<String> = known
        .into_iter()
        .take(SETTLE_KNOWN)
        .map(|row| compact_summary(&row.content))
        .collect();
    // The stretch as it was said, from its first line (a little before, so
    // what they answered is there too), up to where the notes gone over now
    // end.
    let venue = present.group_id();
    let begins = said_from.unwrap_or(from) - chrono::Duration::minutes(2);
    let lines = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.role, m.content, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
               AND m.role IN ('user', 'assistant') AND (m.metadata->>'piece') IS NULL AND m.created_at >= $3 \
               AND ($5::timestamptz IS NULL OR m.created_at < $5) \
             ORDER BY m.created_at DESC LIMIT $4",
            [
                user_id.into(),
                venue.map(str::to_string).into(),
                begins.into(),
                SETTLE_LINES.into(),
                rest.into(),
            ],
        ))
        .await
        .map_err(|_| Failure::Storage)?;
    // Each line with its message, so a line they took something back in is
    // known by its message (`take_back_check`).
    let mut said: Vec<(i32, bool, String)> = lines
        .iter()
        .filter_map(|row| {
            let id: i32 = row.try_get("", "id").ok()?;
            let role: String = row.try_get("", "role").ok()?;
            let content: String = row.try_get("", "content").ok()?;
            Some((id, role == "user", content))
        })
        .collect();
    said.reverse();
    let conversation: Vec<(bool, String)> = said
        .iter()
        .map(|(_, theirs, text)| (*theirs, text.clone()))
        .collect();
    let mut placed = Vec::new();
    let mut site_owner = None;
    if nodes {
        use super::super::memory_nodes::placing::{self, Line};
        let mut stretch: Vec<Line> = lines
            .iter()
            .filter_map(|row| {
                Some(Line {
                    id: row.try_get("", "id").ok()?,
                    theirs: row.try_get::<String>("", "role").ok()? == "user",
                    text: row.try_get("", "content").ok()?,
                    at: row.try_get("", "created_at").ok()?,
                })
            })
            .collect();
        stretch.reverse();
        // Said in more lines than going over is shown: those before are
        // placed later, at her upkeep.
        if lines.len() as i64 >= SETTLE_LINES
            && let Some(oldest) = stretch.first()
            && oldest.at > begins
        {
            placing::backlog::keep(
                db,
                user_id,
                begins,
                oldest.at - chrono::Duration::microseconds(1),
            )
            .await;
        }
        site_owner = crate::services::principal::site_owner_id(db)
            .await
            .ok()
            .flatten();
        placed = placing::place(db, user_id, site_owner, &stretch).await;
    }
    if notes.is_empty() {
        return Ok(Effect::NoChange);
    }
    let theirs = conversation
        .iter()
        .filter(|(theirs, _)| *theirs)
        .map(|(_, text)| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    if theirs.is_empty() {
        return Ok(nothing());
    }
    let lately: Vec<String> = notes
        .iter()
        .map(|row| compact_summary(&row.content))
        .collect();
    // The topics on her nodes this stretch most likely touches, to put
    // what lasts on (none outside a private talk).
    let offered = if nodes {
        super::super::memory_nodes::facts::shown_topics(db, user_id, site_owner, &theirs, &placed)
            .await
    } else {
        Vec::new()
    };
    let input = input(
        &conversation,
        &lately,
        &super::super::clock::local_now()
            .format("%Y-%m-%d (%A)")
            .to_string(),
        &shown_topics(&offered),
    );
    let raw =
        super::super::call::Ask::new(super::super::call::Voice::Thorough, user_id, "chat_settle")
            .within(SETTLE_TIMEOUT)
            .json_raw(
                &myriad_merope::chat_remember::settle_system_prompt(
                    &known,
                    myriad_merope::chat_remember::Examples::for_words(&theirs),
                ),
                &input,
                myriad_merope::chat_remember::SETTLE_SCHEMA_NAME,
                &myriad_merope::chat_remember::settle_schema(),
            )
            .await?;
    let myriad_merope::chat_remember::Settled {
        lasting,
        took_back,
        unkept_replaces,
    } = myriad_merope::chat_remember::parse_settle(
        &raw,
        &theirs,
        &conversation
            .iter()
            .map(|(theirs, _)| *theirs)
            .collect::<Vec<_>>(),
        &known,
        notes.len(),
        offered.len(),
    )
    .ok_or(super::super::call::Failure::InvalidOutput)?;
    // A fact written in another language than they spoke is left to its
    // notes; how many, never what.
    let changed = myriad_merope::chat_remember::settle_changed_language(&raw, &theirs);
    if changed > 0 {
        tracing::info!(
            user_id,
            changed,
            "[Merope] chat settle: facts in another language than they spoke were left to their notes"
        );
    }
    // What only facts not kept said they replaced stays as it was: how
    // many, never what.
    if unkept_replaces > 0 {
        tracing::info!(
            entries = unkept_replaces,
            "[chat-remember] what facts not kept would have replaced stays as it was"
        );
    }
    let left = myriad_merope::chat_remember::settle_left(&raw, &lasting, notes.len());
    if lasting.is_empty() && took_back.is_empty() && left.is_empty() {
        return Ok(nothing());
    }
    let mut updates = ChatMemoryUpdates::default();
    let mut settled = Vec::new();
    // What they took back first, with nothing in its place: let go as a
    // taking back, never as replaced by a fact kept beside it. Each on its
    // own: one already gone does not keep the others from going.
    for one in took_back {
        settled.push(Vec::new());
        if let Some((message, _, _)) = one.said.and_then(|index| said.get(index)) {
            updates
                .taken_back_in
                .insert(updates.updates.len(), *message);
        }
        updates.updates.push(ChatMemoryUpdate {
            standing: Default::default(),
            fact: None,
            supersedes: vec![one.entry],
            evidence: None,
            concepts: Vec::new(),
            topic: None,
            cues: Vec::new(),
        });
    }
    for item in lasting {
        let mut from: Vec<String> = item
            .from
            .iter()
            .map(|index| notes[*index].id.clone())
            .collect();
        from.sort();
        from.dedup();
        settled.push(from);
        updates.updates.push(ChatMemoryUpdate {
            standing: item.standing.clone(),
            fact: Some(compact_summary(&item.fact)),
            // What it says has changed: it says how it is now.
            supersedes: item.replaces.clone(),
            evidence: Some(item.evidence),
            concepts: unified::clean_concepts(
                serde_json::from_value(item.concepts).unwrap_or_default(),
            ),
            cues: item.when_for,
            topic: goes_under(&item.topic, &offered),
        });
    }
    for index in left {
        let note = notes[index];
        settled.push(vec![note.id.clone()]);
        updates.updates.push(ChatMemoryUpdate {
            standing: Default::default(),
            fact: Some(compact_summary(&note.content)),
            supersedes: Vec::new(),
            evidence: note.evidence.clone(),
            concepts: unified::clean_concepts(note.concepts.clone()),
            topic: None,
            cues: Vec::new(),
        });
    }
    Ok(Effect::Settle {
        updates,
        settled,
        rest,
    })
}

/// Of the notes taken at `at` (oldest first), how many are gone over now:
/// at most [`SETTLE_NOTES`]. Notes taken at the same moment are never
/// parted, so the next pass, gone over from where its first note was taken,
/// holds none of these; should more than that share one moment, they are
/// gone over together.
pub(in crate::services::agent::merope) fn first_pass(
    at: &[chrono::DateTime<chrono::FixedOffset>],
) -> usize {
    if at.len() <= SETTLE_NOTES {
        return at.len();
    }
    let mut end = SETTLE_NOTES;
    while end > 0 && at[end] == at[end - 1] {
        end -= 1;
    }
    if end == 0 {
        end = at.iter().take_while(|taken| **taken == at[0]).count();
    }
    end
}

/// The topics `offered` as she sees them beside a stretch.
pub(in crate::services::agent::merope) fn shown_topics(
    offered: &[super::super::memory_nodes::facts::OfferedTopic],
) -> serde_json::Value {
    let topics: Vec<myriad_merope::topics::Topic> = offered
        .iter()
        .map(|topic| myriad_merope::topics::Topic {
            name: &topic.name,
            aliases: &topic.aliases,
            holds: &topic.holds,
        })
        .collect();
    myriad_merope::topics::shown(&topics, &(0..topics.len()).collect::<Vec<_>>())
}

/// Where going over or sorting put a fact, by the topics it was `offered`;
/// `None` only for a label that is not among them.
pub(in crate::services::agent::merope) fn goes_under(
    under: &myriad_merope::topics::Under,
    offered: &[super::super::memory_nodes::facts::OfferedTopic],
) -> Option<super::super::memory_nodes::facts::GoesUnder> {
    use myriad_merope::topics::Under;
    match under {
        Under::Nowhere => Some(super::super::memory_nodes::facts::GoesUnder::Nowhere),
        // About them as a person: under no topic, on their own node.
        Under::AboutThem => Some(super::super::memory_nodes::facts::GoesUnder::AboutThem),
        // One of her nodes shown beside the topics: a topic by its names
        // here (`memory_nodes::facts` finds the node by them again).
        Under::Offered(index) => offered.get(*index).map(|topic| {
            super::super::memory_nodes::facts::GoesUnder::New {
                name: topic.name.clone(),
                aliases: topic.aliases.clone(),
            }
        }),
        Under::New { name, aliases } => Some(super::super::memory_nodes::facts::GoesUnder::New {
            name: name.clone(),
            aliases: aliases.clone(),
        }),
    }
}

/// What going over a stretch is given: the stretch as it was said (`true`
/// on their lines), the short notes taken from it, today, and the topics
/// she keeps facts under that it most likely touches (`shown_topics`).
pub(super) fn input(
    conversation: &[(bool, String)],
    lately: &[String],
    today: &str,
    topics: &serde_json::Value,
) -> String {
    json!({
        "conversation": conversation
            .iter()
            .map(|(theirs, text)| json!({
                "who": if *theirs { "them" } else { "you" },
                "text": text.chars().take(600).collect::<String>(),
            }))
            .collect::<Vec<_>>(),
        "lately": lately,
        "today": today,
        "topics": topics,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{SETTLE_NOTES, first_pass};

    fn at(seconds: &[i64]) -> Vec<chrono::DateTime<chrono::FixedOffset>> {
        seconds
            .iter()
            .map(|second| {
                chrono::DateTime::from_timestamp(1_700_000_000 + second, 0)
                    .unwrap()
                    .fixed_offset()
            })
            .collect()
    }

    #[test]
    fn a_long_stretch_is_gone_over_nine_notes_at_a_time_never_parting_a_moment() {
        assert_eq!(SETTLE_NOTES, 9);
        // Few enough: all at once.
        assert_eq!(first_pass(&at(&[])), 0);
        assert_eq!(first_pass(&at(&[1, 2, 3, 4, 5, 6, 7, 8, 9])), 9);
        // More: the oldest nine, the rest from the tenth.
        assert_eq!(first_pass(&at(&(1..=20).collect::<Vec<_>>())), 9);
        // The ninth and tenth taken at once: both wait for the next pass,
        // which begins where they were taken.
        assert_eq!(first_pass(&at(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 9, 10])), 8);
        assert_eq!(first_pass(&at(&[1, 1, 1, 1, 1, 1, 1, 1, 9, 9, 9, 9])), 8);
        // More than nine taken at once are gone over together.
        assert_eq!(first_pass(&at(&[1; 11])), 11);
        assert_eq!(first_pass(&at(&[1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 3])), 10);
    }
}
