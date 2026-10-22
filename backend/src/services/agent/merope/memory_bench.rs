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
//! type; `MEROPE_MEMORY_BENCH_ONLY` a comma list of question ids;
//! `MEROPE_MEMORY_BENCH_TIMES=3` runs each question three times, each by
//! someone new, and reports pass^3 (right all three times) beside the
//! average. Writes go to a fresh schema that is dropped afterwards.
//!
//! The Chinese set adds what LongMemEval does not ask: sessions had in a
//! group and questions asked there (`haystack_venues`, `question_venue`),
//! things they asked her to forget, and everyday questions asked with what
//! she keeps of them and with nothing (`memories`). The answering and
//! grading prompts are tests/merope/memory-bench.json, which the native
//! app's runner reads too.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::TimeZone;
use futures::StreamExt;
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseBackend, Set, Statement};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::models::entities::agent_addressee_state;
use crate::services::agent::memory::unified::{Audience, Priming};

mod apart;
mod chinese;
mod days;
mod full_size;
mod longmemeval;
mod making;
mod memes;
mod probes;
mod settle;
mod since_up;
mod wishing;

use probes::*;

#[derive(Deserialize, Clone)]
struct Question {
    question_id: String,
    question_type: String,
    question: String,
    answer: Value,
    question_date: String,
    haystack_dates: Vec<String>,
    haystack_sessions: Vec<Vec<Turn>>,
    /// Where each session was had, `private` or `group:<id>`; private
    /// when not given.
    #[serde(default)]
    haystack_venues: Vec<String>,
    /// Where the question is asked; private when not given.
    #[serde(default)]
    question_venue: Option<String>,
    /// `other` when someone else in the group asks about them.
    #[serde(default)]
    question_speaker: Option<String>,
    /// What she already keeps of them, unrelated to the question (the
    /// noise questions): kept as written, nothing extracted.
    #[serde(default)]
    memories: Vec<String>,
}

impl Question {
    fn venue(&self, session: usize) -> &str {
        self.haystack_venues
            .get(session)
            .map_or("private", String::as_str)
    }
}

/// Who is present at `venue` (`private` or `group:<id>`), as she would
/// answer `user_id` there. Each run has a group of its own (`run`): in a
/// group she remembers what everyone there said, and runs share a database.
fn audience_at(venue: &str, user_id: i32, run: &str) -> Audience {
    match venue.strip_prefix("group:") {
        Some(group) => Audience::group(format!("{group}:{run}"), user_id),
        None => Audience::private(user_id),
    }
}

/// The answering and grading prompts, shared with the native app's runner
/// (tests/merope/memory-bench.json) so both ends are graded alike.
fn prompts() -> &'static Value {
    static PROMPTS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    PROMPTS.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../../../tests/merope/memory-bench.json"
        ))
        .expect("memory bench prompts")
    })
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

/// `MEROPE_MEMORY_BENCH_AT_ONCE` when set: with little credit left, the
/// gateway refuses calls whose held-back output together exceeds it.
fn at_once() -> usize {
    std::env::var("MEROPE_MEMORY_BENCH_AT_ONCE")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|at_once| *at_once > 0)
        .unwrap_or(AT_ONCE)
}

fn date_of(text: &str) -> chrono::DateTime<chrono::FixedOffset> {
    // "2023/04/10 (Mon) 17:50"
    let cleaned: String = text
        .split_whitespace()
        .filter(|part| !part.starts_with('('))
        .collect::<Vec<_>>()
        .join(" ");
    let naive = chrono::NaiveDateTime::parse_from_str(&cleaned, "%Y/%m/%d %H:%M")
        .unwrap_or_else(|_| panic!("unreadable date {text}"));
    // Their local time, as a chat's times are: in her zone.
    let zone = super::clock::zone();
    zone.from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| naive.and_utc().with_timezone(&zone))
        .fixed_offset()
}

/// As she would answer in a chat: from what she remembers of them (each
/// with the date she learned it), with what she generally knows for advice
/// or a recommendation; what she does not remember of them she does not make
/// up. In a group, she knows others read along.
fn answer_system(question: &Question, present: &Audience) -> String {
    let answer = &prompts()["answer"];
    let mut system = answer["system"].as_str().unwrap().to_string();
    if present.is_group() {
        system = format!("{system} {}", answer["group"].as_str().unwrap());
        if question.question_speaker.as_deref() == Some("other") {
            system = format!("{system} {}", answer["other"].as_str().unwrap());
        }
    }
    system
}

