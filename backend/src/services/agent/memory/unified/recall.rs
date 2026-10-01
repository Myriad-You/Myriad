//! Recall: which memories come to mind for what is said now, primed by what came to mind just before, and where her thoughts wander from there.

use super::*;

/// Share of a recalled memory's rank that comes from being named directly;
/// the rest is its activation after spreading.
pub(super) const DIRECT_WEIGHT: f64 = 0.6;

/// Activation an unnamed memory needs before it comes to mind at all.
pub(super) const ASSOCIATED_MIN: f64 = 0.15;

/// Share of a memory's activation still there one turn later.
pub(super) const PRIMING_FADE: f64 = 0.5;

/// How many memories stay on the mind between turns.
pub(super) const PRIMING_KEPT: usize = 16;

/// How far one bout of mind-wandering drifts.
pub(super) const WANDER_STEPS: usize = 3;

/// A concept she knows this few things about is one she knows only a little
/// about: curiosity peaks between knowing nothing and knowing plenty.
pub(super) const THINLY_KNOWN: usize = 2;

#[cfg(test)]
tokio::task_local! {
    static JUST_LOOKING: ();
}

/// Run `work` without what comes to mind counting as recalled: a real turn
/// answered again to look at, which must not leave the site's memories
/// fresher than they were.
#[cfg(test)]
pub(crate) async fn just_looking<F: std::future::Future>(work: F) -> F::Output {
    JUST_LOOKING.scope((), work).await
}

/// Whether what comes to mind now counts as recalled.
fn counts_as_recalled() -> bool {
    #[cfg(test)]
    {
        JUST_LOOKING.try_with(|_| ()).is_err()
    }
    #[cfg(not(test))]
    {
        true
    }
}

/// How readily a memory comes to mind now (see `strength`).
pub(super) fn readiness(
    row: &agent_memories::Model,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> f64 {
    super::super::strength::of_row(row.created_at, row.access_count, row.last_accessed_at, now)
}

/// How much a memory's readiness counts in recall: named or brought to mind,
/// an old untouched one still comes, a little behind a fresh one.
pub(super) const READINESS_WEIGHT: f64 = 0.3;

/// Readiness in steps this wide ranks alike, newest first.
pub(super) const READINESS_STEP: f64 = 0.05;

/// Activation left over from recent turns, by memory id: what was on the
/// person's mind a moment ago. It seeds the next recall, so a topic carries
/// over a turn that does not name it ("它又吐了" after talking about the cat),
/// and fades within a few turns unless the talk keeps it alive.
///
/// Only ids are kept, never text. A memory retired or no longer admitted for
/// the present audience in the meantime is simply not among the rows it can
/// seed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Priming {
    pub(super) by_id: std::collections::HashMap<String, f64>,
}

impl Priming {
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub(super) fn of(&self, id: &str) -> f64 {
        self.by_id.get(id).copied().unwrap_or(0.0)
    }

    #[cfg(test)]
    pub(crate) fn with(id: &str, activation: f64) -> Self {
        Self {
            by_id: [(id.to_string(), activation)].into(),
        }
    }
}

/// Relevant active memories of `user_id` that may be said in front of
/// `present`. With a query, rows it truly names (BM25, see `lexical`) come to
/// mind first, then what they bring along by association (see
/// `association`). When nothing is named, recency. Recalled rows count as
/// used.
pub async fn recall<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    query: Option<&str>,
    kinds: &[MemoryKind],
    limit: usize,
) -> Result<Vec<MemoryRecord>, DbErr> {
    let priming = Priming::default();
    let (recalled, _) =
        recall_primed(db, user_id, present, query, kinds, limit, &priming, 1.0).await?;
    Ok(recalled)
}

