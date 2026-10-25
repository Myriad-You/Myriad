//! Recall, answering and grading for the memory bench.

use super::super::remembering;
use super::*;

/// What a chat turn would recall for `question`: from their words, and
/// the cues when there are any.
pub(super) async fn recalled_for(
    db: &sea_orm::DatabaseConnection,
    question: &Question,
    user_id: i32,
    present: &Audience,
    cues: Option<&myriad_merope::remembering::Cues>,
    thorough: usize,
) -> Vec<String> {
    remembering::recall_with(
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
        recalled
            .named
            .into_iter()
            .chain(recalled.brought_to_mind)
            .collect::<Vec<_>>()
    })
    .unwrap_or_default()
}

pub(super) struct Answered {
    pub(super) retrieved: bool,
    pub(super) correct: Option<bool>,
    /// Noise questions: whether the answer brought in what she keeps of
    /// them where the question did not call for it.
    pub(super) dragged: Option<bool>,
    pub(super) answer: String,
    pub(super) recalled: Vec<String>,
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
    let recalled = recalled_for(db, question, user_id, present, cues, thorough).await;
    let retrieved = answering
        .iter()
        .any(|fact| recalled.iter().any(|line| line.contains(fact.as_str())));
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
    let input = json!({
        "today": question.question_date,
        "whatYouRemember": recalled,
        "scrollingBack": scrolled,
        "question": question.question,
    })
    .to_string();
    // A reply the provider dropped is asked again, as production does; one
    // that never comes is unanswered, not wrong.
    let mut answer = String::new();
    for _ in 0..3 {
        answer = lite
            .analyze_with_system(&answer_system(question, present), &input)
            .await
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
    }
}
