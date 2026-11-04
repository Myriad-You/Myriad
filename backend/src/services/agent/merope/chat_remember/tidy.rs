//! The topics she keeps of someone tidied, at upkeep: topics under one
//! name go together, free; the small ones changed since they were last
//! tidied she looks at with the others closest to them, and keeps, puts
//! into another, or lets go (see `myriad_merope::topics::tidy_system_prompt`).
//! Only what changed is looked at, and only once enough has, so tidying
//! costs as much as what was added, not as all that is kept.

use std::time::Duration;

use myriad_merope::topics::{
    TIDY_DUE, TIDY_OTHERS, offered, read_tidied, tidy_input, tidy_schema, tidy_system_prompt,
};
use sea_orm::{DatabaseConnection, TransactionTrait};

use super::super::call::{self, Voice};
use super::super::store;
use crate::services::agent::memory::unified::Audience;

/// People gone over in one upkeep.
const PEOPLE: i64 = 10;
/// Asked once this many topics wait, or once one has waited [`WAITS_AT_MOST`].
const ASK_AT: usize = 6;
const WAITS_AT_MOST: chrono::Duration = chrono::Duration::days(1);
const CALL_TIMEOUT: Duration = Duration::from_secs(90);
pub(in crate::services::agent::merope) const SCHEMA_NAME: &str = "topic_tidy";

/// Tidy the topics of a few people whose topics wait, longest waiting first.
pub async fn tidy_waiting(db: &DatabaseConnection) {
    let waiting = store::with_untidied(db, PEOPLE, ASK_AT, WAITS_AT_MOST)
        .await
        .unwrap_or_default();
    for user_id in waiting {
        tidy_for(db, user_id, false).await;
    }
}

/// What tidying them is to ask: the topics to tidy, at most [`TIDY_DUE`],
/// longest waiting first, and the others closest to them, at most
/// [`TIDY_OTHERS`]. Nothing until enough wait, unless `now`.
pub(in crate::services::agent::merope) async fn to_ask(
    db: &DatabaseConnection,
    user_id: i32,
    now: bool,
) -> Option<(Vec<store::DueTopic>, Vec<store::OfferedTopic>)> {
    let present = Audience::private(user_id);
    let (mut due, others) = store::untidied(db, user_id, &present).await.ok()?;
    let waited = due
        .first()
        .is_some_and(|first| crate::clock::now().fixed_offset() - first.changed >= WAITS_AT_MOST);
    if due.is_empty() || (!now && due.len() < ASK_AT && !waited) {
        return None;
    }
    due.truncate(TIDY_DUE);
    let words = due
        .iter()
        .flat_map(|due| {
            std::iter::once(due.topic.name.clone())
                .chain(due.topic.aliases.iter().cloned())
                .chain(due.topic.holds.iter().cloned())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let seen: Vec<myriad_merope::topics::Topic> = others.iter().map(shown).collect();
    let mut closest = offered(&words, &seen);
    closest.truncate(TIDY_OTHERS);
    let others = closest.into_iter().map(|at| others[at].clone()).collect();
    Some((due, others))
}

/// Their topics tidied, once; `now` asks however few wait. Under the lock
/// going over a stretch writes under.
pub(in crate::services::agent::merope) async fn tidy_for(
    db: &DatabaseConnection,
    user_id: i32,
    now: bool,
) {
    let merged = async {
        let tx = db.begin().await?;
        store::memory_jobs::lock(&tx).await?;
        let merged = store::merge_same_names(&tx, user_id).await?;
        tx.commit().await?;
        anyhow::Ok(merged)
    }
    .await
    .unwrap_or_default();
    let Some((due, others)) = to_ask(db, user_id, now).await else {
        return;
    };
    let due_shown: Vec<myriad_merope::topics::Topic> =
        due.iter().map(|due| shown(&due.topic)).collect();
    let others_shown: Vec<myriad_merope::topics::Topic> = others.iter().map(shown).collect();
    let Ok(raw) = call::Ask::new(Voice::Thorough, user_id, "memory_topics_tidy")
        .within(CALL_TIMEOUT)
        .json_raw(
            &tidy_system_prompt(),
            &tidy_input(&due_shown, &others_shown),
            SCHEMA_NAME,
            &tidy_schema(),
        )
        .await
    else {
        return;
    };
    let tidied = read_tidied(&raw, due.len(), others.len());
    let written = async {
        let tx = db.begin().await?;
        store::memory_jobs::lock(&tx).await?;
        store::apply_tidied(&tx, user_id, &due, &others, &tidied).await?;
        tx.commit().await?;
        anyhow::Ok(())
    }
    .await;
    match written {
        Ok(()) => tracing::info!(
            looked_at = due.len(),
            merged,
            changed = tidied
                .iter()
                .filter(|what| !matches!(what, myriad_merope::topics::Tidied::Keep))
                .count(),
            "[Merope] tidied the topics she keeps"
        ),
        Err(error) => tracing::warn!(%error, "[Merope] could not tidy the topics she keeps"),
    }
}

pub(in crate::services::agent::merope) fn shown(
    topic: &store::OfferedTopic,
) -> myriad_merope::topics::Topic<'_> {
    myriad_merope::topics::Topic {
        name: &topic.name,
        aliases: &topic.aliases,
        holds: &topic.holds,
    }
}
