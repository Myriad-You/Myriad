//! How their life stands now (「现在」, `node.standing` in the core), written
//! from a private talk on the site. What is asked, how the answer is read
//! and the rows it makes are the core's (`myriad_nodes::stands`, the same on
//! every end); here is only what the site does around it: which facts,
//! what she recalls for each, the nodes as they are folded here, the model
//! call, and writing the rows.
//!
//! - **After a stretch is gone over** ([`after_settled`]): each fact it
//!   kept, one call each, in the order kept.
//! - **What she knew before stands first** ([`build`]): in a talk where none
//!   of hers stands yet, before the first stretch's facts are read, each of
//!   her nodes there with none of hers on it, once. A rule's 「现在」 (where
//!   a session is `now`) is not hers, and the site owner's talk holds those
//!   from the start.
//! - **Kept on the site's side** (`Sync::Local`) while a side of the
//!   owner's would refuse it, written to go once every side reads it
//!   (`share::sync_for`); what was kept before goes then too
//!   (`share::share_kept`, when a side comes).
//!
//! The memory bench's trial of a kept list
//! (`memory_bench::background::state_list`) builds through [`build`] too.

use std::collections::{BTreeMap, HashMap};

use myriad_nodes::stands::{
    self, BuildDueInput, BuildInput, BuildReadInput, FactIn, KnownIn, NodeIn, StandingRowIn,
    StepInput, StepPrepared, StepRead, StepReadInput,
};
use sea_orm::DatabaseConnection;

use super::{Author, Error, RowIn, facts::Fact, placements, read, write};
use crate::services::agent::memory::unified::{self, Audience, MemoryKind};
use crate::services::her_log::pen::Thought;

/// The day of `micros` in her zone, as the core takes a day.
pub(super) fn day_of(micros: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_micros(micros).map(|at| {
        at.with_timezone(&crate::services::agent::merope::clock::zone())
            .format("%Y-%m-%d")
            .to_string()
    })
}

/// The authors and items of `owner`'s `audience`'s 「现在」 rows, each
/// once: her line on a repository is hers but no one's life, and only the
/// core tells them apart. A forgotten row (a stub, its body gone) stands
/// for nothing.
async fn standing_authors(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
) -> Result<Vec<StandingRowIn>, Error> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT author, \
             CASE WHEN body LIKE '{%' THEN body::jsonb->>'item' END AS item FROM agent_node_rows \
             WHERE user_id = $1 AND audience = $2 AND kind = $3 AND body <> ''",
            [owner.into(), audience.into(), stands::KIND.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(StandingRowIn {
                author: row.try_get("", "author")?,
                item: row.try_get("", "item")?,
            })
        })
        .collect()
}

/// Times the fast model is asked before what it was asked goes unanswered:
/// a call that failed (the provider, the network) is asked again, since
/// what it would have written is otherwise never written until the same
/// thing comes up again. An answer, even one that reads as nothing, is not.
///
/// Asked again here, after a wait, rather than kept to ask at the next
/// stretch: a fact kept for later would be one more copy for forgetting to
/// reach, and this runs behind her reply, where a wait costs nobody.
const ASKED: usize = 3;

/// How long before each ask again: the second after 2 s, the third after
/// 8 s, so a provider down for a moment is past it.
#[cfg(not(test))]
const WAIT_BEFORE: [std::time::Duration; ASKED - 1] = [
    std::time::Duration::from_secs(2),
    std::time::Duration::from_secs(8),
];
#[cfg(test)]
const WAIT_BEFORE: [std::time::Duration; ASKED - 1] = [std::time::Duration::ZERO; ASKED - 1];

/// How writing 「现在」 went wrong since the site started, for its status:
/// calls asked again, asks that got no answer at all (what they would have
/// written is lost), answers that read as nothing, and nodes that could not
/// take their rows. Counts only, never what was asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Failures {
    pub(crate) asked_again: u64,
    pub(crate) unanswered: u64,
    pub(crate) unreadable: u64,
    pub(crate) not_written: u64,
}

static ASKED_AGAIN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static UNANSWERED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static UNREADABLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static NOT_WRITTEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// [`Failures`] so far.
pub(crate) fn failures() -> Failures {
    use std::sync::atomic::Ordering::Relaxed;
    Failures {
        asked_again: ASKED_AGAIN.load(Relaxed),
        unanswered: UNANSWERED.load(Relaxed),
        unreadable: UNREADABLE.load(Relaxed),
        not_written: NOT_WRITTEN.load(Relaxed),
    }
}

