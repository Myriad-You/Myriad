//! Why a run got a question wrong, found by asking it again with the thing
//! it may have lacked put in front of her, not by reading the answer: a
//! wrong answer is memory's only when the same question, with what answers
//! it in front of her and nothing else, comes out right.
//!
//! Each wrong row is asked up to twice more, alone:
//! - with their own words that answer it (the `has_answer` turns, dated)
//!   and nothing else. Still wrong: `answering`. Nothing she could have
//!   kept would have made it right, so it is not counted against memory.
//! - with only what she kept that answers it (the row's `answering`).
//!   Nothing kept, or still wrong: `writing`. Right: what she kept is
//!   enough, so the run lost it later: `recalling` when it was not in
//!   front of her in the run, `presenting` when it was, among the rest.
//!
//! A question another round of the same run got right is marked `unstable`
//! beside its verdict: sometimes right is not the same failure as never.
//!
//! `MEROPE_MEMORY_BENCH_ATTRIBUTE=<a run's .partial.jsonl>` with
//! `MEROPE_MEMORY_BENCH=<its questions>` (and the models as the run names
//! them) writes `<that file>.attributed.jsonl`, one row for each wrong one.
//! Rows made void, never answered, or of a question that must not be
//! answered (`_abs`, `_forget`, `_private`, noise) are left out: there is
//! nothing to put in front of her for those. Only wrong rows are paid for.

use super::*;

/// Where a wrong answer came from. `words`: right with their own words in
/// front of her (`None` when the set marks none, or no reply came).
/// `kept`: right with only what she kept that answers it (`None` when she
/// kept nothing that does). `in_front`: whether what it needs was in front
/// of her in the run that got it wrong.
fn verdict(words: Option<bool>, kept: Option<bool>, in_front: bool) -> &'static str {
    match (words, kept) {
        (Some(false), _) => "answering",
        (_, Some(true)) if in_front => "presenting",
        (_, Some(true)) => "recalling",
        (Some(true), _) => "writing",
        // No words of theirs to hold it against: what she kept does not
        // answer it, and whether anything would have is not known.
        (None, _) => "unclear",
    }
}

/// Their own words that answer `question`, each with the day it was said,
/// as lines she would have kept had she kept them whole.
fn their_words(question: &Question) -> Vec<String> {
    question
        .haystack_sessions
        .iter()
        .zip(&question.haystack_dates)
        .flat_map(|(session, date)| {
            session
                .iter()
                .filter(|turn| turn.has_answer)
                .map(move |turn| format!("({date}) {} said: {}", turn.role, turn.content))
        })
        .collect()
}

/// Whether what `row`'s question needs was in front of her when it was
/// answered: every needed fact line by line where the question names them,
/// else any line she kept that answers it.
fn in_front(row: &Value) -> bool {
    match row["needs"].as_array().filter(|needs| !needs.is_empty()) {
        Some(needs) => needs.iter().all(|need| need["shown"] == true),
        None => row["wide"]["retrieved"] == true,
    }
}

