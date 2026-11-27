//! A LongMemEval run scored, and kept as it goes: the report of each way
//! of recalling, and the partial report a run starts again past.

use super::*;

/// What one remembering attempt came to (same four buckets as production's
/// `remembering::counts()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum CuesTry {
    Cues,
    Empty,
    Unreadable,
    Unanswered,
}

impl CuesTry {
    fn classify(raw: Option<&str>, parsed: Option<&myriad_merope::remembering::Cues>) -> Self {
        match (raw, parsed) {
            (None, _) => Self::Unanswered,
            (Some(_), None) => Self::Unreadable,
            (Some(_), Some(cues)) if cues.cues.is_empty() => Self::Empty,
            (Some(_), Some(_)) => Self::Cues,
        }
    }
}

async fn ask_cues_attempt<F, Fut>(ask: F) -> (Option<String>, CuesTry)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    let raw = ask().await;
    let parsed = raw.as_deref().and_then(myriad_merope::remembering::parse);
    let outcome = CuesTry::classify(raw.as_deref(), parsed.as_ref());
    let kept = raw.filter(|_| parsed.is_some());
    (kept, outcome)
}

/// Ask the judge once what to look for, matching production's single-attempt
/// `JsonShape::InPrompt` call, and classify the first-attempt outcome.
///
/// Note: bench calls have no 6-second production wall-clock timeout, so
/// `scores.cuesTries.firstAttemptFailedRate` measures a optimistic/lower bound
/// relative to production `remembering::counts()`. Additionally, only the main
/// `longmemeval` (`answer_for`) path records `cuesTries`; `weeks`, `full_size`,
/// and `background` call `ask_cues` / `ask_cues_with` without bucket tracking.
pub(in super::super) async fn ask_cues_once_with(
    judge: &crate::services::analyzer::AiAnalyzer,
    system: &str,
    input: &str,
) -> (Option<String>, CuesTry) {
    ask_cues_attempt(|| async {
        judge
            .analyze_json_as(
                system,
                input,
                myriad_merope::remembering::SCHEMA_NAME,
                Some(&myriad_merope::remembering::schema()),
                crate::services::analyzer::JsonShape::InPrompt,
            )
            .await
            .ok()
    })
    .await
}

pub(in super::super) async fn ask_cues_with(
    judge: &crate::services::analyzer::AiAnalyzer,
    system: &str,
    input: &str,
) -> Option<String> {
    ask_cues_once_with(judge, system, input).await.0
}

