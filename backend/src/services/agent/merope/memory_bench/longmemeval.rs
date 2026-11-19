//! Her memory on LongMemEval, question by question.

use super::*;

pub(super) use super::scored::{finished, keep_partial, partial_row, recalled_chars, report};

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_memory_on_longmemeval() {
    // With RUST_LOG set, what production would log goes to the run's log:
    // a step that gave up shows why.
    if std::env::var_os("RUST_LOG").is_some() {
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
        let _ = tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .try_init();
    }
    let path = std::env::var("MEROPE_MEMORY_BENCH").expect("MEROPE_MEMORY_BENCH");
    let report_path = std::env::var("MEROPE_MEMORY_BENCH_REPORT").expect("report path");
    assert!(
        !std::path::Path::new(&report_path).exists(),
        "report must not exist"
    );
    let per_type: usize = std::env::var("MEROPE_MEMORY_BENCH_PER_TYPE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5);
    let only: Option<Vec<String>> = std::env::var("MEROPE_MEMORY_BENCH_ONLY")
        .ok()
        .map(|ids| ids.split(',').map(str::to_string).collect());
    let all: Vec<Question> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
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
    let (judge, lite) = the_models().await;
    // Read before anything is paid for: a value it does not take stops here.
    let write_thinking = crate::services::analyzer::write_thinking::asked();
    // `MEROPE_MEMORY_BENCH_READER=<model>`: also answer and grade the wide
    // way with that model on Lite's provider, as published results did.
    let reader = match std::env::var("MEROPE_MEMORY_BENCH_READER") {
        Ok(model) => Some(
            crate::services::ai::create_lite_ai_analyzer_as(&model, Some(Duration::from_secs(60)))
                .await
                .expect("reader model"),
        ),
        Err(_) => None,
    };
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "memory_bench").await;
    let db = isolated.db.clone();
    let times = times();
    // `MEROPE_MEMORY_BENCH_KEEP_WRITTEN=1`: each question written into a
    // schema of its own and kept; `MEROPE_MEMORY_BENCH_FROM_WRITTEN`: asked
    // from what an earlier run kept, nothing written again.
    let (keeping, from_written) = (
        super::background::keep_written(),
        super::background::from_written(),
    );
    let runs: Vec<(Question, usize)> = (0..times)
        .flat_map(|round| {
            questions
                .iter()
                .cloned()
                .map(move |question| (question, round))
        })
        .filter(|(question, round)| match &from_written {
            Some(kept) if !kept.contains_key(&(question.question_id.clone(), *round)) => {
                println!(
                    "{} #{round} not started: nothing it wrote was kept",
                    question.question_id
                );
                false
            }
            _ => true,
        })
        .collect();
    let (url, from_written) = (&url, &from_written);
    let (outcomes, not_started) = started_while_allowed(
        runs,
        at_once(),
        crate::services::ai_cost_ledger::spent::stop_starting,
        |(question, round)| {
            let (db, judge, lite, reader) = (db.clone(), &judge, &lite, reader.as_ref());
            let report_path = &report_path;
            async move {
                let (mut outcome, failed, kept) = if keeping || from_written.is_some() {
                    let earlier = from_written
                        .as_ref()
                        .and_then(|kept| kept.get(&(question.question_id.clone(), round)));
                    on_its_own(
                        url,
                        (judge, lite, reader),
                        &question,
                        round,
                        keeping,
                        earlier,
                    )
                    .await
                } else {
                    let (outcome, failed) = crate::services::ai_cost_ledger::spent::failures_in(
                        ROUND.scope(round, run_one(&db, judge, lite, reader, &question, round)),
                    )
                    .await;
                    (outcome, failed, None)
                };
                outcome.failed = failed;
                println!(
                    "{} {} #{round} void={} written={} plain={:?} cued={:?} wide={:?} read={:?}",
                    question.question_type,
                    question.question_id,
                    outcome.void(),
                    outcome.written,
                    outcome.plain.as_ref().map(|plain| plain.correct),
                    outcome.cued.as_ref().map(|cued| cued.correct),
                    outcome.wide.correct,
                    outcome.read.as_ref().map(|read| read.correct)
                );
                // Kept as it is done, so a run stopped partway keeps what it
                // finished (the full report is written only at the end).
                let mut row = partial_row("", &question, round, &outcome);
                if let Some((schema, written, write_failed)) = kept {
                    row["keptWritten"] =
                        super::background::kept_written_row(&schema, &written, &write_failed);
                }
                keep_partial(report_path, &row);
                (question, round, outcome)
            }
        },
    )
    .await;
    isolated.drop().await;
    // Scored over what was run: a question past the limit is not in it.
    let questions: Vec<Question> = questions
        .into_iter()
        .filter(|question| {
            outcomes
                .iter()
                .any(|(done, _, _)| done.question_id == question.question_id)
        })
        .collect();
    let mut report = report(&questions, times, &outcomes);
    report["notStarted"] = json!(not_started);
    report["spent"] = crate::services::ai_cost_ledger::spent::take();
    report["lite"] = lite_in_use().await;
    // Asked from what an earlier run wrote, nothing here was written.
    report["writeThinking"] = json!(
        write_thinking
            .filter(|_| from_written.is_none())
            .map(|thinking| format!("{thinking:?}"))
    );
    println!("{}", report["scores"]);
    std::fs::write(&report_path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}

/// A question in a schema of its own: written there and, when `keeping`,
/// a copy of what it wrote kept (`memory_long_…`, as the long questions'
/// are) before it is asked; or, from what an `earlier` run kept, only
/// asked, on a copy, so what was kept stays as it was. Also every failure
/// said, writing's included, and what was kept: its schema, what of it
/// answers the question, and the failures writing it.
async fn on_its_own(
    url: &str,
    models: Models<'_>,
    question: &Question,
    round: usize,
    keeping: bool,
    earlier: Option<&super::background::KeptWritten>,
) -> (Outcome, Vec<String>, Option<(String, Written, Vec<String>)>) {
    use crate::services::ai_cost_ledger::spent::failures_in;
    let name = format!("lme-{}-{round}", question.question_id);
    match earlier {
        Some(earlier) => {
            let copy = super::background::restore(url, &earlier.schema).await;
            let user_id = user_named(&copy.db, &name).await;
            let (outcome, failed) = failures_in(ROUND.scope(
                round,
                answer_for(
                    &copy.db,
                    models,
                    question,
                    &name,
                    (user_id, &name),
                    earlier.written.clone(),
                    earlier.asked.clone(),
                ),
            ))
            .await;
            copy.drop().await;
            let failed = earlier.failed.iter().cloned().chain(failed).collect();
            let kept = (
                earlier.schema.clone(),
                earlier.written.clone(),
                earlier.failed.clone(),
            );
            (outcome, failed, Some(kept))
        }
        None => {
            let own = crate::db::IsolatedSchema::migrated(url, "memory_bench").await;
            let user_id = new_user(&own.db, &name).await;
            // `MEROPE_MEMORY_BENCH_WRITE_THINKING`: written thinking as asked.
            let (written, write_failed) = failures_in(ROUND.scope(
                round,
                crate::services::analyzer::write_thinking::when_asked(write_for(
                    &own.db,
                    models.0,
                    question,
                    &name,
                    (user_id, &name),
                    Vec::new(),
                )),
            ))
            .await;
            let kept = match keeping {
                true => Some(
                    super::background::restore(url, own.name())
                        .await
                        .keep()
                        .await,
                ),
                false => None,
            };
            let (outcome, failed) = failures_in(ROUND.scope(
                round,
                answer_for(
                    &own.db,
                    models,
                    question,
                    &name,
                    (user_id, &name),
                    written.clone(),
                    None,
                ),
            ))
            .await;
            own.drop().await;
            let failed = write_failed.iter().cloned().chain(failed).collect();
            (
                outcome,
                failed,
                kept.map(|schema| (schema, written, write_failed)),
            )
        }
    }
}

/// The id of the one named `name`, in a copy of what was kept.
async fn user_named(db: &sea_orm::DatabaseConnection, name: &str) -> i32 {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT id FROM users WHERE username = $1",
        [name.into()],
    ))
    .await
    .unwrap()
    .expect("the one the question was written for")
    .try_get::<i32>("", "id")
    .unwrap()
}

