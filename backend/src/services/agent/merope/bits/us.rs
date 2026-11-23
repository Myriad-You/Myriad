//! What someone is to her, kept as it changes, and where it started.

use super::*;

/// What they are to her as she last put it.
pub(super) async fn us_row(
    db: &DatabaseConnection,
    user_id: i32,
) -> Option<crate::models::entities::agent_memories::Model> {
    unified::venue_source_rows(db, Some(user_id), "private", US_SOURCE, 1)
        .await
        .ok()?
        .into_iter()
        .next()
}

/// Keep what they are to her now, when she said it anew; the one it
/// replaces goes with it as how things were before.
pub(super) async fn put_us(
    db: &DatabaseConnection,
    user_id: i32,
    said: &str,
    was: Option<&crate::models::entities::agent_memories::Model>,
) {
    let now: String = said.trim().chars().take(US_CHARS).collect();
    if now.is_empty() || was.is_some_and(|row| row.content.trim() == now) {
        return;
    }
    let before = was.map(|row| us_evidence_after(row).to_string());
    let replaced: Vec<String> = was.iter().map(|row| row.id.clone()).collect();
    let kept = unified::remember_in_place_of(
        db,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Fact,
            content: now,
            evidence: before,
            speaker: unified::Speaker::Agent,
            source: US_SOURCE,
            audience: Audience::private(user_id),
            importance: 0.7,
            concepts: Vec::new(),
        },
        unified::InPlaceOf {
            user_id,
            ids: &replaced,
            reason: "superseded",
        },
    )
    .await;
    if let Ok(unified::Replaced { id: Some(id), .. }) = &kept {
        tracing::info!(user_id, "[Merope] what someone is to her, put anew");
        super::super::memory_nodes::hanging::kept(db, id).await;
    }
}

/// What they are to her as she now puts it, when she put it so, and what it
/// was before, if she has put it differently.
pub struct Us {
    pub now: String,
    pub since: DateTime<FixedOffset>,
    pub before: Option<String>,
    /// How she first put it, and when.
    pub first: Option<(String, DateTime<FixedOffset>)>,
}

pub async fn us(db: &DatabaseConnection, user_id: i32) -> Option<Us> {
    us_row(db, user_id).await.map(us_of)
}

/// What they are to her as `row` puts it.
fn us_of(row: crate::models::entities::agent_memories::Model) -> Us {
    let evidence: Value = row
        .evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str(evidence).ok())
        .unwrap_or_default();
    let before = evidence
        .get("before")
        .filter(|_| !is_withheld(&evidence, "before"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Us {
        now: row.content,
        since: row.created_at,
        before,
        first: first_of(&evidence),
    }
}

/// What a new version of what they are to her keeps of the one it
/// replaces: that one as before, and where it all started, which the one
/// it replaces carries, or was.
pub(super) fn us_evidence_after(was: &crate::models::entities::agent_memories::Model) -> Value {
    let evidence: Value = was
        .evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str(evidence).ok())
        .unwrap_or_default();
    let first = evidence
        .get("first")
        .cloned()
        .unwrap_or_else(|| json!({ "us": was.content, "at": was.created_at.to_rfc3339() }));
    let mut after = json!({ "before": was.content, "first": first });
    if is_withheld(&evidence, "first") {
        after[WITHHELD] = json!(["first"]);
    }
    after
}

/// Of what a version keeps of those before, the keys she is not shown any
/// more: they asked her not to keep it ([`withhold_us_before`]). Still kept,
/// so it can come back.
const WITHHELD: &str = "withheld";

fn is_withheld(evidence: &Value, key: &str) -> bool {
    evidence
        .get(WITHHELD)
        .and_then(Value::as_array)
        .is_some_and(|keys| keys.iter().any(|one| one == key))
}

/// What `row` says she made of them before, the last time and at first, each
/// with the keys it is kept under.
pub(in crate::services::agent::merope) fn us_before(
    row: &crate::models::entities::agent_memories::Model,
) -> Vec<(Vec<&'static str>, String)> {
    if row.source != super::US_SOURCE {
        return Vec::new();
    }
    let us = us_of(row.clone());
    let mut said: Vec<(Vec<&'static str>, String)> =
        (us.before.into_iter().map(|before| (vec!["before"], before))).collect();
    if let Some((first, _)) = us.first {
        match said.iter_mut().find(|(_, before)| *before == first) {
            Some((keys, _)) => keys.push("first"),
            None => said.push((vec!["first"], first)),
        }
    }
    said
}

/// Of `row`, what she made of them before under `keys` ([`us_before`]) is
/// not shown her any more.
pub(in crate::services::agent::merope) async fn withhold_us_before(
    db: &DatabaseConnection,
    row: &crate::models::entities::agent_memories::Model,
    keys: &[&str],
) -> Result<(), sea_orm::DbErr> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories \
         SET evidence = jsonb_set(evidence::jsonb, ARRAY[$4], \
               COALESCE(evidence::jsonb -> $4, '[]'::jsonb) || to_jsonb($5::text[]))::text, \
             updated_at = $2 \
         WHERE id = $1 AND source = $3",
        [
            row.id.clone().into(),
            crate::clock::now().fixed_offset().into(),
            super::US_SOURCE.into(),
            WITHHELD.into(),
            (keys.iter().map(|key| key.to_string()).collect::<Vec<_>>()).into(),
        ],
    ))
    .await?;
    Ok(())
}

/// How she first put what they are to her, kept with each later version.
pub(super) fn first_of(evidence: &Value) -> Option<(String, DateTime<FixedOffset>)> {
    let first = evidence
        .get("first")
        .filter(|_| !is_withheld(evidence, "first"))?;
    Some((
        first.get("us")?.as_str()?.to_string(),
        DateTime::parse_from_rfc3339(first.get("at")?.as_str()?).ok()?,
    ))
}
