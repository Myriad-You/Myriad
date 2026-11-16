//! The part of moving her memory that asks the model ([`super::migrate`],
//! plan N8): what she knows for good of someone and has on no node yet,
//! sorted onto her nodes through the one sorting path
//! ([`super::sorting::answer`], [`myriad_nodes::entries::SORT_BATCH`] at
//! a time) and put where she says, as going over a stretch puts it
//! ([`super::facts::put_about`]).
//!
//! What is sorted: every memory still held of the sorted sources and kinds
//! ([`super::sorting::SORTED`], [`super::sorting::KINDS`]), in a private
//! talk or a group, on no node and under no old topic (those are moved
//! without the model, [`super::migrate_topics`]): what she looked at and
//! put under none (the old 「看过不挂」, now asked again, as the answer can
//! say 「关于你」 or none at last), what she kept before topics, and a
//! group's facts. A private talk's are its account's, billed to it; a
//! group's are the site owner's, what is only about a member put on that
//! member's node there and, when they said it, on 「关于你」 in their own
//! private talk too ([`super::group_facts`]).
//!
//! **The cap.** Before the first ask the whole of it is estimated by count
//! ([`estimate`]): asks, and tokens as the plan counts them (a CJK
//! character one, four others one) for the prompt, the facts, the topics
//! shown, the answer and her thinking. Each fact is asked of at most once
//! a run. Once what was spent (the usage the calls report, or the length
//! of the exchange when they report none) is more than twice the estimate,
//! the run stops, saying how far apart they were; the next run goes on
//! with what is still on no node.

use std::collections::{BTreeMap, BTreeSet};

use myriad_merope::topics::{OFFERED, SORT_BATCH};
use myriad_nodes::entries::{Placement, sort_system_prompt};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    Statement,
};

use super::{
    Error, audience, facts, group_facts, known,
    migrate::{Estimate, Halt, Report, her_pen, tokens_of},
    sorting,
};
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified::{MemoryKind, MemoryRecord};
use crate::services::ai_cost_ledger::{AiUsageMeter, with_ai_usage_meter};

/// The operation the asks are billed under, apart from the daily sorting.
pub(crate) const OPERATION: &str = "memory_node_migrate_sort";
/// Each topic shown with a batch, in tokens (its names and three of what
/// is on it), and each fact's answer.
const TOPIC_TOKENS: u64 = 60;
const ANSWER_TOKENS: u64 = 30;
/// Her thinking on one ask (deep thinking): uncertain.
const THINKING_TOKENS: u64 = 1500;
/// The prompt's framing of each fact (its number and day).
const FACT_FRAME_TOKENS: u64 = 6;
/// The plan's price a million tokens, in dollars: uncertain.
const USD_IN: f64 = 1.0;
const USD_OUT: f64 = 5.0;
/// Spent past this many times the estimate, the run stops.
const OVER: u64 = 2;

/// Where one audience's facts are sorted: whose copy of her (billed), the
/// audience, and the facts by id with their text.
#[derive(Debug, Clone)]
struct Batch {
    user_id: i32,
    audience: String,
    facts: Vec<(String, String)>,
}

fn kinds() -> Vec<String> {
    sorting::KINDS
        .iter()
        .map(|kind| kind.as_str().to_string())
        .collect()
}

/// What is to be sorted, by audience, oldest first in each.
async fn batches(db: &DatabaseConnection, site_owner: Option<i32>) -> Result<Vec<Batch>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.user_id, m.venue, m.content FROM agent_memories m \
             WHERE ((m.venue = 'private' AND m.user_id > 0) OR m.venue LIKE 'group:%') \
               AND m.source = ANY($1) AND m.kind = ANY($2) \
               AND m.invalid_at IS NULL AND m.superseded_by IS NULL \
               AND NOT EXISTS (SELECT 1 FROM agent_node_placements p WHERE p.memory_id = m.id) \
               AND NOT EXISTS (SELECT 1 FROM agent_memory_topics t \
                               WHERE t.memory_id = m.id AND t.topic_id IS NOT NULL) \
             ORDER BY m.created_at, m.id",
            [
                sorting::SORTED
                    .iter()
                    .map(|source| source.to_string())
                    .collect::<Vec<_>>()
                    .into(),
                kinds().into(),
            ],
        ))
        .await?;
    let mut by: BTreeMap<(i32, String), Vec<(String, String)>> = BTreeMap::new();
    for row in rows {
        let venue: String = row.try_get("", "venue")?;
        let home = match venue.strip_prefix("group:") {
            Some(group) => match site_owner {
                Some(owner) if !group.is_empty() => (owner, audience::group(group)),
                _ => continue,
            },
            None => {
                let Some(user_id) = row.try_get::<Option<i32>>("", "user_id")? else {
                    continue;
                };
                (user_id, audience::private_talk(user_id, site_owner))
            }
        };
        by.entry(home)
            .or_default()
            .push((row.try_get("", "id")?, row.try_get("", "content")?));
    }
    Ok(by
        .into_iter()
        .map(|((user_id, audience), facts)| Batch {
            user_id,
            audience,
            facts,
        })
        .collect())
}