/// Whether `question`, asked with `lines` alone in front of her, is
/// answered right; `None` when no reply or no grade came.
async fn right_with(
    judge: &crate::services::analyzer::AiAnalyzer,
    lite: &crate::services::analyzer::AiAnalyzer,
    question: &Question,
    lines: &[String],
) -> Option<bool> {
    let venue = question.question_venue.as_deref().unwrap_or("private");
    let system = answer_system(question, &audience_at(venue, 1, "attributed"), lines);
    let input = json!({"today": question.question_date, "scrollingBack": null,
        "goingThroughAll": null, "question": question.question})
    .to_string();
    let mut answer = String::new();
    for _ in 0..3 {
        answer = lite
            .analyze_with_system(&system, &input)
            .await
            .inspect_err(|error| eprintln!("bench call failed: answering: {error:#}"))
            .unwrap_or_default();
        if !answer.trim().is_empty() {
            break;
        }
    }
    if answer.trim().is_empty() {
        return None;
    }
    let asked = json!({"question": question.question, "correctAnswer": question.answer,
        "response": answer});
    let raw = judge
        .analyze_json(
            &judge_system(question),
            &asked.to_string(),
            "memory_bench_grade",
            Some(&grade_schema(question)),
        )
        .await
        .inspect_err(|error| eprintln!("bench call failed: grading: {error:#}"))
        .ok()?;
    let json = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim());
    serde_json::from_str::<Value>(json.as_deref().unwrap_or(raw.trim())).ok()?["correct"].as_bool()
}

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
async fn wrong_answers_are_attributed() {
    use std::io::Write;
    let path = std::env::var("MEROPE_MEMORY_BENCH_ATTRIBUTE").expect("a run's .partial.jsonl");
    let questions: Vec<Question> = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MEROPE_MEMORY_BENCH").expect("questions")).unwrap(),
    )
    .unwrap();
    let rows: Vec<Value> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let right_elsewhere = |row: &Value| {
        rows.iter().any(|other| {
            other["arm"] == row["arm"]
                && other["id"] == row["id"]
                && other["wide"]["correct"] == true
        })
    };
    let (judge, lite) = super::longmemeval::the_models().await;
    let mut out = std::fs::File::create(format!("{path}.attributed.jsonl")).unwrap();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for row in &rows {
        let Some(question) = questions
            .iter()
            .find(|question| Some(question.question_id.as_str()) == row["id"].as_str())
        else {
            continue;
        };
        if row["void"] == true
            || row["wide"]["correct"] != false
            || question.question_type == "noise"
            || matches!(rule_key(question), "abstention" | "forget" | "private")
        {
            continue;
        }
        let said = their_words(question);
        let words = match said.is_empty() {
            true => None,
            false => right_with(&judge, &lite, question, &said).await,
        };
        let answering: Vec<String> = row["answering"]
            .as_array()
            .map(|lines| {
                lines
                    .iter()
                    .filter_map(|line| line.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        // Wrong even with their words: what she kept is not asked about.
        let kept = match words == Some(false) || answering.is_empty() {
            true => None,
            false => right_with(&judge, &lite, question, &answering).await,
        };
        let unstable = right_elsewhere(row);
        let verdict = verdict(words, kept, in_front(row));
        *counts.entry(verdict).or_default() += 1;
        println!(
            "attributed {} #{} {verdict}{} words={words:?} kept={kept:?}",
            question.question_id,
            row["round"],
            if unstable { " (unstable)" } else { "" },
        );
        let _ = writeln!(
            out,
            "{}",
            json!({"arm": row["arm"], "id": question.question_id,
                "type": question.question_type, "round": row["round"],
                "verdict": verdict, "unstable": unstable, "rightWithTheirWords": words,
                "rightWithWhatSheKept": kept, "keptLines": answering.len(),
                "inFront": in_front(row)})
        );
    }
    println!("attributed: {counts:?}");
}

#[test]
fn a_wrong_answer_is_memorys_only_when_what_answers_it_would_have_made_it_right() {
    // Wrong even with their own words: nothing memory could have done.
    assert_eq!(verdict(Some(false), None, true), "answering");
    assert_eq!(verdict(Some(false), None, false), "answering");
    // Right with their words, but she kept nothing, or not enough.
    assert_eq!(verdict(Some(true), None, false), "writing");
    assert_eq!(verdict(Some(true), Some(false), true), "writing");
    // What she kept is enough: lost on the way back, or among the rest.
    assert_eq!(verdict(Some(true), Some(true), false), "recalling");
    assert_eq!(verdict(Some(true), Some(true), true), "presenting");
    assert_eq!(verdict(None, Some(true), false), "recalling");
    // No words of theirs to hold it against, and what she kept fails.
    assert_eq!(verdict(None, Some(false), true), "unclear");
    assert_eq!(verdict(None, None, false), "unclear");
}

#[test]
fn what_is_put_in_front_of_her_is_what_they_said_that_answers_it() {
    let question: Question = serde_json::from_value(json!({
        "question_id": "q", "question_type": "single-session-user",
        "question": "我的猫叫什么？", "answer": "团子",
        "question_date": "2024/05/02 (Thu) 10:00",
        "haystack_dates": ["2024/04/01 (Mon) 09:00", "2024/04/20 (Sat) 21:00"],
        "haystack_sessions": [
            [{"role": "user", "content": "今天好累"},
             {"role": "assistant", "content": "辛苦了"}],
            [{"role": "user", "content": "我家猫叫团子", "has_answer": true}]],
    }))
    .unwrap();
    assert_eq!(
        their_words(&question),
        ["(2024/04/20 (Sat) 21:00) user said: 我家猫叫团子"]
    );
    // Named needs are checked one by one; without them, any answering line.
    assert!(in_front(
        &json!({"needs": [{"shown": true}, {"shown": true}]})
    ));
    assert!(!in_front(
        &json!({"needs": [{"shown": true}, {"shown": false}],
        "wide": {"retrieved": true}})
    ));
    assert!(in_front(&json!({"needs": [], "wide": {"retrieved": true}})));
    assert!(!in_front(&json!({"wide": {"retrieved": false}})));
}
