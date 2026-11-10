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
/// Her nodes kept up after this many sessions, as a day's upkeep would.
const UPKEEP_EVERY: usize = 10;
/// Beside the snapshot's own tables: what each background session left
/// her, never copied into a question's copy.
const LEFT: &str = "bench_background_left";
/// Beside them too: the last background session written in full.
const DONE: &str = "bench_background_done";

/// What is written is kept apart to go on from after this many sessions
/// (`MEROPE_MEMORY_BENCH_KEEP_EVERY`, as often as her nodes are kept up
/// when not said).
fn keep_every() -> usize {
    std::env::var("MEROPE_MEMORY_BENCH_KEEP_EVERY")
        .ok()
        .and_then(|every| every.parse().ok())
        .filter(|every| *every > 0)
        .unwrap_or(UPKEEP_EVERY)
}

/// The background written into a schema of its own, the settle way; from
/// `resume` (a copy kept while writing) on, when given.
async fn write_snapshot(
    url: &str,
    judge: &crate::services::analyzer::AiAnalyzer,
    background: &Background,
    resume: Option<&str>,
) -> crate::db::IsolatedSchema {
    let (snapshot, user_id, start) = match resume {
        Some(kept) => {
            let snapshot = copy_whole(url, kept).await;
            let (user_id, _) = read_snapshot(&snapshot.db).await;
            let done = done_upto(&snapshot.db).await;
            println!("background resumed from {kept} after session {done:?}");
            (snapshot, user_id, done.map_or(0, |done| done + 1))
        }
        None => {
            let snapshot = crate::db::IsolatedSchema::migrated(url, "memory_background").await;
            let user_id = new_user(&snapshot.db, PERSON).await;
            snapshot
                .db
                .execute_unprepared(&format!(
                    "CREATE TABLE {LEFT} (session INTEGER NOT NULL, fact TEXT NOT NULL); \
                     CREATE TABLE {DONE} (upto INTEGER NOT NULL)"
                ))
                .await
                .unwrap();
            (snapshot, user_id, 0)
        }
    };
    let db = &snapshot.db;
    let mut checkpoint: Option<String> = resume.map(str::to_string);
    // Short notes still fresh at the end stay, as on the day after.
    let end = &background.sessions.last().expect("a background").date;
    for (index, day) in background.sessions.iter().enumerate().skip(start) {
        if let Some(why) = crate::services::ai_cost_ledger::spent::stop_starting() {
            panic!(
                "background stopped before session {index}: {why}; \
                 MEROPE_MEMORY_BENCH_RESUME={}",
                checkpoint.as_deref().unwrap_or("(none yet)")
            );
        }
        let at = date_of(&day.date);
        let here = audience_at(day.venue(), user_id, PERSON);
        let turns = day.marked();
        keep_chat(db, &format!("{PERSON}-{index}"), user_id, at, &turns, &here).await;
        let (mut left, mut kept) = (Vec::new(), 0);
        settle::go_over(
            db,
            judge,
            user_id,
            &[(at, &turns, here)],
            end,
            &mut left,
            &mut kept,
        )
        .await;
        for fact in &left {
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &format!("INSERT INTO {LEFT} (session, fact) VALUES ($1, $2)"),
                [(index as i32).into(), fact.clone().into()],
            ))
            .await
            .unwrap();
        }
        println!(
            "background session {index}: {} left, charged ${:.4}",
            left.len(),
            crate::services::ai_cost_ledger::spent::charged_so_far()
        );
        if (index + 1) % UPKEEP_EVERY == 0 {
            settle::keep_up_nodes(db, user_id).await;
        }
        if (index + 1) % keep_every() == 0 {
            mark_done(db, index).await;
            // What is written so far kept apart, to go on from; the copy
            // before it is no longer needed.
            let kept = copy_whole(url, snapshot.name()).await.keep().await;
            println!("background kept after session {index}: MEROPE_MEMORY_BENCH_RESUME={kept}");
            if let Some(before) = checkpoint.replace(kept) {
                crate::db::IsolatedSchema::existing(url, &before)
                    .await
                    .drop()
                    .await;
            }
        }
        // To try going on from a kept copy without paying for a whole run.
        if std::env::var("MEROPE_MEMORY_BENCH_STOP_AFTER")
            .ok()
            .and_then(|after| after.parse::<usize>().ok())
            == Some(index)
        {
            panic!(
                "background stopped after session {index} as asked; \
                 MEROPE_MEMORY_BENCH_RESUME={}",
                checkpoint.as_deref().unwrap_or("(none yet)")
            );
        }
    }
    settle::keep_up_nodes(db, user_id).await;
    mark_done(db, background.sessions.len() - 1).await;
    if let Some(before) = checkpoint {
        crate::db::IsolatedSchema::existing(url, &before)
            .await
            .drop()
            .await;
    }
    snapshot
}

