//! Unified Agent memory in `agent_memories`.
//!
//! One row is one remembered thing: typed, attributed to who said it, carrying
//! its evidence, valid over a span of time, and bounded to the audience that
//! was present when it was learned. Chat and Work read the same rows.
//!
//! Rules this module owns:
//! - A memory is surfaced only where everyone present belongs to its original
//!   audience (`admits`). A private memory stays with its person; what was
//!   learned in a group stays in that group, and nothing private ever
//!   reaches a group, where people outside the community may be listening.
//! - Retired rows (superseded, deleted, faded) are filtered here, at retrieval,
//!   never left to the caller.
//! - Recalled text is data. Callers inject it through an untrusted block.

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DbErr, EntityTrait,
    ExprTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::json;

use crate::models::entities::agent_memories;

mod evidence;
mod lookup;
mod own;
mod recall;
mod upkeep;
mod venue;

pub use evidence::*;
pub use lookup::*;
pub use own::*;
pub use recall::*;
pub use upkeep::*;
pub use venue::*;

/// Active rows kept per person. Past this, the least important and least
/// recently used rows fade: excluded from recall, free to be learned again.
pub const MAX_ACTIVE_PER_USER: u64 = 1000;
/// Stored text is a single fact, not a transcript.
pub const MAX_CONTENT_CHARS: usize = 400;
/// A memory is about a few things, not a topic list.
pub const MAX_CONCEPTS: usize = 5;
pub const MAX_ALIASES: usize = 5;
const MAX_CONCEPT_CHARS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    /// Something true about the person ("works nights", "has a cat").
    Fact,
    /// What the person likes or wants done a certain way.
    Preference,
    /// How a kind of task went wrong and what to do instead.
    Lesson,
    /// A way of doing a task that worked.
    Pattern,
    /// A day of her own life, told by her. Belongs to no one else and names
    /// no one (see [`write_own_day`]).
    Narrative,
    /// Something she found out on her own, in her own words. Kept with the
    /// audience of the conversation that made her curious.
    Knowledge,
}

impl MemoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Preference => "preference",
            Self::Lesson => "lesson",
            Self::Pattern => "pattern",
            Self::Narrative => "narrative",
            Self::Knowledge => "knowledge",
        }
    }

    /// Everything a person told us about themselves.
    pub const ABOUT_PERSON: [Self; 2] = [Self::Fact, Self::Preference];
    /// How Work should go about things for this person.
    pub const FOR_WORK: [Self; 4] = [Self::Preference, Self::Fact, Self::Lesson, Self::Pattern];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    User,
    Agent,
    /// Carried over from the pre-unified stores; who said it is unknown.
    Import,
}

impl Speaker {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Import => "import",
        }
    }
}

/// Who was present when something was said, and where.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Audience {
    members: Vec<i32>,
    venue: Venue,
}

/// Where a conversation happens.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Venue {
    /// One person and her.
    Private,
    /// A group chat, identified by platform and chat (`telegram:-100123`).
    /// Open: people outside the community may be listening, so only what was
    /// said in front of this same group may be said here.
    Group(String),
}

/// Longest stored venue: `group:` plus a platform and chat id.
pub const MAX_VENUE_CHARS: usize = 96;

impl Audience {
    pub fn private(user_id: i32) -> Self {
        Self {
            members: vec![user_id],
            venue: Venue::Private,
        }
    }

    /// A group chat where `speaker` is the community member she answers.
    pub fn group(id: impl Into<String>, speaker: i32) -> Self {
        let id: String = id.into();
        Self {
            members: vec![speaker],
            venue: Venue::Group(id.chars().take(MAX_VENUE_CHARS - 6).collect()),
        }
    }

    pub fn members(&self) -> &[i32] {
        &self.members
    }

    pub fn is_group(&self) -> bool {
        matches!(self.venue, Venue::Group(_))
    }

    /// The group chat's id (`telegram:-100123`), in a group.
    pub fn group_id(&self) -> Option<&str> {
        match &self.venue {
            Venue::Private => None,
            Venue::Group(id) => Some(id),
        }
    }

    /// The stored form: `private`, or `group:<id>`.
    pub fn venue(&self) -> String {
        match &self.venue {
            Venue::Private => "private".into(),
            Venue::Group(id) => format!("group:{id}"),
        }
    }
}

/// Whether a stored memory may be said in front of `present`. In a group:
/// only what was learned in that same group. In private: never what was
/// learned in a group, and only if everyone present was there.
fn admits(row: &agent_memories::Model, present: &Audience) -> bool {
    if present.is_group() {
        return row.venue == present.venue();
    }
    !row.venue.starts_with("group:") && audience_admits(&audience_of(row), present)
}

/// Whether a memory learned before `original` may be said in front of
/// `present`: everyone present must have been there.
pub fn audience_admits(original: &[i32], present: &Audience) -> bool {
    !present.members.is_empty() && present.members.iter().all(|id| original.contains(id))
}

pub use myriad_agent_rules::Concept;