/// [`recall`] that also starts from what was active a moment ago, and returns
/// what is active now for the next turn. `breadth` (`0..=1`) is how far
/// thought may wander: the share of the budget association may fill, from
/// none (only what was named) to half.
#[allow(clippy::too_many_arguments)]
pub async fn recall_primed<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    query: Option<&str>,
    kinds: &[MemoryKind],
    limit: usize,
    priming: &Priming,
    breadth: f64,
) -> Result<(Vec<MemoryRecord>, Priming), DbErr> {
    if user_id <= 0 || limit == 0 {
        return Ok((Vec::new(), Priming::default()));
    }
    let rows = rows_for(db, user_id, present, kinds).await?;
    let by_meaning = match query {
        Some(query) => super::super::meaning::standing_out(
            &super::super::meaning::closeness(db, &rows, query, super::super::meaning::FILL).await,
        ),
        None => std::collections::HashMap::new(),
    };
    let (chosen, next) = rank_marked(rows, query, limit, priming, breadth, &by_meaning);
    if !chosen.is_empty() && counts_as_recalled() {
        let now = Utc::now().fixed_offset();
        agent_memories::Entity::update_many()
            .col_expr(
                agent_memories::Column::AccessCount,
                sea_orm::sea_query::Expr::col(agent_memories::Column::AccessCount).add(1),
            )
            .col_expr(
                agent_memories::Column::LastAccessedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(agent_memories::Column::Id.is_in(chosen.iter().map(|(row, _)| row.id.clone())))
            .exec(db)
            .await?;
    }
    Ok((
        chosen
            .into_iter()
            .map(|(row, brought_to_mind)| MemoryRecord {
                brought_to_mind,
                ..MemoryRecord::from(row)
            })
            .collect(),
        next,
    ))
}

#[cfg(test)]
pub(super) fn rank(
    rows: Vec<agent_memories::Model>,
    query: Option<&str>,
    limit: usize,
) -> Vec<agent_memories::Model> {
    rank_primed(rows, query, limit, &Priming::default(), 1.0).0
}

#[cfg(test)]
pub(super) fn rank_primed(
    rows: Vec<agent_memories::Model>,
    query: Option<&str>,
    limit: usize,
    priming: &Priming,
    breadth: f64,
) -> (Vec<agent_memories::Model>, Priming) {
    let (chosen, next) = rank_marked(
        rows,
        query,
        limit,
        priming,
        breadth,
        &std::collections::HashMap::new(),
    );
    (chosen.into_iter().map(|(row, _)| row).collect(), next)
}

/// How named each memory is, by its words and by its meaning together: each
/// stands by its place among those its words matched and among those that
/// stand out by meaning, so one found both ways comes first; the first
/// counts 1.
pub(super) fn named_by_words_or_meaning(words: &[f64], meaning: &[f64]) -> Vec<f64> {
    /// How much a place matters against being found both ways (as in
    /// `myriad_merope::remembering::merged`).
    const FUSED_AT: f64 = 6.0;
    let places = |values: &[f64]| {
        let mut order: Vec<usize> = (0..values.len()).filter(|i| values[*i] > 0.0).collect();
        order.sort_by(|left, right| values[*right].total_cmp(&values[*left]));
        let mut place = vec![None; values.len()];
        for (rank, index) in order.into_iter().enumerate() {
            place[index] = Some(rank);
        }
        place
    };
    let (by_words, by_meaning) = (places(words), places(meaning));
    let fused: Vec<f64> = by_words
        .iter()
        .zip(&by_meaning)
        .map(|(words, meaning)| {
            [words, meaning]
                .into_iter()
                .flatten()
                .map(|rank| 1.0 / (FUSED_AT + *rank as f64))
                .sum()
        })
        .collect();
    let top = fused.iter().copied().fold(0.0, f64::max);
    if top <= 0.0 {
        return fused;
    }
    fused.into_iter().map(|value| value / top).collect()
}

/// [`rank_primed`], marking each row that came to mind by association with
/// what was named rather than being named itself. `by_meaning` holds the
/// memories that stand out by meaning from the rest (memory id → how far,
/// [`super::super::meaning::standing_out`]): they count as named as well.
pub(super) fn rank_marked(
    rows: Vec<agent_memories::Model>,
    query: Option<&str>,
    limit: usize,
    priming: &Priming,
    breadth: f64,
    by_meaning: &std::collections::HashMap<String, f64>,
) -> (Vec<(agent_memories::Model, bool)>, Priming) {
    // Blank and repeated legacy rows must not spend the recall budget.
    let mut seen = std::collections::HashSet::new();
    let rows: Vec<agent_memories::Model> = rows
        .into_iter()
        .filter(|row| {
            let content = normalize_content(&row.content);
            !content.is_empty() && seen.insert(content)
        })
        .collect();
    let concepts: Vec<Vec<Concept>> = rows.iter().map(concepts_of).collect();
    let documents: Vec<super::super::lexical::Document> = rows
        .iter()
        .zip(&concepts)
        .map(|(row, concepts)| super::super::lexical::Document {
            text: &row.content,
            concepts,
        })
        .collect();
    let scores = super::super::lexical::score_all(query.unwrap_or(""), &documents);
    let strongest = scores
        .iter()
        .filter(|score| score.strong)
        .map(|score| score.value)
        .fold(0.0, f64::max);
    let named: Vec<f64> = scores
        .iter()
        .map(|score| {
            if score.strong {
                score.value / strongest
            } else {
                0.0
            }
        })
        .collect();
    let (named, strongest) = if by_meaning.is_empty() {
        (named, strongest)
    } else {
        let meaning: Vec<f64> = rows
            .iter()
            .map(|row| by_meaning.get(&row.id).copied().unwrap_or(0.0))
            .collect();
        let named = named_by_words_or_meaning(&named, &meaning);
        let strongest = named.iter().copied().fold(0.0, f64::max);
        (named, strongest)
    };
    let residual: Vec<f64> = rows.iter().map(|row| priming.of(&row.id)).collect();
    let now = Utc::now().fixed_offset();
    let ready: Vec<f64> = rows.iter().map(|row| readiness(row, now)).collect();
    // By weak evidence, then how readily each comes to mind, then recency
    // (stable: newest-first from the query).
    let mut by_recency: Vec<(f64, usize)> = scores
        .iter()
        .enumerate()
        .map(|(index, score)| (score.value, index))
        // Noted in passing (a game they played) or looked up: recalled when
        // the talk really comes to it (named, by words or by meaning), not
        // as the recent context or on a word or two in common.
        .filter(|(_, index)| {
            named[*index] > 0.0 || !RECALLED_WHEN_NAMED.contains(&rows[*index].source.as_str())
        })
        .collect();
    let step = |index: usize| (ready[index] / READINESS_STEP).floor();
    by_recency.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| step(right.1).total_cmp(&step(left.1)))
    });
    if named.iter().chain(&residual).all(|seed| *seed <= 0.0) {
        // Nothing named and nothing on the mind: no association starts from
        // a guess.
        let mut rows: Vec<Option<agent_memories::Model>> = rows.into_iter().map(Some).collect();
        let chosen = by_recency
            .into_iter()
            .take(limit)
            .filter_map(|(_, index)| rows[index].take())
            .map(|row| (row, false))
            .collect();
        return (chosen, Priming::default());
    }
    let seeds: Vec<f64> = named
        .iter()
        .zip(&residual)
        .map(|(named, residual)| f64::max(*named, *residual))
        .collect();
    let nodes: Vec<super::super::association::Node> = rows
        .iter()
        .zip(&concepts)
        .map(|(row, concepts)| super::super::association::Node {
            concepts,
            at: row.created_at,
        })
        .collect();
    let activation = super::super::association::spread(&seeds, &nodes);
    let next = next_priming(&rows, &activation);
    let mut scored: Vec<(f64, bool, usize)> = named
        .iter()
        .zip(&activation)
        .enumerate()
        .filter(|(_, (named, activation))| **named > 0.0 || **activation >= ASSOCIATED_MIN)
        .map(|(index, (named, activation))| {
            (
                (DIRECT_WEIGHT * named + (1.0 - DIRECT_WEIGHT) * activation)
                    * (1.0 - READINESS_WEIGHT + READINESS_WEIGHT * ready[index]),
                *named > 0.0,
                index,
            )
        })
        .collect();
    scored.sort_by(|left, right| right.0.total_cmp(&left.0));
    // What the query named comes first in number; association fills in, up
    // to half the budget when thought is free to wander.
    let wander = breadth.clamp(0.0, 1.0);
    let mut associated_left = if wander > 0.0 {
        ((limit as f64 * 0.5 * wander).floor() as usize).max(1)
    } else {
        0
    };
    let picked: Vec<(bool, usize)> = scored
        .into_iter()
        .filter(|(_, direct, _)| {
            *direct
                || (associated_left > 0 && {
                    associated_left -= 1;
                    true
                })
        })
        .map(|(_, direct, index)| (direct, index))
        .take(limit)
        .collect();
    // Brought to mind only when something was actually named; a topic merely
    // lingering from before is not a new association.
    let brought: std::collections::HashSet<usize> = picked
        .iter()
        .filter(|(direct, _)| !direct && strongest > 0.0)
        .map(|(_, index)| *index)
        .collect();
    let mut order: Vec<usize> = picked.into_iter().map(|(_, index)| index).collect();
    if strongest <= 0.0 {
        // Only the lingering topic came to mind: the rest of the budget is
        // the ordinary recent context, as when nothing is on the mind.
        for (_, index) in by_recency {
            if order.len() == limit {
                break;
            }
            if !order.contains(&index) {
                order.push(index);
            }
        }
    }
    let mut rows: Vec<Option<agent_memories::Model>> = rows.into_iter().map(Some).collect();
    let chosen = order
        .into_iter()
        .filter_map(|index| {
            rows[index]
                .take()
                .map(|row| (row, brought.contains(&index)))
        })
        .collect();
    (chosen, next)
}

