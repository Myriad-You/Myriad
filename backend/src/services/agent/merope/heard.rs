//! What she heard in groups about things (see `myriad_merope::heard`): taken
//! in from a stretch of a group's talk while it is still in mind, kept as her
//! own with no one's name on it but the group's, and brought to mind in any
//! group whose talk touches it. What was about a person was never kept.

use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::services::agent::memory::unified::{self, Concept};

/// Her own rows of what she heard.
pub const SOURCE: &str = "heard";
/// Heard and never brought up again, it fades.
const FADE_AFTER: chrono::Duration = chrono::Duration::days(60);
const HELD: u64 = 300;

/// Keep what she heard in the group `venue` (as sessions keep it), read
/// from a stretch of its talk (see `bits::go_over_stretch`): the same news
/// told twice, or in other words, is kept once. What it hangs on of hers is
/// kept with it, as what brings it back.
pub async fn keep(
    db: &DatabaseConnection,
    venue: &str,
    things: Vec<myriad_merope::group_reading::Heard>,
) {
    if things.is_empty() || !super::is_enabled().await {
        return;
    }
    let mut held: Vec<String> = unified::own_rows(db, SOURCE, HELD)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| row.content)
        .collect();
    let mut kept = 0;
    for heard in things {
        let thing = heard.thing;
        if myriad_merope::heard::already_heard(held.iter().map(String::as_str), &thing) {
            continue;
        }
        held.push(thing.clone());
        let evidence = json!({
            "heard": "in a group chat",
            "group": venue,
            "hangsOn": heard.hangs_on,
            "because": heard.because,
        })
        .to_string();
        let concepts: Vec<Concept> = heard
            .hangs_on
            .into_iter()
            .map(|name| Concept {
                name,
                aliases: Vec::new(),
            })
            .collect();
        if matches!(
            unified::remember_own(db, &thing, &evidence, concepts, SOURCE).await,
            Ok(Some(_))
        ) {
            kept += 1;
        }
    }
    if kept > 0 {
        tracing::info!(kept, "[Merope] kept what she heard in a group");
    }
    if let Err(error) = unified::fade_source(db, SOURCE, FADE_AFTER).await {
        tracing::warn!(%error, "[Merope] could not let old things she heard fade");
    }
}

/// What she heard that the talk in the group `here` (as sessions keep it)
/// brings back, by its words or its meaning, best first: where, and when.
pub async fn brought_back(
    db: &DatabaseConnection,
    here: &str,
    talk: &str,
    limit: usize,
) -> Vec<myriad_merope::heard::Recalled> {
    use myriad_merope::heard::{Recalled, Where};
    let rows = unified::own_rows(db, SOURCE, HELD)
        .await
        .unwrap_or_default();
    let now = crate::services::agent::memory::utc_now();
    let mut recalled = Vec::new();
    for row in unified::named_among(db, rows, talk, limit).await {
        let group = row
            .evidence
            .as_deref()
            .and_then(|evidence| serde_json::from_str::<serde_json::Value>(evidence).ok())
            .and_then(|evidence| evidence.get("group")?.as_str().map(str::to_string));
        let heard = match group {
            Some(group) if group == here => Where::Here,
            Some(group) => match crate::services::channel_group::title(&group).await {
                Some(name) => Where::In(name),
                None => Where::SomeGroup,
            },
            None => Where::SomeGroup,
        };
        recalled.push(Recalled {
            thing: row.content,
            heard,
            ago: myriad_merope::doing::ago_text(now - row.created_at.with_timezone(&chrono::Utc)),
        });
    }
    recalled
}
