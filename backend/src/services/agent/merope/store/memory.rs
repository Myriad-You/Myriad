//! What chat teaches her about someone, and recalling it.

use super::*;

/// Commit a validated extraction atomically. The input anchor is captured when
/// the utterance is persisted, before reply generation and model extraction.
/// Later activity/mood writes are not new inputs. A later user utterance is.
#[cfg(test)]
pub(crate) async fn apply_chat_memory_update(
    db: &DatabaseConnection,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    update: &super::super::chat_remember::ChatMemoryUpdate,
) -> Result<bool, anyhow::Error> {
    let present = crate::services::agent::memory::unified::Audience::private(user_id);
    apply_chat_memory_update_in(db, user_id, input_at, update, &present).await
}

/// [`apply_chat_memory_update`] for what was said in front of `present`: in a
/// group, the fact is kept for that group, and only facts the group heard can
/// be corrected there.
#[cfg(test)]
pub(crate) async fn apply_chat_memory_update_in(
    db: &DatabaseConnection,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    update: &super::super::chat_remember::ChatMemoryUpdate,
    present: &crate::services::agent::memory::unified::Audience,
) -> Result<bool, anyhow::Error> {
    let transaction = db.begin().await?;
    let applied =
        apply_chat_memory_update_on(&transaction, user_id, input_at, update, present).await?;
    transaction.commit().await?;
    Ok(applied)
}

/// Everything one message left her, applied in order: each single update
/// as `apply_chat_memory_update_on`, then what she said worth keeping, all
/// only while the message is still the person's latest. Whether anything
/// was kept.
pub(in crate::services::agent::merope) async fn apply_chat_memory_updates_on<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    updates: &super::super::chat_remember::ChatMemoryUpdates,
    present: &crate::services::agent::memory::unified::Audience,
) -> Result<bool, anyhow::Error> {
    let mut kept = false;
    for update in &updates.updates {
        kept |= apply_chat_memory_update_on(db, user_id, input_at, update, present).await?;
    }
    if (updates.said.is_empty() && updates.put_onto.is_empty())
        || !chat_memory_input_is_current(db, user_id, input_at).await?
    {
        return Ok(kept);
    }
    use crate::services::agent::memory::unified;
    // What they put her onto is hers now, as what she heard in a group is:
    // about the thing, no one named.
    if !updates.put_onto.is_empty() {
        let held: Vec<String> = unified::own_rows(db, super::super::heard::SOURCE, 300)
            .await?
            .into_iter()
            .map(|row| row.content)
            .collect();
        for thing in &updates.put_onto {
            if held.iter().any(|held| held == thing) {
                continue;
            }
            let evidence = serde_json::json!({ "heard": "suggested to her in a chat" }).to_string();
            kept |= unified::remember_own(
                db,
                thing,
                &evidence,
                Vec::new(),
                super::super::heard::SOURCE,
            )
            .await?
            .is_some();
        }
    }
    for said in &updates.said {
        let remembered = unified::remember(
            db,
            unified::NewMemory {
                user_id,
                kind: unified::MemoryKind::Fact,
                content: said.said.clone(),
                evidence: Some(said.evidence.clone()),
                speaker: unified::Speaker::Agent,
                source: SAID_SOURCE,
                audience: present.clone(),
                importance: 0.4,
                concepts: Vec::new(),
            },
        )
        .await?;
        kept |= remembered.is_some();
    }
    Ok(kept)
}

pub(in crate::services::agent::merope) async fn apply_chat_memory_update_on<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    update: &super::super::chat_remember::ChatMemoryUpdate,
    present: &crate::services::agent::memory::unified::Audience,
) -> Result<bool, anyhow::Error> {
    if user_id <= 0 || (update.fact.is_none() && update.supersedes.is_empty()) {
        return Ok(false);
    }
    // Always acquire in this order. Event-memory writers only take the second.
    lock_addressee(db, user_id).await?;
    lock_persona_memory(db, user_id).await?;
    if !chat_memory_input_is_current(db, user_id, input_at).await? {
        return Ok(false);
    }
    use crate::services::agent::memory::unified;
    let mut targets = Vec::new();
    let mut found = std::collections::HashSet::new();
    let mut duplicate = false;
    for note in unified::active_in(db, user_id, present, &unified::MemoryKind::ABOUT_PERSON).await?
    {
        let content = super::super::ingest::compact_summary(&note.content);
        if update.supersedes.contains(&content) {
            targets.push(note.id);
            found.insert(content);
        } else if update.fact.as_ref() == Some(&content) {
            duplicate = true;
        }
    }
    // Another extraction already replaced a target: reject the whole edit,
    // rather than appending an ungrounded new fact after a partial correction.
    if found.len() != update.supersedes.len() {
        return Ok(false);
    }
    unified::retire(db, user_id, &targets, "superseded").await?;
    let insert = update.fact.as_ref().filter(|_| !duplicate);
    if let Some(fact) = insert {
        unified::remember(
            db,
            unified::NewMemory {
                user_id,
                kind: unified::MemoryKind::Fact,
                content: fact.clone(),
                evidence: update.evidence.clone(),
                speaker: unified::Speaker::User,
                source: "chat",
                audience: present.clone(),
                importance: 0.6,
                concepts: update.concepts.clone(),
            },
        )
        .await?;
    }
    Ok(!targets.is_empty() || insert.is_some())
}