fn clean_name(text: &str) -> Option<String> {
    let text: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_CONCEPT_CHARS)
        .collect();
    // A lone Latin letter or digit would match inside almost anything.
    let meaningful = text.chars().filter(|ch| ch.is_alphanumeric()).count() >= 2
        || text
            .chars()
            .any(|ch| ch.is_alphanumeric() && !ch.is_ascii());
    meaningful.then_some(text)
}

/// Trim, cap and de-duplicate model-written concepts. Order is kept.
pub fn clean_concepts(raw: Vec<Concept>) -> Vec<Concept> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for concept in raw {
        let Some(name) = clean_name(&concept.name) else {
            continue;
        };
        if !seen.insert(name.to_lowercase()) {
            continue;
        }
        let mut aliases: Vec<String> = Vec::new();
        for alias in concept.aliases.iter().filter_map(|alias| clean_name(alias)) {
            let key = alias.to_lowercase();
            if key != name.to_lowercase() && !aliases.iter().any(|kept| kept.to_lowercase() == key)
            {
                aliases.push(alias);
            }
            if aliases.len() == MAX_ALIASES {
                break;
            }
        }
        out.push(Concept { name, aliases });
        if out.len() == MAX_CONCEPTS {
            break;
        }
    }
    out
}

fn concepts_of(model: &agent_memories::Model) -> Vec<Concept> {
    serde_json::from_value(model.concepts.clone()).unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct NewMemory {
    pub user_id: i32,
    pub kind: MemoryKind,
    pub content: String,
    pub evidence: Option<String>,
    pub speaker: Speaker,
    /// `chat`, `work`, `event`, `steering`, `import`.
    pub source: &'static str,
    pub audience: Audience,
    pub importance: f64,
    pub concepts: Vec<Concept>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryRecord {
    pub id: String,
    pub user_id: Option<i32>,
    pub kind: String,
    pub content: String,
    pub evidence: Option<String>,
    pub source: String,
    /// Who said it: `user` for what they told her, `agent` for what she
    /// gathered, `import` for rows carried over from before.
    pub speaker: String,
    pub importance: f64,
    pub access_count: i32,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
    /// Recalled because what they said brought it to mind by association,
    /// not because they named it.
    pub brought_to_mind: bool,
}

impl From<agent_memories::Model> for MemoryRecord {
    fn from(model: agent_memories::Model) -> Self {
        Self {
            id: model.id,
            user_id: model.user_id,
            kind: model.kind,
            content: model.content,
            evidence: model.evidence,
            source: model.source,
            speaker: model.speaker,
            importance: model.importance,
            access_count: model.access_count,
            created_at: model.created_at,
            brought_to_mind: false,
        }
    }
}

/// Whitespace-collapsed, capped text used both to store and to compare.
pub fn normalize_content(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_CONTENT_CHARS)
        .collect()
}

fn audience_of(model: &agent_memories::Model) -> Vec<i32> {
    let members: Vec<i32> = model
        .audience
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_i64())
                .map(|v| v as i32)
                .collect()
        })
        .unwrap_or_default();
    // Rows without a recorded audience were private to their person.
    if members.is_empty() {
        model.user_id.into_iter().collect()
    } else {
        members
    }
}

/// The rows a conversation could draw on before the audience check: in a
/// group, everything learned in that group, whoever it was about; in
/// private, the person's own memories.
async fn rows_for<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    kinds: &[MemoryKind],
) -> Result<Vec<agent_memories::Model>, DbErr> {
    let rows = if present.is_group() {
        let mut query = agent_memories::Entity::find()
            .filter(agent_memories::Column::Venue.eq(present.venue()))
            .filter(agent_memories::Column::InvalidAt.is_null());
        if !kinds.is_empty() {
            query = query
                .filter(agent_memories::Column::Kind.is_in(kinds.iter().map(|kind| kind.as_str())));
        }
        query
            .order_by_desc(agent_memories::Column::CreatedAt)
            .order_by_desc(agent_memories::Column::Id)
            .limit(MAX_ACTIVE_PER_USER)
            .all(db)
            .await?
    } else {
        active_rows(db, user_id, kinds).await?
    };
    Ok(rows
        .into_iter()
        .filter(|row| admits(row, present))
        // What only the two of them share, and her notes on people from
        // outside, have their own place in her mind.
        .filter(|row| !KEPT_APART.contains(&row.source.as_str()))
        .collect())
}

/// Sources of rows recalled on their own, not with ordinary memories: bits
/// (see `merope::bits`), notes on people outside the community (see
/// `merope::strangers`), what she meant to come back to with someone (see
/// `merope::threads`), what a group told her about herself (see
/// `merope::making_sense`), what someone is to her, what days in a group
/// were like (see `merope::bits`), what still stings with someone (see
/// `merope::sore`), whom she takes someone in a group for (see
/// `merope::recognizing`), her speaking up unasked in a group (see
/// `merope::others`), how she comes across (see `merope::bits`), and the
/// line for each day with someone (see `merope::chat_days`).
pub(crate) const KEPT_APART: [&str; 11] = [
    "bit",
    "stranger",
    "thread",
    "about_me",
    "us",
    "group_day",
    "sore",
    "maybe_is",
    "spoke_up",
    "lands",
    "day_with",
];

