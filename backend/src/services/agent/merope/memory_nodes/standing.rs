//! How their life stands now (「现在」, `node.standing` in the core), written
//! from a private talk on the site. What is asked, how the answer is read
//! and the rows it makes are the core's (`myriad_nodes::stands`, the same on
//! every end); here is only what the site does around it: which facts,
//! what she recalls for each, the nodes as they are folded here, the model
//! call, and writing the rows.
//!
//! - **After a stretch is gone over** ([`after_settled`]): each fact it
//!   kept, one call each, in the order kept.
//! - **What she knew before stands first** ([`build`]): each node a
//!   stretch's facts are put on that has never held an item of hers, before
//!   those facts are read; by node, not by talk (the core's `build_due`).
//!   A rule's 「现在」 (where a session is `now`) is not hers, and the site
//!   owner's talk holds those from the start. A node no stretch has put a
//!   fact on since is built at upkeep ([`build_waiting`],
//!   `super::unbuilt`).
//! - **Kept on the site's side** (`Sync::Local`) while a side of the
//!   owner's would refuse it, written to go once every side reads it
//!   (`share::sync_for`); what was kept before goes then too
//!   (`share::share_kept`, when a side comes).
//!
//! The memory bench's trial of a kept list
//! (`memory_bench::background::state_list`) builds through [`build`] too.

use std::collections::{BTreeMap, HashMap};

use myriad_nodes::stands::{
    self, BuildDueInput, BuildInput, BuildReadInput, DueNodeIn, FactIn, KnownIn, NodeIn,
    StandingRowIn, StepInput, StepPrepared, StepRead, StepReadInput,
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

/// Each of `nodes` (of `owner`'s `audience`) with the authors and items of
/// its 「现在」 rows, each once, ended ones too: her line on a repository
/// is hers but no one's life, and only the core tells them apart. A
/// forgotten row (a stub, its body gone) stands for nothing, nor does one
/// hidden as resting on a memory they took back ([`super::hidden`]).
pub(super) async fn standing_on<C: sea_orm::ConnectionTrait>(
    db: &C,
    owner: i32,
    audience: &str,
    nodes: &[String],
) -> Result<Vec<DueNodeIn>, Error> {
    use sea_orm::{DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT row_id, node_id, author, \
             CASE WHEN body LIKE '{%' THEN body::jsonb->>'item' END AS item FROM agent_node_rows \
             WHERE user_id = $1 AND audience = $2 AND kind = $3 AND body <> '' \
             AND node_id = ANY($4)",
            [
                owner.into(),
                audience.into(),
                stands::KIND.into(),
                nodes.to_vec().into(),
            ],
        ))
        .await?;
    let mut on: Vec<DueNodeIn> = nodes
        .iter()
        .map(|node| DueNodeIn {
            node: node.clone(),
            standing: Vec::new(),
        })
        .collect();
    let hidden = if rows.is_empty() {
        Default::default()
    } else {
        super::hidden::going_of(db, owner, audience).await?
    };
    let mut seen = std::collections::HashSet::new();
    for row in &rows {
        let id: String = row.try_get("", "row_id")?;
        let node: String = row.try_get("", "node_id")?;
        let author: String = row.try_get("", "author")?;
        let item: Option<String> = row.try_get("", "item")?;
        if hidden.contains(&id) || !seen.insert((node.clone(), author, item)) {
            continue;
        }
        let standing = StandingRowIn {
            author: row.try_get("", "author")?,
            item: row.try_get("", "item")?,
        };
        if let Some(due) = on.iter_mut().find(|due| due.node == node) {
            due.standing.push(standing);
        }
    }
    Ok(on)
}

/// Times the fast model is asked before what it was asked goes unanswered:
/// a call that failed (the provider, the network) is asked again, since
/// what it would have written is otherwise never written until the same
/// thing comes up again. An answer, even one that reads as nothing, is not.
///
/// Asked again here, after a wait, rather than kept to ask at the next
/// stretch: a fact kept for later would be one more copy for forgetting to
/// reach, and this runs behind her reply, where a wait costs nobody. As
/// many times, after as long, as a memory job is tried again
/// (`store::memory_jobs`), so a connection that dropped for half a minute is
/// waited out here as there.
const ASKED: usize = super::super::store::memory_jobs::MAX_ATTEMPTS as usize;