/// Let the mind wander over a person's memories that `present` may hear:
/// start from what is still on the mind (else somewhere recent), drift a few
/// steps along association, and return where it settled. `avoid` holds ids
/// thought of lately, which are never landed on. Nothing is counted as used:
/// a passing thought is not a recall until it is said.
pub async fn wander<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    priming: &Priming,
    avoid: &std::collections::HashSet<String>,
    roll: &mut impl FnMut() -> f64,
) -> Result<Option<Wandered>, DbErr> {
    if user_id <= 0 {
        return Ok(None);
    }
    let mut seen = std::collections::HashSet::new();
    let rows: Vec<agent_memories::Model> =
        rows_for(db, user_id, present, &MemoryKind::ABOUT_PERSON)
            .await?
            .into_iter()
            .filter(|row| {
                let content = normalize_content(&row.content);
                !content.is_empty() && seen.insert(content)
            })
            .collect();
    if rows.len() < 2 {
        return Ok(None);
    }
    let concepts: Vec<Vec<Concept>> = rows.iter().map(concepts_of).collect();
    let nodes: Vec<super::super::association::Node> = rows
        .iter()
        .zip(&concepts)
        .map(|(row, concepts)| super::super::association::Node {
            concepts,
            at: row.created_at,
        })
        .collect();
    let primed = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (priming.of(&row.id), index))
        .filter(|(activation, _)| *activation > 0.0)
        .max_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, index)| index);
    // Rows are newest first; squaring leans a random start toward recent.
    let start = primed.unwrap_or_else(|| {
        let at = roll().clamp(0.0, 1.0);
        ((at * at) * rows.len() as f64) as usize % rows.len()
    });
    let avoid: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| avoid.contains(&row.id))
        .map(|(index, _)| index)
        .collect();
    let landed = super::super::association::wander(&nodes, start, WANDER_STEPS, &avoid, roll);
    let counts = concept_counts(&concepts);
    Ok(landed.map(|index| Wandered {
        memory: MemoryRecord::from(rows[index].clone()),
        gap: thinly_known(&concepts[index], &counts),
    }))
}

