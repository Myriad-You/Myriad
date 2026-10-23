//! Going over a stretch of talk once it is over (see the module above): the
//! short notes taken from it, with the whole stretch in view.

use serde_json::json;

use super::super::ingest::compact_summary;
use super::{ChatMemoryUpdate, ChatMemoryUpdates, EXTRACT_TIMEOUT};

/// Lines of a stretch, at most, shown when it is gone over.
const SETTLE_LINES: i64 = 80;
/// What she knows of them for good, at most, shown beside it.
const SETTLE_KNOWN: usize = 40;

/// Once a stretch of their talk in one place is over: the short notes taken
/// from it since `from`, gone over with the whole stretch in view. What
/// lasts is kept for good (put together, as it is now), what it shows has
/// changed is replaced, and the notes it puts together are taken back in
/// its favor; the rest stay notes and fade. Nothing to go over, no call.
pub(in crate::services::agent::merope) async fn settle(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    from: chrono::DateTime<chrono::FixedOffset>,
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
    if notes.is_empty() {
        return Ok(Effect::NoChange);
    }
    notes.sort_by_key(|row| row.created_at);
    let mut known: Vec<&unified::MemoryRecord> =
        rows.iter().filter(|row| row.source == "chat").collect();
    known.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    let known: Vec<String> = known
        .into_iter()
        .take(SETTLE_KNOWN)
        .map(|row| compact_summary(&row.content))
        .collect();
    // The stretch as it was said, from its first line (a little before, so
    // what they answered is there too).
    let venue = present.group_id();
    let lines = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.role, m.content FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
               AND m.role IN ('user', 'assistant') AND m.created_at >= $3 \
             ORDER BY m.created_at DESC LIMIT $4",
            [
                user_id.into(),
                venue.map(str::to_string).into(),
                (from - chrono::Duration::minutes(2)).into(),
                SETTLE_LINES.into(),
            ],
        ))
        .await
        .map_err(|_| Failure::Storage)?;
    let mut conversation: Vec<(bool, String)> = lines
        .iter()
        .filter_map(|row| {
            let role: String = row.try_get("", "role").ok()?;
            let content: String = row.try_get("", "content").ok()?;
            Some((role == "user", content))
        })
        .collect();
    conversation.reverse();
    let theirs = conversation
        .iter()
        .filter(|(theirs, _)| *theirs)
        .map(|(_, text)| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    if theirs.is_empty() {
        return Ok(Effect::NoChange);
    }
    let lately: Vec<String> = notes
        .iter()
        .map(|row| compact_summary(&row.content))
        .collect();
    let input = input(
        &conversation,
        &lately,
        &super::super::clock::local_now()
            .format("%Y-%m-%d (%A)")
            .to_string(),
    );
    let raw =
        super::super::call::Ask::new(super::super::call::Voice::Thorough, user_id, "chat_settle")
            .within(EXTRACT_TIMEOUT)
            .json_raw(
                &myriad_merope::chat_remember::settle_system_prompt(&known),
                &input,
                myriad_merope::chat_remember::SETTLE_SCHEMA_NAME,
                &myriad_merope::chat_remember::settle_schema(),
            )
            .await?;
    let (lasting, replaces) =
        myriad_merope::chat_remember::parse_settle(&raw, &theirs, &known, notes.len())
            .ok_or(super::super::call::Failure::InvalidOutput)?;
    if lasting.is_empty() && replaces.is_empty() {
        return Ok(Effect::NoChange);
    }
    let mut updates = ChatMemoryUpdates::default();
    let mut settled = Vec::new();
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
            fact: Some(compact_summary(&item.fact)),
            // What it changes goes with the first: it says how it is now.
            supersedes: if updates.updates.is_empty() {
                replaces.clone()
            } else {
                Vec::new()
            },
            evidence: Some(item.evidence),
            concepts: unified::clean_concepts(
                serde_json::from_value(item.concepts).unwrap_or_default(),
            ),
            cues: item.when_for,
        });
    }
    Ok(Effect::Settle { updates, settled })
}

/// What going over a stretch is given: the stretch as it was said (`true`
/// on their lines), the short notes taken from it, and today.
pub(super) fn input(conversation: &[(bool, String)], lately: &[String], today: &str) -> String {
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
    })
    .to_string()
}