/// How long before asking again after the `failed`th ask failed: as long as
/// a memory job waits (`store::memory_jobs::again_in`). Not at all in a test
/// build, outside a memory bench question, whose network is real.
fn wait_before(failed: usize) -> std::time::Duration {
    let failed = u8::try_from(failed).unwrap_or(u8::MAX);
    let wait = std::time::Duration::from_secs(
        super::super::store::memory_jobs::again_in(failed).unsigned_abs(),
    );
    #[cfg(test)]
    let wait = if crate::services::ai_cost_ledger::spent::counting() {
        wait
    } else {
        std::time::Duration::ZERO
    };
    wait
}

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
    // In a memory bench question, what a failed ask failed of, kept until an
    // answer makes up for it: one dropped connection, waited out, does not
    // cost the question.
    #[cfg(test)]
    let mut before = Vec::new();
    for asked in 1..=ASKED {
        if asked > 1 {
            tokio::time::sleep(wait_before(asked - 1)).await;
        }
        let answer =
            super::super::call::Ask::new(super::super::call::Voice::Judge, user_id, operation)
                .output_at_most(output)
                .json_raw(
                    &prepared.system,
                    &prepared.input,
                    schema_name,
                    &prepared.schema,
                );
        #[cfg(not(test))]
        let answer = answer.await;
        #[cfg(test)]
        let answer = {
            use crate::services::ai_cost_ledger::spent;
            let (answer, failed) = spent::failures_in(answer).await;
            match answer {
                Ok(_) => {
                    spent::made_up_for(std::mem::take(&mut before));
                    spent::said(failed);
                }
                Err(_) => before.extend(failed),
            }
            answer
        };
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
    #[cfg(test)]
    crate::services::ai_cost_ledger::spent::said(before);
    None
}

