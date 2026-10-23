//! What chat teaches her about someone, and recalling it.

use super::*;
use chrono::Datelike;

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
        apply_chat_memory_update_on(&transaction, user_id, input_at, update, present, Keep::Note)
            .await?;
    transaction.commit().await?;
    Ok(applied)
}

/// Everything one message left her, applied in order: each single update
/// as `apply_chat_memory_update_on`, then what she said worth keeping, all
/// only while the message is still the person's latest. Whether anything
/// was kept.
/// Where what they said goes: a short note as they talk, or kept for good
/// once the stretch of talk is over (see `chat_remember::settle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::services::agent::merope) enum Keep {
    /// What they said lately: it can only take back other short notes, and
    /// what she knows for good is changed only once the talk is gone over.
    Note,
    /// Kept for good: it changes only what she knows for good.
    Lasting,
}

impl Keep {
    fn source(self) -> &'static str {
        match self {
            Self::Note => crate::services::agent::memory::unified::SHORT_NOTE_SOURCE,
            Self::Lasting => "chat",
        }
    }
}

pub(in crate::services::agent::merope) async fn apply_chat_memory_updates_on<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    updates: &super::super::chat_remember::ChatMemoryUpdates,
    present: &crate::services::agent::memory::unified::Audience,
    keep: Keep,
) -> Result<bool, anyhow::Error> {
    let mut kept = false;
    for update in &updates.updates {
        kept |= apply_chat_memory_update_on(db, user_id, input_at, update, present, keep).await?;
    }
    if (updates.said.is_empty() && updates.put_onto.is_empty())
        || !chat_memory_input_is_current(db, user_id, input_at, present).await?
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
    if kept {
        mark_applied(db, user_id, input_at, present).await?;
    }
    Ok(kept)
}

pub(in crate::services::agent::merope) async fn apply_chat_memory_update_on<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    update: &super::super::chat_remember::ChatMemoryUpdate,
    present: &crate::services::agent::memory::unified::Audience,
    keep: Keep,
) -> Result<bool, anyhow::Error> {
    if user_id <= 0 || (update.fact.is_none() && update.supersedes.is_empty()) {
        return Ok(false);
    }
    // Always acquire in this order. Event-memory writers only take the second.
    lock_addressee(db, user_id).await?;
    lock_persona_memory(db, user_id).await?;
    // Lines are gone over one by one, so an older one must not undo a newer
    // one. A stretch is gone over on its own, once, with all of it in view:
    // it is not fenced by the lines, nor does it fence them.
    if keep == Keep::Note && !chat_memory_input_is_current(db, user_id, input_at, present).await? {
        return Ok(false);
    }
    use crate::services::agent::memory::unified;
    use unified::SHORT_NOTE_SOURCE;
    let rows = unified::active_in(db, user_id, present, &unified::MemoryKind::ABOUT_PERSON).await?;
    let own =
        |row: &unified::MemoryRecord| (row.source == SHORT_NOTE_SOURCE) == (keep == Keep::Note);
    // A short note takes back only short notes: what she knows for good
    // changes once the stretch is gone over, with all of it in view.
    let supersedes: Vec<String> = match keep {
        Keep::Note => update
            .supersedes
            .iter()
            .filter(|old| {
                rows.iter().any(|row| {
                    own(row) && &super::super::ingest::compact_summary(&row.content) == *old
                })
            })
            .cloned()
            .collect(),
        Keep::Lasting => update.supersedes.clone(),
    };
    if update.fact.is_none() && supersedes.is_empty() {
        return Ok(false);
    }
    let mut targets = Vec::new();
    let mut found = std::collections::HashSet::new();
    // The row that already says the new fact, when one does: a short note
    // is not taken of what she already knows anywhere; what is kept for
    // good is not kept twice there.
    let mut duplicate = None;
    for note in rows {
        let content = super::super::ingest::compact_summary(&note.content);
        if own(&note) && supersedes.contains(&content) {
            targets.push(note.id);
            found.insert(content);
        } else if update.fact.as_ref() == Some(&content) && (keep == Keep::Note || own(&note)) {
            duplicate = Some(note.id);
        }
    }
    // Another extraction already replaced a target: reject the whole edit,
    // rather than appending an ungrounded new fact after a partial correction.
    if found.len() != supersedes.len() {
        return Ok(false);
    }
    // The corrected rows point at what holds the fact now: the new row, or
    // the one that already said it; nothing when it was only taken back.
    let insert = update.fact.as_ref().filter(|_| duplicate.is_none());
    match insert {
        Some(fact) => {
            let replaced = unified::remember_in_place_of(
                db,
                unified::NewMemory {
                    user_id,
                    kind: unified::MemoryKind::Fact,
                    content: fact.clone(),
                    evidence: update.evidence.clone(),
                    speaker: unified::Speaker::User,
                    source: keep.source(),
                    audience: present.clone(),
                    importance: 0.6,
                    concepts: update.concepts.clone(),
                },
                unified::InPlaceOf {
                    user_id,
                    ids: &targets,
                    reason: "superseded",
                },
            )
            .await?;
            if let Some(id) = replaced.id {
                unified::keep_cues(db, &id, &update.cues).await?;
            }
        }
        None => {
            unified::retire_in_favor_of(db, user_id, &targets, "superseded", duplicate.as_deref())
                .await?;
        }
    }
    let applied = !targets.is_empty() || insert.is_some();
    if applied && keep == Keep::Note {
        mark_applied(db, user_id, input_at, present).await?;
    }
    Ok(applied)
}

