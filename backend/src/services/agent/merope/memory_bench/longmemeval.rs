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
    let outcomes: Vec<(Question, Outcome)> = futures::stream::iter(questions)
        .map(|question| {
            let (db, judge, lite) = (db.clone(), &judge, &lite);
            async move {
                let outcome = run_one(&db, judge, lite, &question).await;
                println!(
                    "{} {} written={} plain={:?} cued={:?}",
                    question.question_type,
                    question.question_id,
                    outcome.written,
                    outcome.plain.correct,
                    outcome.cued.correct
                );
                (question, outcome)
            }
        })
        .buffer_unordered(at_once())
        .collect()
        .await;
    isolated.drop().await;
    // Per type: questions, written, then retrieved and correct for recall
    // from their words alone and with cues.
    let mut summary: BTreeMap<String, [usize; 8]> = BTreeMap::new();
    let mut rows = Vec::new();
    for (question, outcome) in &outcomes {
        let kind = if question.question_id.ends_with("_abs") {
            "abstention".to_string()
        } else {
            question.question_type.clone()
        };
        let slot = summary.entry(kind).or_default();
        slot[0] += 1;
        slot[1] += usize::from(outcome.written);
        slot[2] += usize::from(outcome.plain.retrieved);
        slot[3] += usize::from(outcome.plain.correct == Some(true));
        slot[4] += usize::from(outcome.cued.retrieved);
        slot[5] += usize::from(outcome.cued.correct == Some(true));
        slot[6] += usize::from(outcome.wide.retrieved);
        slot[7] += usize::from(outcome.wide.correct == Some(true));
        let answered = |answered: &Answered| {
            json!({"retrieved":answered.retrieved,"correct":answered.correct,
                "recalled":answered.recalled,"response":answered.answer})
        };
        rows.push(
            json!({"id":question.question_id,"type":question.question_type,
            "question":question.question,"answer":question.answer,
            "written":outcome.written,"kept":outcome.kept,
            "plain":answered(&outcome.plain),"cued":answered(&outcome.cued),
            "wide":answered(&outcome.wide),
            "memories":outcome.memories,"concepts":outcome.concepts}),
        );
    }
    let total = outcomes.len();
    let count = |cued: bool| {
        outcomes
            .iter()
            .filter(|(_, outcome)| {
                (if cued { &outcome.cued } else { &outcome.plain }).correct == Some(true)
            })
            .count()
    };
    let (plain, cued) = (count(false), count(true));
    let summary: Value = summary
        .into_iter()
        .map(
            |(
                kind,
                [
                    n,
                    written,
                    plain_found,
                    plain_right,
                    cued_found,
                    cued_right,
                    wide_found,
                    wide_right,
                ],
            )| {
                (
                    kind,
                    json!({"questions":n,"written":written,
                    "plain":{"retrieved":plain_found,"correct":plain_right},
                    "cued":{"retrieved":cued_found,"correct":cued_right},
                    "wide":{"retrieved":wide_found,"correct":wide_right}}),
                )
            },
        )
        .collect::<serde_json::Map<_, _>>()
        .into();
    println!("summary {summary} plain {plain}/{total} cued {cued}/{total}");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(
            &json!({"summary":summary,"plain":plain,"cued":cued,"total":total,"rows":rows}),
        )
        .unwrap(),
    )
    .unwrap();
}