pub(crate) async fn chat_memory_input_is_current<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
) -> Result<bool, sea_orm::DbErr> {
    if user_id <= 0 {
        return Ok(false);
    }
    Ok(agent_addressee_state::Entity::find_by_id(user_id)
        .one(db)
        .await?
        .and_then(|state| state.last_user_message_at)
        == Some(input_at))
}

/// What the persona remembers about this person, most relevant to `query`
/// first (recent first without one). Private: only this person is present.
pub async fn recall_remembered(
    db: &DatabaseConnection,
    user_id: i32,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<String>, anyhow::Error> {
    let priming = Priming::default();
    let present = crate::services::agent::memory::unified::Audience::private(user_id);
    let (recalled, _) = crate::services::agent::memory::unified::recall_primed(
        db,
        user_id,
        &present,
        query.filter(|query| !query.trim().is_empty()),
        &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
        limit,
        &priming,
        1.0,
    )
    .await?;
    Ok(recalled
        .iter()
        .map(as_known)
        .filter(|fact| !fact.is_empty())
        .collect())
}

/// A kept fact as she holds it: what they told her, plainly; anything else
/// with how she came by it, so she holds it as loosely as it deserves.
pub(crate) fn as_known(note: &crate::services::agent::memory::unified::MemoryRecord) -> String {
    let content = super::super::ingest::compact_summary(&note.content);
    if content.is_empty() {
        return content;
    }
    // When she came to know it: what happened "last week" is placed in time.
    let when = note.created_at.format("%Y-%m-%d");
    let evidence = note.evidence.as_deref().unwrap_or("");
    let how = match (note.source.as_str(), note.speaker.as_str()) {
        ("chat", "user") => return format!("[{when}] {content}"),
        // A sore she let go stays among what she knows of them.
        ("chat", "agent") if evidence.contains("\"letGo\"") => "it happened between you",
        // Noted in a group before they were someone she knew by name.
        ("chat", "agent") if evidence.contains("\"who\"") => {
            "you noted this in a group, before you knew them here"
        }
        (SAID_SOURCE, _) => return format!("[{when}] {content} (what you told them)"),
        ("event", _) => "you gathered this from their activity on the site, not from them",
        ("work", _) => "you noted this while doing a task for them",
        ("presence", _) => "you saw this in what they were playing",
        ("game", _) => "from a game with them",
        _ => "kept from before; you no longer know how you came by it",
    };
    format!("[{when}] {content} ({how})")
}

/// Source of what she herself told someone and would remember saying (a
/// recommendation, a promise, an answer they may come back to).
pub const SAID_SOURCE: &str = "said";

/// What a turn recalls, split: what they named (or recent context), and what
/// that brought to mind by association.
pub struct Recalled {
    pub named: Vec<String>,
    pub brought_to_mind: Vec<String>,
}

/// [`recall_remembered_primed`], keeping apart what was named and what it
/// brought to mind.
#[allow(clippy::too_many_arguments)]
pub async fn recall_remembered_split(
    db: &DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    query: Option<&str>,
    limit: usize,
    priming: &Priming,
    breadth: f64,
) -> Result<(Recalled, Priming), anyhow::Error> {
    use crate::services::agent::memory::unified;
    let (recalled, next) = unified::recall_primed(
        db,
        user_id,
        present,
        query.filter(|query| !query.trim().is_empty()),
        &unified::MemoryKind::ABOUT_PERSON,
        limit,
        priming,
        breadth,
    )
    .await?;
    let mut split = Recalled {
        named: Vec::new(),
        brought_to_mind: Vec::new(),
    };
    for note in recalled {
        let content = as_known(&note);
        if content.is_empty() {
            continue;
        }
        if note.brought_to_mind {
            split.brought_to_mind.push(content);
        } else {
            split.named.push(content);
        }
    }
    Ok((split, next))
}

/// [`recall_remembered`] for a chat turn: also starts from what the previous
/// turn left on the mind, and returns what this one leaves.
#[allow(clippy::too_many_arguments)]
pub async fn recall_remembered_primed(
    db: &DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    query: Option<&str>,
    limit: usize,
    priming: &Priming,
    breadth: f64,
) -> Result<(Vec<String>, Priming), anyhow::Error> {
    use crate::services::agent::memory::unified;
    let (recalled, next) = unified::recall_primed(
        db,
        user_id,
        present,
        query.filter(|query| !query.trim().is_empty()),
        &unified::MemoryKind::ABOUT_PERSON,
        limit,
        priming,
        breadth,
    )
    .await?;
    let recalled = recalled
        .into_iter()
        .map(|note| super::super::ingest::compact_summary(&note.content))
        .filter(|content| !content.is_empty())
        .collect();
    Ok((recalled, next))
}
