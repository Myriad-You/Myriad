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
    naive.and_utc().fixed_offset()
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
        db, judge, lite, question, user_id, &present, None, &answering,
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
        &answering,
    )
    .await;
    Outcome {
        written: !answering.is_empty(),
        plain,
        cued,
        kept,
    }
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
    answering: &[String],
) -> Answered {
    let recalled = super::remembering::recall_with(
        db,
        user_id,
        present,
        &question.question,
        cues,
        RECALLED,
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
    .unwrap_or_default();
    let retrieved = answering
        .iter()
        .any(|fact| recalled.iter().any(|line| line.contains(fact.as_str())));
    let input = json!({
        "today": question.question_date,
        "whatYouRemember": recalled,
        "question": question.question,
    })
    .to_string();
    let answer = lite
        .analyze_with_system(ANSWER_SYSTEM, &input)
        .await
        .unwrap_or_default();
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
    let mut summary: BTreeMap<String, [usize; 6]> = BTreeMap::new();
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
        let answered = |answered: &Answered| {
            json!({"retrieved":answered.retrieved,"correct":answered.correct,
                "recalled":answered.recalled,"response":answered.answer})
        };
        rows.push(
            json!({"id":question.question_id,"type":question.question_type,
            "question":question.question,"answer":question.answer,
            "written":outcome.written,"kept":outcome.kept,
            "plain":answered(&outcome.plain),"cued":answered(&outcome.cued)}),
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
            |(kind, [n, written, plain_found, plain_right, cued_found, cued_right])| {
                (
                    kind,
                    json!({"questions":n,"written":written,
                    "plain":{"retrieved":plain_found,"correct":plain_right},
                    "cued":{"retrieved":cued_found,"correct":cued_right}}),
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
