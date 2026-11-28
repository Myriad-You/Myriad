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
    let recall = recalled_and_resident(db, question, user_id, present, cues, thorough).await;
    (recall.recalled(), recall.listed)
}

/// Of what recall ranked, how many lines something they said found (by
/// its words or its meaning), and how many only filled the room as recent
/// context ([`crate::services::agent::merope::store::Recalled::recent`]).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Found {
    pub(super) found: usize,
    pub(super) filling: usize,
}

impl Found {
    fn of(recalled: &crate::services::agent::merope::store::Recalled) -> Self {
        let filling = recalled
            .named
            .iter()
            .filter(|line| recalled.recent.contains(line))
            .count();
        Self {
            found: recalled.named.len() - filling,
            filling,
        }
    }
}

/// [`recalled_for`], each kind of thing apart as production lays it out,
/// with how their life stands as production shows it every time in a
/// private talk (`Recollection::resident`): not recalled; and how much of
/// what recall ranked something they said found.
async fn recalled_and_resident(
    db: &sea_orm::DatabaseConnection,
    question: &Question,
    user_id: i32,
    present: &Audience,
    cues: Option<&myriad_merope::remembering::Cues>,
    thorough: usize,
) -> super::aligned::Recall {
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
                let found = Found::of(&recalled);
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
                let mut recall = super::aligned::Recall {
                    ongoing: nodes.ongoing_section(&recollection.picked),
                    named: recollection.named,
                    heard: recollection.heard,
                    brought_to_mind: recalled.brought_to_mind.clone(),
                    widened: Vec::new(),
                    listed: recollection.listed,
                    not_all_here: recollection.not_all_here,
                    resident: recollection.resident,
                    found,
                };
                let mut lines = recall.recalled();
                let shown = lines.len();
                widened(&mut lines, &recalled, cues, more, thorough_more);
                recall.widened = lines.split_off(shown);
                return recall;
            }
            (_, Err(error)) => eprintln!("bench call failed: reading her nodes: {error:#}"),
            _ => {}
        }
        return Default::default();
    }
    remembering::recall_with(
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
        let found = Found::of(&recalled);
        // Going through all of it, as production shows it: with their words.
        let named = if cues.is_some_and(|cues| cues.thorough) {
            recalled.with_their_words()
        } else {
            recalled.named
        };
        super::aligned::Recall {
            named,
            brought_to_mind: recalled.brought_to_mind,
            found,
            ..Default::default()
        }
    })
    .unwrap_or_default()
}

/// `MEROPE_MEMORY_BENCH_STANDING=<file>` (question id to lines): how things
/// stand with them now, as a list she would always have, shown first among
/// what she remembers when she answers. A trial: not recalled, and not
/// counted as recalled. Without the file, the list production shows.
fn standing_of(id: &str) -> Option<Vec<String>> {
    static STANDING: std::sync::OnceLock<std::collections::HashMap<String, Vec<String>>> =
        std::sync::OnceLock::new();
    STANDING
        .get_or_init(|| {
            std::env::var("MEROPE_MEMORY_BENCH_STANDING")
                .map(|path| {
                    serde_json::from_str(&std::fs::read_to_string(&path).expect("the list"))
                        .expect("question ids, each to its lines")
                })
                .unwrap_or_default()
        })
        .get(id)
        .cloned()
}

/// `MEROPE_MEMORY_BENCH_RECALL_ONLY=1`: recall each way and ask nothing,
/// to see what a change to recall changes before paying for answers.
fn recall_only() -> bool {
    std::env::var("MEROPE_MEMORY_BENCH_RECALL_ONLY").as_deref() == Ok("1")
}

/// What another run recalled and answered the wide way, and what she was
/// asked then ([`asked_as`]).
#[derive(Clone, Default)]
struct Earlier {
    recalled: Vec<String>,
    correct: Option<bool>,
    why: Option<String>,
    answer: String,
    prompt: String,
}

/// What she is asked, as a run's rows keep it: the SHA-256 of her answering
/// prompt and the message, in hex.
fn asked_as(system: &str, input: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(format!("{system}\n\n{input}").as_bytes()))
}

/// Whether another run's answer is this one's: the same lines recalled,
/// and asked of her the same way, word for word (`prompt`, [`asked_as`]).
fn asked_the_same(earlier: &Earlier, recalled: &[String], prompt: &str) -> bool {
    earlier.recalled == recalled && earlier.prompt == prompt
}

/// Of the run in `MEROPE_MEMORY_BENCH_ANSWERS_OF` (its `.partial.jsonl`),
/// what this question and round recalled and answered the wide way.
fn answered_before(id: &str, round: usize) -> Option<Earlier> {
    static OF: std::sync::OnceLock<HashMap<(String, usize), Earlier>> = std::sync::OnceLock::new();
    OF.get_or_init(|| {
        let Ok(path) = std::env::var("MEROPE_MEMORY_BENCH_ANSWERS_OF") else {
            return HashMap::new();
        };
        let text = std::fs::read_to_string(&path).expect("the run answered before");
        answers_in(&text)
    })
    .get(&(id.to_string(), round))
    .cloned()
}