fn counted(counter: &std::sync::atomic::AtomicU64) -> u64 {
    counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// Output a step's answer may take, thinking included: at most 1,002
/// measured.
const STEP_OUTPUT: u32 = 2048;
/// Output a node's first building may take, thinking included: at most
/// 1,332 measured.
const BUILD_OUTPUT: u32 = 3072;

/// Asks the fast model what `prepared` asks, its answer at most `output`
/// tokens: its answer as it came, or none (nothing to ask, or no answer
/// after [`ASKED`] calls).
async fn ask(
    user_id: i32,
    operation: &'static str,
    schema_name: &str,
    output: u32,
    prepared: &StepPrepared,
) -> Option<String> {
    if prepared.skip {
        return None;
    }
    for asked in 1..=ASKED {
        if asked > 1 {
            tokio::time::sleep(WAIT_BEFORE[asked - 2]).await;
        }
        let answer =
            super::super::call::Ask::new(super::super::call::Voice::Judge, user_id, operation)
                .output_at_most(output)
                .json_raw(
                    &prepared.system,
                    &prepared.input,
                    schema_name,
                    &prepared.schema,
                )
                .await;
        match answer {
            Ok(answer) => return Some(answer),
            // What failed, not what was asked: no words of theirs.
            Err(_) if asked < ASKED => {
                let total = counted(&ASKED_AGAIN);
                tracing::info!(
                    operation,
                    asked,
                    total,
                    "[memory-nodes] how things stand: asked again"
                );
            }
            Err(_) => {
                let total = counted(&UNANSWERED);
                tracing::warn!(
                    operation,
                    asked,
                    total,
                    "[memory-nodes] how things stand: no answer, not written"
                );
            }
        }
    }
    None
}

/// Writes `read`'s rows, a node at a time: a node that cannot take its rows
/// leaves the others. How many were written.
async fn write_rows(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    read: StepRead,
    pen: &Thought,
) -> usize {
    let mut by_node: Vec<(String, Vec<RowIn>)> = Vec::new();
    for row in read.rows {
        let row = RowIn {
            id: row.id,
            node: row.node,
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        };
        match by_node.iter_mut().find(|(at, _)| *at == row.node) {
            Some((_, rows)) => rows.push(row),
            None => by_node.push((row.node.clone(), vec![row])),
        }
    }
    let mut written = 0;
    for (node, rows) in by_node {
        match write::write(db, owner, audience, &rows, Some(pen)).await {
            Ok(_) => written += rows.len(),
            Err(error) => {
                let total = counted(&NOT_WRITTEN);
                tracing::warn!(%error, node, total, "[memory-nodes] how things stand not written")
            }
        }
    }
    written
}

/// How things stand on each of her nodes of `audience` (`owner`'s) that
/// holds none of hers yet, from what she knows there: a topic's, and
/// theirs (「关于你」). One call to the fast model per node with something
/// on it; `leave_out` are memories not to go by (what a stretch just kept,
/// which the step after then reads as new). How many rows were written.
pub(crate) async fn build(
    db: &DatabaseConnection,
    user_id: i32,
    owner: i32,
    audience: &str,
    leave_out: &[String],
    pen: &Thought,
) -> Result<usize, Error> {
    let mut written = 0;
    for head in read::heads(db, owner, audience, None).await? {
        if !matches!(head.kind.as_deref(), Some("topic") | Some("person")) {
            continue;
        }
        let Some(folded) = read::node(db, owner, audience, &head.node_id).await? else {
            continue;
        };
        let ids: Vec<String> = read::entries_of(&folded)
            .into_iter()
            .flat_map(|entry| entry.memories)
            .collect();
        let held: Vec<String> = super::known::held(db, ids.clone())
            .await?
            .into_iter()
            .filter(|id| !leave_out.contains(id))
            .collect();
        let learned: BTreeMap<String, String> = super::known::learned(db, ids)
            .await?
            .into_iter()
            .filter_map(|(id, at)| Some((id, day_of(at)?)))
            .collect();
        let input = BuildInput {
            node: NodeIn {
                id: head.node_id.clone(),
                name: head.name.clone(),
                kind: head.kind.clone(),
                folded,
            },
            held,
            learned,
        };
        let prepared = stands::build_prepare(input.clone())?;
        if prepared.skip {
            continue;
        }
        let Some(output) = ask(
            user_id,
            "memory_standing_build",
            "standing_build",
            BUILD_OUTPUT,
            &prepared,
        )
        .await
        else {
            tracing::info!("[memory-nodes] how things stand on a node not built: no answer");
            continue;
        };
        let read = stands::build_read(BuildReadInput {
            output,
            sync: super::share::sync_for(db, owner).await,
            node: input.node,
            held: input.held,
            learned: input.learned,
        })?;
        if read.unreadable {
            let total = counted(&UNREADABLE);
            tracing::info!(
                total,
                "[memory-nodes] how things stand on a node not built: the answer read as nothing"
            );
            continue;
        }
        // Items of the answer the core left out: how many, and why; never
        // the words.
        if !read.dropped.is_empty() {
            tracing::info!(
                node = %head.node_id,
                kept = read.rows.len(),
                dropped = ?read.dropped,
                "[memory-nodes] how things stand on a node: items left out"
            );
        }
        written += write_rows(db, owner, audience, read, pen).await;
    }
    Ok(written)
}

/// What each fact a stretch of `user_id`'s private talk kept does to how
/// their life stands, written on the nodes of `audience` (`owner`'s): one
/// call to the fast model per fact on a node, in the order they were kept.
/// `replaced` are the facts that took the place of something she knew.
/// How many rows were written.
pub(crate) async fn after_settled(
    db: &DatabaseConnection,
    user_id: i32,
    owner: i32,
    present: &Audience,
    audience: &str,
    facts: &[Fact],
    replaced: &[String],
    pen: &Thought,
) -> Result<usize, Error> {
    let mut written = 0;
    // None of hers stands here yet (what she kept before 「现在」 was
    // written): first how things stood before this stretch, so what it
    // changes is changed, not only started.
    let due = BuildDueInput {
        facts: facts.len(),
        standing: standing_authors(db, owner, audience).await?,
    };
    if stands::build_due(&due) {
        let kept: Vec<String> = facts.iter().map(|fact| fact.memory.clone()).collect();
        // Boxed, as is `ask` below: in the debug build the site runs, this
        // frame would otherwise hold room for each of them on the stack.
        written += Box::pin(build(db, user_id, owner, audience, &kept, pen)).await?;
    }
    let heads: HashMap<String, read::Head> = read::heads(db, owner, audience, None)
        .await?
        .into_iter()
        .map(|head| (head.node_id.clone(), head))
        .collect();
    for fact in facts {
        let step = step_of(
            db, user_id, owner, present, audience, facts, fact, replaced, &heads,
        )
        .await?;
        let prepared = stands::step_prepare(step.clone())?;
        let Some(output) = Box::pin(ask(
            user_id,
            "memory_standing_step",
            "standing_step",
            STEP_OUTPUT,
            &prepared,
        ))
        .await
        else {
            if !prepared.skip {
                tracing::info!("[memory-nodes] how things stand not asked: no answer");
            }
            continue;
        };
        let read = stands::step_read(StepReadInput {
            output,
            sync: super::share::sync_for(db, owner).await,
            fact: step.fact,
            known: step.known,
            nodes: step.nodes,
        })?;
        if read.unreadable {
            let total = counted(&UNREADABLE);
            tracing::info!(
                total,
                "[memory-nodes] how things stand not written: the answer read as nothing"
            );
            continue;
        }
        // Each item left as another person's: which, and whose on each
        // side; ids and keys, never the words.
        for left in &read.left {
            tracing::info!(
                memory = %fact.memory,
                item = %left.item,
                item_whose = %left.item_whose,
                fact_whose = %left.fact_whose,
                "[memory-nodes] how another person's life stands left as it is"
            );
        }
        written += write_rows(db, owner, audience, read, pen).await;
    }
    Ok(written)
}

/// What the core is given of one fact: the fact, what it brings to mind
/// (read quietly, not her remembering; not what this stretch kept), and
/// the nodes either is kept on, folded.
#[allow(clippy::too_many_arguments)]
async fn step_of(
    db: &DatabaseConnection,
    user_id: i32,
    owner: i32,
    present: &Audience,
    audience: &str,
    facts: &[Fact],
    fact: &Fact,
    replaced: &[String],
    heads: &HashMap<String, read::Head>,
) -> Result<StepInput, Error> {
    let on = |memory: String| async move {
        Ok::<_, Error>(
            placements::placement(db, &memory)
                .await?
                .and_then(|placement| placement.node_id)
                .into_iter()
                .collect::<Vec<String>>(),
        )
    };
    let own = on(fact.memory.clone()).await?;
    let (recalled, _) = unified::quietly(unified::recall_primed(
        db,
        user_id,
        present,
        Some(&fact.text),
        &MemoryKind::ABOUT_PERSON,
        stands::KNOWN * 2,
        &Default::default(),
        1.0,
    ))
    .await?;
    let mut known: Vec<KnownIn> = Vec::new();
    for record in recalled
        .into_iter()
        .filter(|record| !facts.iter().any(|kept| kept.memory == record.id))
        .take(stands::KNOWN)
    {
        known.push(KnownIn {
            text: crate::services::agent::merope::ingest::compact_summary(&record.content),
            on: on(record.id.clone()).await?,
        });
    }
    let mut nodes: Vec<NodeIn> = Vec::new();
    for id in own
        .iter()
        .chain(known.iter().flat_map(|known| known.on.iter()))
    {
        if nodes.iter().any(|node| &node.id == id) {
            continue;
        }
        if let Some(folded) = read::node(db, owner, audience, id).await? {
            let head = heads.get(id);
            nodes.push(NodeIn {
                id: id.clone(),
                name: head.and_then(|head| head.name.clone()),
                kind: head.and_then(|head| head.kind.clone()),
                folded,
            });
        }
    }
    let learned = super::known::learned(db, vec![fact.memory.clone()])
        .await?
        .get(&fact.memory)
        .and_then(|at| day_of(*at));
    Ok(StepInput {
        fact: FactIn {
            memory: fact.memory.clone(),
            text: fact.text.clone(),
            happened: fact
                .standing
                .happened
                .as_ref()
                .map(|happened| happened.from.clone()),
            learned,
            replaced: replaced.contains(&fact.memory),
            on: own,
        },
        known,
        nodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared() -> StepPrepared {
        StepPrepared {
            skip: false,
            system: "how things stand".into(),
            schema: serde_json::json!({"type": "object"}),
            input: "{}".into(),
        }
    }

    /// The counts are the site's, shared by every test thread: one test
    /// of them at a time.
    static COUNTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// The model, as the scene answers it: `answers` in turn, `None` a
    /// call that failed.
    fn answering(answers: Vec<Option<&str>>) {
        let mut world = crate::services::channel_group::g0::World::new(0.0);
        world.models.insert(
            "memory_standing_step".into(),
            answers
                .into_iter()
                .map(|answer| answer.map(str::to_string))
                .collect(),
        );
        crate::services::channel_group::g0::enter(world);
    }

    /// A call that fails is asked again, and every one counted: two asks
    /// again and then none answered is one lost, for the status to show.
    #[tokio::test]
    async fn a_call_that_keeps_failing_is_asked_again_and_counted() {
        let _one = COUNTING.lock().await;
        let before = failures();
        answering(vec![None, None, None]);
        let answer = ask(
            7,
            "memory_standing_step",
            "standing_step",
            STEP_OUTPUT,
            &prepared(),
        )
        .await;
        crate::services::channel_group::g0::leave();
        assert_eq!(answer, None);
        let after = failures();
        assert_eq!(after.asked_again - before.asked_again, 2);
        assert_eq!(after.unanswered - before.unanswered, 1);
    }

    #[tokio::test]
    async fn a_call_answered_the_second_time_is_not_lost() {
        let _one = COUNTING.lock().await;
        let before = failures();
        answering(vec![None, Some("{\"items\":[]}")]);
        let answer = ask(
            7,
            "memory_standing_step",
            "standing_step",
            STEP_OUTPUT,
            &prepared(),
        )
        .await;
        crate::services::channel_group::g0::leave();
        assert_eq!(answer.as_deref(), Some("{\"items\":[]}"));
        let after = failures();
        assert_eq!(after.asked_again - before.asked_again, 1);
        assert_eq!(after.unanswered, before.unanswered);
    }
}