/// The last background session written in full, as the snapshot says.
async fn done_upto(db: &sea_orm::DatabaseConnection) -> Option<usize> {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        format!("SELECT MAX(upto) AS upto FROM {DONE}"),
    ))
    .await
    .unwrap()
    .and_then(|row| row.try_get::<Option<i32>>("", "upto").unwrap())
    .map(|upto| upto as usize)
}

async fn mark_done(db: &sea_orm::DatabaseConnection, index: usize) {
    db.execute_unprepared(&format!("DELETE FROM {DONE}"))
        .await
        .unwrap();
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &format!("INSERT INTO {DONE} (upto) VALUES ($1)"),
        [(index as i32).into()],
    ))
    .await
    .unwrap();
}

/// A copy of `snapshot` with what is kept beside it: to write on from.
async fn copy_whole(url: &str, snapshot: &str) -> crate::db::IsolatedSchema {
    let copy = restore(url, snapshot).await;
    let to = copy.name().to_string();
    for side in [LEFT, DONE] {
        copy.db
            .execute_unprepared(&format!(
                "CREATE TABLE \"{to}\".{side} AS SELECT * FROM \"{snapshot}\".{side}"
            ))
            .await
            .unwrap();
    }
    copy
}

/// What the snapshot holds of the person: their id, and what each
/// background session left her.
async fn read_snapshot(db: &sea_orm::DatabaseConnection) -> (i32, BTreeMap<usize, Vec<String>>) {
    let user_id = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE username = $1",
            [PERSON.into()],
        ))
        .await
        .unwrap()
        .expect("the person")
        .try_get::<i32>("", "id")
        .unwrap();
    let mut left: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for row in db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!("SELECT session, fact FROM {LEFT} ORDER BY session"),
        ))
        .await
        .unwrap()
    {
        left.entry(row.try_get::<i32>("", "session").unwrap() as usize)
            .or_default()
            .push(row.try_get("", "fact").unwrap());
    }
    (user_id, left)
}

/// The snapshot's tables, as copies take them.
async fn tables(db: &sea_orm::DatabaseConnection, schema: &str) -> Vec<String> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT table_name::text AS name FROM information_schema.tables \
         WHERE table_schema = $1 AND table_type = 'BASE TABLE' \
           AND table_name NOT IN ('seaql_migrations', $2, $3) ORDER BY 1",
        [schema.into(), LEFT.into(), DONE.into()],
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get("", "name").unwrap())
    .collect()
}