pub(in super::super) async fn ask_cues(
    judge: &crate::services::analyzer::AiAnalyzer,
    question: &Question,
) -> Option<String> {
    ask_cues_with(
        judge,
        &myriad_merope::remembering::system(),
        &myriad_merope::remembering::input(&question.question, None, &question.question_date),
    )
    .await
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
        let mut by_kind: BTreeMap<String, [usize; 9]> = BTreeMap::new();
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
                // And in front of her at all, however it came: recalled,
                // standing, or scrolled back to.
                slot[8] += runs
                    .iter()
                    .filter(|outcome| {
                        answered(outcome)
                            .is_some_and(|a| question.met_in(&a.shown()).iter().all(|met| *met))
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
            .map(|(kind, slot)| {
                let [n, right, run, every, written, retrieved, ..] = slot;
                let [.., needing, recalled, shown] = slot;
                (
                    kind,
                    // `writtenAny`, `retrievedAny`: a run counts when any
                    // one answering fact was written or recalled, so they
                    // read high. `recalledNeeded`, `shownNeeded`: of the
                    // `needing` runs, those with every needed fact, checked
                    // line by line.
                    json!({"questions": n, "runs": run, "writtenAny": written,
                            "retrievedAny": retrieved, "right": right, "every": every,
                            "needing": needing, "recalledNeeded": recalled,
                            "shownNeeded": shown}),
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
            "cues":answered.cues,"through":answered.through,
            "scrolled":answered.scrolled,"standing":answered.standing})
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
    let mut cue_counts = [0usize; 4];
    for (_, _, outcome) in outcomes {
        match outcome.cues_try {
            Some(CuesTry::Cues) => cue_counts[0] += 1,
            Some(CuesTry::Empty) => cue_counts[1] += 1,
            Some(CuesTry::Unreadable) => cue_counts[2] += 1,
            Some(CuesTry::Unanswered) => cue_counts[3] += 1,
            None => {}
        }
    }
    let cue_total: usize = cue_counts.iter().sum();
    if cue_total > 0 {
        let failed = cue_counts[2] + cue_counts[3];
        scores.insert(
            "cuesTries".into(),
            json!({
                "total": cue_total,
                "cues": cue_counts[0],
                "empty": cue_counts[1],
                "unreadable": cue_counts[2],
                "unanswered": cue_counts[3],
                "firstAttemptFailedRate": failed as f64 / cue_total as f64,
            }),
        );
    }
    json!({"times": times, "wider": wider(), "widerArm": wider_arm(), "scores": scores,
        "voided": voided, "rows": rows})
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
        "answering": outcome.answering, "asked": outcome.asked,
        "needs": question.needs.iter().enumerate().map(|(fact, words)| json!({
            "any": words,
            "written": outcome.answering.iter().any(|row| question.holds(fact, row)),
            "wide": question.met_in(&outcome.wide.recalled)[fact],
            "shown": question.met_in(&outcome.wide.shown())[fact],
            "wider": outcome.wider.as_ref().map(|wider| question.met_in(&wider.recalled)[fact]),
            "rank": outcome.need_ranks.get(fact).and_then(|ranks| ranks.1),
            "rankShallow": outcome.need_ranks.get(fact).and_then(|ranks| ranks.0),
        })).collect::<Vec<_>>(),
        "wide": {"correct": outcome.wide.correct, "retrieved": outcome.wide.retrieved,
            "response": outcome.wide.answer, "why": outcome.wide.why,
            "recalled": outcome.wide.recalled,
            "recalledChars": recalled_chars(&outcome.wide),
            "scrolled": outcome.wide.scrolled, "standing": outcome.wide.standing},
        "wider": outcome.wider.as_ref().map(|wider| json!({"correct": wider.correct,
            "retrieved": wider.retrieved, "recalled": wider.recalled,
            "recalledChars": recalled_chars(wider)})),
        "raw": outcome.raw.as_ref().map(|raw| json!({"correct": raw.correct,
            "response": raw.answer, "why": raw.why, "scrolled": raw.scrolled,
            "scrolledChars": raw.scrolled.as_ref().map_or(0, |scrolled| scrolled.chars().count())}))})
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_question(id: &str) -> Question {
        Question {
            question_id: id.to_string(),
            question_type: "single-session-user".to_string(),
            question: "What do I drink?".to_string(),
            answer: json!("coffee"),
            question_date: "2026/11/26 (Thu) 12:00".to_string(),
            haystack_dates: Vec::new(),
            haystack_sessions: Vec::new(),
            haystack_venues: Vec::new(),
            question_venue: None,
            question_speaker: None,
            memories: Vec::new(),
            category: None,
            pair: None,
            background_sessions: Vec::new(),
            needs: Vec::new(),
        }
    }

    fn sample_outcome(cues_try: Option<CuesTry>) -> Outcome {
        Outcome {
            written: true,
            plain: None,
            cued: None,
            wide: Answered {
                retrieved: true,
                correct: Some(true),
                dragged: None,
                why: None,
                answer: "coffee".to_string(),
                recalled: Vec::new(),
                cues: None,
                through: None,
                scrolled: None,
                standing: Vec::new(),
            },
            bare: None,
            wider: None,
            raw: None,
            answering: Vec::new(),
            read: None,
            in_private: None,
            memories: Vec::new(),
            concepts: Vec::new(),
            nodes: Vec::new(),
            kept: 0,
            failed: Vec::new(),
            need_ranks: Vec::new(),
            asked: None,
            cues_try,
        }
    }

    #[test]
    fn report_summarizes_cues_tries_and_first_attempt_failure_rate() {
        let q1 = sample_question("q1");
        let q2 = sample_question("q2");
        let q3 = sample_question("q3");
        let q4 = sample_question("q4");
        let report = report(
            &[q1.clone(), q2.clone(), q3.clone(), q4.clone()],
            1,
            &[
                (q1, 0, sample_outcome(Some(CuesTry::Cues))),
                (q2, 0, sample_outcome(Some(CuesTry::Empty))),
                (q3, 0, sample_outcome(Some(CuesTry::Unreadable))),
                (q4, 0, sample_outcome(Some(CuesTry::Unanswered))),
            ],
        );
        let tries = &report["scores"]["cuesTries"];
        assert_eq!(tries["total"], 4);
        assert_eq!(tries["cues"], 1);
        assert_eq!(tries["empty"], 1);
        assert_eq!(tries["unreadable"], 1);
        assert_eq!(tries["unanswered"], 1);
        assert_eq!(tries["firstAttemptFailedRate"], 0.5);
    }

    #[tokio::test]
    async fn bench_cues_attempt_does_not_retry_and_classifies_outcomes() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let (kept, outcome) = super::ask_cues_attempt(|| {
            c.fetch_add(1, Ordering::SeqCst);
            async move { None }
        })
        .await;
        assert!(kept.is_none());
        assert_eq!(outcome, CuesTry::Unanswered);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let (kept, outcome) =
            super::ask_cues_attempt(|| async { Some("not json".to_string()) }).await;
        assert!(kept.is_none());
        assert_eq!(outcome, CuesTry::Unreadable);

        let (kept, outcome) = super::ask_cues_attempt(|| async {
            Some(r#"{"cues":[],"thorough":false,"lookBack":false,"aboutYou":false}"#.to_string())
        })
        .await;
        assert!(kept.is_some());
        assert_eq!(outcome, CuesTry::Empty);

        let (kept, outcome) = super::ask_cues_attempt(|| async {
            Some(
                r#"{"cues":["turbinado sugar"],"thorough":false,"lookBack":false,"aboutYou":false}"#
                    .to_string(),
            )
        })
        .await;
        assert!(kept.is_some());
        assert_eq!(outcome, CuesTry::Cues);
    }
}
