//! Her memory against LongMemEval (Wu et al., ICLR 2025), run through the
//! same path her chats take: each user turn of each past session is written
//! as production writes it (the extraction on the judgment model, then the
//! same transaction), dated as the session was; then the question is
//! recalled as a chat turn recalls, answered from what came back only, and
//! graded. Three numbers per question tell where it failed: was what
//! answers it written at all, did recall bring it back, was the answer right.
//!
//! Opt-in and spends on the site's models:
//! `MEROPE_MEMORY_BENCH=<longmemeval json> MEROPE_MEMORY_BENCH_REPORT=<new
//! file> MYRIAD_MEDIA_TEST_DATABASE_URL=… DATABASE_URL=… cargo test -p
//! myriad-backend --bin myriad-backend -- --ignored her_memory_on_longmemeval
//! --nocapture`. `MEROPE_MEMORY_BENCH_PER_TYPE` (default 5) questions of each
//! type; `MEROPE_MEMORY_BENCH_ONLY` a comma list of question ids. Writes go
//! to a fresh schema that is dropped afterwards.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::TimeZone;
use futures::StreamExt;
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseBackend, Set, Statement};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::models::entities::agent_addressee_state;
use crate::services::agent::memory::unified::{Audience, Priming};

#[derive(Deserialize, Clone)]
struct Question {
    question_id: String,
    question_type: String,
    question: String,
    answer: Value,
    question_date: String,
    haystack_dates: Vec<String>,
    haystack_sessions: Vec<Vec<Turn>>,
}

#[derive(Deserialize, Clone)]
struct Turn {
    role: String,
    content: String,
    #[serde(default)]
    has_answer: bool,
}

const RECALLED: usize = 8;
const THOROUGH: usize = super::remembering::THOROUGH;
const AT_ONCE: usize = 6;

fn date_of(text: &str) -> chrono::DateTime<chrono::FixedOffset> {
    // "2023/04/10 (Mon) 17:50"
    let cleaned: String = text
        .split_whitespace()
        .filter(|part| !part.starts_with('('))
        .collect::<Vec<_>>()
        .join(" ");
    let naive = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y/%m/%d %H:%M")
        .unwrap_or_else(|_| panic!("unreadable date {text}"));
    // Their local time, as a chat's times are.
    chrono::Local
        .from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| naive.and_utc().with_timezone(&chrono::Local))
        .fixed_offset()
}

/// As she would answer in a chat: from what she remembers of them (each
/// with the date she learned it), with what she generally knows for advice
/// or a recommendation; what she does not remember of them she does not make up.
const ANSWER_SYSTEM: &str = "Someone you have talked with before asks you something. What you remember of them is listed, each with the date you learned it; it is all you know of your past conversations. Today is the date given. If they ask for advice or a recommendation, use what you remember of them together with what you generally know. If they ask about something from your past conversations that you do not remember, say you do not know. Answer briefly.";

fn judge_system(question: &Question) -> String {
    let abstention = question.question_id.ends_with("_abs");
    let rule = if abstention {
        "The question cannot be answered from the conversations. Answer yes only if the response says it does not know or cannot tell."
    } else {
        match question.question_type.as_str() {
            "temporal-reasoning" => {
                "Answer yes if the response contains the correct answer. Do not penalize off-by-one errors in counting days, weeks or months."
            }
            "knowledge-update" => {
                "Answer yes if the response contains the correct answer. If it also contains earlier information along with the updated answer, it is still correct as long as the updated answer is the required one."
            }
            "single-session-preference" => {
                "The correct answer is a rubric of what the user would want. Answer yes if the response recalls and uses the user's personal information correctly."
            }
            _ => {
                "Answer yes if the response contains the correct answer, or is equivalent to it. If it contains only part of what the answer needs, answer no."
            }
        }
    };
    format!(
        "You grade an answer to a question about past conversations. {rule} Reply with only a JSON object {{\"correct\": true or false}}."
    )
}

fn grade_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["correct"],
        "properties":{"correct":{"type":"boolean"}}})
}

async fn new_user(db: &sea_orm::DatabaseConnection, name: &str) -> i32 {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO users (username) VALUES ($1) RETURNING id",
        [name.into()],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "id")
    .unwrap()
}

