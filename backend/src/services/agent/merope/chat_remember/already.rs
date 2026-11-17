//! A new fact checked against what she had about them but the extraction
//! did not see: alike ones are asked about once, and the answer says which
//! are had already and which they replace.

use std::time::Duration;

use super::super::call;
use super::super::ingest::compact_summary;
use super::{ChatMemoryUpdate, ChatMemoryUpdates};

const ALREADY_TIMEOUT: Duration = Duration::from_secs(15);
/// A fact she had looks like a new one when this much of the shorter is in
/// both, or this much and the new fact's subject is named in it.
const LOOKS_ALIKE: f64 = 0.5;
const LOOKS_ALIKE_SAME_SUBJECT: f64 = 0.25;
/// Facts she had checked against each new one, and in all, at most.
const ALIKE_PER_FACT: usize = 2;
const ALIKE_AT_MOST: usize = 6;

/// The extraction saw only the facts their words brought to mind. Each new
/// fact is checked, at no cost, against the rest of what she has about them
/// here: one that looks alike and was not seen may be the same thing said
/// another way, or what it now replaces. Only then is the judgment asked,
/// once for all of them (see `myriad_merope::chat_remember::already_system`).
/// Unsure or unanswered, both are kept.
pub(super) async fn check_against_unshown(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    shown: &[String],
    updates: &mut ChatMemoryUpdates,
) {
    use crate::services::agent::memory::unified;
    use myriad_merope::chat_remember::{
        ALREADY_SCHEMA_NAME, already_input, already_schema, already_system, parse_already,
    };
    if updates.updates.iter().all(|update| update.fact.is_none()) {
        return;
    }
    let Ok(rows) =
        unified::active_in(db, user_id, present, &unified::MemoryKind::ABOUT_PERSON).await
    else {
        return;
    };
    let unshown: Vec<Unshown> = rows
        .into_iter()
        .map(|row| Unshown {
            text: compact_summary(&row.content),
            concepts: row.concepts,
        })
        .filter(|known| !known.text.is_empty() && !shown.contains(&known.text))
        .filter(|known| {
            !updates
                .updates
                .iter()
                .any(|update| update.supersedes.contains(&known.text))
        })
        .collect();
    let pairs = alike_unshown(&updates.updates, &unshown);
    if pairs.is_empty() {
        return;
    }
    let texts: Vec<(&str, &str)> = pairs
        .iter()
        .filter_map(|(index, known)| {
            Some((updates.updates[*index].fact.as_deref()?, known.as_str()))
        })
        .collect();
    let Ok(raw) = call::Ask::new(call::Voice::Judge, user_id, "chat_already")
        .within(ALREADY_TIMEOUT)
        // At most 410 measured, thinking included.
        .output_at_most(1024)
        .json_raw(
            &already_system(),
            &already_input(&texts),
            ALREADY_SCHEMA_NAME,
            &already_schema(),
        )
        .await
    else {
        return;
    };
    let verdicts = parse_already(&raw, pairs.len());
    settle_already(&mut updates.updates, &pairs, &verdicts);
}

/// A fact she had but the extraction did not see, with what it is about.
pub(super) struct Unshown {
    pub(super) text: String,
    pub(super) concepts: Vec<crate::services::agent::memory::unified::Concept>,
}

/// Every name a fact is about answers to, folded for comparison; names of
/// one character say too little to tell two subjects apart.
fn subject_names(concepts: &[crate::services::agent::memory::unified::Concept]) -> Vec<String> {
    concepts
        .iter()
        .flat_map(|concept| concept.surface_forms())
        .map(|name| name.trim().to_lowercase())
        .filter(|name| name.chars().count() >= 2)
        .collect()
}

/// Two facts that both say what they are about, with no name in common and
/// neither naming the other's, are about different things: one cannot be
/// the other said again, nor what replaces it, however many words they
/// share. A fact that says nothing of what it is about is never kept apart.
fn about_other_things(fact: &str, subject: &[String], known: &str, theirs: &[String]) -> bool {
    if subject.is_empty() || theirs.is_empty() {
        return false;
    }
    let (fact, known) = (fact.to_lowercase(), known.to_lowercase());
    !subject
        .iter()
        .any(|name| theirs.contains(name) || known.contains(name.as_str()))
        && !theirs.iter().any(|name| fact.contains(name.as_str()))
}

/// For each new fact, the facts she had but the extraction did not see that
/// look most like it (by update index), at most a few. What is plainly about
/// something else is not asked about.
pub(super) fn alike_unshown(
    updates: &[ChatMemoryUpdate],
    unshown: &[Unshown],
) -> Vec<(usize, String)> {
    use crate::services::agent::memory::lexical::overlap;
    let theirs: Vec<Vec<String>> = unshown
        .iter()
        .map(|known| subject_names(&known.concepts))
        .collect();
    let mut pairs = Vec::new();
    for (index, update) in updates.iter().enumerate() {
        let Some(fact) = update.fact.as_deref() else {
            continue;
        };
        let subject = subject_names(&update.concepts);
        let mut alike: Vec<(f64, &String)> = unshown
            .iter()
            .zip(&theirs)
            .filter(|(known, theirs)| !about_other_things(fact, &subject, &known.text, theirs))
            .filter_map(|(known, _)| {
                let score = overlap(fact, &known.text);
                let lower = known.text.to_lowercase();
                let same_subject = subject.iter().any(|name| lower.contains(name.as_str()));
                (score >= LOOKS_ALIKE || (same_subject && score >= LOOKS_ALIKE_SAME_SUBJECT))
                    .then_some((score, &known.text))
            })
            .collect();
        alike.sort_by(|left, right| right.0.total_cmp(&left.0));
        pairs.extend(
            alike
                .into_iter()
                .take(ALIKE_PER_FACT)
                .map(|(_, known)| (index, known.clone())),
        );
    }
    pairs.truncate(ALIKE_AT_MOST);
    pairs
}

/// Apply the judgment: a new fact she already had is not kept again; one
/// that is how it is now replaces what she had; one that cannot be true
/// with what she had, with nothing to say which is, is kept as the fact
/// holding both, in place of the one she had (the first such pair says it).
/// A fact both already had and replacing something is already had: nothing
/// is replaced on its account. A short note takes the place only of short
/// notes; what she knows for good waits for the stretch to be gone over.
pub(super) fn settle_already(
    updates: &mut Vec<ChatMemoryUpdate>,
    pairs: &[(usize, String)],
    verdicts: &[myriad_merope::chat_remember::Already],
) {
    use myriad_merope::chat_remember::Already;
    let had: std::collections::HashSet<usize> = pairs
        .iter()
        .zip(verdicts)
        .filter(|(_, verdict)| **verdict == Already::Same)
        .map(|((index, _), _)| *index)
        .collect();
    let mut held_both = std::collections::HashSet::new();
    for ((index, known), verdict) in pairs.iter().zip(verdicts) {
        if had.contains(index) {
            continue;
        }
        let Some(update) = updates.get_mut(*index) else {
            continue;
        };
        match verdict {
            Already::Replaces => {}
            Already::Unclear(together) if held_both.insert(*index) => {
                update.fact = Some(together.clone());
            }
            _ => continue,
        }
        if !update.supersedes.contains(known) {
            update.supersedes.push(known.clone());
        }
    }
    for index in had {
        if let Some(update) = updates.get_mut(index) {
            update.fact = None;
        }
    }
    updates.retain(|update| update.fact.is_some() || !update.supersedes.is_empty());
}
