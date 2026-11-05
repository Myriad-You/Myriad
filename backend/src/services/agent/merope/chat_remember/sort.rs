//! What she knows of someone sorted under topics, for facts kept before she
//! put facts under topics, or kept without one (a note going over passed
//! over): a batch at a time, at upkeep, billed to whom they are about (see
//! `myriad_merope::topics::sort_system_prompt`). Going over a stretch of
//! talk sorts what it keeps itself; this sorts the rest the same way.

use std::time::Duration;

use myriad_merope::topics::{SORT_BATCH, read_sorted, sort_input, sort_schema, sort_system_prompt};
use sea_orm::{DatabaseConnection, TransactionTrait};

use super::super::call::{self, Voice};
use super::super::store::{self, GoesUnder};
use crate::services::agent::memory::unified::{Audience, MemoryRecord};

/// People gone over in one upkeep, and how many batches each.
const PEOPLE: i64 = 10;
const ASKS: usize = 3;
const CALL_TIMEOUT: Duration = Duration::from_secs(90);
pub(in crate::services::agent::merope) const SCHEMA_NAME: &str = "topic_sort";

/// Sort what is still to be sorted, for a few people; and, while her nodes
/// are written, what is on no node yet onto them, the same way
/// (`memory_nodes::sorting`).
pub async fn sort_missing(db: &DatabaseConnection) {
    for user_id in store::with_unsorted(db, PEOPLE).await.unwrap_or_default() {
        sort_for(db, user_id, ASKS).await;
    }
    super::super::memory_nodes::sorting::sort_missing(db).await;
}

/// Up to `asks` batches of what she knows of them in private; how many
/// facts got where they go. Stops at the first batch that sorts nothing.
pub(in crate::services::agent::merope) async fn sort_for(
    db: &DatabaseConnection,
    user_id: i32,
    asks: usize,
) -> usize {
    let present = Audience::private(user_id);
    let mut sorted = 0;
    for _ in 0..asks {
        let rows = store::unsorted(db, user_id, &present, SORT_BATCH)
            .await
            .unwrap_or_default();
        if rows.is_empty() {
            break;
        }
        let done = sort_batch(db, user_id, &present, &rows).await;
        sorted += done;
        if done == 0 {
            break;
        }
    }
    if sorted > 0 {
        tracing::info!(sorted, "[Merope] sorted what she knows under topics");
    }
    sorted
}

/// Ask once for `rows`; how many were sorted.
async fn sort_batch(
    db: &DatabaseConnection,
    user_id: i32,
    present: &Audience,
    rows: &[MemoryRecord],
) -> usize {
    let facts: Vec<String> = rows.iter().map(store::as_known).collect();
    let offered = store::offered_topics(db, user_id, present, &facts.join("\n"))
        .await
        .unwrap_or_default();
    let Ok(raw) = call::Ask::new(Voice::Thorough, user_id, "memory_topics")
        .within(CALL_TIMEOUT)
        .json_raw(
            &sort_system_prompt(),
            &sort_input(super::settle::shown_topics(&offered), &facts),
            SCHEMA_NAME,
            &sort_schema(),
        )
        .await
    else {
        return 0;
    };
    let placed = placed(&raw, rows, &offered);
    if placed.is_empty() {
        return 0;
    }
    // Under the lock going over a stretch writes under, so a topic one
    // starts is seen by the other.
    let written = async {
        let tx = db.begin().await?;
        store::memory_jobs::lock(&tx).await?;
        for (id, goes) in &placed {
            store::put_under(&tx, user_id, present, id, &[], Some(goes)).await?;
        }
        tx.commit().await?;
        anyhow::Ok(())
    }
    .await;
    match written {
        Ok(()) => placed.len(),
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not sort what she knows under topics");
            0
        }
    }
}

/// Where her answer puts each of `rows`, by memory id. A fact she gave no
/// readable place stays to be sorted: she is asked of it again at a later
/// upkeep, beside others.
pub(in crate::services::agent::merope) fn placed(
    raw: &str,
    rows: &[MemoryRecord],
    offered: &[store::OfferedTopic],
) -> Vec<(String, GoesUnder)> {
    read_sorted(raw, offered.len(), rows.len())
        .into_iter()
        .filter_map(|(index, under)| {
            Some((
                rows[index].id.clone(),
                super::settle::goes_under(&under, offered)?,
            ))
        })
        .collect()
}