/// This turn is the person's latest message, as the chat that carried it
/// would have recorded.
async fn said_at(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    at: chrono::DateTime<chrono::FixedOffset>,
) {
    let state = super::store::get_or_create_state(db, user_id)
        .await
        .unwrap();
    let mut state: agent_addressee_state::ActiveModel = state.into();
    state.last_user_message_at = Set(Some(at));
    state.update(db).await.unwrap();
}

struct Outcome {
    written: bool,
    plain: Answered,
    cued: Answered,
    wide: Answered,
    memories: Vec<String>,
    /// Each memory's concepts, as kept beside it.
    concepts: Vec<Value>,
    kept: usize,
}

async fn run_one(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    lite: &crate::services::analyzer::AiAnalyzer,
    question: &Question,
) -> Outcome {
    let user_id = new_user(db, &format!("lme-{}", question.question_id)).await;
    let present = Audience::private(user_id);
    let mut answering: Vec<String> = Vec::new();
    let mut kept = 0;
    let mut sessions: Vec<(chrono::DateTime<chrono::FixedOffset>, &Vec<Turn>)> = question
        .haystack_dates
        .iter()
        .map(|date| date_of(date))
        .zip(question.haystack_sessions.iter())
        .collect();
    sessions.sort_by_key(|(at, _)| *at);
    // The chat itself, as the site keeps it, for scrolling back through.
    for (number, (session_at, turns)) in sessions.iter().enumerate() {
        let session = format!("lme-{}-{number}", question.question_id);
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_sessions (id, user_id, message_count, archived, created_at, last_active_at) \
             VALUES ($1, $2, $3, false, $4, $4)",
            [
                session.clone().into(),
                user_id.into(),
                (turns.len() as i32).into(),
                (*session_at).into(),
            ],
        ))
        .await
        .unwrap();
        for (index, turn) in turns.iter().enumerate() {
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO agent_messages (session_id, role, content, created_at) VALUES ($1, $2, $3, $4)",
                [
                    session.clone().into(),
                    turn.role.clone().into(),
                    turn.content.clone().into(),
                    (*session_at + chrono::Duration::minutes(index as i64)).into(),
                ],
            ))
            .await
            .unwrap();
        }
    }
    for (session_at, turns) in sessions {
        for (index, turn) in turns.iter().enumerate() {
            if turn.role != "user" {
                continue;
            }
            let reply = turns
                .get(index + 1)
                .filter(|next| next.role == "assistant")
                .map(|next| next.content.as_str())
                .unwrap_or("");
            let at = session_at + chrono::Duration::minutes(index as i64);
            let text: String = turn
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if text.chars().count() > 2_000 {
                continue;
            }
            let (existing, _) = super::store::recall_remembered_primed(
                db,
                user_id,
                &present,
                Some(&text),
                8,
                &Priming::default(),
                1.0,
            )
            .await
            .unwrap();
            if !super::chat_remember::should_extract_chat_remember_against(&text, &existing) {
                continue;
            }
            let input = super::chat_remember::extract_input(
                &text,
                reply,
                &super::TurnContext::default(),
                at.with_timezone(&chrono::Local),
            );
            let Ok(raw) = judge
                .analyze_json(
                    &myriad_merope::chat_remember::extract_system_prompt(&existing),
                    &input,
                    myriad_merope::chat_remember::EXTRACT_SCHEMA_NAME,
                    Some(&myriad_merope::chat_remember::extract_schema()),
                )
                .await
            else {
                continue;
            };
            let Some(updates) =
                super::chat_remember::parse_chat_memory_updates(&raw, &text, reply, &existing)
            else {
                continue;
            };
            said_at(db, user_id, at).await;
            let before = chrono::Utc::now().fixed_offset();
            let transaction = sea_orm::TransactionTrait::begin(db).await.unwrap();
            let applied = super::store::apply_chat_memory_updates_on(
                &transaction,
                user_id,
                at,
                &updates,
                &present,
            )
            .await
            .unwrap_or(false);
            transaction.commit().await.unwrap();
            if applied {
                // Dated as the session was, not as the bench ran.
                db.execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE agent_memories SET created_at = $1, valid_from = $1, updated_at = $1 \
                     WHERE user_id = $2 AND created_at >= $3",
                    [at.into(), user_id.into(), before.into()],
                ))
                .await
                .unwrap();
                kept += 1;
                // What answers the question may be in their words or hers.
                let answer_here =
                    turn.has_answer || turns.get(index + 1).is_some_and(|next| next.has_answer);
                if answer_here {
                    answering.extend(
                        updates
                            .updates
                            .iter()
                            .filter_map(|update| update.fact.as_deref())
                            .chain(updates.said.iter().map(|said| said.said.as_str()))
                            .map(super::ingest::compact_summary),
                    );
                }
            }
        }
    }
    // As a chat turn does: think what to look for, then recall with it.
    let cues = judge
        .analyze_json(
            &myriad_merope::remembering::system(),
            &myriad_merope::remembering::input(&question.question, None, &question.question_date),
            myriad_merope::remembering::SCHEMA_NAME,
            Some(&myriad_merope::remembering::schema()),
        )
        .await
        .ok()
        .and_then(|raw| myriad_merope::remembering::parse(&raw));
    // The same memories, recalled once from their words alone and once
    // with the cues: the difference is recall's, not writing's.
    let plain = answer_one(
        db, judge, lite, question, user_id, &present, None, THOROUGH, false, &answering,
    )
    .await;
    let cued = answer_one(
        db,
        judge,
        lite,
        question,
        user_id,
        &present,
        cues.as_ref(),
        THOROUGH,
        false,
        &answering,
    )
    .await;
    // The same, also scrolling back through the chat when answering needs
    // all of it, as production does.
    let wide = answer_one(
        db,
        judge,
        lite,
        question,
        user_id,
        &present,
        cues.as_ref(),
        THOROUGH,
        true,
        &answering,
    )
    .await;
    // Everything she kept, to tell unwritten from unfound afterwards.
    let kept_rows = crate::services::agent::memory::unified::active_in(
        db,
        user_id,
        &present,
        &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
    )
    .await
    .unwrap_or_default();
    let concepts_by_id: std::collections::HashMap<String, Value> = {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        crate::models::entities::agent_memories::Entity::find()
            .filter(
                crate::models::entities::agent_memories::Column::Id
                    .is_in(kept_rows.iter().map(|row| row.id.clone())),
            )
            .all(db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|row| (row.id, row.concepts))
            .collect()
    };
    let (memories, concepts): (Vec<String>, Vec<Value>) = kept_rows
        .into_iter()
        .map(|row| {
            let concepts = concepts_by_id.get(&row.id).cloned().unwrap_or(json!([]));
            (row.content, concepts)
        })
        .unzip();
    Outcome {
        written: !answering.is_empty(),
        plain,
        cued,
        wide,
        kept,
        memories,
        concepts,
    }
}