/// Writes `read`'s rows, a node at a time: a node that cannot take its rows
/// leaves the others. A node's first building (`building`) is left out on
/// a node one of hers stands on by the time it is written: a stretch and
/// upkeep may build the same node at once. How many were written.
async fn write_rows(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    read: StepRead,
    building: bool,
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
        let beside = write::Beside {
            unless_built: building,
            ..Default::default()
        };
        match write::write_with(db, owner, audience, &rows, Some(pen), &beside).await {
            // Left out whole: one of hers stood there by then.
            Ok(done) if building && done.rows.is_empty() => {}
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
    let built = Box::pin(build_on(
        db, user_id, owner, audience, leave_out, None, None, None, pen,
    ))
    .await?;
    Ok(built.written)
}

/// Each topic or person of `user_id`'s `audience` (their own) that holds
/// nothing of hers yet and has something new on it since it was last built
/// to nothing (`super::unbuilt`), built at upkeep, at most `at_most` calls
/// when given. How many calls were made.
pub(crate) async fn build_waiting(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    at_most: Option<usize>,
    pen: &Thought,
) -> Result<usize, Error> {
    let waiting = super::unbuilt::waiting(db, user_id, audience).await?;
    if waiting.is_empty() {
        return Ok(0);
    }
    let built = Box::pin(build_on(
        db,
        user_id,
        user_id,
        audience,
        &[],
        Some(&waiting),
        at_most,
        Some(super::upkeep::DRIVER),
        pen,
    ))
    .await?;
    if built.written > 0 {
        tracing::info!(
            asked = built.asked,
            written = built.written,
            "[memory-nodes] how things stand built at upkeep"
        );
    }
    Ok(built.asked)
}

/// What [`build_on`] did: rows written, and calls made.
struct BuiltOn {
    written: usize,
    asked: usize,
}

/// [`build`], on `only` those nodes when given, at most `at_most` calls
/// when given; under the driver `going`, when given, its watch hears it is
/// still going before every call (a long night of her upkeep).
#[allow(clippy::too_many_arguments)]
async fn build_on(
    db: &DatabaseConnection,
    user_id: i32,
    owner: i32,
    audience: &str,
    leave_out: &[String],
    only: Option<&[String]>,
    at_most: Option<usize>,
    going: Option<&'static str>,
    pen: &Thought,
) -> Result<BuiltOn, Error> {
    let mut built = BuiltOn {
        written: 0,
        asked: 0,
    };
    for head in read::heads(db, owner, audience, None).await? {
        if !matches!(head.kind.as_deref(), Some("topic") | Some("person"))
            || only.is_some_and(|only| !only.contains(&head.node_id))
        {
            continue;
        }
        if at_most.is_some_and(|at_most| built.asked >= at_most) {
            break;
        }
        let Some(folded) = read::node(db, owner, audience, &head.node_id).await? else {
            continue;
        };
        // Which entries it holds as read: kept if it is built to nothing,
        // so it is not asked again until they change.
        let seen = super::unbuilt::seen(&folded);
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
            // Nothing to go by (nothing on it held): not read again at each
            // upkeep until something on it changes. Not when a stretch left
            // its facts out: the step after reads those.
            if leave_out.is_empty() {
                super::unbuilt::built_empty(db, owner, audience, &head.node_id, &seen).await;
            }
            continue;
        }
        built.asked += 1;
        if let Some(driver) = going {
            crate::services::agent::alive::still_going(driver);
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
            // As one built to nothing: the same again is not asked hour
            // after hour.
            super::unbuilt::built_empty(db, owner, audience, &head.node_id, &seen).await;
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
        if read.rows.is_empty() {
            super::unbuilt::built_empty(db, owner, audience, &head.node_id, &seen).await;
            continue;
        }
        let started = read.rows.clone();
        built.written += write_rows(db, owner, audience, read, true, pen).await;
        super::kept::started(db, owner, audience, &started).await;
    }
    Ok(built)
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
    // What rests on a memory they took back goes before anything is read
    // here: forgetting never reached it if it was taken back on another of
    // her ends, or before forgetting reached 「现在」.
    super::forget::taken_back_here(db, owner, audience).await?;
    // A node a fact is put on that none of hers ever stood on (what she
    // kept there before 「现在」 was written): first how things stood there
    // before this stretch, so what it changes is changed, not only started.
    let mut touched: Vec<String> = Vec::new();
    for fact in facts {
        if let Some(node) = placements::placement(db, &fact.memory)
            .await?
            .and_then(|placement| placement.node_id)
            && !touched.contains(&node)
        {
            touched.push(node);
        }
    }
    let due = stands::build_due(&BuildDueInput {
        nodes: standing_on(db, owner, audience, &touched).await?,
    });
    if due.due {
        let kept: Vec<String> = facts.iter().map(|fact| fact.memory.clone()).collect();
        // Boxed, as is `ask` below: in the debug build the site runs, this
        // frame would otherwise hold room for each of them on the stack.
        written += Box::pin(build_on(
            db,
            user_id,
            owner,
            audience,
            &kept,
            Some(&due.nodes),
            None,
            None,
            pen,
        ))
        .await?
        .written;
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
        let still = read.still_so.clone();
        written += write_rows(db, owner, audience, read, false, pen).await;
        super::kept::seen(db, owner, audience, &fact.memory, &still).await;
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

    /// In a memory bench question an ask again waits as a memory job does,
    /// 5 s and then 30 s, and an answer that comes leaves the run unfailed.
    #[tokio::test(start_paused = true)]
    async fn in_a_bench_question_an_ask_again_waits_as_a_memory_job_does() {
        let _one = COUNTING.lock().await;
        answering(vec![None, None, Some("{\"items\":[]}")]);
        let started = tokio::time::Instant::now();
        let (answer, failed) = crate::services::ai_cost_ledger::spent::failures_in(ask(
            7,
            "memory_standing_step",
            "standing_step",
            STEP_OUTPUT,
            &prepared(),
        ))
        .await;
        crate::services::channel_group::g0::leave();
        assert_eq!(answer.as_deref(), Some("{\"items\":[]}"));
        assert_eq!(started.elapsed(), std::time::Duration::from_secs(35));
        assert!(failed.is_empty(), "{failed:?}");
    }
}