/// `runs`, `at_once` at a time; each one asks `stop` first and is not
/// started once it says why (the spend is past the limit, or the account
/// is out of credit). What is under way finishes. Also what was not
/// started, as `id #round: why`.
pub(super) async fn started_while_allowed<Run, Done, Fut>(
    runs: Vec<Run>,
    at_once: usize,
    stop: impl Fn() -> Option<String>,
    run: impl Fn(Run) -> Fut,
) -> (Vec<Done>, Vec<String>)
where
    Run: Named,
    Fut: std::future::Future<Output = Done>,
{
    let not_started = std::sync::Mutex::new(Vec::new());
    let (stop, run, not_started_ref) = (&stop, &run, &not_started);
    let done: Vec<Done> = futures::stream::iter(runs)
        .map(|each| async move {
            if let Some(why) = stop() {
                let line = format!("{} not started: {why}", each.name());
                println!("{line}");
                not_started_ref
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(line);
                return None;
            }
            Some(run(each).await)
        })
        .buffer_unordered(at_once)
        .filter_map(|done| async move { done })
        .collect()
        .await;
    (
        done,
        not_started
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// What a run is called where it is said not to have started.
pub(super) trait Named {
    fn name(&self) -> String;
}

impl Named for (Question, usize) {
    fn name(&self) -> String {
        format!("{} #{}", self.0.question_id, self.1)
    }
}

impl Named for &str {
    fn name(&self) -> String {
        self.to_string()
    }
}

/// With a limit far under what one question spends, the first question
/// runs and the next ones are not started; what was under way finishes.
#[tokio::test]
async fn past_the_spend_limit_the_next_question_is_not_started() {
    let spent = std::sync::Mutex::new(0.0_f64);
    let limit = 0.01;
    let over = || {
        let spent = *spent.lock().unwrap();
        (spent > limit).then(|| format!("charged ${spent:.4} past the limit ${limit}"))
    };
    let (done, not_started) = started_while_allowed(vec!["a", "b", "c"], 1, over, |name| {
        let spent = &spent;
        async move {
            *spent.lock().unwrap() += 0.2;
            name
        }
    })
    .await;
    assert_eq!(done, ["a"]);
    assert_eq!(not_started.len(), 2);
    assert!(not_started[0].starts_with("b not started: charged $0.2000 past the limit"));

    // Two at once: both start before either has spent, and both finish.
    *spent.lock().unwrap() = 0.0;
    let (mut done, not_started) =
        started_while_allowed(vec!["a", "b", "c", "d"], 2, over, |name| {
            let spent = &spent;
            async move {
                tokio::task::yield_now().await;
                *spent.lock().unwrap() += 0.2;
                name
            }
        })
        .await;
    done.sort();
    assert_eq!(done, ["a", "b"]);
    assert_eq!(not_started.len(), 2);

    // No limit: everything runs.
    let (done, not_started) =
        started_while_allowed(vec!["a", "b"], 1, || None, |name| async move { name }).await;
    assert_eq!((done.len(), not_started.len()), (2, 0));
}

/// The site's models as the bench uses them: the judgment model, and her
/// voice thinking little, as production asks; embedding as asked.
pub(super) async fn the_models() -> (
    crate::services::analyzer::AiAnalyzer,
    crate::services::analyzer::AiAnalyzer,
) {
    let config_db = super::super::super::semantic_eval::load_configured_lite().await;
    config_db.close().await.ok();
    lite_as_asked(
        &mut *crate::GLOBAL_DYNAMIC_CONFIG.write().await,
        std::env::var("MEROPE_MEMORY_BENCH_LITE").ok().as_deref(),
    );
    embedding_as_asked().await;
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    .expect("judgment model");
    let lite = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    .expect("Lite model")
    .with_light_thinking();
    // First in its log: which models answer and judge this run.
    println!("bench models: {}", lite_in_use().await);
    if std::env::var_os("MEROPE_BENCH_SPEND_LIMIT").is_some() {
        for model in [&judge, &lite] {
            if !model.cost_known() {
                panic!(
                    "not run: a spend limit is set, and what {} costs is not known \
                     (its source says no cost and it has no list price)",
                    model.model_name()
                );
            }
        }
    }
    if let Some(why) = judged_unlike_asked(
        &*crate::GLOBAL_DYNAMIC_CONFIG.read().await,
        std::env::var("MEROPE_MEMORY_BENCH_JUDGED_BY")
            .ok()
            .as_deref(),
    ) {
        panic!("not run: {why}");
    }
    (judge, lite)
}

/// Why a run would not be judged as meant, if so. The site's judgment
/// model is used as chat unless its source said in time that it answers
/// decisions (`learn_judge_model_kind`, two seconds): unlearned, a
/// decisions model would be sent every judgment and fail them all. With
/// `expected` (`MEROPE_MEMORY_BENCH_JUDGED_BY=<model>`), judged by another
/// model is not run either.
fn judged_unlike_asked(
    config: &crate::config::DynamicConfig,
    expected: Option<&str>,
) -> Option<String> {
    let configured = config.aux_judge_model.trim();
    if !configured.is_empty()
        && !configured.starts_with('~')
        && !crate::config::is_decisions_model(configured)
        && crate::config::model_kind(configured).is_none()
    {
        return Some(format!(
            "whether the judgment model {configured} answers decisions was not learned; \
             judged by it as chat every judgment may fail"
        ));
    }
    let judged = config
        .resolve_lite_judge_ai_config()
        .map(|judge| judge.model);
    match expected
        .map(str::trim)
        .filter(|expected| !expected.is_empty())
    {
        Some(expected) if judged.as_deref() != Some(expected) => {
            Some(format!("judged by {judged:?}, not {expected} as asked"))
        }
        _ => None,
    }
}

/// A run is judged by the model asked for, and not at all while the site's
/// judgment model is of a kind not yet learned.
#[test]
fn a_run_judged_otherwise_than_asked_is_not_run() {
    let mut config = crate::config::DynamicConfig {
        lite_ai_model: "anthropic/claude-haiku-5.5".to_string(),
        aux_judge_model: "acme/bench-kind-unknown-1".to_string(),
        ..crate::config::DynamicConfig::default()
    };
    assert!(
        judged_unlike_asked(&config, None).is_some(),
        "a model whose kind its source never said"
    );
    // Its source said it answers decisions: text judgments stay on Lite.
    crate::config::remember_model_kind("acme/bench-kind-unknown-1", true);
    assert_eq!(judged_unlike_asked(&config, None), None);
    assert_eq!(
        judged_unlike_asked(&config, Some("anthropic/claude-haiku-5.5")),
        None
    );
    config.aux_judge_model = "acme/bench-chat-1".to_string();
    crate::config::remember_model_kind("acme/bench-chat-1", false);
    assert!(
        judged_unlike_asked(&config, Some("anthropic/claude-haiku-5.5")).is_some(),
        "a chat judgment model judges itself, not the one asked for"
    );
    config.aux_judge_model = String::new();
    assert_eq!(
        judged_unlike_asked(&config, Some(" anthropic/claude-haiku-5.5 ")),
        None,
        "none set: Lite judges"
    );
}

/// `MEROPE_MEMORY_BENCH_LITE=<source>:<model>` (say
/// `openrouter:anthropic/claude-haiku-5.5`): Lite for this run only, on that
/// source, whatever the site has set; unset, the site's. A run begun on one
/// model goes on on it after the site moves to another, and the site's own
/// settings are never touched.
pub(super) fn lite_as_asked(config: &mut crate::config::DynamicConfig, asked: Option<&str>) {
    let Some(asked) = asked.map(str::trim).filter(|asked| !asked.is_empty()) else {
        return;
    };
    let (source, model) = asked
        .split_once(':')
        .map(|(source, model)| (source.trim(), model.trim()))
        .filter(|(source, model)| !source.is_empty() && !model.is_empty())
        .expect("MEROPE_MEMORY_BENCH_LITE is <source>:<model>");
    config.lite_ai_source = source.to_string();
    config.lite_ai_model = model.to_string();
}

/// Which Lite the run spoke with, and judged by, for its report.
pub(super) async fn lite_in_use() -> Value {
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    let judged = config
        .resolve_lite_judge_ai_config()
        .map(|judge| judge.model);
    json!({
        "source": config.tier_source(crate::config::ModelTier::Lite),
        "model": config.resolve_strict_lite_ai_config().map(|lite| lite.model),
        "judgedBy": judged,
    })
}

/// `MEROPE_MEMORY_BENCH_TIMES` (default 1): how many times each question
/// is run, each time by someone new, written down again from the start.
pub(super) fn times() -> usize {
    std::env::var("MEROPE_MEMORY_BENCH_TIMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|times| *times > 0)
        .unwrap_or(1)
}

#[test]
fn a_run_asked_for_another_lite_speaks_and_judges_with_it() {
    let mut config = crate::config::DynamicConfig::default();
    config.lite_ai_source = "openai".into();
    config.lite_ai_model = "gpt-6-luna".into();
    config.aux_judge_model = String::new();
    let site = |config: &crate::config::DynamicConfig| {
        config.resolve_strict_lite_ai_config().unwrap().model
    };
    lite_as_asked(&mut config, None);
    lite_as_asked(&mut config, Some("  "));
    assert_eq!(site(&config), "gpt-6-luna");

    lite_as_asked(
        &mut config,
        Some(" openrouter : anthropic/claude-haiku-5.5 "),
    );
    assert_eq!(
        config.tier_source(crate::config::ModelTier::Lite),
        "openrouter"
    );
    let lite = config.resolve_strict_lite_ai_config().unwrap();
    assert_eq!(lite.model, "anthropic/claude-haiku-5.5");
    assert_eq!(lite.base_url, "https://openrouter.ai/api/v1");
    assert_eq!(
        config.resolve_lite_judge_ai_config().unwrap().model,
        "anthropic/claude-haiku-5.5"
    );

    // Only the first colon parts source from model.
    lite_as_asked(&mut config, Some("openrouter:vendor/model:free"));
    assert_eq!(site(&config), "vendor/model:free");
}

#[test]
#[should_panic(expected = "<source>:<model>")]
fn a_lite_asked_for_without_its_source_is_refused() {
    lite_as_asked(
        &mut crate::config::DynamicConfig::default(),
        Some("anthropic/claude-haiku-5.5"),
    );
}
