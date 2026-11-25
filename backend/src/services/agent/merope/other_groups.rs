//! What she brings into a group from the other groups she is in (see
//! `myriad_merope::other_groups`): the same person met there, what they said
//! about themselves there, and those groups' own bits and days. Each by the
//! group's name, as its platform gives it.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use myriad_merope::other_groups::{FromGroup, Theirs};

use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified;

/// Their own memories from other groups the talk can bring back from.
const LOOKED_OVER: u64 = 300;
/// What they said in other groups a turn brings back.
const SAID_SHOWN: usize = 3;
/// Other groups she names them from.
const GROUPS_SHOWN: i64 = 5;
/// Other groups' bits and days the talk can bring back from.
const THEIRS_LOOKED_OVER: u64 = 500;
/// Other groups' bits and days a turn brings back.
const THEIRS_SHOWN: usize = 3;

/// Rows kept in other groups, each with that group's name.
async fn named(rows: Vec<agent_memories::Model>) -> Vec<FromGroup> {
    let mut from = Vec::new();
    for row in rows {
        let group = match row.venue.strip_prefix("group:") {
            Some(venue) => crate::services::channel_group::title(venue).await,
            None => None,
        };
        from.push(FromGroup {
            group,
            text: row.content,
        });
    }
    from
}

/// Someone from the community talking in the group `here` (as sessions
/// keep it): the other groups she talks with them in, and what they said
/// about themselves there that `words` bring back.
pub async fn they_section(
    db: &DatabaseConnection,
    user_id: i32,
    here: &str,
    words: &str,
) -> Option<String> {
    let venues: Vec<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT context->>'venue' AS venue FROM agent_sessions \
             WHERE user_id = $1 AND context->>'mode' = 'chat' AND (context->>'past') IS NULL \
               AND context->>'venue' IS NOT NULL AND context->>'venue' <> $2 \
             GROUP BY 1 ORDER BY max(last_active_at) DESC LIMIT $3",
            [user_id.into(), here.into(), GROUPS_SHOWN.into()],
        ))
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| row.try_get::<String>("", "venue").ok())
        .collect();
    let mut groups = Vec::new();
    for venue in &venues {
        groups.push(crate::services::channel_group::title(venue).await);
    }
    let here = unified::Audience::group(here, user_id).venue();
    let theirs = unified::in_other_groups(
        db,
        Some(user_id),
        &here,
        &["chat", unified::SHORT_NOTE_SOURCE],
        LOOKED_OVER,
    )
    .await;
    let said = named(unified::named_among(db, theirs, words, SAID_SHOWN).await).await;
    myriad_merope::other_groups::they_section(&groups, &said)
}

/// What `words`, said in the group `here` (as sessions keep it), bring back
/// of her other groups' own bits and days, each by the group's name.
pub async fn theirs_section(db: &DatabaseConnection, here: &str, words: &str) -> Option<String> {
    let here = unified::Audience::group(here, 0).venue();
    let rows = unified::in_other_groups(
        db,
        None,
        &here,
        &[super::bits::SOURCE, super::bits::DAY_SOURCE],
        THEIRS_LOOKED_OVER,
    )
    .await;
    let mut brought = Vec::new();
    for row in unified::named_among(db, rows, words, THEIRS_SHOWN).await {
        let evidence: serde_json::Value = row
            .evidence
            .as_deref()
            .and_then(|evidence| serde_json::from_str(evidence).ok())
            .unwrap_or_default();
        let text = |key: &str| {
            evidence
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        let theirs = if row.source == super::bits::SOURCE {
            let Some(handle) = text("handle") else {
                continue;
            };
            Theirs::Bit {
                handle,
                how: row.content,
            }
        } else {
            Theirs::Day {
                date: text("day").unwrap_or_else(|| row.created_at.format("%Y-%m-%d").to_string()),
                what: row.content,
            }
        };
        let group = match row.venue.strip_prefix("group:") {
            Some(venue) => crate::services::channel_group::title(venue).await,
            None => None,
        };
        brought.push((group, theirs));
    }
    myriad_merope::other_groups::theirs_section(&brought)
}
