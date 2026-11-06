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

mod answering;
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
mod weeks;
mod wishing;
mod writing;

pub(super) use writing::remember_turn;
use writing::{apply_at, keep_chat, new_user};

use answering::{Answered, answer_one, recalled_for};
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
/// What she answers on: `answer.sectioned`, with what she remembers laid
/// out after it by the core, as a chat turn lays it out
/// (`myriad_merope::speaking::format_remembered_section`), so the bench and
/// production say alike when a memory is worth bringing up. (`answer.system`
/// is the native runner's until it lays memories out the same way.)
fn answer_system(question: &Question, present: &Audience, remembered: &[String]) -> String {
    let answer = &prompts()["answer"];
    let mut system = answer["sectioned"].as_str().unwrap().to_string();
    if present.is_group() {
        system = format!("{system} {}", answer["group"].as_str().unwrap());
        if question.question_speaker.as_deref() == Some("other") {
            system = format!("{system} {}", answer["other"].as_str().unwrap());
        }
    }
    match myriad_merope::speaking::format_remembered_section(remembered) {
        Some(section) => format!("{system}\n\n{section}"),
        None => system,
    }
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

struct Outcome {
    written: bool,
    plain: Answered,
    cued: Answered,
    wide: Answered,
    /// Asked with nothing kept of them (noise questions): what the noise
    /// changed.
    bare: Option<Answered>,
    /// The wide way answered and graded by `MEROPE_MEMORY_BENCH_READER`
    /// instead, from the same memories: comparable with results published
    /// with that reader and judge.
    read: Option<Answered>,
    /// Asked where something private must not come up: whether recall in
    /// private would have brought it, so not bringing it there is the
    /// audience's doing, not a memory that was never kept.
    in_private: Option<bool>,
    memories: Vec<String>,
    /// Each memory's concepts, as kept beside it.
    concepts: Vec<Value>,
    /// The topic each memory went under, if any.
    topics: Vec<Option<String>>,
    /// Where each memory was put on her nodes, while they are written: the
    /// node's name, `about` (「关于你」), `nowhere`, or none when it was
    /// never decided.
    nodes: Vec<Option<String>>,
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
    reader: Option<&crate::services::analyzer::AiAnalyzer>,
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
        keep_chat(
            db,
            &format!("{name}-{number}"),
            user_id,
            *session_at,
            turns,
            here,
        )
        .await;
    }
    // What she already keeps of them, a week before they ask.
    if !question.memories.is_empty() {
        let updates = super::chat_remember::ChatMemoryUpdates {
            updates: question
                .memories
                .iter()
                .map(|fact| super::chat_remember::ChatMemoryUpdate {
                    standing: Default::default(),
                    fact: Some(fact.clone()),
                    supersedes: Vec::new(),
                    evidence: Some(fact.clone()),
                    concepts: Vec::new(),
                    topic: None,
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
    // Kept before she put facts under topics: sorted afterwards, as upkeep
    // does, to see what sorting gets that going over would have.
    if settle && settle::topics_later() {
        settle::sort_all(db, judge, user_id).await;
    }
    if settle && settle::topics_tidied() {
        settle::tidy_all(db, judge, user_id).await;
    }
    if settle {
        settle::keep_up_nodes(db, user_id).await;
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
    let read = match reader {
        Some(reader) => Some(
            answer_one(
                db,
                reader,
                reader,
                question,
                user_id,
                &present,
                cues.as_ref(),
                THOROUGH,
                true,
                &answering,
            )
            .await,
        ),
        None => None,
    };
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
        .await
        .0;
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
    let topic_by_id: std::collections::HashMap<String, String> = {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.memory_id, t.name FROM agent_memory_topics m \
             JOIN agent_topic_nodes t ON t.user_id = m.user_id AND t.id = m.topic_id \
             WHERE m.user_id = $1",
            [user_id.into()],
        ))
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            Some((
                row.try_get::<String>("", "memory_id").ok()?,
                row.try_get::<String>("", "name").ok()?,
            ))
        })
        .collect()
    };
    let topics: Vec<Option<String>> = kept_rows
        .iter()
        .map(|row| topic_by_id.get(&row.id).cloned())
        .collect();
    // A node merged away since keeps its id here, as the placement does.
    let node_by_id: std::collections::HashMap<String, String> = {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT p.memory_id, p.how, p.node_id, h.name FROM agent_node_placements p \
             LEFT JOIN agent_node_heads h ON h.user_id = p.user_id \
               AND h.audience = p.audience AND h.node_id = p.node_id \
             WHERE p.user_id = $1",
            [user_id.into()],
        ))
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            let how = row.try_get::<String>("", "how").ok()?;
            let node = row.try_get::<Option<String>>("", "node_id").ok()?;
            let name = row.try_get::<Option<String>>("", "name").ok()?;
            let shown = match how.as_str() {
                "node" => name.or(node)?,
                _ => how,
            };
            Some((row.try_get::<String>("", "memory_id").ok()?, shown))
        })
        .collect()
    };
    let nodes: Vec<Option<String>> = kept_rows
        .iter()
        .map(|row| node_by_id.get(&row.id).cloned())
        .collect();
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
        read,
        in_private,
        kept,
        memories,
        concepts,
        topics,
        nodes,
    }
}