async fn active_rows<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    kinds: &[MemoryKind],
) -> Result<Vec<agent_memories::Model>, DbErr> {
    let mut query = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidAt.is_null());
    if !kinds.is_empty() {
        query = query
            .filter(agent_memories::Column::Kind.is_in(kinds.iter().map(|kind| kind.as_str())));
    }
    query
        .order_by_desc(agent_memories::Column::CreatedAt)
        .order_by_desc(agent_memories::Column::Id)
        .limit(MAX_ACTIVE_PER_USER)
        .all(db)
        .await
}

/// Store one memory unless an active row for the same person already says it.
/// Returns the new id, or `None` when it was a duplicate or empty.
pub async fn remember<C: ConnectionTrait>(
    db: &C,
    memory: NewMemory,
) -> Result<Option<String>, DbErr> {
    let content = normalize_content(&memory.content);
    if memory.user_id <= 0 || content.is_empty() {
        return Ok(None);
    }
    // The same thing said privately and again in a group is two memories:
    // one for the person, one the group shares.
    let venue = memory.audience.venue();
    let duplicate = active_rows(db, memory.user_id, &[])
        .await?
        .iter()
        .any(|row| row.venue == venue && normalize_content(&row.content) == content);
    if duplicate {
        return Ok(None);
    }
    let now = Utc::now().fixed_offset();
    let id = format!("mem_{}", uuid::Uuid::new_v4().simple());
    agent_memories::ActiveModel {
        id: Set(id.clone()),
        user_id: Set(Some(memory.user_id)),
        kind: Set(memory.kind.as_str().into()),
        content: Set(content),
        evidence: Set(memory.evidence.as_deref().map(bounded_evidence)),
        speaker: Set(memory.speaker.as_str().into()),
        source: Set(memory.source.into()),
        venue: Set(venue),
        audience: Set(json!(memory.audience.members())),
        concepts: Set(json!(clean_concepts(memory.concepts))),
        importance: Set(memory.importance.clamp(0.0, 1.0)),
        access_count: Set(0),
        last_accessed_at: Set(None),
        valid_from: Set(now),
        invalid_at: Set(None),
        invalid_reason: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;
    fade_excess(db, memory.user_id).await?;
    Ok(Some(id))
}

/// A person's active memories of `kinds` (all when empty), newest first,
/// without counting them as used.
pub async fn active<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    kinds: &[MemoryKind],
) -> Result<Vec<MemoryRecord>, DbErr> {
    Ok(active_rows(db, user_id, kinds)
        .await?
        .into_iter()
        .map(MemoryRecord::from)
        .collect())
}

/// A person's active memories of `kinds` that may be said in front of
/// `present` (in a group: only those learned in that group), newest first,
/// without counting them as used.
pub async fn active_in<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    kinds: &[MemoryKind],
) -> Result<Vec<MemoryRecord>, DbErr> {
    Ok(active_rows(db, user_id, kinds)
        .await?
        .into_iter()
        .filter(|row| admits(row, present))
        .map(MemoryRecord::from)
        .collect())
}

/// Whether the person took this back (corrected it or deleted it). Faded rows
/// do not count: forgetting is not a refusal.
pub async fn retracted_by_person<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    content: &str,
) -> Result<bool, DbErr> {
    let content = normalize_content(content);
    Ok(agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidReason.is_in(["superseded", "deleted"]))
        .all(db)
        .await?
        .iter()
        .any(|row| normalize_content(&row.content) == content))
}

/// Retire rows by id. `reason` is `superseded`, `deleted` or `faded`.
pub async fn retire<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    ids: &[String],
    reason: &str,
) -> Result<u64, DbErr> {
    if ids.is_empty() {
        return Ok(0);
    }
    let now = Utc::now().fixed_offset();
    let result = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            invalid_at: Set(Some(now)),
            invalid_reason: Set(Some(reason.chars().take(16).collect())),
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Id.is_in(ids.iter().cloned()))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

async fn fade_excess<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<(), DbErr> {
    let mut rows = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .all(db)
        .await?;
    if rows.len() as u64 <= MAX_ACTIVE_PER_USER {
        return Ok(());
    }
    rows.sort_by(|left, right| fade_order(left, right));
    let excess = rows.len() - MAX_ACTIVE_PER_USER as usize;
    let ids: Vec<String> = rows.into_iter().take(excess).map(|row| row.id).collect();
    retire(db, user_id, &ids, "faded").await?;
    Ok(())
}

/// First to fade: least important, then least ready to come to mind (see
/// `strength`): seldom recalled, and not lately.
fn fade_order(left: &agent_memories::Model, right: &agent_memories::Model) -> std::cmp::Ordering {
    let now = Utc::now().fixed_offset();
    left.importance
        .partial_cmp(&right.importance)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| readiness(left, now).total_cmp(&readiness(right, now)))
}

#[cfg(test)]
mod db_tests;
#[cfg(test)]
mod tests;