/// The asks and tokens sorting `batches` is estimated at, `topics` the
/// topics of each audience now.
fn estimate_of(batches: &[(Batch, usize)]) -> Estimate {
    let system = tokens_of(&sort_system_prompt());
    let mut estimate = Estimate::default();
    for (batch, topics) in batches {
        let shown = (*topics).min(OFFERED) as u64 * TOPIC_TOKENS;
        for chunk in batch.facts.chunks(SORT_BATCH) {
            estimate.asks += 1;
            estimate.input_tokens += system
                + shown
                + chunk
                    .iter()
                    .map(|(_, text)| tokens_of(text) + FACT_FRAME_TOKENS)
                    .sum::<u64>();
            estimate.output_tokens += THINKING_TOKENS + chunk.len() as u64 * ANSWER_TOKENS;
        }
        estimate.facts += batch.facts.len();
    }
    estimate.audiences = batches.len();
    estimate.usd = (estimate.input_tokens as f64 * USD_IN
        + estimate.output_tokens as f64 * USD_OUT)
        / 1_000_000.0;
    estimate
}

async fn with_topics(
    db: &DatabaseConnection,
    batches: Vec<Batch>,
) -> Result<Vec<(Batch, usize)>, Error> {
    let mut out = Vec::with_capacity(batches.len());
    for batch in batches {
        let topics = known::known(db, batch.user_id, &batch.audience)
            .await?
            .iter()
            .filter(|node| node.is_topic())
            .count();
        out.push((batch, topics));
    }
    Ok(out)
}

/// What sorting what is still on no node is estimated to cost, by count.
pub(crate) async fn estimate(
    db: &DatabaseConnection,
    site_owner: Option<i32>,
) -> Result<Estimate, Error> {
    Ok(estimate_of(
        &with_topics(db, batches(db, site_owner).await?).await?,
    ))
}

/// The rows of `ids` still held and on no node, as sorting takes them,
/// oldest first.
async fn records(db: &DatabaseConnection, ids: &[String]) -> Result<Vec<MemoryRecord>, Error> {
    let rows = agent_memories::Entity::find()
        .filter(agent_memories::Column::Id.is_in(ids.to_vec()))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::SupersededBy.is_null())
        .all(db)
        .await?;
    let placed: BTreeSet<String> = {
        let found = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT memory_id FROM agent_node_placements WHERE memory_id = ANY($1)",
                [ids.to_vec().into()],
            ))
            .await?;
        found
            .iter()
            .map(|row| row.try_get::<String>("", "memory_id"))
            .collect::<Result<_, _>>()?
    };
    let mut records: Vec<MemoryRecord> = rows
        .into_iter()
        .filter(|row| !placed.contains(&row.id))
        .map(MemoryRecord::from)
        .collect();
    records.sort_by_key(|row| (row.created_at, row.id.clone()));
    Ok(records)
}

/// Puts what she answered for `batch`'s audience: in a private talk on
/// its account's nodes; in a group, member by member.
async fn put(
    db: &DatabaseConnection,
    batch: &Batch,
    site_owner: Option<i32>,
    rows: &[MemoryRecord],
    mut sorted: Vec<facts::Fact>,
) -> Result<usize, Halt> {
    let pen = her_pen()?;
    if !batch.audience.starts_with("group:") {
        return Ok(facts::put(db, batch.user_id, &batch.audience, &sorted, &pen).await?);
    }
    let mut by_member: BTreeMap<i32, Vec<facts::Fact>> = BTreeMap::new();
    for fact in sorted.drain(..) {
        let member = rows
            .iter()
            .find(|row| row.id == fact.memory)
            .and_then(|row| row.user_id)
            .filter(|user_id| *user_id > 0);
        let mut fact = fact;
        // About someone no one knows: of no node.
        if member.is_none() && fact.placement == Placement::AboutThem {
            fact.placement = Placement::Nowhere;
        }
        by_member.entry(member.unwrap_or(0)).or_default().push(fact);
    }
    let mut placed = 0;
    for (member, facts) in by_member {
        placed +=
            facts::put_about(db, batch.user_id, &batch.audience, member, &facts, &pen).await?;
        if member > 0 {
            group_facts::to_their_talk(db, member, site_owner, &batch.audience, &facts, &pen).await;
        }
    }
    Ok(placed)
}