/// Which grading rule a question takes: by its id's ending for those that
/// must not be answered (`_abs`, `_forget`, `_private`), else by its type.
fn rule_key(question: &Question) -> &str {
    let id = question.question_id.as_str();
    if id.ends_with("_abs") {
        "abstention"
    } else if id.ends_with("_forget") {
        "forget"
    } else if id.ends_with("_private") {
        "private"
    } else {
        question.question_type.as_str()
    }
}

/// The rule itself; a type with none of its own takes the default.
fn rule_of(question: &Question) -> &'static str {
    let rules = &prompts()["grade"]["rules"];
    rules[rule_key(question)]
        .as_str()
        .or_else(|| rules["default"].as_str())
        .unwrap()
}

fn judge_system(question: &Question) -> String {
    let system = if question.question_type == "noise" {
        "noise_system"
    } else {
        "system"
    };
    prompts()["grade"][system]
        .as_str()
        .unwrap()
        .replace("{rule}", rule_of(question))
}

fn grade_schema(question: &Question) -> Value {
    if question.question_type == "noise" {
        return json!({"type":"object","additionalProperties":false,
            "required":["correct","dragged"],
            "properties":{"correct":{"type":"boolean"},"dragged":{"type":"boolean"}}});
    }
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

/// One turn of theirs written down as production writes it: what she
/// already knows, whether it is worth writing, the extraction on the judgment
/// model, then the same transaction, dated as the turn was. The updates
/// applied, if any.
pub(super) async fn remember_turn(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    user_id: i32,
    present: &Audience,
    text: &str,
    reply: &str,
    at: chrono::DateTime<chrono::FixedOffset>,
) -> Option<super::chat_remember::ChatMemoryUpdates> {
    let (existing, _) = super::store::recall_remembered_primed(
        db,
        user_id,
        present,
        Some(text),
        8,
        &Priming::default(),
        1.0,
    )
    .await
    .ok()?;
    if !super::chat_remember::should_extract_chat_remember_against(text, &existing) {
        return None;
    }
    let input = super::chat_remember::extract_input(
        text,
        reply,
        &super::TurnContext::default(),
        at.with_timezone(&super::clock::zone()),
    );
    let raw = judge
        .analyze_json(
            &myriad_merope::chat_remember::extract_system_prompt(&existing),
            &input,
            myriad_merope::chat_remember::EXTRACT_SCHEMA_NAME,
            Some(&myriad_merope::chat_remember::extract_schema()),
        )
        .await
        .ok()?;
    let updates = super::chat_remember::parse_chat_memory_updates(&raw, text, reply, &existing)?;
    apply_at(db, user_id, present, at, &updates).await?;
    Some(updates)
}

/// Keep `updates` as production does, dated `at`; none when nothing was kept.
async fn apply_at(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &Audience,
    at: chrono::DateTime<chrono::FixedOffset>,
    updates: &super::chat_remember::ChatMemoryUpdates,
) -> Option<()> {
    said_at(db, user_id, at).await;
    let before = chrono::Utc::now().fixed_offset();
    let transaction = sea_orm::TransactionTrait::begin(db).await.ok()?;
    // Written as kept for good: the bench compares ways of going over talk,
    // with the short notes' fading stood in for by SHORT_NOTES_LAST.
    let applied = super::store::apply_chat_memory_updates_on(
        &transaction,
        user_id,
        at,
        updates,
        present,
        super::store::Keep::Lasting,
    )
    .await
    .unwrap_or(false);
    transaction.commit().await.ok()?;
    if !applied {
        return None;
    }
    // Dated as the turn was, not as the bench ran.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = $1, valid_from = $1, updated_at = $1 \
         WHERE user_id = $2 AND created_at >= $3",
        [at.into(), user_id.into(), before.into()],
    ))
    .await
    .ok()?;
    Some(())
}

struct Outcome {
    written: bool,
    plain: Answered,
    cued: Answered,
    wide: Answered,
    /// Asked with nothing kept of them (noise questions): what the noise
    /// changed.
    bare: Option<Answered>,
    /// Asked where something private must not come up: whether recall in
    /// private would have brought it, so not bringing it there is the
    /// audience's doing, not a memory that was never kept.
    in_private: Option<bool>,
    memories: Vec<String>,
    /// Each memory's concepts, as kept beside it.
    concepts: Vec<Value>,
    kept: usize,
}

/// A session of the question: when, its turns, and who was present.
type Session<'a> = (
    chrono::DateTime<chrono::FixedOffset>,
    &'a Vec<Turn>,
    Audience,
);

