//! Which memories come to mind for what is said: named by their words or
//! their meaning, brought along by association, readier ones a little ahead,
//! the recent ones as context when nothing is named. Over plain rows, so the
//! site and any end that hosts her rank the same (D2b).

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, FixedOffset};

use super::{association, lexical, strength};
use crate::Concept;

/// Share of a recalled memory's rank that comes from being named directly;
/// the rest is its activation after spreading.
pub const DIRECT_WEIGHT: f64 = 0.6;
/// Activation an unnamed memory needs before it comes to mind at all.
pub const ASSOCIATED_MIN: f64 = 0.15;
/// Share of a memory's activation still there one turn later.
pub const PRIMING_FADE: f64 = 0.5;
/// How many memories stay on the mind between turns.
pub const PRIMING_KEPT: usize = 16;
/// How much a memory's readiness counts in recall.
pub const READINESS_WEIGHT: f64 = 0.3;
/// Readiness in steps this wide ranks alike, newest first.
pub const READINESS_STEP: f64 = 0.05;
/// Sources recalled only when the talk comes to them, never as the recent
/// context: what she noted about someone in passing (what they played,
/// games she played with them), and what she looked up (one search, put
/// before her every turn, became a story she kept retelling).
pub const RECALLED_WHEN_NAMED: [&str; 3] = ["presence", "game", "lookup"];
/// What they said lately, true for now: fades faster.
pub const SHORT_NOTE_SOURCE: &str = "chat_note";
/// Longest a stored memory's text is.
pub const MAX_CONTENT_CHARS: usize = 400;
/// A memory stands out by meaning when it is this many deviations above how
/// close the rest of them are.
pub const STANDS_OUT: f64 = 2.0;
/// Fewer memories with vectors than this give no sense of the rest.
pub const AT_LEAST: usize = 8;
/// What a topic brings back by meaning must be at least this close to it,
/// however it stands out from the rest: among what she heard in groups,
/// real lines' best few stood out (two deviations) almost every time, and
/// below this they were unrelated ("霸天虎…c盘爆满" bringing back a CD
/// player). Measured with the embedding model in use on 2026-10-20
/// (perplexity pplx-embed-v1-0.6b); another model's scale may differ.
pub const BROUGHT_BACK_AT_LEAST: f64 = 0.35;

/// A stored memory as recall reads it.
#[derive(Debug, Clone)]
pub struct Row<'a> {
    pub id: &'a str,
    pub content: &'a str,
    pub concepts: &'a [Concept],
    pub source: &'a str,
    pub created_at: DateTime<FixedOffset>,
    pub access_count: i32,
    pub last_accessed_at: Option<DateTime<FixedOffset>>,
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

/// How readily a memory comes to mind now.
pub fn readiness(row: &Row, now: DateTime<FixedOffset>) -> f64 {
    if row.source == SHORT_NOTE_SOURCE {
        return strength::short_of_row(row.created_at, row.access_count, row.last_accessed_at, now);
    }
    strength::of_row(row.created_at, row.access_count, row.last_accessed_at, now)
}

/// How named each memory is, by its words and by its meaning together.
pub fn named_by_words_or_meaning(words: &[f64], meaning: &[f64]) -> Vec<f64> {
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

/// What stays on the mind for the next turn: the most active memories, each
/// fading by half per turn and dropped once it no longer clears the bar.
pub fn next_priming(ids: &[&str], activation: &[f64]) -> HashMap<String, f64> {
    let mut active: Vec<(f64, &str)> = activation
        .iter()
        .zip(ids)
        .map(|(activation, id)| (activation * PRIMING_FADE, *id))
        .filter(|(activation, _)| *activation >= ASSOCIATED_MIN)
        .collect();
    active.sort_by(|left, right| right.0.total_cmp(&left.0));
    active
        .into_iter()
        .take(PRIMING_KEPT)
        .map(|(activation, id)| (id.to_string(), activation))
        .collect()
}

/// What [`rank_marked`] chose, as indices into the rows it was given.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ranked {
    /// In order, each with whether it came to mind by association.
    pub chosen: Vec<(usize, bool)>,
    pub next: HashMap<String, f64>,
    /// Those that only fill the budget as recent context.
    pub filler: Vec<usize>,
}

