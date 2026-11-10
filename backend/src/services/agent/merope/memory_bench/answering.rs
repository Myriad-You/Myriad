//! Recall, answering and grading for the memory bench.

use std::collections::HashMap;

use super::super::remembering;
use super::*;

/// What a chat turn would recall for `question`: from their words, and
/// the cues when there are any; and, in a private talk, going through all
/// of it, all of the kind as code lists it from her nodes.
pub(super) async fn recalled_for(
    db: &sea_orm::DatabaseConnection,
    question: &Question,
    user_id: i32,
    present: &Audience,
    cues: Option<&myriad_merope::remembering::Cues>,
    thorough: usize,
) -> (Vec<String>, Option<String>) {
    // The control that only recalls more, when asked for.
    let wider = wider();
    let (more, thorough_more) = ((wider - 1) * RECALLED, (wider - 1) * thorough);
    let (limit, thorough) = (RECALLED * wider, thorough * wider);
    // In a private talk, as production does: ranked, then what comes to
    // mind of her nodes on the day it is asked.
    if super::settle::on_nodes(present) {
        let now = date_of(&question.question_date).with_timezone(&chrono::Utc);
        let ranked = remembering::ranked_with(
            db,
            user_id,
            present,
            &question.question,
            cues,
            limit,
            thorough,
            &Priming::default(),
            1.0,
        )
        .await;
        let nodes = super::super::memory_nodes::context::load(db, user_id, now).await;
        match (ranked, nodes) {
            (Ok((recalled, _)), Ok(nodes)) => {
                // Wider, her nodes bring as many times more too: else the
                // nodes, three of three entries, hold it near what it was.
                let recollection = super::super::memory_nodes::recall::recollect_times(
                    &nodes.shown,
                    &nodes.own,
                    &recalled,
                    cues,
                    now,
                    wider,
                );
                let mut lines: Vec<String> = recollection
                    .named
                    .into_iter()
                    .chain(recollection.heard)
                    .chain(recalled.brought_to_mind.iter().cloned())
                    .collect();
                widened(&mut lines, &recalled, cues, more, thorough_more);
                return (lines, recollection.listed);
            }
            (_, Err(error)) => eprintln!("bench call failed: reading her nodes: {error:#}"),
            _ => {}
        }
        return Default::default();
    }
    let named = remembering::recall_with(
        db,
        user_id,
        present,
        &question.question,
        cues,
        limit,
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
        named
            .into_iter()
            .chain(recalled.brought_to_mind)
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    (named, None)
}

/// How far down the ranking by line [`ranks_of_needs`] looks.
pub(super) const RANKS_LOOKED_AT: usize = 200;

/// Where, in the ranking by line that recall starts from (not what her
/// nodes then show), the first line holding each thing `question` needs
/// comes: 1 for the first, `None` past [`RANKS_LOOKED_AT`] or nowhere. To
/// tell a fact that ranked just past what is shown from one recall never
/// came near.
pub(super) async fn ranks_of_needs(
    db: &sea_orm::DatabaseConnection,
    question: &Question,
    user_id: i32,
    present: &Audience,
    cues: Option<&myriad_merope::remembering::Cues>,
) -> Vec<Option<usize>> {
    let Ok((recalled, _)) = remembering::ranked_with(
        db,
        user_id,
        present,
        &question.question,
        cues,
        RANKS_LOOKED_AT,
        RANKS_LOOKED_AT,
        &Priming::default(),
        1.0,
    )
    .await
    else {
        return vec![None; question.needs.len()];
    };
    (0..question.needs.len())
        .map(|fact| {
            recalled
                .named
                .iter()
                .position(|line| question.holds(fact, line))
                .map(|at| at + 1)
        })
        .collect()
}

/// The wider control on her nodes: after what they brought, the ranked
/// lines they left out, in their order, up to `more` (`thorough_more` when
/// answering needs all of it); a line only filling as recent talk does not
/// count, as it would not going through all of it.
fn widened(
    lines: &mut Vec<String>,
    recalled: &crate::services::agent::merope::store::Recalled,
    cues: Option<&myriad_merope::remembering::Cues>,
    more: usize,
    thorough_more: usize,
) {
    let more = if cues.is_some_and(|cues| cues.thorough) {
        thorough_more
    } else {
        more
    };
    let left_out: Vec<String> = recalled
        .named
        .iter()
        .filter(|line| !recalled.recent.contains(line))
        .filter(|line| !lines.iter().any(|shown| shown.contains(line.as_str())))
        .take(more)
        .cloned()
        .collect();
    lines.extend(left_out);
}

/// `MEROPE_MEMORY_BENCH_RECALL_CHARS_OF=<report>`: the wide way of each
/// question is recalled at most as long, in characters, as the same
/// question and round recalled the wide way in that run (its
/// `.partial.jsonl` or its report), so two ways are compared at the same
/// length of what she answers on, not one answering on more.
fn chars_of(id: &str, round: usize) -> Option<usize> {
    static OF: std::sync::OnceLock<HashMap<(String, usize), usize>> = std::sync::OnceLock::new();
    OF.get_or_init(|| {
        let Ok(path) = std::env::var("MEROPE_MEMORY_BENCH_RECALL_CHARS_OF") else {
            return HashMap::new();
        };
        let text = std::fs::read_to_string(&path).expect("the run to match in length");
        let rows: Vec<Value> = if path.ends_with(".jsonl") {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        } else {
            serde_json::from_str::<Value>(&text).expect("a report")["rows"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        };
        rows.iter()
            .filter(|row| !row["void"].as_bool().unwrap_or(false))
            .filter_map(|row| {
                let recalled = row["wide"]["recalled"].as_array()?;
                let chars = recalled
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|line| line.chars().count())
                    .sum();
                Some((
                    (
                        row["id"].as_str()?.to_string(),
                        row["round"].as_u64()? as usize,
                    ),
                    chars,
                ))
            })
            .collect()
    })
    .get(&(id.to_string(), round))
    .copied()
}

/// `lines` kept whole, in order, while they fit in `most` characters.
fn cut_to(lines: &mut Vec<String>, most: usize) {
    let mut used = 0;
    let mut kept = 0;
    for line in lines.iter() {
        let chars = line.chars().count();
        if used + chars > most {
            break;
        }
        used += chars;
        kept += 1;
    }
    lines.truncate(kept);
}

pub(super) struct Answered {
    pub(super) retrieved: bool,
    pub(super) correct: Option<bool>,
    /// Noise questions: whether the answer brought in what she keeps of
    /// them where the question did not call for it.
    pub(super) dragged: Option<bool>,
    /// Why the grader judged it so, in a few words, when it said.
    pub(super) why: Option<String>,
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
    let (mut recalled, listed) = recalled_for(db, question, user_id, present, cues, thorough).await;
    // The way production answers is held to the length another run
    // answered on, when asked (`MEROPE_MEMORY_BENCH_RECALL_CHARS_OF`).
    if back_when_thorough
        && wider() == 1
        && let Some(most) = chars_of(&question.question_id, round())
    {
        cut_to(&mut recalled, most);
    }
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
            None => myriad_merope::speaking::format_going_through_all_section(),
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
            why: None,
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
        why: graded
            .as_ref()
            .and_then(|value| value["why"].as_str())
            .map(str::to_string),
        answer,
        recalled,
        cues: cues.map(|cues| format!("{cues:?}")),
        through: going_through,
    }
}
