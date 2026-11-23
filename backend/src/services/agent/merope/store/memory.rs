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
    /// A line that only takes something back, saying nothing new ('当我没说'),
    /// or corrects it to what she already knew for good: let go at once
    /// wherever it is kept, for a stretch that leaves no short note is never
    /// gone over.
    LetGo,
    /// Kept for good: it changes only what she knows for good.
    Lasting,
    /// Kept for good, as what they asked her not to keep from now on.
    Asked,
}

impl Keep {
    fn source(self) -> &'static str {
        match self {
            Self::Note | Self::LetGo => crate::services::agent::memory::unified::SHORT_NOTE_SOURCE,
            Self::Lasting => "chat",
            Self::Asked => crate::services::agent::memory::unified::ASKED_SOURCE,
        }
    }

    /// Kept for good, whichever way.
    fn for_good(self) -> bool {
        matches!(self, Self::Lasting | Self::Asked)
    }
}

pub(in crate::services::agent::merope) async fn apply_chat_memory_updates_on<
    C: ConnectionTrait + sea_orm::TransactionTrait,
>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    updates: &super::super::chat_remember::ChatMemoryUpdates,
    present: &crate::services::agent::memory::unified::Audience,
    keep: Keep,
) -> Result<bool, anyhow::Error> {
    // A line that says nothing new takes back what it lets go of; one whose
    // new fact she already had replaces it by that. Either is let go at
    // once wherever it is kept. One that corrects, its new fact kept, is a
    // short note like any other.
    use super::super::chat_remember::SaidNew;
    let keep = if keep == Keep::Note
        && match updates.said_new {
            SaidNew::Nothing => updates
                .updates
                .iter()
                .enumerate()
                .all(|(index, update)| update.fact.is_none() || updates.asked.contains(&index)),
            SaidNew::WhatSheHad => true,
            SaidNew::Something => false,
        } {
        Keep::LetGo
    } else {
        keep
    };
    let tells = updates
        .updates
        .iter()
        .enumerate()
        .any(|(index, update)| update.fact.is_some() && !updates.asked.contains(&index));
    let mut kept = false;
    for (index, update) in updates.updates.iter().enumerate() {
        let said = Said::of(updates, index);
        let keep = if updates.asked.contains(&index) {
            Keep::Asked
        } else if keep == Keep::Note && update.fact.is_none() && !updates.asked.is_empty() && !tells
        {
            // Beside asking her not to keep something, with nothing else
            // told, what it lets go of is that, wherever it is kept; with a
            // fact told, it may be what that corrects.
            Keep::LetGo
        } else {
            keep
        };
        kept |= apply_one(db, user_id, input_at, update, present, keep, &[], said).await?;
    }
    if (updates.said.is_empty() && updates.put_onto.is_empty())
        || (!keep.for_good()
            && !chat_memory_input_is_current(db, user_id, input_at, present).await?)
    {
        return Ok(kept);
    }
    kept |= keep_what_she_said(db, user_id, updates, present).await?;
    if kept && !keep.for_good() {
        mark_applied(db, user_id, input_at, present).await?;
    }
    Ok(kept)
}

#[path = "memory_hers.rs"]
mod hers;
use hers::keep_what_she_said;
pub(crate) use hers::said_of_herself;

#[cfg(test)]
#[path = "memory_asked_tests.rs"]
mod asked_tests;

#[cfg(test)]
#[path = "memory_own_tests.rs"]
mod own_tests;

/// One update alone, as [`apply_chat_memory_updates_on`] applies each, with
/// no line it was taken back in.
#[cfg(test)]
pub(in crate::services::agent::merope) async fn apply_chat_memory_update_on<
    C: ConnectionTrait + sea_orm::TransactionTrait,
>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    update: &super::super::chat_remember::ChatMemoryUpdate,
    present: &crate::services::agent::memory::unified::Audience,
    keep: Keep,
) -> Result<bool, anyhow::Error> {
    apply_one(
        db,
        user_id,
        input_at,
        update,
        present,
        keep,
        &[],
        Said::default(),
    )
    .await
}

/// A stretch gone over: what it keeps for good, each with the short notes it
/// was put together from (`settled`, one list per update). A note is let go
/// only for the memory that holds it now, the one written or the one that
/// already said it; when nothing holds it, it stays a note and fades.
pub(in crate::services::agent::merope) async fn settle_chat_memory_on<
    C: ConnectionTrait + sea_orm::TransactionTrait,
