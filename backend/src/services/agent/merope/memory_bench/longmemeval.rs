//! Her memory on LongMemEval, question by question.

use super::*;

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
    let runs: Vec<(Question, usize)> = (0..times)
        .flat_map(|round| {
            questions
                .iter()
                .cloned()
                .map(move |question| (question, round))
        })
        .collect();
    let outcomes: Vec<(Question, usize, Outcome)> = futures::stream::iter(runs)
        .map(|(question, round)| {
            let (db, judge, lite, reader) = (db.clone(), &judge, &lite, reader.as_ref());
            let report_path = &report_path;
            async move {
                let (mut outcome, failed) = crate::services::ai_cost_ledger::spent::failures_in(
                    ROUND.scope(round, run_one(&db, judge, lite, reader, &question, round)),
                )
                .await;
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
                keep_partial(report_path, &partial_row("", &question, round, &outcome));
                (question, round, outcome)
            }
        })
        .buffer_unordered(at_once())
        .collect()
        .await;
    isolated.drop().await;
    let mut report = report(&questions, times, &outcomes);
    report["spent"] = crate::services::ai_cost_ledger::spent::take();
    report["lite"] = lite_in_use().await;
    println!("{}", report["scores"]);
    std::fs::write(&report_path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
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
    (judge, lite)
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

/// Each way of recalling scored by its run: right once on average, and
/// pass^n, the questions right every time they were run.
pub(super) fn report(
    questions: &[Question],
    times: usize,
    outcomes: &[(Question, usize, Outcome)],
) -> Value {
    let arms: [(&str, fn(&Outcome) -> Option<&Answered>); 5] = [
        ("plain", |outcome| outcome.plain.as_ref()),
        ("cued", |outcome| outcome.cued.as_ref()),
        ("wide", |outcome| Some(&outcome.wide)),
        ("wider", |outcome| outcome.wider.as_ref()),
        ("read", |outcome| outcome.read.as_ref()),
    ];
    let of = |id: &str| -> Vec<&Outcome> {
        let mut runs: Vec<(usize, &Outcome)> = outcomes
            .iter()
            .filter(|(question, _, _)| question.question_id == id)
            .map(|(_, round, outcome)| (*round, outcome))
            .collect();
        runs.sort_by_key(|(round, _)| *round);
        runs.into_iter().map(|(_, outcome)| outcome).collect()
    };
    let kind_of = |question: &Question| {
        if question.question_id.ends_with("_abs") {
            "abstention".to_string()
        } else if question.question_id.starts_with("zh-long-") {
            format!("long/{}", question.question_type)
        } else if let Some(category) = &question.category {
            format!("{}/{category}", question.question_type)
        } else {
            question.question_type.clone()
        }
    };
    let mut scores = serde_json::Map::new();
    for (arm, answered) in arms {
        if outcomes
            .iter()
            .all(|(_, _, outcome)| answered(outcome).is_none())
        {
            continue;
        }
        let right_of =
            |outcome: &Outcome| answered(outcome).is_some_and(|a| a.correct == Some(true));
        let mut by_kind: BTreeMap<String, [usize; 8]> = BTreeMap::new();
        let mut per_round = vec![0usize; times];
        let (mut right, mut every) = (0, 0);
        for question in questions {
            let runs = of(&question.question_id);
            let rights: Vec<bool> = runs.iter().map(|outcome| right_of(outcome)).collect();
            let slot = by_kind.entry(kind_of(question)).or_default();
            slot[0] += 1;
            slot[1] += rights.iter().filter(|right| **right).count();
            slot[2] += runs.len();
            let all = rights.len() == times && rights.iter().all(|right| *right);
            slot[3] += usize::from(all);
            slot[4] += runs.iter().filter(|outcome| outcome.written).count();
            slot[5] += runs
                .iter()
                .filter(|outcome| answered(outcome).is_some_and(|a| a.retrieved))
                .count();
            // Checked line by line: every fact it needs in what she recalled.
            if !question.needs.is_empty() {
                slot[6] += runs.len();
                slot[7] += runs
                    .iter()
                    .filter(|outcome| {
                        answered(outcome)
                            .is_some_and(|a| question.met_in(&a.recalled).iter().all(|met| *met))
                    })
                    .count();
            }
            every += usize::from(all);
            right += rights.iter().filter(|right| **right).count();
            for (round, outcome) in runs.iter().enumerate() {
                per_round[round] += usize::from(right_of(outcome));
            }
        }
        // Pairs: right only when both sides are, in the same round.
        let mut pairs: BTreeMap<&str, Vec<&Question>> = BTreeMap::new();
        for question in questions {
            if let Some(pair) = &question.pair {
                pairs.entry(pair.as_str()).or_default().push(question);
            }
        }
        let (mut pair_right, mut pair_every) = (0, 0);
        for sides in pairs.values() {
            let rounds: Vec<bool> = (0..times)
                .map(|round| {
                    sides.iter().all(|side| {
                        of(&side.question_id)
                            .get(round)
                            .is_some_and(|outcome| right_of(outcome))
                    })
                })
                .collect();
            pair_right += rounds.iter().filter(|right| **right).count();
            pair_every += usize::from(rounds.iter().all(|right| *right));
        }
        // How much she answered on, to compare ways of recalling at the
        // same length.
        let shown: Vec<&Answered> = outcomes
            .iter()
            .filter_map(|(_, _, outcome)| answered(outcome))
            .collect();
        let per_answer = |count: fn(&Answered) -> usize| {
            shown.iter().map(|answered| count(answered)).sum::<usize>() as f64
                / shown.len().max(1) as f64
        };
        let kinds: serde_json::Map<String, Value> = by_kind
            .into_iter()
            .map(
                |(kind, [n, right, run, every, written, retrieved, needing, recalled])| {
                    (
                        kind,
                        json!({"questions": n, "runs": run, "written": written,
                            "retrieved": retrieved, "right": right, "every": every,
                            "needing": needing, "recalledNeeded": recalled}),
                    )
                },
            )
            .collect();
        scores.insert(
            arm.to_string(),
            json!({
                "accuracy": right as f64 / (questions.len() * times).max(1) as f64,
                "perRound": per_round,
                format!("pass^{times}"): every,
                "questions": questions.len(),
                "byKind": kinds,
                "pairs": (!pairs.is_empty()).then(|| json!({"pairs": pairs.len(),
                    "right": format!("{pair_right}/{}", pairs.len() * times),
                    format!("pass^{times}"): pair_every})),
                "recalledLines": per_answer(|answered| answered.recalled.len()),
                "recalledChars": per_answer(recalled_chars),
            }),
        );
    }
    // Noise: the same everyday questions with what she keeps of them and
    // with nothing, as production answers (the wide way).
    let noise: Vec<&Outcome> = outcomes
        .iter()
        .filter(|(question, _, _)| question.question_type == "noise")
        .map(|(_, _, outcome)| outcome)
        .collect();
    if !noise.is_empty() {
        let count = |pick: fn(&Outcome) -> Option<bool>| {
            noise
                .iter()
                .filter(|outcome| pick(outcome) == Some(true))
                .count()
        };
        scores.insert(
            "noise".into(),
            json!({
                "runs": noise.len(),
                "rightWithMemories": count(|outcome| outcome.wide.correct),
                "rightWithout": count(|outcome| outcome.bare.as_ref().and_then(|bare| bare.correct)),
                "draggedWithMemories": count(|outcome| outcome.wide.dragged),
                "draggedWithout": count(|outcome| outcome.bare.as_ref().and_then(|bare| bare.dragged)),
            }),
        );
    }
    // Private things asked in a group: would recall in private have found
    // them (so keeping them out was the audience's doing).
    let private: Vec<bool> = outcomes
        .iter()
        .filter_map(|(_, _, outcome)| outcome.in_private)
        .collect();
    if !private.is_empty() {
        scores.insert(
            "privateFoundInPrivate".into(),
            json!(format!(
                "{}/{}",
                private.iter().filter(|found| **found).count(),
                private.len()
            )),
        );
    }
    let answered = |answered: &Answered| {
        json!({"retrieved":answered.retrieved,"correct":answered.correct,
            "dragged":answered.dragged,"why":answered.why,"recalled":answered.recalled,
            "response":answered.answer,
            "cues":answered.cues,"through":answered.through})
    };
    let rows: Vec<Value> = outcomes
        .iter()
        .map(|(question, round, outcome)| {
            json!({"id":question.question_id,"type":question.question_type,
            "category":question.category,"pair":question.pair,"round":round,
            "question":question.question,"answer":question.answer,
            "written":outcome.written,"kept":outcome.kept,
            "plain":outcome.plain.as_ref().map(answered),"cued":outcome.cued.as_ref().map(answered),
            "wide":answered(&outcome.wide),"wider":outcome.wider.as_ref().map(answered),
            "read":outcome.read.as_ref().map(answered),
            "bare":outcome.bare.as_ref().map(answered),"inPrivate":outcome.in_private,
            "memories":outcome.memories,"concepts":outcome.concepts,
            "nodes":outcome.nodes,"failed":outcome.failed,"void":outcome.void()})
        })
        .collect();
    let voided = outcomes
        .iter()
        .filter(|(_, _, outcome)| outcome.void())
        .count();
    json!({"times": times, "wider": wider(), "widerArm": wider_arm(), "scores": scores,
        "voided": voided, "rows": rows})
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

/// How long what she answered on was: the lines recalled, in characters.
pub(super) fn recalled_chars(answered: &Answered) -> usize {
    answered
        .recalled
        .iter()
        .map(|line| line.chars().count())
        .sum()
}

/// One finished run of a question as the partial report keeps it: enough
/// to score it and to tell its way (`arm`) apart, and to start again past
/// it.
pub(super) fn partial_row(
    arm: &str,
    question: &Question,
    round: usize,
    outcome: &Outcome,
) -> Value {
    json!({"arm": arm, "id": question.question_id, "type": question.question_type,
        "round": round, "void": outcome.void(), "written": outcome.written,
        "failed": outcome.failed,
        "answering": outcome.answering,
        "needs": question.needs.iter().enumerate().map(|(fact, words)| json!({
            "any": words,
            "written": outcome.answering.iter().any(|row| question.holds(fact, row)),
            "wide": question.met_in(&outcome.wide.recalled)[fact],
            "wider": outcome.wider.as_ref().map(|wider| question.met_in(&wider.recalled)[fact]),
            "rank": outcome.need_ranks.get(fact).copied().flatten(),
        })).collect::<Vec<_>>(),
        "wide": {"correct": outcome.wide.correct, "retrieved": outcome.wide.retrieved,
            "response": outcome.wide.answer, "why": outcome.wide.why,
            "recalled": outcome.wide.recalled,
            "recalledChars": recalled_chars(&outcome.wide)},
        "wider": outcome.wider.as_ref().map(|wider| json!({"correct": wider.correct,
            "retrieved": wider.retrieved, "recalled": wider.recalled,
            "recalledChars": recalled_chars(wider)}))})
}

/// `row` added to `{report}.partial.jsonl` as soon as it is done.
pub(super) fn keep_partial(report_path: &str, row: &Value) {
    use std::io::Write;
    if let Ok(mut done) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{report_path}.partial.jsonl"))
    {
        let _ = writeln!(done, "{row}");
    }
}

/// What an earlier run under the same report already finished, as
/// (way, question, round): started again, a run goes on past them instead
/// of paying for them twice. A run made void is finished too: it is kept
/// as void, not run again.
pub(super) fn finished(report_path: &str) -> std::collections::HashSet<(String, String, usize)> {
    std::fs::read_to_string(format!("{report_path}.partial.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|row| {
            Some((
                row["arm"].as_str().unwrap_or("").to_string(),
                row["id"].as_str()?.to_string(),
                row["round"].as_u64()? as usize,
            ))
        })
        .collect()
}
