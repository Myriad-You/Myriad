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
    if let Some(row) = was {
        let _ = unified::retire(db, user_id, &[row.id.clone()], "superseded").await;
    }
    let kept = unified::remember(
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
    )
    .await;
    if matches!(kept, Ok(Some(_))) {
        tracing::info!(user_id, "[Merope] what someone is to her, put anew");
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
    let row = us_row(db, user_id).await?;
    let evidence: Value = row
        .evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str(evidence).ok())
        .unwrap_or_default();
    let before = evidence
        .get("before")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(Us {
        now: row.content,
        since: row.created_at,
        before,
        first: first_of(&evidence),
    })
}

/// What a new version of what they are to her keeps of the one it
/// replaces: that one as before, and where it all started, which the one
/// it replaces carries, or was.
pub(super) fn us_evidence_after(was: &crate::models::entities::agent_memories::Model) -> Value {
    let first = was
        .evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
        .and_then(|evidence| evidence.get("first").cloned())
        .unwrap_or_else(|| json!({ "us": was.content, "at": was.created_at.to_rfc3339() }));
    json!({ "before": was.content, "first": first })
}

/// How she first put what they are to her, kept with each later version.
pub(super) fn first_of(evidence: &Value) -> Option<(String, DateTime<FixedOffset>)> {
    let first = evidence.get("first")?;
    Some((
        first.get("us")?.as_str()?.to_string(),
        DateTime::parse_from_rfc3339(first.get("at")?.as_str()?).ok()?,
    ))
}