/// A fresh schema holding what the snapshot holds, row for row, its
/// sequences going on from there.
async fn restore(url: &str, snapshot: &str) -> crate::db::IsolatedSchema {
    use sea_orm::TransactionTrait;
    let copy = crate::db::IsolatedSchema::migrated(url, "memory_long").await;
    let db = &copy.db;
    let to = copy.name().to_string();
    let tables = tables(db, snapshot).await;
    let transaction = db.begin().await.unwrap();
    transaction
        .execute_unprepared("SET LOCAL session_replication_role = replica")
        .await
        .unwrap();
    // What the migrations seeded makes way for what the snapshot has.
    let all: Vec<String> = tables
        .iter()
        .map(|table| format!("\"{to}\".\"{table}\""))
        .collect();
    transaction
        .execute_unprepared(&format!("TRUNCATE {}", all.join(", ")))
        .await
        .unwrap();
    for table in &tables {
        let columns: Vec<String> = transaction
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT column_name::text AS name FROM information_schema.columns \
                 WHERE table_schema = $1 AND table_name = $2 AND is_generated = 'NEVER' \
                 ORDER BY ordinal_position",
                [snapshot.into(), table.clone().into()],
            ))
            .await
            .unwrap()
            .iter()
            .map(|row| format!("\"{}\"", row.try_get::<String>("", "name").unwrap()))
            .collect();
        let columns = columns.join(", ");
        transaction
            .execute_unprepared(&format!(
                "INSERT INTO \"{to}\".\"{table}\" ({columns}) OVERRIDING SYSTEM VALUE \
                 SELECT {columns} FROM \"{snapshot}\".\"{table}\""
            ))
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
    let sequences = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT table_name::text AS t, column_name::text AS c, \
               pg_get_serial_sequence(format('%I.%I', table_schema, table_name), column_name) AS s \
             FROM information_schema.columns WHERE table_schema = $1 \
               AND pg_get_serial_sequence(format('%I.%I', table_schema, table_name), column_name) \
                 IS NOT NULL",
            [to.clone().into()],
        ))
        .await
        .unwrap();
    for row in &sequences {
        let (table, column, sequence): (String, String, String) = (
            row.try_get("", "t").unwrap(),
            row.try_get("", "c").unwrap(),
            row.try_get("", "s").unwrap(),
        );
        db.execute_unprepared(&format!(
            "SELECT setval('{sequence}', COALESCE((SELECT MAX(\"{column}\") FROM \
             \"{to}\".\"{table}\"), 0) + 1, false)"
        ))
        .await
        .unwrap();
    }
    copy
}

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
                let copy = restore(url, name).await;
                let answering: Vec<String> = question
                    .background_sessions
                    .iter()
                    .flat_map(|index| left.get(index).cloned().unwrap_or_default())
                    .collect();
                let (mut outcome, failed) =
                    crate::services::ai_cost_ledger::spent::failures_in(run_for(
                        &copy.db,
                        (judge, lite, None),
                        &question,
                        &format!("long-{}-{round}", question.question_id),
                        (user_id, PERSON),
                        answering,
                    ))
                    .await;
                outcome.failed = failed;
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
                longmemeval::keep_partial(
                    report_path,
                    &longmemeval::partial_row("long", &question, round, &outcome),
                );
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
    });
    println!("{}", report["long"]["scores"]);
    std::fs::write(&report_path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}