/// Sorts what is still on no node, within the cap ([module docs](self)).
pub(crate) async fn run(
    db: &DatabaseConnection,
    site_owner: Option<i32>,
    report: &mut Report,
) -> Result<(), Halt> {
    let cap = report.estimate.tokens() * OVER;
    if report.estimate.asks == 0 {
        return Ok(());
    }
    let meter = AiUsageMeter::new();
    let mut exchanged: u64 = 0;
    for batch in batches(db, site_owner).await? {
        let ids: Vec<String> = batch.facts.iter().map(|(id, _)| id.clone()).collect();
        // Each fact in one chunk only: asked of once a run.
        for chunk in ids.chunks(SORT_BATCH) {
            let rows = records(db, chunk).await?;
            if rows.is_empty() {
                continue;
            }
            her_pen()?;
            let answer = with_ai_usage_meter(
                meter.clone(),
                sorting::answer(db, batch.user_id, &batch.audience, &rows, OPERATION),
            )
            .await?;
            report.spent.asks += 1;
            report.counts.asks += 1;
            if let Some(answer) = answer {
                exchanged += answer.tokens;
                if !answer.facts.is_empty() {
                    report.counts.sorted +=
                        put(db, &batch, site_owner, &rows, answer.facts).await?;
                }
            }
            report.spent.tokens = meter.total_tokens().max(exchanged);
            if report.spent.tokens > cap {
                return Err(Halt::Stopped(over(report)));
            }
        }
    }
    Ok(())
}

/// Why the run stopped over the cap, in numbers.
fn over(report: &Report) -> String {
    let asks = report.spent.asks.max(1) as u64;
    let estimated = report.estimate.tokens() / report.estimate.asks.max(1) as u64;
    format!(
        "spent {} tokens on {} of {} asks, more than {}× the estimate of {} tokens \
         ({} in, {} out); an ask came to about {} tokens against {} estimated: longer \
         answers or more thinking than counted for. The rest is still on no node and the \
         next run goes on with it",
        report.spent.tokens,
        report.spent.asks,
        report.estimate.asks,
        OVER,
        report.estimate.tokens(),
        report.estimate.input_tokens,
        report.estimate.output_tokens,
        report.spent.tokens / asks,
        estimated,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_estimate_counts_one_ask_a_batch_and_what_goes_in_and_out() {
        let facts = |n: usize| -> Vec<(String, String)> {
            (0..n)
                .map(|at| (format!("m{at}"), "养了一盆绿萝".to_string()))
                .collect()
        };
        let batches = vec![
            (
                Batch {
                    user_id: 7,
                    audience: "personal:7".into(),
                    facts: facts(SORT_BATCH + 1),
                },
                3,
            ),
            (
                Batch {
                    user_id: 1,
                    audience: "group:qq:5".into(),
                    facts: facts(2),
                },
                40,
            ),
        ];
        let estimate = estimate_of(&batches);
        assert_eq!(
            (estimate.facts, estimate.audiences, estimate.asks),
            (SORT_BATCH + 3, 2, 3)
        );
        let system = tokens_of(&sort_system_prompt());
        let fact = tokens_of("养了一盆绿萝") + FACT_FRAME_TOKENS;
        assert_eq!(fact, 6 + FACT_FRAME_TOKENS);
        assert_eq!(
            estimate.input_tokens,
            3 * system
                + 2 * 3 * TOPIC_TOKENS
                + OFFERED as u64 * TOPIC_TOKENS
                + (SORT_BATCH as u64 + 3) * fact
        );
        assert_eq!(
            estimate.output_tokens,
            3 * THINKING_TOKENS + (SORT_BATCH as u64 + 3) * ANSWER_TOKENS
        );
        assert!(estimate.usd > 0.0);
        assert_eq!(estimate_of(&[]), Estimate::default());
    }

    #[test]
    fn tokens_are_counted_as_the_plan_counts_them() {
        assert_eq!(tokens_of(""), 0);
        assert_eq!(tokens_of("abcd"), 1);
        assert_eq!(tokens_of("abcde"), 2);
        assert_eq!(tokens_of("绿萝"), 2);
        assert_eq!(tokens_of("绿萝 ab"), 3);
    }
}