fn answers_in(text: &str) -> HashMap<(String, usize), Earlier> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| !row["void"].as_bool().unwrap_or(false))
        .filter(|row| {
            !row["wide"]["response"]
                .as_str()
                .unwrap_or("")
                .trim()
                .is_empty()
        })
        .filter_map(|row| {
            let wide = &row["wide"];
            Some((
                (
                    row["id"].as_str()?.to_string(),
                    row["round"].as_u64()? as usize,
                ),
                Earlier {
                    recalled: wide["recalled"]
                        .as_array()?
                        .iter()
                        .filter_map(|line| line.as_str().map(str::to_string))
                        .collect(),
                    correct: wide["correct"].as_bool(),
                    why: wide["why"].as_str().map(str::to_string),
                    answer: wide["response"].as_str()?.to_string(),
                    // A run that does not say what she was asked (one from
                    // before her prompt said it) is not answered again.
                    prompt: wide["prompt"].as_str()?.to_string(),
                },
            ))
        })
        .collect()
}

/// How far down the ranking by line [`ranks_of_needs`] looks.
pub(super) const RANKS_LOOKED_AT: usize = 200;

/// Where, in the ranking by line that recall starts from (not what her
/// nodes then show), the first line holding each thing `question` needs
/// comes, 1 for the first: looking as deep as answering does (`thorough`
/// when it goes through all of it), and looking [`RANKS_LOOKED_AT`] deep.
/// `None` where it is not. To tell a fact ranked but not shown, one ranked
/// just past the depth, and one never a candidate at all. The deep order is
/// not the shallow one: each try brings more, so they merge otherwise.
pub(super) async fn ranks_of_needs(
    db: &sea_orm::DatabaseConnection,
    question: &Question,
    user_id: i32,
    present: &Audience,
    cues: Option<&myriad_merope::remembering::Cues>,
    thorough: usize,
) -> Vec<(Option<usize>, Option<usize>)> {
    let at = |limit: usize, thorough: usize| async move {
        let Ok((recalled, _)) = remembering::ranked_with(
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
            .collect::<Vec<_>>()
    };
    let shallow = at(RECALLED, thorough).await;
    let deep = at(RANKS_LOOKED_AT, RANKS_LOOKED_AT).await;
    shallow.into_iter().zip(deep).collect()
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

#[derive(Clone)]
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
    /// What she read scrolling back through the chat before answering, as
    /// it was put in front of her: not recalled, but what she answered on
    /// too, so what judges recall can be shown it.
    pub(super) scrolled: Option<String>,
    /// How their life stands, as it was put in front of her before what she
    /// recalled (in a private talk): not recalled either.
    pub(super) standing: Vec<String>,
    /// What she was asked ([`asked_as`]), when she was: another run takes
    /// her answer only where it was asked the same.
    pub(super) prompt: Option<String>,
}

impl Answered {
    /// Every line she answered on, however it came: how things stand, what
    /// she recalled, the list code made when she went through all of a
    /// kind, what she scrolled back to. What a question needs is looked for
    /// here to tell whether it was in front of her at all. A run that only
    /// recalled, or was answered as another run answered, keeps nothing
    /// scrolled back to or listed: for it this is short of them.
    pub(super) fn shown(&self) -> Vec<String> {
        let sections = [&self.through, &self.scrolled];
        self.standing
            .iter()
            .chain(&self.recalled)
            .map(String::as_str)
            .chain(sections.into_iter().flatten().flat_map(|text| text.lines()))
            .map(str::to_string)
            .collect()
    }
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
    let mut recall = recalled_and_resident(db, question, user_id, present, cues, thorough).await;
    let standing = standing_of(&question.question_id)
        .unwrap_or_else(|| std::mem::take(&mut recall.resident));
    // The way production answers is held to the length another run
    // answered on, when asked (`MEROPE_MEMORY_BENCH_RECALL_CHARS_OF`).
    if back_when_thorough
        && wider() == 1
        && let Some(most) = chars_of(&question.question_id, round())
    {
        recall.cut_to(most);
    }
    let (recalled, listed, recall_found) = (recall.recalled(), recall.listed.clone(), recall.found);
    let retrieved = answering.iter().any(|fact| {
        recalled.iter().any(|line| line.contains(fact.as_str()))
            || listed
                .as_ref()
                .is_some_and(|list| list.contains(fact.as_str()))
    });
    // Only recalling (`MEROPE_MEMORY_BENCH_RECALL_ONLY=1`): nothing asked.
    let unasked = |recalled: Vec<String>, earlier: Option<Earlier>| {
        let earlier = earlier.unwrap_or_default();
        let prompt = Some(earlier.prompt).filter(|prompt| !prompt.is_empty());
        Answered {
            retrieved,
            correct: earlier.correct,
            dragged: None,
            why: earlier.why,
            answer: earlier.answer,
            recalled,
            cues: cues.map(|cues| format!("{cues:?}")),
            through: None,
            scrolled: None,
            standing: standing.clone(),
            prompt,
        }
    };
    if recall_only() {
        return unasked(recalled, None);
    }
    // Asked about what was said in detail, she scrolls back, as in a chat;
    // the wide way also when their words plainly reach back, as production
    // does. Never in a group: what she scrolls through is the private chat.
    let reaching_back =
        back_when_thorough && myriad_merope::remembering::reaches_back(&question.question);
    let found = match cues
        .filter(|_| !present.is_group())
        .filter(|cues| cues.look_back || (back_when_thorough && cues.thorough) || reaching_back)
    {
        Some(cues) => {
            let query = std::iter::once(question.question.as_str())
                .chain(cues.cues.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            remembering::look_back(
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
            .await
        }
        None => Vec::new(),
    };
    // The raw-chat arm: the raw chat searched every time as well, whatever
    // the cues said, beside what the look back found.
    let found = match super::raw_back::raw_way()
        .filter(|_| !present.is_group())
        .filter(|_| super::raw_back::weak_enough(&question.question_id, recall_found))
    {
        Some(way) => super::raw_back::with_raw(db, user_id, &question.question, way, found).await,
        None => found,
    };
    let scrolled = myriad_merope::remembering::looked_back_section(&found);
    // Needing all of it, she goes through it as production has her do.
    // From her nodes, the list code made stands for it, as production shows.
    let going_through = recall.going_through(cues);
    // All of it in her prompt, each in its own section and in production's
    // order: what she scrolled back to and what she goes through too.
    let system = super::aligned::answer_system(
        question,
        present,
        &recall,
        &standing,
        going_through.as_deref(),
        scrolled.as_deref(),
    );
    let input = json!({
        "today": question.question_date,
        "question": question.question,
    })
    .to_string();
    let prompt = asked_as(&system, &input);
    // The wide way recalling just what another run did, on the same kept
    // copy, and asked of her just as it was, is answered as it answered
    // (`MEROPE_MEMORY_BENCH_ANSWERS_OF`).
    if back_when_thorough
        && wider() == 1
        && let Some(earlier) = answered_before(&question.question_id, round())
        && asked_the_same(&earlier, &recalled, &prompt)
    {
        return unasked(recalled, Some(earlier));
    }
    // A reply the provider dropped is asked again, as production does; one
    // that never comes is unanswered, not wrong.
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
        return Answered {
            retrieved,
            correct: None,
            dragged: None,
            why: None,
            answer,
            recalled,
            cues: cues.map(|cues| format!("{cues:?}")),
            through: going_through,
            scrolled,
            standing,
            prompt: Some(prompt),
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
        scrolled,
        standing,
        prompt: Some(prompt),
    }
}

/// Another run's answers are taken only where it answered: not from a void
/// row, not from one that got no reply.
#[test]
fn only_what_was_answered_is_answered_again() {
    let rows = [
        json!({"id": "a", "round": 0, "void": false,
            "wide": {"recalled": ["x"], "correct": true, "why": "ok", "response": "是的",
                "prompt": "p"}}),
        json!({"id": "b", "round": 0, "void": true,
            "wide": {"recalled": ["y"], "correct": false, "response": "不", "prompt": "p"}}),
        json!({"id": "c", "round": 0, "void": false,
            "wide": {"recalled": ["z"], "correct": null, "response": "  ", "prompt": "p"}}),
    ];
    let text: String = rows.iter().map(|row| format!("{row}\n")).collect();
    let earlier = answers_in(&text);
    assert_eq!(earlier.len(), 1);
    let a = &earlier[&("a".to_string(), 0)];
    assert_eq!(a.recalled, ["x"]);
    assert_eq!((a.correct, a.answer.as_str()), (Some(true), "是的"));
}

/// A run that does not say what she was asked (every run from before what
/// she answers on was laid out in sections) is not answered again: the same
/// lines recalled were not the same prompt.
#[test]
fn a_run_that_does_not_say_what_she_was_asked_is_not_answered_again() {
    let row = json!({"id": "a", "round": 0, "void": false,
        "wide": {"recalled": ["x"], "correct": true, "why": "ok", "response": "是的"}});
    assert!(answers_in(&format!("{row}\n")).is_empty());
}

/// Another run's answer is taken only for the same lines recalled and the
/// same prompt, word for word.
#[test]
fn an_answer_is_taken_only_where_she_was_asked_the_same() {
    let prompt = asked_as("system", "input");
    assert_eq!(prompt, asked_as("system", "input"));
    assert_ne!(prompt, asked_as("system\n\n## Going on now", "input"));
    let earlier = Earlier {
        recalled: vec!["x".to_string()],
        prompt: prompt.clone(),
        ..Default::default()
    };
    assert!(asked_the_same(&earlier, &["x".to_string()], &prompt));
    assert!(!asked_the_same(&earlier, &["x".to_string()], &asked_as("other", "input")));
    assert!(!asked_the_same(&earlier, &["y".to_string()], &prompt));
}
