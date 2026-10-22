//! Recall: which memories come to mind for what is said now, primed by what came to mind just before, and where her thoughts wander from there.

use super::*;
use myriad_agent_rules::memory::recall::{concept_counts, thinly_known};

/// How far one bout of mind-wandering drifts.
pub(super) const WANDER_STEPS: usize = 3;

tokio::task_local! {
    static QUIETLY: ();
}

/// Run `work` without what it reads counting as recalled. A memory grows
/// readier each time it comes to mind (see `strength`): when she speaks,
/// when she wanders, when she is asked to remember. A turn also reads them
/// behind the scenes, several times (to keep facts, to reflect, to weigh
/// how words landed, to decide whether to speak or write first); those
/// reads are not her remembering, and counting them made what she reads
/// most in the background the readiest, whatever she actually thought of.
/// A real turn answered again to look at is read quietly too.
pub(crate) async fn quietly<F: std::future::Future>(work: F) -> F::Output {
    QUIETLY.scope((), work).await
}

/// Whether what comes to mind now counts as recalled.
fn counts_as_recalled() -> bool {
    QUIETLY.try_with(|_| ()).is_err()
}

/// How readily a memory comes to mind now (see `strength`).
pub(super) fn readiness(
    row: &agent_memories::Model,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> f64 {
    if row.source == super::SHORT_NOTE_SOURCE {
        return super::super::strength::short_of_row(
            row.created_at,
            row.access_count,
            row.last_accessed_at,
            now,
        );
    }
    super::super::strength::of_row(row.created_at, row.access_count, row.last_accessed_at, now)
}

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
    recall_among(db, rows, query, limit, priming, breadth).await
}

/// [`recall`] for Work: what it is still trying comes to mind too.
pub async fn recall_for_work<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    query: &str,
    kinds: &[MemoryKind],
    limit: usize,
) -> Result<Vec<MemoryRecord>, DbErr> {
    if user_id <= 0 || limit == 0 {
        return Ok(Vec::new());
    }
    let rows = rows_with(db, user_id, present, kinds, true).await?;
    let (recalled, _) =
        recall_among(db, rows, Some(query), limit, &Priming::default(), 1.0).await?;
    Ok(recalled)
}

