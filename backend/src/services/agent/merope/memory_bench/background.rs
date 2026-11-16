//! Her memory after months with someone: the same kinds of question as
//! tests/merope/memory-implicit-zh.json, asked of a person she has talked
//! with for three and a half months (tests/merope/memory-long-background-zh.json,
//! made up: some three hundred sessions, mostly in private, a few in two
//! groups), so that what answers is one of hundreds of memories, not one of
//! a dozen.
//!
//! The background is written once the settle way, as production writes it
//! (each session's lines as short notes, the session gone over once it is
//! over, her nodes kept up every few sessions), into a schema of its own:
//! the snapshot. Each question, each round, starts from a copy of it, then
//! has its own sessions written after it (the change, the near miss, the
//! link) and is asked; so questions never see each other's sessions, and
//! the background is paid for once. What answers a question is what the
//! background sessions it rests on (`background_sessions`) left her, and
//! what its own sessions left. With `MEROPE_MEMORY_BENCH_SMALL=1` each
//! question is also asked with only the sessions it rests on and its own,
//! written from the start: the same question at the size of the small set,
//! so the report can say what size alone changes.
//!
//! Opt-in and spends on the site's models: `MEROPE_MEMORY_BENCH=<questions>
//! MEROPE_MEMORY_BENCH_BACKGROUND=<background> MEROPE_MEMORY_BENCH_REPORT=<new
//! file> MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test -p myriad-backend --bin
//! myriad-backend -- --ignored her_memory_after_a_long_background
//! --nocapture`, with `MEROPE_MEMORY_BENCH_TIMES`, `_PER_TYPE`, `_ONLY` and
//! `_WIDER_ARM` as for the other sets. The snapshot is kept, never dropped
//! by the run (its name is printed);
//! `MEROPE_MEMORY_BENCH_SNAPSHOT=<name>` starts from one kept earlier
//! instead of writing the background again (and leaves it). Writing keeps
//! a copy of what is written every `UPKEEP_EVERY` sessions and prints its
//! name; `MEROPE_MEMORY_BENCH_RESUME=<name>` goes on writing from that copy
//! after a run stopped partway, so what was paid for is not paid again.
//! Questions finished under the same report (its `.partial.jsonl`) are not
//! run again; `MEROPE_MEMORY_BENCH_SMALL_ONLY=1` asks only the small way.
//! `MEROPE_MEMORY_BENCH_KEEP_WRITTEN=1` keeps what each long question wrote
//! of its own sessions as a schema of its own (named in its row,
//! `keptWritten`; dropped by hand when done with), and
//! `MEROPE_MEMORY_BENCH_FROM_WRITTEN=<that run's .partial.jsonl>` asks each
//! from it instead of writing again: a change to recall or answering is
//! measured on the same writing, for what asking costs.
//! `MEROPE_MEMORY_BENCH_THEN=<file>` with it writes one more session onto
//! the questions it names first, paying only for that session.
//! Copying needs
//! a role that may set `session_replication_role` (the test database's
//! owner): the rows go in as they are, constraints and all.
//!
//! Since the background is written once, the rounds measure recall and
//! answering again and again, not writing.

use super::*;

/// Sessions of the background, in order.
#[derive(Deserialize)]
pub(super) struct Background {
    sessions: Vec<Day>,
}

#[derive(Deserialize)]
struct Day {
    date: String,
    /// `private` or `group:<id>`; private when not given.
    #[serde(default)]
    venue: Option<String>,
    turns: Vec<Turn>,
}

impl Day {
    fn venue(&self) -> &str {
        self.venue.as_deref().unwrap_or("private")
    }

    /// Its turns, every line of theirs marked as answering: what it left
    /// her is told apart by session, not by line.
    fn marked(&self) -> Vec<Turn> {
        self.turns
            .iter()
            .cloned()
            .map(|mut turn| {
                turn.has_answer = turn.role == "user";
                turn
            })
            .collect()
    }
}

/// The person's name, and the tag their groups carry, in every copy.
const PERSON: &str = "background";

mod kept;
#[cfg(test)]
mod set_tests;
mod snapshot;
mod state_links;
mod state_list;