/// What a chat turn would recall for `question`: from their words, and
/// the cues when there are any.
async fn recalled_for(
    db: &sea_orm::DatabaseConnection,
    question: &Question,
    user_id: i32,
    present: &Audience,
    cues: Option<&myriad_merope::remembering::Cues>,
    thorough: usize,
) -> Vec<String> {
    super::remembering::recall_with(
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

struct Answered {
    retrieved: bool,
    correct: Option<bool>,
    answer: String,
    recalled: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
async fn answer_one(
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
    // Asked about what was said in detail, she scrolls back, as in a chat.
    let scrolled = match cues.filter(|cues| cues.look_back || (back_when_thorough && cues.thorough))
    {
        Some(cues) => {
            let query = std::iter::once(question.question.as_str())
                .chain(cues.cues.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            let found = super::remembering::look_back(
                db,
                user_id,
                &query,
                &question.question,
                if back_when_thorough && cues.thorough {
                    super::remembering::LOOK_BACK_THOROUGH
                } else {
                    super::remembering::LOOK_BACK
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
            .analyze_with_system(ANSWER_SYSTEM, &input)
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
            answer,
            recalled,
        };
    }
    let graded = judge
        .analyze_json(
            &judge_system(question),
            &json!({"question": question.question, "correctAnswer": question.answer,
                "response": answer})
            .to_string(),
            "memory_bench_grade",
            Some(&grade_schema()),
        )
        .await
        .ok()
        .and_then(|raw| {
            let json = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim());
            serde_json::from_str::<Value>(json.as_deref().unwrap_or(raw.trim())).ok()
        })
        .and_then(|value| value["correct"].as_bool());
    Answered {
        retrieved,
        correct: graded,
        answer,
        recalled,
    }
}

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
async fn her_memory_on_longmemeval() {
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
    let config_db = super::super::semantic_eval::load_configured_lite().await;
    config_db.close().await.ok();
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
        .buffer_unordered(AT_ONCE)
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

/// The Chinese set (tests/merope/memory-zh.json): her kind of chat, in the
/// shape the bench reads, every type covered, each answer's turn marked.
#[test]
fn the_chinese_memory_set_reads_as_the_bench_needs() {
    let questions: Vec<Question> =
        serde_json::from_str(include_str!("../../../../../tests/merope/memory-zh.json")).unwrap();
    let types: std::collections::BTreeSet<&str> = questions
        .iter()
        .map(|question| question.question_type.as_str())
        .collect();
    assert_eq!(types.len(), 6);
    for question in &questions {
        assert_eq!(
            question.haystack_dates.len(),
            question.haystack_sessions.len()
        );
        for date in &question.haystack_dates {
            date_of(date);
        }
        let marked = question
            .haystack_sessions
            .iter()
            .flatten()
            .any(|turn| turn.has_answer);
        assert_eq!(
            marked,
            !question.question_id.ends_with("_abs"),
            "{}",
            question.question_id
        );
    }
}

/// What she keeps of one person tops out at a thousand memories (older,
/// weaker ones fade past it): whether recall still finds what answers a
/// question when the rest of those thousand are about other things. The
/// memories come from a finished bench report (`memories` of each row), so
/// nothing is extracted again: each question is answered twice from the
/// same memories, once alone and once among others' up to the cap, with
/// the same cues. `MEROPE_MEMORY_SCALE=<report> MEROPE_MEMORY_BENCH=<its
/// questions> MEROPE_MEMORY_BENCH_REPORT=<new file>`, optional
/// `MEROPE_MEMORY_BENCH_PER_TYPE`.
///
/// The thousand build up over half a year, the question's own scattered
/// among the rest, and the question alone has its own at the same dates:
/// the others are the only difference. Those dates are the timeline's, not
/// the conversations', so questions about when are left out of answering.
/// With `MEROPE_MEMORY_SCALE_LABELS=<file>` (id → `needed`, the indices of
/// its memories answering rests on) only recall is measured, from their
/// words alone and with cues, and no answer is asked for. Reports from
/// before concepts were kept beside memories have none, and recall also
/// searches them: what is found from those is lower than production for
/// both, alike.
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
async fn her_memory_at_its_full_size() {
    use crate::services::agent::memory::unified::MemoryKind;
    const FULL: usize = 1000;
    let source: Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MEROPE_MEMORY_SCALE").expect("scale source"))
            .unwrap(),
    )
    .unwrap();
    let questions: Vec<Question> = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MEROPE_MEMORY_BENCH").expect("questions")).unwrap(),
    )
    .unwrap();
    let report_path = std::env::var("MEROPE_MEMORY_BENCH_REPORT").expect("report path");
    assert!(
        !std::path::Path::new(&report_path).exists(),
        "report must not exist"
    );
    let per_type: usize = std::env::var("MEROPE_MEMORY_BENCH_PER_TYPE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(7);
    let rows = source["rows"].as_array().expect("rows");
    let mut taken: BTreeMap<String, usize> = BTreeMap::new();
    let chosen: Vec<(Question, Vec<String>)> = rows
        .iter()
        .filter(|row| row["type"] != "single-session-assistant")
        .filter_map(|row| {
            let id = row["id"].as_str()?;
            let question = questions.iter().find(|q| q.question_id == id)?.clone();
            let count = taken.entry(question.question_type.clone()).or_default();
            *count += 1;
            (*count <= per_type).then(|| {
                let memories = row["memories"]
                    .as_array()
                    .map(|all| {
                        all.iter()
                            .filter_map(|m| m.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                (question, memories)
            })
        })
        .collect();
    let everyone: Vec<String> = rows
        .iter()
        .flat_map(|row| row["memories"].as_array().cloned().unwrap_or_default())
        .filter_map(|m| m.as_str().map(str::to_string))
        .collect();
    // Concepts kept beside each memory, where the report has them.
    let concepts_of: std::collections::HashMap<String, Value> = rows
        .iter()
        .flat_map(|row| {
            let memories = row["memories"].as_array().cloned().unwrap_or_default();
            let concepts = row["concepts"].as_array().cloned().unwrap_or_default();
            memories.into_iter().zip(concepts)
        })
        .filter_map(|(memory, concepts)| Some((memory.as_str()?.to_string(), concepts)))
        .collect();
    let labels: Option<BTreeMap<String, Vec<usize>>> = std::env::var("MEROPE_MEMORY_SCALE_LABELS")
        .ok()
        .map(|path| {
            let raw: BTreeMap<String, Value> =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            raw.into_iter()
                .map(|(id, label)| {
                    let needed = label["needed"]
                        .as_array()
                        .map(|all| {
                            all.iter()
                                .filter_map(|index| index.as_u64().map(|index| index as usize))
                                .collect()
                        })
                        .unwrap_or_default();
                    (id, needed)
                })
                .collect()
        });
    let config_db = super::super::semantic_eval::load_configured_lite().await;
    config_db.close().await.ok();
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
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "memory_scale").await;
    let db = isolated.db.clone();
    // Kept as production keeps a memory, in one insert: remembering them one
    // by one checks the whole store each time.
    let keep = |user_id: i32, dated: Vec<(String, chrono::DateTime<chrono::FixedOffset>)>| {
        let db = db.clone();
        let concepts_of = &concepts_of;
        async move {
            use sea_orm::EntityTrait;
            let audience = Audience::private(user_id);
            let rows: Vec<crate::models::entities::agent_memories::ActiveModel> = dated
                .into_iter()
                .map(|(content, at)| {
                    let concepts = concepts_of.get(&content).cloned().unwrap_or(json!([]));
                    (content, concepts, at)
                })
                .map(|(content, concepts, at)| {
                    crate::models::entities::agent_memories::ActiveModel {
                        id: Set(format!("mem_{}", uuid::Uuid::new_v4().simple())),
                        user_id: Set(Some(user_id)),
                        kind: Set(MemoryKind::Fact.as_str().into()),
                        content: Set(content),
                        evidence: Set(None),
                        speaker: Set("user".into()),
                        source: Set("chat".into()),
                        venue: Set(audience.venue()),
                        audience: Set(json!(audience.members())),
                        concepts: Set(concepts),
                        importance: Set(0.5),
                        access_count: Set(0),
                        last_accessed_at: Set(None),
                        valid_from: Set(at),
                        invalid_at: Set(None),
                        invalid_reason: Set(None),
                        created_at: Set(at),
                        updated_at: Set(at),
                    }
                })
                .collect();
            for chunk in rows.chunks(200) {
                crate::models::entities::agent_memories::Entity::insert_many(chunk.to_vec())
                    .exec(&db)
                    .await
                    .unwrap();
            }
        }
    };
    let chosen: Vec<(Question, Vec<String>)> = chosen
        .into_iter()
        .filter(|(question, _)| labels.is_some() || question.question_type != "temporal-reasoning")
        .collect();
    // `MEROPE_MEMORY_SCALE_CUES=<file>`: the cues thought of for each
    // question, read when there and written after.
    let cue_path = std::env::var("MEROPE_MEMORY_SCALE_CUES").ok();
    let cue_cache: std::sync::Mutex<BTreeMap<String, String>> = std::sync::Mutex::new(
        cue_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
    );
    let outcomes: Vec<Value> = futures::stream::iter(chosen)
        .map(|(question, own)| {
            let (db, judge, lite, everyone, labels, cue_cache) =
                (db.clone(), &judge, &lite, &everyone, &labels, &cue_cache);
            let keep = &keep;
            async move {
                let alone = new_user(&db, &format!("alone-{}", question.question_id)).await;
                let among = new_user(&db, &format!("among-{}", question.question_id)).await;
                let others: Vec<String> = everyone
                    .iter()
                    .filter(|m| !own.contains(m))
                    .take(FULL.saturating_sub(own.len()))
                    .cloned()
                    .collect();
                let (own_dated, others_dated) =
                    over_half_a_year(&question.question_id, &own, &others);
                keep(alone, own_dated.clone()).await;
                keep(among, others_dated).await;
                keep(among, own_dated).await;
                // Thought of once and kept, so runs compare recall and not
                // what the cues happened to be.
                let thought = cue_cache
                    .lock()
                    .ok()
                    .and_then(|cache| cache.get(&question.question_id).cloned());
                let raw = match thought {
                    Some(raw) => Some(raw),
                    None => {
                        let raw = judge
                            .analyze_json(
                                &myriad_merope::remembering::system(),
                                &myriad_merope::remembering::input(
                                    &question.question,
                                    None,
                                    &question.question_date,
                                ),
                                myriad_merope::remembering::SCHEMA_NAME,
                                Some(&myriad_merope::remembering::schema()),
                            )
                            .await
                            .ok();
                        if let (Some(raw), Ok(mut cache)) = (&raw, cue_cache.lock()) {
                            cache.insert(question.question_id.clone(), raw.clone());
                        }
                        raw
                    }
                };
                let cues = raw.and_then(|raw| myriad_merope::remembering::parse(&raw));
                if let Some(labels) = labels {
                    let needed: Vec<&String> = labels
                        .get(&question.question_id)
                        .map(|indices| indices.iter().filter_map(|i| own.get(*i)).collect())
                        .unwrap_or_default();
                    let found = |recalled: &[String]| {
                        needed
                            .iter()
                            .filter(|memory| {
                                recalled.iter().any(|line| line.contains(memory.as_str()))
                            })
                            .count()
                    };
                    let mut row = json!({"id":question.question_id,"type":question.question_type,
                        "needed":needed.len(),"cued":cues.is_some()});
                    for (name, user_id) in [("alone", alone), ("among", among)] {
                        for (way, with) in [("plain", None), ("cued", cues.as_ref())] {
                            let recalled = recalled_for(
                                &db,
                                &question,
                                user_id,
                                &Audience::private(user_id),
                                with,
                                THOROUGH,
                            )
                            .await;
                            row[format!("{name}_{way}")] = json!(found(&recalled));
                        }
                    }
                    // Where each needed memory stands in the whole ranking,
                    // from their words and at best over the cues: just past
                    // the budget is a matter of room, far down of matching.
                    let place = |query: String| {
                        let db = db.clone();
                        async move {
                            super::store::recall_remembered_split(
                                &db,
                                among,
                                &Audience::private(among),
                                Some(&query),
                                FULL,
                                &Priming::default(),
                                0.0,
                            )
                            .await
                            .map(|(recalled, _)| recalled.named)
                            .unwrap_or_default()
                        }
                    };
                    let mut rankings = vec![place(question.question.clone()).await];
                    for cue in cues.iter().flat_map(|cues| cues.cues.iter()) {
                        rankings.push(place(cue.clone()).await);
                    }
                    let at = |ranking: &[String], memory: &str| {
                        ranking.iter().position(|line| line.contains(memory))
                    };
                    row["places"] = json!(
                        needed
                            .iter()
                            .map(|memory| json!({
                                "plain": at(&rankings[0], memory),
                                "best": rankings.iter().filter_map(|ranking| at(ranking, memory)).min(),
                            }))
                            .collect::<Vec<_>>()
                    );
                    println!("{row}");
                    return row;
                }
                let small = answer_one(
                    &db,
                    judge,
                    lite,
                    &question,
                    alone,
                    &Audience::private(alone),
                    cues.as_ref(),
                    THOROUGH,
                    false,
                    &[],
                )
                .await;
                let large = answer_one(
                    &db,
                    judge,
                    lite,
                    &question,
                    among,
                    &Audience::private(among),
                    cues.as_ref(),
                    THOROUGH,
                    false,
                    &[],
                )
                .await;
                let kept = small
                    .recalled
                    .iter()
                    .filter(|line| large.recalled.contains(line))
                    .count();
                println!(
                    "{} {} alone={:?} among={:?} kept {kept}/{}",
                    question.question_type,
                    question.question_id,
                    small.correct,
                    large.correct,
                    small.recalled.len()
                );
                json!({"id":question.question_id,"type":question.question_type,
                    "alone":small.correct,"among":large.correct,
                    "recalledAlone":small.recalled.len(),"stillRecalled":kept,
                    "responseAmong":large.answer})
            }
        })
        .buffer_unordered(AT_ONCE)
        .collect()
        .await;
    isolated.drop().await;
    if let (Some(path), Ok(cache)) = (&cue_path, cue_cache.lock()) {
        std::fs::write(path, serde_json::to_string_pretty(&*cache).unwrap()).unwrap();
    }
    if labels.is_some() {
        // Per type: memories needed, then found alone and among, from their
        // words and with cues; and questions with all of them found.
        let mut summary: BTreeMap<String, [usize; 9]> = BTreeMap::new();
        for row in &outcomes {
            let slot = summary
                .entry(row["type"].as_str().unwrap_or_default().to_string())
                .or_default();
            let needed = row["needed"].as_u64().unwrap_or(0) as usize;
            slot[0] += needed;
            for (at, key) in ["alone_plain", "alone_cued", "among_plain", "among_cued"]
                .iter()
                .enumerate()
            {
                let found = row[*key].as_u64().unwrap_or(0) as usize;
                slot[1 + at] += found;
                slot[5 + at] += usize::from(needed > 0 && found == needed);
            }
        }
        for (kind, slot) in &summary {
            println!(
                "{kind:<28} needed {:>3}  found alone {}/{}  among {}/{}  complete alone {}/{}  among {}/{}",
                slot[0], slot[1], slot[2], slot[3], slot[4], slot[5], slot[6], slot[7], slot[8]
            );
        }
        std::fs::write(
            &report_path,
            serde_json::to_string_pretty(&json!({"summary":summary,"rows":outcomes})).unwrap(),
        )
        .unwrap();
        return;
    }
    let count = |key: &str| outcomes.iter().filter(|row| row[key] == true).count();
    let (alone, among) = (count("alone"), count("among"));
    let recalled: usize = outcomes
        .iter()
        .filter_map(|row| row["recalledAlone"].as_u64())
        .sum::<u64>() as usize;
    let still: usize = outcomes
        .iter()
        .filter_map(|row| row["stillRecalled"].as_u64())
        .sum::<u64>() as usize;
    println!(
        "alone {alone} among {among} of {}; recalled lines kept {still}/{recalled}",
        outcomes.len()
    );
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&json!({"alone":alone,"among":among,"total":outcomes.len(),
            "recalledAlone":recalled,"stillRecalled":still,"rows":outcomes}))
        .unwrap(),
    )
    .unwrap();
}

/// One person's memories as they build up over half a year: `own`
/// scattered among `others` in an order `seed` fixes, each dated at its
/// place in time.
#[allow(clippy::type_complexity)]
fn over_half_a_year(
    seed: &str,
    own: &[String],
    others: &[String],
) -> (
    Vec<(String, chrono::DateTime<chrono::FixedOffset>)>,
    Vec<(String, chrono::DateTime<chrono::FixedOffset>)>,
) {
    use std::hash::{Hash, Hasher};
    let place = |text: &str| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (seed, text).hash(&mut hasher);
        hasher.finish()
    };
    let mut all: Vec<(bool, &String)> = own
        .iter()
        .map(|memory| (true, memory))
        .chain(others.iter().map(|memory| (false, memory)))
        .collect();
    all.sort_by_key(|(_, memory)| place(memory));
    let now = chrono::Utc::now().fixed_offset();
    let count = all.len().max(1) as i32;
    let step = chrono::Duration::days(180) / count;
    let (mut hers, mut theirs) = (Vec::new(), Vec::new());
    for (position, (own, memory)) in all.into_iter().enumerate() {
        let at = now - step * (count - position as i32);
        if own {
            hers.push((memory.clone(), at));
        } else {
            theirs.push((memory.clone(), at));
        }
    }
    (hers, theirs)
}

/// Before spending on a run: do the site's models answer at all, and if
/// not, what do they say. Prints each model's reply or its error chain.
#[tokio::test]
#[ignore = "calls the site's models once each"]
async fn the_models_answer() {
    let config_db = super::super::semantic_eval::load_configured_lite().await;
    config_db.close().await.ok();
    let lite = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
        Duration::from_secs(30),
    ))
    .await
    .expect("Lite model");
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(
        Duration::from_secs(30),
    ))
    .await
    .expect("judgment model");
    for (name, analyzer) in [("lite", &lite), ("judge", &judge)] {
        match analyzer
            .analyze_with_system("Reply with one word.", "ping")
            .await
        {
            Ok(reply) => println!("{name}: ok {}", reply.chars().take(40).collect::<String>()),
            Err(error) => {
                let chain: Vec<String> = error.chain().map(ToString::to_string).collect();
                println!("{name}: error {}", chain.join(" <- "));
            }
        }
    }
}