/// What of `rows` comes to mind for `query`, counted as used.
async fn recall_among<C: ConnectionTrait>(
    db: &C,
    rows: Vec<agent_memories::Model>,
    query: Option<&str>,
    limit: usize,
    priming: &Priming,
    breadth: f64,
) -> Result<(Vec<MemoryRecord>, Priming), DbErr> {
    // Each read with when it would come to mind; given back as it was said.
    let (rows, said) = super::cues::with_cues(db, rows).await;
    let by_meaning = match query {
        Some(query) => super::super::meaning::standing_out(
            &super::super::meaning::closeness(db, &rows, query, super::super::meaning::FILL).await,
        ),
        None => std::collections::HashMap::new(),
    };
    let (chosen, next, filler) = rank_marked(rows, query, limit, priming, breadth, &by_meaning);
    // Only what came to mind counts: the recent rows filling the rest were
    // shown, and counting them would make what is shown most come easiest.
    let recalled: Vec<String> = chosen
        .iter()
        .map(|(row, _)| row.id.clone())
        .filter(|id| !filler.contains(id))
        .collect();
    if !recalled.is_empty() && counts_as_recalled() {
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
            .filter(agent_memories::Column::Id.is_in(recalled))
            .exec(db)
            .await?;
    }
    Ok((
        chosen
            .into_iter()
            .map(|(row, brought_to_mind)| MemoryRecord {
                brought_to_mind,
                ..MemoryRecord::from(super::cues::as_said(row, &said))
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
    let (chosen, next, _) = rank_marked(
        rows,
        query,
        limit,
        priming,
        breadth,
        &std::collections::HashMap::new(),
    );
    (chosen.into_iter().map(|(row, _)| row).collect(), next)
}

#[cfg(test)]
pub(super) use myriad_agent_rules::memory::rank::named_by_words_or_meaning;

/// [`rank_primed`], marking each row that came to mind by association with
/// what was named rather than being named itself. `by_meaning` holds the
/// memories that stand out by meaning from the rest (memory id → how far,
/// [`super::super::meaning::standing_out`]): they count as named as well.
/// Also the ids of rows that only fill the budget as recent context: they
/// were shown, not recalled.
pub(super) fn rank_marked(
    rows: Vec<agent_memories::Model>,
    query: Option<&str>,
    limit: usize,
    priming: &Priming,
    breadth: f64,
    by_meaning: &std::collections::HashMap<String, f64>,
) -> (Vec<(agent_memories::Model, bool)>, Priming, Vec<String>) {
    use myriad_agent_rules::memory::rank;
    let concepts: Vec<Vec<Concept>> = rows.iter().map(concepts_of).collect();
    let view: Vec<rank::Row> = rows
        .iter()
        .zip(&concepts)
        .map(|(row, concepts)| rank::Row {
            id: &row.id,
            content: &row.content,
            concepts,
            source: &row.source,
            created_at: row.created_at,
            access_count: row.access_count,
            last_accessed_at: row.last_accessed_at,
        })
        .collect();
    let now = super::super::utc_now().fixed_offset();
    let ranked = rank::rank_marked(
        &view,
        query,
        limit,
        &priming.by_id,
        breadth,
        by_meaning,
        now,
    );
    drop(view);
    let filler = ranked
        .filler
        .iter()
        .map(|index| rows[*index].id.clone())
        .collect();
    let mut rows: Vec<Option<agent_memories::Model>> = rows.into_iter().map(Some).collect();
    let chosen = ranked
        .chosen
        .into_iter()
        .filter_map(|(index, brought)| rows[index].take().map(|row| (row, brought)))
        .collect();
    (chosen, Priming { by_id: ranked.next }, filler)
}

/// Of `rows`, at most `limit` that `query` names by their words or their
/// meaning, best first; none when it names none. For what is kept apart from
/// ordinary recall and shown freshest first (a group's bits, its days): an
/// older one still comes back when the talk comes to it. Reading them is not
/// recalling them; what they keep of their own decides how long they last.
pub async fn named_among<C: ConnectionTrait>(
    db: &C,
    rows: Vec<agent_memories::Model>,
    query: &str,
    limit: usize,
) -> Vec<agent_memories::Model> {
    if query.trim().is_empty() || rows.is_empty() || limit == 0 {
        return Vec::new();
    }
    let (rows, said) = super::cues::with_cues(db, rows).await;
    let by_meaning = myriad_agent_rules::memory::rank::brought_back(
        &super::super::meaning::closeness(db, &rows, query, super::super::meaning::FILL).await,
    );
    let (chosen, _, filler) = rank_marked(
        rows,
        Some(query),
        limit,
        &Priming::default(),
        0.0,
        &by_meaning,
    );
    chosen
        .into_iter()
        .filter(|(row, brought_to_mind)| !brought_to_mind && !filler.contains(&row.id))
        .map(|(row, _)| super::cues::as_said(row, &said))
        .collect()
}

/// What Work did for the person that `query` names, at most `limit`, best
/// first; none when it names none. A record of each run is kept, so they
/// never fill the budget as recent context: an old run comes back only when
/// what is asked now is about it.
pub async fn named_done<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    query: &str,
    limit: usize,
) -> Result<Vec<MemoryRecord>, DbErr> {
    if user_id <= 0 {
        return Ok(Vec::new());
    }
    let rows = rows_for(db, user_id, present, &[MemoryKind::Done]).await?;
    Ok(named_among(db, rows, query, limit)
        .await
        .into_iter()
        .map(MemoryRecord::from)
        .collect())
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
    let counts = concept_counts(concepts.iter().map(Vec::as_slice));
    Ok(landed.map(|index| Wandered {
        memory: MemoryRecord::from(rows[index].clone()),
        gap: thinly_known(&concepts[index], &counts),
    }))
}

/// What she always has in mind about `user_id` here, whatever they are
/// saying now (see [`myriad_agent_rules::memory::recall::always_known`]):
/// in a group, only what is about them and was learned there. Shown, not
/// recalled: it is not counted as used and primes nothing.
pub async fn always_known<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
) -> Result<Vec<MemoryRecord>, DbErr> {
    use myriad_agent_rules::memory::rank;
    if user_id <= 0 {
        return Ok(Vec::new());
    }
    let rows: Vec<agent_memories::Model> =
        rows_for(db, user_id, present, &MemoryKind::ABOUT_PERSON)
            .await?
            .into_iter()
            .filter(|row| row.user_id == Some(user_id))
            .collect();
    let concepts: Vec<Vec<Concept>> = rows.iter().map(concepts_of).collect();
    let view: Vec<rank::Row> = rows
        .iter()
        .zip(&concepts)
        .map(|(row, concepts)| rank::Row {
            id: &row.id,
            content: &row.content,
            concepts,
            source: &row.source,
            created_at: row.created_at,
            access_count: row.access_count,
            last_accessed_at: row.last_accessed_at,
        })
        .collect();
    let chosen = myriad_agent_rules::memory::recall::always_known(
        &view,
        super::super::utc_now().fixed_offset(),
    );
    Ok(chosen
        .into_iter()
        .map(|index| MemoryRecord::from(rows[index].clone()))
        .collect())
}

/// Where a bout of mind-wandering settled.
#[derive(Debug, Clone)]
pub struct Wandered {
    pub memory: MemoryRecord,
    /// Something in it she knows only a little about, and how many memories
    /// she has of it, if anything.
    pub gap: Option<(String, usize)>,
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

/// [`myriad_agent_rules::memory::recall::gap_in`] over stored rows.
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
    myriad_agent_rules::memory::recall::gap_in(&documents, query)
}