async fn run_one(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    lite: &crate::services::analyzer::AiAnalyzer,
    question: &Question,
    round: usize,
) -> Outcome {
    let name = format!("lme-{}-{round}", question.question_id);
    let user_id = new_user(db, &name).await;
    let present = audience_at(
        question.question_venue.as_deref().unwrap_or("private"),
        user_id,
        &name,
    );
    let mut answering: Vec<String> = Vec::new();
    let mut kept = 0;
    let mut sessions: Vec<Session> = question
        .haystack_dates
        .iter()
        .map(|date| date_of(date))
        .zip(question.haystack_sessions.iter())
        .enumerate()
        .map(|(number, (at, turns))| {
            (
                at,
                turns,
                audience_at(question.venue(number), user_id, &name),
            )
        })
        .collect();
    sessions.sort_by_key(|(at, _, _)| *at);
    // The chat itself, as the site keeps it, for scrolling back through.
    for (number, (session_at, turns, here)) in sessions.iter().enumerate() {
        let session = format!("{name}-{number}");
        let context = here
            .group_id()
            .map(|group| json!({"mode": "chat", "venue": group}));
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_sessions (id, user_id, message_count, archived, created_at, last_active_at, context) \
             VALUES ($1, $2, $3, false, $4, $4, $5)",
            [
                session.clone().into(),
                user_id.into(),
                (turns.len() as i32).into(),
                (*session_at).into(),
                context.into(),
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
    // What she already keeps of them, a week before they ask.
    if !question.memories.is_empty() {
        let updates = super::chat_remember::ChatMemoryUpdates {
            updates: question
                .memories
                .iter()
                .map(|fact| super::chat_remember::ChatMemoryUpdate {
                    fact: Some(fact.clone()),
                    supersedes: Vec::new(),
                    evidence: Some(fact.clone()),
                    concepts: Vec::new(),
                    cues: Vec::new(),
                })
                .collect(),
            ..Default::default()
        };
        let at = date_of(&question.question_date) - chrono::Duration::days(7);
        if apply_at(db, user_id, &Audience::private(user_id), at, &updates)
            .await
            .is_some()
        {
            kept += question.memories.len();
        }
    }
    let settle = std::env::var("MEROPE_MEMORY_BENCH_WAY").as_deref() == Ok("settle");
    if settle {
        settle::go_over(
            db,
            judge,
            user_id,
            &sessions,
            &question.question_date,
            &mut answering,
            &mut kept,
        )
        .await;
    }
    for (session_at, turns, here) in sessions.iter().filter(|_| !settle) {
        for (index, turn) in turns.iter().enumerate() {
            if turn.role != "user" {
                continue;
            }
            let reply = turns
                .get(index + 1)
                .filter(|next| next.role == "assistant")
                .map(|next| next.content.as_str())
                .unwrap_or("");
            let at = *session_at + chrono::Duration::minutes(index as i64);
            let text: String = turn
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if text.chars().count() > 2_000 {
                continue;
            }
            let Some(updates) = remember_turn(db, judge, user_id, here, &text, reply, at).await
            else {
                continue;
            };
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
    let bare = if question.memories.is_empty() {
        None
    } else {
        let nobody = new_user(db, &format!("{name}-bare")).await;
        Some(
            answer_one(
                db,
                judge,
                lite,
                question,
                nobody,
                &Audience::private(nobody),
                cues.as_ref(),
                THOROUGH,
                true,
                &[],
            )
            .await,
        )
    };
    let in_private = if present.is_group() && question.question_id.ends_with("_private") {
        let recalled = recalled_for(
            db,
            question,
            user_id,
            &Audience::private(user_id),
            cues.as_ref(),
            THOROUGH,
        )
        .await;
        Some(
            answering
                .iter()
                .any(|fact| recalled.iter().any(|line| line.contains(fact.as_str()))),
        )
    } else {
        None
    };
    // Everything she kept, to tell unwritten from unfound afterwards: in
    // each place she kept something.
    let mut places: Vec<Audience> = Vec::new();
    for (_, _, here) in &sessions {
        if !places.contains(here) {
            places.push(here.clone());
        }
    }
    if places.is_empty() {
        places.push(Audience::private(user_id));
    }
    let mut kept_rows = Vec::new();
    for place in &places {
        for row in crate::services::agent::memory::unified::active_in(
            db,
            user_id,
            place,
            &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
        )
        .await
        .unwrap_or_default()
        {
            if !kept_rows.iter().any(
                |kept: &crate::services::agent::memory::unified::MemoryRecord| kept.id == row.id,
            ) {
                kept_rows.push(row);
            }
        }
    }
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
        bare,
        in_private,
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
    /// Noise questions: whether the answer brought in what she keeps of
    /// them where the question did not call for it.
    dragged: Option<bool>,
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
