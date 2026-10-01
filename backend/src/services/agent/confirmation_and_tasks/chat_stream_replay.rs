//! Her real private talk, answered again by the build as it is now, and the
//! shapes that made her read as a performance rather than someone (see the
//! 2026-10-01 review) counted for both what she said then and what she says
//! now: replies that end by telling them what to do, exclamation marks,
//! length, words from her own notes, and openings that repeat. Counted in
//! code, so no reviewer is needed and a prompt change can be compared on
//! the same real inputs.
//!
//! Read only (the site's database), and it asks the chat model once per
//! turn. What she knows now (her day, her memory, the hour) is today's, not
//! what it was when the turn happened: it compares how she answers the same
//! words, not a rerun of the past.

use super::*;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::{Value, json};

/// Words that, at the end of a reply, tell them what to do or hurry them.
const URGES: [&str; 16] = [
    "赶紧",
    "快说",
    "快去",
    "快听",
    "快来",
    "快点",
    "快快",
    "快发",
    "记得",
    "小心",
    "别忘",
    "早点睡",
    "多喝",
    "歇会",
    "告诉我",
    "跟我说说",
];
/// Words her own notes lean on, which read as jargon in talk.
const NOTE_WORDS: [&str; 4] = ["撞", "劲", "笼子", "不是这个"];

fn said_lines(reply: &str) -> Vec<&str> {
    reply
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("[["))
        .collect()
}

#[derive(Default)]
struct Shape {
    replies: usize,
    urging: usize,
    bangs: usize,
    lines: usize,
    chars: Vec<usize>,
    note_words: usize,
}

impl Shape {
    fn add(&mut self, reply: &str) {
        let lines = said_lines(reply);
        if lines.is_empty() {
            return;
        }
        self.replies += 1;
        if lines
            .last()
            .is_some_and(|last| URGES.iter().any(|urge| last.contains(urge)))
        {
            self.urging += 1;
        }
        self.bangs += reply.matches(['！', '!']).count();
        self.lines += lines.len();
        self.chars
            .push(lines.iter().map(|line| line.chars().count()).sum());
        if NOTE_WORDS.iter().any(|word| reply.contains(word)) {
            self.note_words += 1;
        }
    }

    fn line(&self) -> String {
        let n = self.replies.max(1) as f64;
        let mut chars = self.chars.clone();
        chars.sort_unstable();
        let median = chars.get(chars.len() / 2).copied().unwrap_or(0);
        format!(
            "replies {}  urging-end {:.0}%  bangs/reply {:.1}  lines/reply {:.1}  chars median {}  note-words {:.0}%",
            self.replies,
            self.urging as f64 / n * 100.0,
            self.bangs as f64 / n,
            self.lines as f64 / n,
            median,
            self.note_words as f64 / n * 100.0,
        )
    }
}

async fn history_before(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    before: chrono::DateTime<chrono::FixedOffset>,
) -> Vec<crate::services::agent::ConversationMessage> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.role, m.content, m.metadata, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
               AND m.created_at < $2 ORDER BY m.created_at DESC LIMIT 20",
            [user_id.into(), before.into()],
        ))
        .await
        .unwrap();
    rows.iter()
        .rev()
        .map(|row| {
            let at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "created_at").unwrap();
            crate::services::agent::chat_prompt::reconstruct_conversation_message(
                row.try_get("", "role").unwrap(),
                row.try_get("", "content").unwrap(),
                Some(at.to_rfc3339()),
                row.try_get::<Option<Value>>("", "metadata")
                    .unwrap()
                    .as_ref(),
                true,
            )
        })
        .collect()
}

/// MEROPE_REPLAY_N real turns (default 16), the latest, answered again;
/// MEROPE_REPLAY_REPORT=<file> keeps each turn's words, then and now.
#[tokio::test]
#[ignore = "reads the site's database and asks its model"]
async fn her_real_talk_answered_again() {
    let db = crate::services::agent::semantic_eval::load_configured_lite().await;
    crate::services::process_db::set_process_database(db.clone());
    let n: i64 = std::env::var("MEROPE_REPLAY_N")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(16);
    let turns = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "WITH t AS (SELECT s.user_id, m.session_id, m.role, m.content, m.created_at, \
               lead(m.role) OVER w AS next_role, lead(m.content) OVER w AS next_content \
               FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
               WHERE s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
               WINDOW w AS (PARTITION BY s.user_id ORDER BY m.created_at)) \
             SELECT user_id, session_id, content, created_at, next_content FROM t \
             WHERE role = 'user' AND next_role = 'assistant' ORDER BY created_at DESC LIMIT $1",
            [n.into()],
        ))
        .await
        .unwrap();
    let analyzer = chat_analyzer().await.expect("chat model");
    let (mut then, mut now) = (Shape::default(), Shape::default());
    let (mut openers_then, mut openers_now) = (Vec::new(), Vec::new());
    let mut report = Vec::new();
    for turn in turns.iter().rev() {
        let user_id: i32 = turn.try_get("", "user_id").unwrap();
        let session: String = turn.try_get("", "session_id").unwrap();
        let said: String = turn.try_get("", "content").unwrap();
        let at: chrono::DateTime<chrono::FixedOffset> = turn.try_get("", "created_at").unwrap();
        let original: String = turn.try_get("", "next_content").unwrap();
        let history = history_before(&db, user_id, at).await;
        let opening = history
            .last()
            .and_then(|last| last.created_at.as_deref())
            .and_then(|last| chrono::DateTime::parse_from_rfc3339(last).ok())
            .is_none_or(|last| (at - last).num_hours() >= 3);
        let request = UserRequest {
            raw_input: said.clone(),
            timestamp: chrono::Utc::now(),
            user_id,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                session_id: Some(session),
                conversation_history: Some(history),
                ..Default::default()
            }),
        };
        crate::services::agent::merope::remembering::begin(user_id, "private", &said);
        let prompt = Agent::new(db.clone())
            .await
            .chat_response_prompt(&request)
            .await;
        let again = analyzer
            .analyze_stream(&prompt, |_| true)
            .await
            .unwrap_or_default();
        then.add(&original);
        now.add(&again);
        if opening {
            openers_then.push(original.clone());
            openers_now.push(again.clone());
        }
        println!(
            "\n> {said}\n  then: {}\n  now:  {}",
            original.replace('\n', " / "),
            again.replace('\n', " / ")
        );
        report.push(json!({ "said": said, "at": at.to_rfc3339(), "opening": opening, "then": original, "now": again }));
    }
    println!("\nthen  {}", then.line());
    println!("now   {}", now.line());
    let leaning = |openers: &[String]| myriad_merope::vitals::leaned_on(openers, 0.3, 3);
    println!(
        "openers {}: then {:?}  now {:?}",
        openers_now.len(),
        leaning(&openers_then),
        leaning(&openers_now)
    );
    if let Ok(path) = std::env::var("MEROPE_REPLAY_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
}