/// Where a bout of mind-wandering settled.
#[derive(Debug, Clone)]
pub struct Wandered {
    pub memory: MemoryRecord,
    /// Something in it she knows only a little about, and how many memories
    /// she has of it, if anything.
    pub gap: Option<(String, usize)>,
}

/// How many memories each concept (by lowercased name) appears in.
pub(super) fn concept_counts(
    concepts: &[Vec<Concept>],
) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::new();
    for concept in concepts.iter().flatten() {
        *counts.entry(concept.name.to_lowercase()).or_insert(0) += 1;
    }
    counts
}

/// The concept of a memory she knows least about, if she knows only a little,
/// with how many memories she has of it.
pub(super) fn thinly_known(
    concepts: &[Concept],
    counts: &std::collections::HashMap<String, usize>,
) -> Option<(String, usize)> {
    concepts
        .iter()
        .filter_map(|concept| {
            let known = counts.get(&concept.name.to_lowercase()).copied()?;
            (known <= THINLY_KNOWN).then_some((known, concept.name.clone()))
        })
        .min_by_key(|(known, _)| *known)
        .map(|(known, name)| (name, known))
}

/// Something this person just brought up that she knows only a little about
/// — a real gap worth one question — among memories `present` may hear.
pub async fn curiosity_gap<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    query: &str,
) -> Result<Option<(String, usize)>, DbErr> {
    if user_id <= 0 || query.trim().is_empty() {
        return Ok(None);
    }
    let rows = rows_for(db, user_id, present, &MemoryKind::ABOUT_PERSON).await?;
    Ok(gap_in(&rows, query))
}

pub(super) fn gap_in(rows: &[agent_memories::Model], query: &str) -> Option<(String, usize)> {
    let concepts: Vec<Vec<Concept>> = rows.iter().map(concepts_of).collect();
    let documents: Vec<super::super::lexical::Document> = rows
        .iter()
        .zip(&concepts)
        .map(|(row, concepts)| super::super::lexical::Document {
            text: &row.content,
            concepts,
        })
        .collect();
    let scores = super::super::lexical::score_all(query, &documents);
    let counts = concept_counts(&concepts);
    let mut named: Vec<(f64, usize)> = scores
        .iter()
        .enumerate()
        .filter(|(_, score)| score.strong)
        .map(|(index, score)| (score.value, index))
        .collect();
    named.sort_by(|left, right| right.0.total_cmp(&left.0));
    named
        .into_iter()
        .find_map(|(_, index)| thinly_known(&concepts[index], &counts))
}

/// What stays on the mind for the next turn: the most active memories, each
/// fading by half per turn and dropped once it no longer clears the bar.
pub(super) fn next_priming(rows: &[agent_memories::Model], activation: &[f64]) -> Priming {
    let mut active: Vec<(f64, &str)> = activation
        .iter()
        .zip(rows)
        .map(|(activation, row)| (activation * PRIMING_FADE, row.id.as_str()))
        .filter(|(activation, _)| *activation >= ASSOCIATED_MIN)
        .collect();
    active.sort_by(|left, right| right.0.total_cmp(&left.0));
    Priming {
        by_id: active
            .into_iter()
            .take(PRIMING_KEPT)
            .map(|(activation, id)| (id.to_string(), activation))
            .collect(),
    }
}