/// Where what they said was last gone over, per person and place.
const APPLIED_NAMESPACE: &str = "merope_memory_applied";
/// Longer than any memory job is kept: no older input can still arrive.
const APPLIED_KEPT_SECS: i64 = 3 * 86_400;

fn applied_key(
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
) -> String {
    format!("{user_id}:{}", present.venue())
}

/// Whether what they said at `input_at` may still change what she knows of
/// them: nothing they said later has been gone over yet. They may well have
/// said more since; that is gone over after this, never instead of it.
pub(crate) async fn chat_memory_input_is_current<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    present: &crate::services::agent::memory::unified::Audience,
) -> Result<bool, sea_orm::DbErr> {
    if user_id <= 0 {
        return Ok(false);
    }
    let applied = crate::services::runtime_registry::get::<chrono::DateTime<chrono::FixedOffset>>(
        db,
        APPLIED_NAMESPACE,
        &applied_key(user_id, present),
    )
    .await?;
    Ok(applied.is_none_or(|applied| applied <= input_at))
}

/// What they said at `input_at` has been gone over: anything older that
/// arrives late may no longer change what she knows.
async fn mark_applied<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    present: &crate::services::agent::memory::unified::Audience,
) -> Result<(), sea_orm::DbErr> {
    use crate::services::runtime_registry as registry;
    let key = applied_key(user_id, present);
    let latest =
        registry::get::<chrono::DateTime<chrono::FixedOffset>>(db, APPLIED_NAMESPACE, &key)
            .await?
            .map_or(input_at, |applied| applied.max(input_at));
    registry::put(
        db,
        APPLIED_NAMESPACE,
        &key,
        registry::RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: Some(user_id),
            tapp_id: None,
            runtime_id: None,
        },
        &latest,
        crate::clock::now().timestamp() + APPLIED_KEPT_SECS,
    )
    .await
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
    // When she came to know it, on her clock: what happened "last week" is
    // placed in time; the year only when it is not this one.
    let known = note.created_at.with_timezone(&super::super::clock::zone());
    let when = if known.year() == super::super::clock::local_now().year() {
        known.format("%m-%d")
    } else {
        known.format("%Y-%m-%d")
    };
    let evidence = note.evidence.as_deref().unwrap_or("");
    let how = match (note.source.as_str(), note.speaker.as_str()) {
        // Kept for good or a short note of what they said lately: both are
        // what they told her.
        ("chat" | crate::services::agent::memory::unified::SHORT_NOTE_SOURCE, "user") => {
            return format!("[{when}] {content}");
        }
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
        // Said once, in the section's heading.
        _ => return format!("[{when}] {content} (older)"),
    };
    format!("[{when}] {content} ({how})")
}

/// Source of what she herself told someone and would remember saying (a
/// recommendation, a promise, an answer they may come back to).
pub const SAID_SOURCE: &str = "said";

/// What a turn recalls, split: what they named (or recent context), and what
/// that brought to mind by association.
/// Who a kept fact is about, in a group, when they have not been in the
/// talk there lately.
const SOMEONE_ELSE_HERE: &str = "someone else in this group";

/// A kept fact about another member of the group, said as theirs.
pub(crate) fn about_someone_else(content: &str, name: &str) -> String {
    format!("{content} (this is about {name}, not the one talking to you)")
}

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
    // In a group she remembers what everyone there said; what is about
    // someone else says whose it is, so it is never taken for the speaker's:
    // by the name the group knows them by, never their name on the site.
    let mut names: Option<std::collections::HashMap<i32, String>> = None;
    for note in recalled {
        let mut content = as_known(&note);
        if content.is_empty() {
            continue;
        }
        if let Some(venue) = present.group_id()
            && let Some(other) = note.user_id.filter(|other| *other != user_id)
        {
            if names.is_none() {
                names = Some(crate::services::channel_group::names_here(db, venue).await);
            }
            let name = names
                .as_ref()
                .and_then(|names| names.get(&other))
                .map_or(SOMEONE_ELSE_HERE, String::as_str);
            content = about_someone_else(&content, name);
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
