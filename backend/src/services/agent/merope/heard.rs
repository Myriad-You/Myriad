//! What she heard in groups about things (see `myriad_merope::heard`): taken
//! in from a stretch of a group's talk while it is still in mind, kept as her
//! own with no name and no group on it, and brought to mind wherever talk
//! touches it. What was about a person was never kept.

use std::time::Duration;

use myriad_merope::heard::{SCHEMA_NAME, Said, input, parse, schema, system};
use sea_orm::DatabaseConnection;
use serde_json::json;

use super::call::{self, Voice};
use crate::services::agent::memory::lexical;
use crate::services::agent::memory::unified::{self, Concept};

/// Her own rows of what she heard.
pub const SOURCE: &str = "heard";
/// Heard and never brought up again, it fades.
const FADE_AFTER: chrono::Duration = chrono::Duration::days(60);
const CALL_TIMEOUT: Duration = Duration::from_secs(45);
const HELD: u64 = 300;

/// Take in a stretch of a group's talk; the judgment is billed to `owner`.
pub async fn take_in(db: &DatabaseConnection, owner: i32, lines: Vec<Said>) {
    if !super::is_enabled().await || lines.iter().all(|line| line.hers) {
        return;
    }
    let Ok(raw) = call::Ask::new(Voice::Judge, owner, "heard")
        .within(CALL_TIMEOUT)
        .json_raw(system(), &input(&lines), SCHEMA_NAME, &schema())
        .await
    else {
        return;
    };
    let Some(things) = parse(&raw, &lines) else {
        return;
    };
    let held: Vec<String> = unified::own_rows(db, SOURCE, HELD)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| row.content)
        .collect();
    let mut kept = 0;
    for thing in things {
        if held.iter().any(|held| *held == thing) {
            continue;
        }
        let evidence = json!({ "heard": "in a group chat" }).to_string();
        if matches!(
            unified::remember_own(db, &thing, &evidence, Vec::<Concept>::new(), SOURCE).await,
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

/// What she heard that `talk` touches, most relevant first.
pub async fn touched(db: &DatabaseConnection, talk: &str, limit: usize) -> Vec<String> {
    if talk.trim().is_empty() {
        return Vec::new();
    }
    let rows = unified::own_rows(db, SOURCE, HELD)
        .await
        .unwrap_or_default();
    let concepts: Vec<Vec<Concept>> = rows
        .iter()
        .map(|row| serde_json::from_value(row.concepts.clone()).unwrap_or_default())
        .collect();
    let documents: Vec<lexical::Document> = rows
        .iter()
        .zip(&concepts)
        .map(|(row, concepts)| lexical::Document {
            text: &row.content,
            concepts,
        })
        .collect();
    let mut scored: Vec<(usize, f64)> = lexical::score_all(talk, &documents)
        .into_iter()
        .enumerate()
        .filter(|(_, score)| score.strong)
        .map(|(index, score)| (index, score.value))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored
        .into_iter()
        .take(limit)
        .map(|(index, _)| rows[index].content.clone())
        .collect()
}
