//! Answers a run already gave, graded again by the rules as they stand
//! now: when how an answer is graded changes, what was answered before is
//! held to the new rule without being asked again.
//!
//! `MEROPE_MEMORY_BENCH_REGRADE=<a run's .partial.jsonl>` with
//! `MEROPE_MEMORY_BENCH=<its questions>` (and the judge as a run names it):
//! each row's wide answer is graded once more and written to
//! `<that file>.regraded.jsonl` as `{id, type, was, now, why}`. Rows made
//! void or never answered are left out. Only the grading is paid for.

use super::*;

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
async fn answers_are_graded_again() {
    use std::io::Write;
    let path = std::env::var("MEROPE_MEMORY_BENCH_REGRADE").expect("a run's .partial.jsonl");
    let questions: Vec<Question> = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MEROPE_MEMORY_BENCH").expect("questions")).unwrap(),
    )
    .unwrap();
    let (judge, _) = super::longmemeval::the_models().await;
    let mut out = std::fs::File::create(format!("{path}.regraded.jsonl")).unwrap();
    let (mut was_right, mut now_right, mut graded_rows) = (0, 0, 0);
    for row in std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
    {
        let response = row["wide"]["response"].as_str().unwrap_or("").trim();
        let Some(question) = questions
            .iter()
            .find(|question| Some(question.question_id.as_str()) == row["id"].as_str())
        else {
            continue;
        };
        if row["void"] == true || response.is_empty() {
            continue;
        }
        let asked = json!({"question": question.question, "correctAnswer": question.answer,
            "response": response});
        let graded = judge
            .analyze_json(
                &judge_system(question),
                &asked.to_string(),
                "memory_bench_grade",
                Some(&grade_schema(question)),
            )
            .await
            .inspect_err(|error| eprintln!("bench call failed: grading: {error:#}"))
            .ok()
            .and_then(|raw| {
                let json = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim());
                serde_json::from_str::<Value>(json.as_deref().unwrap_or(raw.trim())).ok()
            });
        let was = row["wide"]["correct"].as_bool();
        let now = graded.as_ref().and_then(|value| value["correct"].as_bool());
        graded_rows += 1;
        was_right += usize::from(was == Some(true));
        now_right += usize::from(now == Some(true));
        println!("regraded {} was={was:?} now={now:?}", question.question_id);
        let _ = writeln!(
            out,
            "{}",
            json!({"id": question.question_id, "type": question.question_type,
                "was": was, "now": now, "why": graded.as_ref().map(|value| value["why"].clone())})
        );
    }
    println!("regraded {graded_rows}: right before {was_right}, right now {now_right}");
}