/// A copy of a snapshot is the snapshot, row for row, and copies are apart:
/// what is written in one is in no other, each goes on numbering from where
/// the snapshot stopped, and recalling in two at once, for the same person,
/// finds in each only what that one holds. On the test database
/// (`MYRIAD_MEDIA_TEST_DATABASE_URL`), skipped without it.
#[tokio::test]
async fn a_restored_snapshot_is_the_snapshot_and_copies_are_apart() {
    let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
        return;
    };
    let snapshot = crate::db::IsolatedSchema::migrated(&url, "bench_snapshot").await;
    let user_id = new_user(&snapshot.db, PERSON).await;
    let at = date_of("2026/10/01 (Thu) 21:00");
    let turns: Vec<Turn> = serde_json::from_value(json!([
        {"role": "user", "content": "我家孔雀鱼又生了一窝 二十多条"},
        {"role": "assistant", "content": "鱼缸要挤爆了"}
    ]))
    .unwrap();
    keep_chat(
        &snapshot.db,
        "background-0",
        user_id,
        at,
        &turns,
        &Audience::private(user_id),
    )
    .await;
    remember(
        &snapshot.db,
        user_id,
        at,
        "养了一缸孔雀鱼，又生了二十多条小鱼",
    )
    .await;
    snapshot
        .db
        .execute_unprepared(&format!(
            "CREATE TABLE {LEFT} (session INTEGER NOT NULL, fact TEXT NOT NULL)"
        ))
        .await
        .unwrap();
    let (a, b) = futures::join!(
        restore(&url, snapshot.name()),
        restore(&url, snapshot.name())
    );
    // Row for row; what is beside the snapshot is not copied.
    for table in tables(&snapshot.db, snapshot.name()).await {
        let rows = |schema: String| {
            let (db, table) = (&a.db, table.clone());
            async move {
                db.query_one_raw(Statement::from_string(
                    DatabaseBackend::Postgres,
                    format!(
                        "SELECT md5(COALESCE(string_agg(x::text, '|' ORDER BY x::text), '')) AS h \
                         FROM \"{schema}\".\"{table}\" x"
                    ),
                ))
                .await
                .unwrap()
                .unwrap()
                .try_get::<String>("", "h")
                .unwrap()
            }
        };
        assert_eq!(
            rows(snapshot.name().to_string()).await,
            rows(a.name().to_string()).await,
            "{table}"
        );
    }
    assert!(
        tables(&a.db, a.name())
            .await
            .iter()
            .all(|table| table != LEFT)
    );
    let copied =
        a.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!(
                "SELECT COUNT(*) AS n FROM information_schema.tables \
                 WHERE table_schema = '{}' AND table_name = '{LEFT}'",
                a.name()
            ),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap();
    assert_eq!(copied, 0);
    // Written in one, in that one only; both number on from the snapshot.
    remember(&a.db, user_id, at, "家里那只乌龟叫阿慢，十岁了").await;
    let (in_a, in_b) = (new_user(&a.db, "next").await, new_user(&b.db, "next").await);
    assert_eq!(in_a, in_b);
    assert!(in_a > user_id);
    let question: Question = serde_json::from_value(json!({
        "question_id": "apart", "question_type": "single-session-user",
        "question": "我家乌龟阿慢多大了", "answer": "十岁",
        "question_date": "2026/10/02 (Fri) 21:00",
        "haystack_dates": [], "haystack_sessions": []
    }))
    .unwrap();
    let present = Audience::private(user_id);
    let ((from_a, _), (from_b, _)) = futures::join!(
        recalled_for(&a.db, &question, user_id, &present, None, THOROUGH),
        recalled_for(&b.db, &question, user_id, &present, None, THOROUGH)
    );
    assert!(
        from_a.iter().any(|line| line.contains("阿慢")),
        "{from_a:?}"
    );
    assert!(
        !from_b.iter().any(|line| line.contains("阿慢")),
        "{from_b:?}"
    );
    for schema in [a, b, snapshot] {
        schema.drop().await;
    }
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

const BACKGROUND: &str =
    include_str!("../../../../../../tests/merope/memory-long-background-zh.json");
const LONG: &str = include_str!("../../../../../../tests/merope/memory-long-zh.json");
const HELD_OUT: &str = include_str!("../../../../../../tests/merope/holdout/memory-long-zh.json");

