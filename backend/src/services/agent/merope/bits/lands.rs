//! How she comes across with someone or in a group.

use super::*;

pub(super) async fn lands_row(
    db: &DatabaseConnection,
    circle: &Circle,
) -> Option<crate::models::entities::agent_memories::Model> {
    let user_id = match circle {
        Circle::Person(user_id) => Some(*user_id),
        Circle::Group { .. } => None,
    };
    unified::venue_source_rows(db, user_id, &circle.audience().venue(), LANDS_SOURCE, 1)
        .await
        .ok()?
        .into_iter()
        .next()
}

/// Keep how she comes across there now, when she found it anew; the one it
/// replaces goes, retired as rewritten.
pub(super) async fn put_lands(
    db: &DatabaseConnection,
    circle: &Circle,
    said: &str,
    was: Option<&crate::models::entities::agent_memories::Model>,
) {
    let now: String = said.trim().chars().take(LANDS_CHARS).collect();
    if now.is_empty() || was.is_some_and(|row| row.content.trim() == now) {
        return;
    }
    if let Some(row) = was {
        let kept_with = row.user_id.unwrap_or_else(|| circle.keeper());
        let _ = unified::retire(db, kept_with, &[row.id.clone()], "superseded").await;
    }
    let _ = unified::remember(
        db,
        unified::NewMemory {
            user_id: circle.keeper(),
            kind: unified::MemoryKind::Fact,
            content: now,
            evidence: None,
            speaker: unified::Speaker::Agent,
            source: LANDS_SOURCE,
            audience: circle.audience(),
            importance: 0.6,
            concepts: Vec::new(),
        },
    )
    .await;
}

/// How she comes across with them in private, as she last found it, and
/// when.
pub async fn lands_with(
    db: &DatabaseConnection,
    user_id: i32,
) -> Option<(String, DateTime<FixedOffset>)> {
    lands_row(db, &Circle::Person(user_id))
        .await
        .map(|row| (row.content, row.created_at))
}

/// How she comes across in a group (`venue` as sessions keep it), as she
/// last found it, and when.
pub async fn lands_in(
    db: &DatabaseConnection,
    venue: &str,
) -> Option<(String, DateTime<FixedOffset>)> {
    lands_row(
        db,
        &Circle::Group {
            venue: venue.to_string(),
            keeper: 0,
        },
    )
    .await
    .map(|row| (row.content, row.created_at))
}