>(
    db: &C,
    user_id: i32,
    last: chrono::DateTime<chrono::FixedOffset>,
    updates: &super::super::chat_remember::ChatMemoryUpdates,
    settled: &[Vec<String>],
    present: &crate::services::agent::memory::unified::Audience,
) -> Result<bool, anyhow::Error> {
    let mut kept = false;
    for (index, update) in updates.updates.iter().enumerate() {
        let notes = settled.get(index).map_or(&[][..], Vec::as_slice);
        let said = Said::of(updates, index);
        kept |= apply_one(
            db,
            user_id,
            last,
            update,
            present,
            if updates.asked.contains(&index) {
                Keep::Asked
            } else {
                Keep::Lasting
            },
            notes,
            said,
        )
        .await?;
    }
    // A private talk gone over from what was said: what she said too.
    kept |= keep_what_she_said(db, user_id, updates, present).await?;
    Ok(kept)
}

/// What an update rests on in the talk: their message that took back what
/// it lets go of, and the lines what it keeps is of, each where known.
#[derive(Debug, Clone, Copy, Default)]
struct Said<'a> {
    took_back: Option<i32>,
    rests: Option<&'a super::super::chat_remember::Rests>,
}

impl<'a> Said<'a> {
    fn of(updates: &'a super::super::chat_remember::ChatMemoryUpdates, index: usize) -> Self {
        let evidence = updates
            .updates
            .get(index)
            .filter(|update| update.fact.is_some())
            .and_then(|update| update.evidence.as_ref());
        Self {
            took_back: updates.taken_back_in.get(&index).copied(),
            rests: evidence.and_then(|evidence| updates.said_from.get(evidence)),
        }
    }

    /// Their lines a memory is of, noted there as where it comes from.
    async fn tell<C: ConnectionTrait>(&self, db: &C, id: &str) -> Result<(), sea_orm::DbErr> {
        let Some(rests) = self.rests else {
            return Ok(());
        };
        crate::services::agent::memory::origins::tell(db, &rests.lines, id).await
    }
}

