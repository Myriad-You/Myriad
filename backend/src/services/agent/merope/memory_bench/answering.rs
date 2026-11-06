//! Recall, answering and grading for the memory bench.

use super::super::remembering;
use super::*;

/// What a chat turn would recall for `question`: from their words, and
/// the cues when there are any; and, going through all of it, the topics
/// not all of whose facts are there (`store::Recalled::not_all_here`), or,
/// while she reads her nodes, all of the kind as code lists it.
pub(super) async fn recalled_for(
    db: &sea_orm::DatabaseConnection,
    question: &Question,
    user_id: i32,
    present: &Audience,
    cues: Option<&myriad_merope::remembering::Cues>,
    thorough: usize,
) -> (Vec<String>, Vec<(String, usize, usize)>, Option<String>) {
    // While she reads her nodes, as production does: ranked, then what
    // comes to mind of her nodes on the day it is asked.
    if super::settle::on_nodes(present) && super::super::memory_nodes::mode().reads() {
        let now = date_of(&question.question_date).with_timezone(&chrono::Utc);
        let ranked = remembering::ranked_with(
            db,
            user_id,
            present,
            &question.question,
            cues,
            RECALLED,
            thorough,
            &Priming::default(),
            1.0,
        )
        .await;
        let nodes = super::super::memory_nodes::context::load(db, user_id, now).await;
        match (ranked, nodes) {
            (Ok((recalled, _)), Ok(nodes)) => {
                let recollection = super::super::memory_nodes::recall::recollect(
                    &nodes.shown,
                    &nodes.own,
                    &recalled,
                    cues,
                    now,
                );
                let lines = recollection
                    .named
                    .into_iter()
                    .chain(recollection.heard)
                    .chain(recalled.brought_to_mind)
                    .collect();
                return (lines, Vec::new(), recollection.listed);
            }
            (_, Err(error)) => eprintln!("bench call failed: reading her nodes: {error:#}"),
            _ => {}
        }
        return Default::default();
    }
    let (named, not_all_here) = remembering::recall_with(
        db,
        user_id,
        present,
        &question.question,
        cues,
        RECALLED,
        thorough,
        &Priming::default(),
        1.0,
    )
    .await
    .map(|(recalled, _)| {
        // Going through all of it, as production shows it: with their words.
        let named = if cues.is_some_and(|cues| cues.thorough) {
            recalled.with_their_words()
        } else {
            recalled.named
        };
        (
            named
                .into_iter()
                .chain(recalled.brought_to_mind)
                .collect::<Vec<_>>(),
            recalled.not_all_here,
        )
    })
    .unwrap_or_default();
    (named, not_all_here, None)
}

pub(super) struct Answered {
    pub(super) retrieved: bool,
    pub(super) correct: Option<bool>,
    /// Noise questions: whether the answer brought in what she keeps of
    /// them where the question did not call for it.
    pub(super) dragged: Option<bool>,
    pub(super) answer: String,
    pub(super) recalled: Vec<String>,
    /// What she was to think of before answering, as the cues judged it.
    pub(super) cues: Option<String>,
    /// What she went through when needing all of it: from her nodes, the
    /// list code made.
    pub(super) through: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn answer_one(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    lite: &crate::services::analyzer::AiAnalyzer,
    question: &Question,
    user_id: i32,
    present: &Audience,
    cues: Option<&myriad_merope::remembering::Cues>,
    thorough: usize,
    back_when_thorough: bool,
    answering: &[String],
) -> Answered {
    let (recalled, not_all_here, listed) =
        recalled_for(db, question, user_id, present, cues, thorough).await;
    let retrieved = answering.iter().any(|fact| {
        recalled.iter().any(|line| line.contains(fact.as_str()))
            || listed
                .as_ref()
                .is_some_and(|list| list.contains(fact.as_str()))
    });
    // Asked about what was said in detail, she scrolls back, as in a chat;
    // the wide way also when their words plainly reach back, as production
    // does. Never in a group: what she scrolls through is the private chat.
    let reaching_back =
        back_when_thorough && myriad_merope::remembering::reaches_back(&question.question);
    let scrolled = match cues
        .filter(|_| !present.is_group())
        .filter(|cues| cues.look_back || (back_when_thorough && cues.thorough) || reaching_back)
    {
        Some(cues) => {
            let query = std::iter::once(question.question.as_str())
                .chain(cues.cues.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            let found = remembering::look_back(
                db,
                user_id,
                &query,
                &question.question,
                if back_when_thorough && cues.thorough {
                    remembering::LOOK_BACK_THOROUGH
                } else {
                    remembering::LOOK_BACK
                },
            )
            .await;
            myriad_merope::remembering::looked_back_section(&found)
        }
        None => None,
    };
    // Needing all of it, she goes through it as production has her do.
    // From her nodes, the list code made stands for it, as production shows.
    let going_through = cues
        .filter(|cues| cues.thorough && !recalled.is_empty())
        .map(|_| match &listed {
            Some(list) => list.clone(),
            None => std::iter::once(myriad_merope::speaking::format_going_through_all_section())
                .chain(myriad_merope::speaking::format_not_all_here_section(
                    &not_all_here,
                ))
                .collect::<Vec<_>>()
                .join("\n\n"),
        });
    let input = json!({
        "today": question.question_date,
        "scrollingBack": scrolled,
        "goingThroughAll": going_through,
        "question": question.question,
    })
    .to_string();
    // A reply the provider dropped is asked again, as production does; one
    // that never comes is unanswered, not wrong.
    let mut answer = String::new();
    for _ in 0..3 {
        answer = lite
            .analyze_with_system(&answer_system(question, present, &recalled), &input)
            .await
            .inspect_err(|error| eprintln!("bench call failed: answering: {error:#}"))
            .unwrap_or_default();
        if !answer.trim().is_empty() {
            break;
        }
    }
    if answer.trim().is_empty() {
        return Answered {
            retrieved,
            correct: None,
            dragged: None,
            answer,
            recalled,
            cues: cues.map(|cues| format!("{cues:?}")),
            through: going_through,
        };
    }
    let mut graded = json!({"question": question.question, "correctAnswer": question.answer,
        "response": answer});
    if question.question_type == "noise" {
        graded["memories"] = json!(question.memories);
    }
    let graded = judge
        .analyze_json(
            &judge_system(question),
            &graded.to_string(),
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
    Answered {
        retrieved,
        correct: graded.as_ref().and_then(|value| value["correct"].as_bool()),
        dragged: graded.as_ref().and_then(|value| value["dragged"].as_bool()),
        answer,
        recalled,
        cues: cues.map(|cues| format!("{cues:?}")),
        through: going_through,
    }
}