/// The rows a query brings to mind, at most `limit`, best first (see the
/// module). `rows` come newest first; `priming` is what was on the mind
/// (memory id → activation); `by_meaning` the memories that stand out by
/// meaning ([`standing_out`]).
pub fn rank_marked(
    rows: &[Row],
    query: Option<&str>,
    limit: usize,
    priming: &HashMap<String, f64>,
    breadth: f64,
    by_meaning: &HashMap<String, f64>,
    now: DateTime<FixedOffset>,
) -> Ranked {
    // Blank and repeated legacy rows must not spend the recall budget.
    let mut seen = HashSet::new();
    let kept: Vec<usize> = (0..rows.len())
        .filter(|index| {
            let content = normalize_content(rows[*index].content);
            !content.is_empty() && seen.insert(content)
        })
        .collect();
    let rows: Vec<&Row> = kept.iter().map(|index| &rows[*index]).collect();
    let documents: Vec<lexical::Document> = rows
        .iter()
        .map(|row| lexical::Document {
            text: row.content,
            concepts: row.concepts,
        })
        .collect();
    let scores = lexical::score_all(query.unwrap_or(""), &documents);
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
            .map(|row| by_meaning.get(row.id).copied().unwrap_or(0.0))
            .collect();
        let named = named_by_words_or_meaning(&named, &meaning);
        let strongest = named.iter().copied().fold(0.0, f64::max);
        (named, strongest)
    };
    let residual: Vec<f64> = rows
        .iter()
        .map(|row| priming.get(row.id).copied().unwrap_or(0.0))
        .collect();
    let ready: Vec<f64> = rows.iter().map(|row| readiness(row, now)).collect();
    let mut by_recency: Vec<(f64, usize)> = scores
        .iter()
        .enumerate()
        .map(|(index, score)| (score.value, index))
        .filter(|(_, index)| {
            named[*index] > 0.0 || !RECALLED_WHEN_NAMED.contains(&rows[*index].source)
        })
        .collect();
    let step = |index: usize| (ready[index] / READINESS_STEP).floor();
    by_recency.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| step(right.1).total_cmp(&step(left.1)))
    });
    let original = |index: usize| kept[index];
    if named.iter().chain(&residual).all(|seed| *seed <= 0.0) {
        // Nothing named and nothing on the mind: no association starts from
        // a guess.
        let chosen: Vec<(usize, bool)> = by_recency
            .into_iter()
            .take(limit)
            .map(|(_, index)| (original(index), false))
            .collect();
        let filler = chosen.iter().map(|(index, _)| *index).collect();
        return Ranked {
            chosen,
            next: HashMap::new(),
            filler,
        };
    }
    let seeds: Vec<f64> = named
        .iter()
        .zip(&residual)
        .map(|(named, residual)| f64::max(*named, *residual))
        .collect();
    let nodes: Vec<association::Node> = rows
        .iter()
        .map(|row| association::Node {
            concepts: row.concepts,
            at: row.created_at,
        })
        .collect();
    let activation = association::spread(&seeds, &nodes);
    let ids: Vec<&str> = rows.iter().map(|row| row.id).collect();
    let next = next_priming(&ids, &activation);
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
    let brought: HashSet<usize> = picked
        .iter()
        .filter(|(direct, _)| !direct && strongest > 0.0)
        .map(|(_, index)| *index)
        .collect();
    let mut order: Vec<usize> = picked.into_iter().map(|(_, index)| index).collect();
    let mut filled = HashSet::new();
    if strongest <= 0.0 {
        for (_, index) in by_recency {
            if order.len() == limit {
                break;
            }
            if !order.contains(&index) {
                order.push(index);
                filled.insert(index);
            }
        }
    }
    // Filler ids in the order the set held them, as the site kept them.
    let filler = filled.iter().map(|index| original(*index)).collect();
    let mut taken = vec![false; rows.len()];
    let chosen = order
        .into_iter()
        .filter(|index| !std::mem::replace(&mut taken[*index], true))
        .map(|index| (original(index), brought.contains(&index)))
        .collect();
    Ranked {
        chosen,
        next,
        filler,
    }
}

/// How much each memory stands out by meaning from the rest, for the
/// closeness of all of them: `(closeness - mean) / deviation`, only those at
/// least [`STANDS_OUT`].
pub fn standing_out(closeness: &HashMap<String, f64>) -> HashMap<String, f64> {
    if closeness.len() < AT_LEAST {
        return HashMap::new();
    }
    let count = closeness.len() as f64;
    let mean = closeness.values().sum::<f64>() / count;
    let deviation = (closeness
        .values()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / count)
        .sqrt();
    if deviation <= f64::EPSILON {
        return HashMap::new();
    }
    closeness
        .iter()
        .map(|(id, value)| (id.clone(), (value - mean) / deviation))
        .filter(|(_, z)| *z >= STANDS_OUT)
        .collect()
}

/// What a topic brings back by meaning: those that stand out (see
/// [`standing_out`]) and are close to it in their own right
/// ([`BROUGHT_BACK_AT_LEAST`]).
pub fn brought_back(closeness: &HashMap<String, f64>) -> HashMap<String, f64> {
    standing_out(closeness)
        .into_iter()
        .filter(|(id, _)| {
            closeness
                .get(id)
                .is_some_and(|close| *close >= BROUGHT_BACK_AT_LEAST)
        })
        .collect()
}

/// The cosine of the angle between two vectors (0 when they cannot be
/// compared).
pub fn cosine(left: &[f32], right: &[f32]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let (mut dot, mut l, mut r) = (0.0f64, 0.0f64, 0.0f64);
    for (a, b) in left.iter().zip(right) {
        let (a, b) = (f64::from(*a), f64::from(*b));
        dot += a * b;
        l += a * a;
        r += b * b;
    }
    if l == 0.0 || r == 0.0 {
        0.0
    } else {
        dot / (l.sqrt() * r.sqrt())
    }
}

/// How close each memory is to what was asked, by memory id: none without
/// the question's vector, or with too few memories having one.
pub fn closeness(asked: Option<&[f32]>, vectors: &[(&str, &[f32])]) -> HashMap<String, f64> {
    let Some(asked) = asked else {
        return HashMap::new();
    };
    if vectors.len() < AT_LEAST {
        return HashMap::new();
    }
    vectors
        .iter()
        .map(|(id, vector)| (id.to_string(), cosine(asked, vector)))
        .collect()
}