async fn apply_one<C: ConnectionTrait + sea_orm::TransactionTrait>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    update: &super::super::chat_remember::ChatMemoryUpdate,
    present: &crate::services::agent::memory::unified::Audience,
    keep: Keep,
    settling: &[String],
    said: Said<'_>,
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
    if !keep.for_good() && !chat_memory_input_is_current(db, user_id, input_at, present).await? {
        return Ok(false);
    }
    use crate::services::agent::memory::unified;
    use unified::SHORT_NOTE_SOURCE;
    let mut rows =
        unified::active_in(db, user_id, present, &unified::MemoryKind::ABOUT_PERSON).await?;
    // What they asked her not to keep holds wherever it was asked, and is
    // taken back wherever they take it back.
    let asked = unified::asked_not_to_keep(db, user_id).await?;
    for row in &asked {
        if !rows.iter().any(|one| one.id == row.id) {
            rows.push(row.clone());
        }
    }
    let own = |row: &unified::MemoryRecord| (row.source == SHORT_NOTE_SOURCE) != keep.for_good();
    // A short note takes back only short notes: what she knows for good
    // changes once the stretch is gone over, with all of it in view. What
    // is only taken back is let go wherever it is kept.
    // Going over changes what she knows for good from what they said here
    // and what they asked: not what another end kept, nor an event.
    let said_here =
        |row: &unified::MemoryRecord| row.source == "chat" || row.source == unified::ASKED_SOURCE;
    let may_take = |row: &unified::MemoryRecord| match keep {
        Keep::Note => own(row),
        Keep::LetGo => own(row) || said_here(row),
        Keep::Lasting | Keep::Asked => said_here(row),
    };
    // What it names that is hers to leave as it is (what another end kept,
    // an event), or no longer there, is left out of it: the fact it tells is
    // kept all the same.
    let mut supersedes: Vec<String> = Vec::new();
    for old in &update.supersedes {
        let there = rows.iter().any(|row| {
            may_take(row) && &super::super::ingest::compact_summary(&row.content) == old
        });
        if there && !supersedes.contains(old) {
            supersedes.push(old.clone());
        }
    }
    if update.fact.is_none() && supersedes.is_empty() {
        return Ok(false);
    }
    let mut targets = Vec::new();
    // Of those, what they asked her not to keep: theirs to let her keep
    // again, only once a second look agrees that they do.
    let mut asked_targets = Vec::new();
    // The row that already says the new fact, when one does: a short note
    // is not taken of what she already knows anywhere; what is kept for
    // good is not kept twice there.
    let mut duplicate = None;
    // The short notes this keeps for good: those it was put together from,
    // and any that say just what it says. A note is not what she already
    // knows for good; the fact takes its place and the note points at it.
    let lasting_fact = keep.for_good()
        && update
            .fact
            .as_deref()
            .is_some_and(|fact| !unified::normalize_content(fact).is_empty());
    let mut settles: Vec<String> = Vec::new();
    for note in rows {
        let content = super::super::ingest::compact_summary(&note.content);
        if may_take(&note) && supersedes.contains(&content) {
            if note.source == unified::ASKED_SOURCE && update.fact.is_none() {
                asked_targets.push(note.id);
            } else {
                targets.push(note.id);
            }
        } else if lasting_fact
            && note.source == SHORT_NOTE_SOURCE
            && (settling.contains(&note.id) || update.fact.as_ref() == Some(&content))
        {
            settles.push(note.id);
        } else if update.fact.as_ref() == Some(&content)
            && (!keep.for_good() || own(&note))
            && (keep != Keep::Asked || note.source == unified::ASKED_SOURCE)
        {
            duplicate = Some(note.id);
        }
    }
    if !asked_targets.is_empty() {
        super::super::memory_nodes::take_back_check::hold_asked(
            db,
            user_id,
            &asked_targets,
            said.took_back,
        )
        .await?;
        if targets.is_empty() {
            return Ok(false);
        }
    }
    // The corrected rows point at what holds the fact now: the new row, or
    // the one that already said it; nothing when it was only taken back.
    let insert = update.fact.as_ref().filter(|_| duplicate.is_none());
    match insert {
        Some(fact) => {
            let replaced = unified::remember_in_place_of_each(
                db,
                unified::NewMemory {
                    user_id,
                    kind: unified::MemoryKind::Fact,
                    content: fact.clone(),
                    evidence: update.evidence.clone(),
                    speaker: unified::Speaker::User,
                    source: keep.source(),
                    // What they ask not to be kept holds everywhere, and
                    // is repeated nowhere.
                    audience: if keep == Keep::Asked {
                        unified::Audience::private(unified::NO_ONE)
                    } else {
                        present.clone()
                    },
                    importance: if keep == Keep::Asked { 1.0 } else { 0.6 },
                    concepts: update.concepts.clone(),
                },
                &[
                    unified::InPlaceOf {
                        user_id,
                        ids: &targets,
                        reason: "superseded",
                    },
                    unified::InPlaceOf {
                        user_id,
                        ids: &settles,
                        reason: "settled",
                    },
                ],
            )
            .await?;
            if let Some(id) = replaced.id {
                unified::keep_cues(db, &id, &update.cues).await?;
                if let Some(rests) = said.rests {
                    unified::said_first_at(db, &id, rests.first).await?;
                }
                said.tell(db, &id).await?;
            }
        }
        None => {
            unified::retire_in_favor_of(db, user_id, &targets, "superseded", duplicate.as_deref())
                .await?;
            if let Some(holder) = &duplicate {
                unified::retire_in_favor_of(db, user_id, &settles, "settled", Some(holder)).await?;
                said.tell(db, holder).await?;
                if keep == Keep::Asked {
                    super::super::memory_nodes::take_back_check::held_to(db, user_id, holder)
                        .await?;
                }
            } else if keep != Keep::Note {
                // Taken back, plainly, with nothing in its place: what they
                // took back going over a stretch (`tookBack`), or a line
                // that keeps nothing and only lets go. The memory goes now,
                // and what rests on it is hidden where she reads. Forgetting
                // that on her nodes (or what rests on what it replaced)
                // cannot be undone: it waits for a second look at their
                // line, once this is committed (`take_back_check`). A
                // correction in a line that keeps a new fact beside it is
                // no taking back.
                super::super::memory_nodes::take_back_check::hold(
                    db,
                    user_id,
                    &targets,
                    said.took_back,
                )
                .await?;
            }
        }
    }
    let applied = !targets.is_empty() || insert.is_some();
    if applied && !keep.for_good() {
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
pub(super) async fn mark_applied<C: ConnectionTrait>(
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
    as_known_in(note, super::super::clock::local_now().year())
}

/// [`as_known`] as she speaks in `year` (her clock's, as this turn is put
/// together): a day in it is written without it, here and on every line
/// dated beside these.
pub(crate) fn as_known_in(
    note: &crate::services::agent::memory::unified::MemoryRecord,
    year: i32,
) -> String {
    let content = super::super::ingest::compact_summary(&note.content);
    if content.is_empty() {
        return content;
    }
    // When she came to know it, on her clock: what happened "last week" is
    // placed in time; the year only when it is not this one.
    let known = note.created_at.with_timezone(&super::super::clock::zone());
    let when = if known.year() == year {
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

/// What they said that a kept fact was taken from, when it is their own
/// words: going through all of it, a name or detail the fact left out is
/// still in them (`myriad_merope::speaking::with_their_words`).
pub(crate) fn their_words(
    note: &crate::services::agent::memory::unified::MemoryRecord,
) -> Option<String> {
    let theirs = matches!(
        (note.source.as_str(), note.speaker.as_str()),
        (
            "chat" | crate::services::agent::memory::unified::SHORT_NOTE_SOURCE,
            "user"
        )
    );
    let said = super::super::ingest::compact_summary(note.evidence.as_deref()?);
    // Only what the fact does not already say in full: the words are there
    // for what it left out (a name, a number), not to say it twice.
    (theirs && !said.is_empty() && !note.content.contains(said.as_str()))
        .then(|| said.chars().take(THEIR_WORDS_CHARS).collect())
}

/// How much of what they said goes beside a fact: enough for the sentence
/// a name or detail was in.
const THEIR_WORDS_CHARS: usize = 160;

/// Source of what she herself told someone and would remember saying (a
/// recommendation, a promise, an answer they may come back to).
pub const SAID_SOURCE: &str = "said";

/// What a turn recalls, split: what they named (or recent context), and what
/// that brought to mind by association.
/// Who a kept fact is about, in a group, when they have not been in the
/// talk there lately.
const SOMEONE_ELSE_HERE: &str = "someone else in this group";

/// A kept fact about `other`, another member of the group, said as theirs
/// by the name the group knows them by (`names`), never their name on the
/// site.
pub(crate) fn theirs_here(
    content: &str,
    other: i32,
    names: Option<&std::collections::HashMap<i32, String>>,
) -> String {
    let name = names
        .and_then(|names| names.get(&other))
        .map_or(SOMEONE_ELSE_HERE, String::as_str);
    about_someone_else(content, name)
}

/// A kept fact about another member of the group, said as theirs.
pub(crate) fn about_someone_else(content: &str, name: &str) -> String {
    format!("{content} (this is about {name}, not the one talking to you)")
}

pub struct Recalled {
    pub named: Vec<String>,
    pub brought_to_mind: Vec<String>,
    /// Of `named`, those only filling the budget as recent context.
    pub recent: Vec<String>,
    /// The memory behind each line of `named`, by line.
    pub ids: Vec<(String, String)>,
    /// What they said behind a line of `named`, by line, where it was their
    /// own words ([`their_words`]); shown only going through all of it.
    pub said: std::collections::HashMap<String, String>,
}

impl Recalled {
    /// What was named, each with their words beside it where there are any:
    /// as she sees it going through all of it.
    pub fn with_their_words(&self) -> Vec<String> {
        self.named
            .iter()
            .map(|line| match self.said.get(line) {
                Some(said) => myriad_merope::speaking::with_their_words(line, said),
                None => line.clone(),
            })
            .collect()
    }
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
        recent: Vec::new(),
        ids: Vec::new(),
        said: std::collections::HashMap::new(),
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
            content = theirs_here(&content, other, names.as_ref());
        }
        if note.brought_to_mind {
            split.brought_to_mind.push(content);
        } else {
            if note.recent {
                split.recent.push(content.clone());
            }
            split.ids.push((content.clone(), note.id.clone()));
            if let Some(said) = their_words(&note) {
                split.said.insert(content.clone(), said);
            }
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