use kept::{
    AskAgain, ask_again, from_written, keep_written, kept_written_row, then_question,
    then_sessions, topic_names, with_topics,
};
use snapshot::{read_snapshot, restore, write_snapshot};

impl Question {
    /// The same question at a small size: the background sessions it rests
    /// on, every line of theirs answering, then its own sessions.
    fn small(&self, background: &Background) -> Question {
        let mut small = self.clone();
        let rests: Vec<&Day> = self
            .background_sessions
            .iter()
            .map(|index| &background.sessions[*index])
            .collect();
        let own_venues: Vec<String> = (0..self.haystack_sessions.len())
            .map(|session| self.venue(session).to_string())
            .collect();
        small.haystack_dates = rests
            .iter()
            .map(|day| day.date.clone())
            .chain(self.haystack_dates.iter().cloned())
            .collect();
        small.haystack_sessions = rests
            .iter()
            .map(|day| day.marked())
            .chain(self.haystack_sessions.iter().cloned())
            .collect();
        small.haystack_venues = rests
            .iter()
            .map(|day| day.venue().to_string())
            .chain(own_venues)
            .collect();
        small.background_sessions = Vec::new();
        small
    }
}

/// How often each background memory came up, answering the wide way, for
/// questions it does not answer: the times in all, and the memories it
/// happened to most.
fn off_topic(memories: &[String], outcomes: &[(Question, usize, Outcome)]) -> Value {
    let mut times: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, outcome) in outcomes {
        for memory in memories {
            let answers = outcome
                .answering
                .iter()
                .any(|fact| memory.contains(fact.as_str()) || fact.contains(memory.as_str()));
            let came = outcome
                .wide
                .recalled
                .iter()
                .any(|line| line.contains(memory.as_str()));
            if came && !answers {
                *times.entry(memory.as_str()).or_default() += 1;
            }
        }
    }
    let mut most: Vec<(&str, usize)> = times.into_iter().collect();
    most.sort_by(|left, right| right.1.cmp(&left.1));
    json!({"runs": outcomes.len(), "memories": memories.len(),
        "times": most.iter().map(|(_, count)| count).sum::<usize>(),
        "most": most.into_iter().take(10).collect::<Vec<_>>()})
}