/// The background and the questions asked after it read as the run needs:
/// the background in order, each session theirs first and taking turns;
/// each question resting on sessions the background has, its own sessions
/// after the background and before it is asked, graded by a rule of its
/// type (a detail asked plainly by the default one); both sides of a pair
/// on the same talk; the held-out questions none of the others.
#[test]
fn the_long_background_reads_as_the_bench_needs() {
    let background: Background = serde_json::from_str(BACKGROUND).unwrap();
    let dates: Vec<_> = background
        .sessions
        .iter()
        .map(|day| date_of(&day.date))
        .collect();
    assert!(background.sessions.len() >= 250);
    assert!(dates.windows(2).all(|pair| pair[0] < pair[1]));
    for (index, day) in background.sessions.iter().enumerate() {
        assert!(
            day.venue() == "private" || day.venue().starts_with("group:"),
            "{index}"
        );
        assert!((4..=8).contains(&day.turns.len()), "{index}");
        for (at, turn) in day.turns.iter().enumerate() {
            let theirs = at % 2 == 0;
            assert_eq!(turn.role == "user", theirs, "{index}");
        }
    }
    let end = *dates.last().unwrap();
    let read = |text: &str| -> Vec<Question> { serde_json::from_str(text).unwrap() };
    let (long, held_out) = (read(LONG), read(HELD_OUT));
    for questions in [&long, &held_out] {
        let mut ids = std::collections::BTreeSet::new();
        for question in questions.iter() {
            let id = question.question_id.as_str();
            assert!(ids.insert(id), "{id}");
            assert_eq!(rule_key(question), question.question_type, "{id}");
            let categories: &[&str] = match question.question_type.as_str() {
                "implicit-state" | "stale-premise" | "implicit-use" => &["type-1", "type-2"],
                "associated" => &super::chinese::ASSOCIATED,
                "single-session-user" => &["needle"],
                "feeling-cause" | "still-true" => &[],
                other => panic!("{id}: {other}"),
            };
            match &question.category {
                Some(category) => assert!(categories.contains(&category.as_str()), "{id}"),
                None => assert!(categories.is_empty(), "{id}"),
            }
            assert!(
                question.question_type == "single-session-user"
                    || prompts()["grade"]["rules"][rule_key(question)].is_string(),
                "{id}"
            );
            assert!(!question.background_sessions.is_empty(), "{id}");
            assert!(
                question
                    .background_sessions
                    .iter()
                    .all(|index| *index < background.sessions.len()),
                "{id}"
            );
            assert_eq!(
                question.haystack_dates.len(),
                question.haystack_sessions.len(),
                "{id}"
            );
            let asked = date_of(&question.question_date);
            assert!(asked > end, "{id}");
            for date in &question.haystack_dates {
                let at = date_of(date);
                assert!(at > end && at < asked, "{id}");
            }
            assert_eq!(
                question.pair.is_some(),
                question.category.as_deref() == Some("paired"),
                "{id}"
            );
            if let Some(pair) = &question.pair {
                let sides: Vec<&Question> = questions
                    .iter()
                    .filter(|other| other.pair.as_ref() == Some(pair))
                    .collect();
                assert_eq!(sides.len(), 2, "{id}");
                assert_eq!(
                    sides[0].background_sessions, sides[1].background_sessions,
                    "{id}"
                );
            }
            // At the small size it is its own sessions after those it rests on.
            let small = question.small(&background);
            assert_eq!(
                small.haystack_sessions.len(),
                question.background_sessions.len() + question.haystack_sessions.len(),
                "{id}"
            );
        }
    }
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for question in &long {
        if question.question_id.ends_with("-hold") {
            continue;
        }
        let kind = match &question.category {
            Some(category) => format!("{}/{category}", question.question_type),
            None => question.question_type.clone(),
        };
        *kinds.entry(kind).or_default() += 1;
    }
    let mut expected: BTreeMap<String, usize> = [
        ("feeling-cause".to_string(), 6),
        ("still-true".to_string(), 20),
        ("single-session-user/needle".to_string(), 10),
    ]
    .into_iter()
    .collect();
    for kind in ["implicit-state", "stale-premise", "implicit-use"] {
        for category in ["type-1", "type-2"] {
            expected.insert(format!("{kind}/{category}"), 10);
        }
    }
    for category in super::chinese::ASSOCIATED {
        expected.insert(format!("associated/{category}"), 3);
    }
    assert_eq!(kinds, expected);
    for question in &held_out {
        assert!(
            !long
                .iter()
                .any(|other| other.question_id == question.question_id
                    || other.question == question.question),
            "{}",
            question.question_id
        );
    }
}
