//! Her memory on LongMemEval, question by question.

use super::*;

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_memory_on_longmemeval() {
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
    let config_db = super::super::super::semantic_eval::load_configured_lite().await;
    config_db.close().await.ok();
    embedding_as_asked().await;
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    .expect("judgment model");
    // Her voice thinks little, as production asks.
    let lite = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    .expect("Lite model")
    .with_light_thinking();
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
            let (db, judge, lite) = (db.clone(), &judge, &lite);
            async move {
                let outcome = run_one(&db, judge, lite, &question, round).await;
                println!(
                    "{} {} #{round} written={} plain={:?} cued={:?} wide={:?}",
                    question.question_type,
                    question.question_id,
                    outcome.written,
                    outcome.plain.correct,
                    outcome.cued.correct,
                    outcome.wide.correct
                );
                (question, round, outcome)
            }
        })
        .buffer_unordered(at_once())
        .collect()
        .await;
    isolated.drop().await;
    let report = report(&questions, times, &outcomes);
    println!("{}", report["scores"]);
    std::fs::write(&report_path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}

/// `MEROPE_MEMORY_BENCH_TIMES` (default 1): how many times each question
/// is run, each time by someone new, written down again from the start.
fn times() -> usize {
    std::env::var("MEROPE_MEMORY_BENCH_TIMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|times| *times > 0)
        .unwrap_or(1)
}

/// Each way of recalling scored by its run: right once on average, and
/// pass^n, the questions right every time they were run.
fn report(questions: &[Question], times: usize, outcomes: &[(Question, usize, Outcome)]) -> Value {
    let arms: [(&str, fn(&Outcome) -> &Answered); 3] = [
        ("plain", |outcome| &outcome.plain),
        ("cued", |outcome| &outcome.cued),
        ("wide", |outcome| &outcome.wide),
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
        } else {
            question.question_type.clone()
        }
    };
    let mut scores = serde_json::Map::new();
    for (arm, answered) in arms {
        let mut by_kind: BTreeMap<String, [usize; 6]> = BTreeMap::new();
        let mut per_round = vec![0usize; times];
        let (mut right, mut every) = (0, 0);
        for question in questions {
            let runs = of(&question.question_id);
            let rights: Vec<bool> = runs
                .iter()
                .map(|outcome| answered(outcome).correct == Some(true))
                .collect();
            let slot = by_kind.entry(kind_of(question)).or_default();
            slot[0] += 1;
            slot[1] += rights.iter().filter(|right| **right).count();
            slot[2] += runs.len();
            let all = rights.len() == times && rights.iter().all(|right| *right);
            slot[3] += usize::from(all);
            slot[4] += runs.iter().filter(|outcome| outcome.written).count();
            slot[5] += runs
                .iter()
                .filter(|outcome| answered(outcome).retrieved)
                .count();
            every += usize::from(all);
            right += rights.iter().filter(|right| **right).count();
            for (round, outcome) in runs.iter().enumerate() {
                per_round[round] += usize::from(answered(outcome).correct == Some(true));
            }
        }
        let kinds: serde_json::Map<String, Value> = by_kind
            .into_iter()
            .map(|(kind, [n, right, run, every, written, retrieved])| {
                (
                    kind,
                    json!({"questions": n, "runs": run, "written": written,
                        "retrieved": retrieved, "right": right, "every": every}),
                )
            })
            .collect();
        scores.insert(
            arm.to_string(),
            json!({
                "accuracy": right as f64 / (questions.len() * times).max(1) as f64,
                "perRound": per_round,
                format!("pass^{times}"): every,
                "questions": questions.len(),
                "byKind": kinds,
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
            "dragged":answered.dragged,"recalled":answered.recalled,"response":answered.answer})
    };
    let rows: Vec<Value> = outcomes
        .iter()
        .map(|(question, round, outcome)| {
            json!({"id":question.question_id,"type":question.question_type,"round":round,
            "question":question.question,"answer":question.answer,
            "written":outcome.written,"kept":outcome.kept,
            "plain":answered(&outcome.plain),"cued":answered(&outcome.cued),
            "wide":answered(&outcome.wide),
            "bare":outcome.bare.as_ref().map(answered),"inPrivate":outcome.in_private,
            "memories":outcome.memories,"concepts":outcome.concepts})
        })
        .collect();
    json!({"times": times, "scores": scores, "rows": rows})
}