/// Each kind, the wide way: right after the long background and at the
/// small size.
fn by_size(long: &Value, small: &Value) -> Value {
    let kinds = |report: &Value| report["scores"]["wide"]["byKind"].clone();
    let (long, small) = (kinds(long), kinds(small));
    let mut out = serde_json::Map::new();
    for (kind, scores) in long.as_object().into_iter().flatten() {
        let share = |scores: &Value| {
            format!(
                "{}/{}",
                scores["right"].as_u64().unwrap_or(0),
                scores["runs"].as_u64().unwrap_or(0)
            )
        };
        out.insert(
            kind.clone(),
            json!({"long": share(scores), "small": small.get(kind).map(share)}),
        );
    }
    Value::Object(out)
}

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_memory_after_a_long_background() {
    let report_path = std::env::var("MEROPE_MEMORY_BENCH_REPORT").expect("report path");
    assert!(
        !std::path::Path::new(&report_path).exists(),
        "report must not exist"
    );
    let background: Background = serde_json::from_str(
        &std::fs::read_to_string(
            std::env::var("MEROPE_MEMORY_BENCH_BACKGROUND").expect("background"),
        )
        .unwrap(),
    )
    .unwrap();
    let all: Vec<Question> = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MEROPE_MEMORY_BENCH").expect("questions")).unwrap(),
    )
    .unwrap();
    let per_type: usize = std::env::var("MEROPE_MEMORY_BENCH_PER_TYPE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000);
    let only: Option<Vec<String>> = std::env::var("MEROPE_MEMORY_BENCH_ONLY")
        .ok()
        .map(|ids| ids.split(',').map(str::to_string).collect());
    let mut taken: BTreeMap<String, usize> = BTreeMap::new();
    let questions: Vec<Question> = all
        .into_iter()
        .filter(|question| match &only {
            Some(ids) => ids.contains(&question.question_id),
            None => {
                let count = taken.entry(question.question_type.clone()).or_default();
                *count += 1;
                *count <= per_type
            }
        })
        .collect();
    let (judge, lite) = longmemeval::the_models().await;
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let small_only =
        std::env::var("MEROPE_MEMORY_BENCH_SMALL_ONLY").is_ok_and(|only| only.trim() == "1");
    let kept = std::env::var("MEROPE_MEMORY_BENCH_SNAPSHOT").ok();
    let resume = std::env::var("MEROPE_MEMORY_BENCH_RESUME").ok();
    let snapshot = match &kept {
        Some(name) => crate::db::IsolatedSchema::existing(&url, name).await,
        None if small_only => panic!("asking only the small way runs from a kept snapshot"),
        None => write_snapshot(&url, &judge, &background, resume.as_deref()).await,
    };
    // A snapshot just written is kept before anything else is asked, so
    // a question that fails cannot cost it.
    let (kept, snapshot) = match kept {
        Some(name) => (Some(name), snapshot),
        None => {
            let name = snapshot.keep().await;
            println!("snapshot kept: MEROPE_MEMORY_BENCH_SNAPSHOT={name}");
            (
                Some(name.clone()),
                crate::db::IsolatedSchema::existing(&url, &name).await,
            )
        }
    };
    // Runs this one did not start, having been told to stop.
    let not_started = std::sync::atomic::AtomicUsize::new(0);
    let not_started = &not_started;
    // What an earlier run under this report finished is not run again.
    let finished = longmemeval::finished(&report_path);
    if !finished.is_empty() {
        println!("{} runs finished earlier are not run again", finished.len());
    }
    let (user_id, left) = read_snapshot(&snapshot.db).await;
    let background_spent = crate::services::ai_cost_ledger::spent::take();
    let mut memories: Vec<String> = Vec::new();
    let mut places: Vec<String> = background
        .sessions
        .iter()
        .map(|day| day.venue().to_string())
        .collect();
    places.sort();
    places.dedup();
    for place in &places {
        for row in crate::services::agent::memory::unified::active_in(
            &snapshot.db,
            user_id,
            &audience_at(place, user_id, PERSON),
            &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
        )
        .await
        .unwrap_or_default()
        {
            if !memories.contains(&row.content) {
                memories.push(row.content);
            }
        }
    }
    // How much she holds: rows past the cap fade.
    let count = |sql: String| {
        let db = &snapshot.db;
        async move {
            db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
                .await
                .unwrap()
                .unwrap()
                .try_get::<i64>("", "n")
                .unwrap()
        }
    };
    let held = json!({
        "active": count("SELECT COUNT(*) AS n FROM agent_memories WHERE invalid_at IS NULL".into()).await,
        "faded": count("SELECT COUNT(*) AS n FROM agent_memories WHERE invalid_reason = 'faded'".into()).await,
        "cap": crate::services::agent::memory::unified::MAX_ACTIVE_PER_USER,
    });
    println!("background: {} memories, held {held}", memories.len());
    let times = longmemeval::times();
    let runs: Vec<(Question, usize)> = (0..times)
        .flat_map(|round| questions.iter().cloned().map(move |q| (q, round)))
        .collect();
    let name = snapshot.name().to_string();
    let long_runs: Vec<(Question, usize)> = if small_only {
        Vec::new()
    } else {
        runs.iter()
            .filter(|(question, round)| {
                !finished.contains(&("long".to_string(), question.question_id.clone(), *round))
            })
            .cloned()
            .collect()
    };
    let (keeping, from_written, thens) = (keep_written(), from_written(), then_sessions());
    let asking_again = ask_again();
    assert!(
        asking_again.is_none() || from_written.is_some(),
        "MEROPE_MEMORY_BENCH_ASK_AGAIN asks again from what was kept"
    );
    let (from_written, thens) = (&from_written, &thens);
    let long: Vec<(Question, usize, Outcome)> = futures::stream::iter(long_runs)
        .map(|(question, round)| {
            let (url, name, judge, lite, left) = (&url, &name, &judge, &lite, &left);
            let report_path = &report_path;
            async move {
                if let Some(why) = crate::services::ai_cost_ledger::spent::stop_starting() {
                    println!("long {} #{round} not started: {why}", question.question_id);
                    not_started.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return None;
                }
                let id = format!("long-{}-{round}", question.question_id);
                // What it wrote: written now, or as an earlier run kept it.
                let (copy, written, write_failed, kept_as, asked) = match &from_written {
                    Some(earlier) => {
                        let Some(earlier) = earlier.get(&(question.question_id.clone(), round))
                        else {
                            println!(
                                "long {} #{round} not started: nothing it wrote was kept",
                                question.question_id
                            );
                            not_started.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            return None;
                        };
                        let copy = restore(url, &earlier.schema).await;
                        let (mut written, mut failed) =
                            (earlier.written.clone(), earlier.failed.clone());
                        let kept_as = match thens.get(&question.question_id) {
                            Some(then) => {
                                let (more, more_failed) =
                                    crate::services::ai_cost_ledger::spent::failures_in(write_for(
                                        &copy.db,
                                        judge,
                                        &then_question(&question, then),
                                        &format!("{id}-then"),
                                        (user_id, PERSON),
                                        written.answering.clone(),
                                    ))
                                    .await;
                                written = Written {
                                    answering: more.answering,
                                    kept: written.kept + more.kept,
                                };
                                failed.extend(more_failed);
                                match keeping {
                                    true => Some(restore(url, copy.name()).await.keep().await),
                                    false => None,
                                }
                            }
                            None => keeping.then(|| earlier.schema.clone()),
                        };
                        let asked = match asking_again {
                            None => earlier.asked.clone(),
                            Some(AskAgain::Plain) => None,
                            Some(AskAgain::Topics) => {
                                let topics = topic_names(&copy.db, user_id).await;
                                let (system, input) = with_topics(&question, &topics);
                                let (answer, more_failed) =
                                    crate::services::ai_cost_ledger::spent::failures_in(async {
                                        judge
                                            .analyze_json(
                                                &system,
                                                &input,
                                                myriad_merope::remembering::SCHEMA_NAME,
                                                Some(&myriad_merope::remembering::schema()),
                                            )
                                            .await
                                            .ok()
                                    })
                                    .await;
                                failed.extend(more_failed);
                                answer
                            }
                        };
                        (copy, written, failed, kept_as, asked)
                    }
                    None => {
                        let copy = restore(url, name).await;
                        let answering: Vec<String> = question
                            .background_sessions
                            .iter()
                            .flat_map(|index| left.get(index).cloned().unwrap_or_default())
                            .collect();
                        let (written, failed) =
                            crate::services::ai_cost_ledger::spent::failures_in(write_for(
                                &copy.db,
                                judge,
                                &question,
                                &id,
                                (user_id, PERSON),
                                answering,
                            ))
                            .await;
                        let kept_as = match keeping {
                            true => Some(restore(url, copy.name()).await.keep().await),
                            false => None,
                        };
                        (copy, written, failed, kept_as, None)
                    }
                };
                let (mut outcome, failed) =
                    crate::services::ai_cost_ledger::spent::failures_in(answer_for(
                        &copy.db,
                        (judge, lite, None),
                        &question,
                        &id,
                        (user_id, PERSON),
                        written.clone(),
                        asked,
                    ))
                    .await;
                outcome.failed = write_failed.iter().cloned().chain(failed).collect();
                copy.drop().await;
                println!(
                    "long {} #{round} void={} wide={:?} wider={:?} chars={} charged=${:.4}",
                    question.question_id,
                    outcome.void(),
                    outcome.wide.correct,
                    outcome.wider.as_ref().map(|wider| wider.correct),
                    longmemeval::recalled_chars(&outcome.wide),
                    crate::services::ai_cost_ledger::spent::charged_so_far()
                );
                let mut row = longmemeval::partial_row("long", &question, round, &outcome);
                if let Some(schema) = kept_as {
                    row["keptWritten"] = kept_written_row(&schema, &written, &write_failed);
                }
                longmemeval::keep_partial(report_path, &row);
                Some((question, round, outcome))
            }
        })
        .buffer_unordered(at_once())
        .filter_map(std::future::ready)
        .collect()
        .await;
    let long_spent = crate::services::ai_cost_ledger::spent::take();
    let small = if small_only || std::env::var_os("MEROPE_MEMORY_BENCH_SMALL").is_some() {
        let isolated = crate::db::IsolatedSchema::migrated(&url, "memory_small").await;
        let small_runs: Vec<(Question, usize)> = runs
            .into_iter()
            .filter(|(question, round)| {
                !finished.contains(&("small".to_string(), question.question_id.clone(), *round))
            })
            .collect();
        let small: Vec<(Question, usize, Outcome)> = futures::stream::iter(small_runs)
            .map(|(question, round)| {
                let (db, judge, lite, background) =
                    (isolated.db.clone(), &judge, &lite, &background);
                let report_path = &report_path;
                async move {
                    if let Some(why) = crate::services::ai_cost_ledger::spent::stop_starting() {
                        println!("small {} #{round} not started: {why}", question.question_id);
                        not_started.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return None;
                    }
                    let small = question.small(background);
                    let (mut outcome, failed) =
                        crate::services::ai_cost_ledger::spent::failures_in(run_one(
                            &db, judge, lite, None, &small, round,
                        ))
                        .await;
                    outcome.failed = failed;
                    println!(
                        "small {} #{round} void={} wide={:?} chars={} charged=${:.4}",
                        question.question_id,
                        outcome.void(),
                        outcome.wide.correct,
                        longmemeval::recalled_chars(&outcome.wide),
                        crate::services::ai_cost_ledger::spent::charged_so_far()
                    );
                    longmemeval::keep_partial(
                        report_path,
                        &longmemeval::partial_row("small", &question, round, &outcome),
                    );
                    Some((question, round, outcome))
                }
            })
            .buffer_unordered(at_once())
            .filter_map(std::future::ready)
            .collect()
            .await;
        isolated.drop().await;
        Some(small)
    } else {
        None
    };
    // The snapshot is never dropped here: it is written once and kept to
    // be used again, and dropped by hand when no longer needed.
    println!(
        "snapshot kept: MEROPE_MEMORY_BENCH_SNAPSHOT={}",
        kept.unwrap_or_else(|| snapshot.name().to_string())
    );
    let _ = snapshot.keep().await;
    // A run stopped before all it was to ask writes no report: run again
    // under the same report, it goes on past what it finished.
    let skipped = not_started.load(std::sync::atomic::Ordering::Relaxed);
    if skipped > 0 {
        println!(
            "stopped ({}): {skipped} not started, {} long and {} small finished this time; \
             run again with the same report to go on",
            crate::services::ai_cost_ledger::spent::stop_starting().unwrap_or_default(),
            long.len(),
            small.as_ref().map_or(0, Vec::len)
        );
        println!("spent: {}", crate::services::ai_cost_ledger::spent::take());
        return;
    }
    let long_report = longmemeval::report(&questions, times, &long);
    let small_report = small
        .as_ref()
        .map(|small| longmemeval::report(&questions, times, small));
    let report = json!({
        "background": {"sessions": background.sessions.len(), "memories": memories.len(),
            "held": held, "spent": background_spent},
        "long": long_report,
        "small": small_report,
        "bySize": small_report.as_ref().map(|small| by_size(&long_report, small)),
        "offTopic": off_topic(&memories, &long),
        "spent": {"long": long_spent, "small": crate::services::ai_cost_ledger::spent::take()},
        "lite": longmemeval::lite_in_use().await,
        "finishedEarlier": finished.len(),
        "matchedLengthOf": std::env::var("MEROPE_MEMORY_BENCH_RECALL_CHARS_OF").ok(),
        "keptWritten": keep_written(),
        "fromWritten": std::env::var("MEROPE_MEMORY_BENCH_FROM_WRITTEN").ok(),
    });
    println!("{}", report["long"]["scores"]);
    std::fs::write(&report_path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}

/// One fact kept as the bench keeps what someone already told her.
async fn remember(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    at: chrono::DateTime<chrono::FixedOffset>,
    fact: &str,
) {
    let updates = super::super::chat_remember::ChatMemoryUpdates {
        updates: vec![super::super::chat_remember::ChatMemoryUpdate {
            standing: Default::default(),
            fact: Some(fact.to_string()),
            supersedes: Vec::new(),
            evidence: Some(fact.to_string()),
            concepts: Vec::new(),
            topic: None,
            cues: Vec::new(),
        }],
        ..Default::default()
    };
    apply_at(db, user_id, &Audience::private(user_id), at, &updates)
        .await
        .expect("kept");
}
