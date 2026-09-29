//! Trying to remember before answering (see `myriad_merope::remembering`).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};

use sea_orm::DatabaseConnection;

use super::call::{self, Voice};
use super::store::{self, Recalled};
use crate::services::agent::memory::unified::{Audience, Priming};
use myriad_merope::remembering::{Cues, SCHEMA_NAME, input, merged, parse, schema, system};

/// Past this she answers from what their words alone brought back.
const THINK_WITHIN: Duration = Duration::from_secs(3);
/// What she goes through when answering needs all of it.
pub const THOROUGH: usize = 24;

/// What she would try to remember before answering `message`; billed to
/// `owner`.
async fn think(owner: i32, message: String) -> Option<Cues> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let raw = call::Ask::new(Voice::Judge, owner, "remembering")
        .within(THINK_WITHIN)
        .json_raw(
            &system(),
            &input(&message, None, &today),
            SCHEMA_NAME,
            &schema(),
        )
        .await
        .ok()?;
    parse(&raw)
}

type Thinking = Shared<BoxFuture<'static, Option<Cues>>>;

/// Trying to remember that began as a message came, by person: the
/// message, and the thinking.
static THINKING: LazyLock<Mutex<HashMap<i32, (String, Thinking)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const PEOPLE_KEPT: usize = 4096;

/// Start trying to remember as soon as their message comes, alongside how
/// it lands with her, so the reply does not wait for it after.
pub fn begin(owner: i32, message: &str) {
    let thinking: Thinking = think(owner, message.to_string()).boxed().shared();
    let running = thinking.clone();
    super::background::spawn("remembering", async move {
        running.await;
    });
    if let Ok(mut all) = THINKING.lock() {
        if all.len() >= PEOPLE_KEPT {
            all.clear();
        }
        all.insert(owner, (message.to_string(), thinking));
    }
}

/// What she tried to remember before answering `message`: begun when it
/// came, or now if it was not.
pub async fn cues(owner: i32, message: &str) -> Option<Cues> {
    let begun = THINKING
        .lock()
        .ok()
        .and_then(|mut all| all.remove(&owner))
        .filter(|(said, _)| said == message);
    match begun {
        Some((_, thinking)) => tokio::time::timeout(THINK_WITHIN, thinking)
            .await
            .ok()
            .flatten(),
        None => think(owner, message.to_string()).await,
    }
}

/// Scroll back through the private chat with `user_id` for what `query`
/// names, as a person checks what was said: the few messages that name
/// most of it, each cut to the part that does, oldest first; `asked` (what
/// they just said) is not what she is looking for. Their groups' talk is
/// the groups', not looked through here.
pub async fn look_back(
    db: &DatabaseConnection,
    user_id: i32,
    query: &str,
    asked: &str,
) -> Vec<(String, bool, String)> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SeaValue};
    const LATEST: i32 = 3000;
    const FOUND: usize = 3;
    const SHOWN_CHARS: usize = 600;
    let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.role, m.content, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND (s.context->>'venue') IS NULL \
               AND m.role IN ('user', 'assistant') \
             ORDER BY m.created_at DESC LIMIT $2",
            vec![SeaValue::Int(Some(user_id)), SeaValue::Int(Some(LATEST))],
        ))
        .await
    else {
        return Vec::new();
    };
    // Oldest first, as the chat reads.
    let mut chat: Vec<(chrono::DateTime<chrono::FixedOffset>, bool, String)> = rows
        .iter()
        .filter_map(|row| {
            let role = row.try_get::<String>("", "role").ok()?;
            let content = row
                .try_get::<String>("", "content")
                .ok()
                .filter(|content| content.trim() != asked.trim())?;
            let at = row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .ok()?;
            Some((at, role == "assistant", content))
        })
        .collect();
    chat.sort_by_key(|(at, _, _)| *at);
    let mut named: Vec<(usize, usize)> = chat
        .iter()
        .enumerate()
        .map(|(index, (_, _, content))| {
            (myriad_merope::remembering::overlap(query, content), index)
        })
        .filter(|(named, _)| *named >= 2)
        .collect();
    named.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    // Finding a line, the eye reads on to the one after it: the answer to a
    // question is the reply below it.
    let mut shown: Vec<usize> = named
        .iter()
        .take(FOUND)
        .flat_map(|(_, index)| [*index, index + 1])
        .filter(|index| *index < chat.len())
        .collect();
    shown.sort_unstable();
    shown.dedup();
    shown
        .into_iter()
        .map(|index| {
            let (at, hers, content) = &chat[index];
            let text = if myriad_merope::remembering::overlap(query, content) == 0 {
                content
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
                    .join(" / ")
                    .chars()
                    .take(SHOWN_CHARS)
                    .collect()
            } else {
                myriad_merope::remembering::excerpt(content, query, SHOWN_CHARS)
            };
            (at.format("%Y-%m-%d").to_string(), *hers, text)
        })
        .collect()
}

/// Recall for `message` as a chat turn does, and for each cue besides,
/// merged; `limit` lines, or `thorough` when answering needs all of it
/// (`THOROUGH` in production).
/// What their words brought to mind comes from their words only, and only
/// their words move what stays on her mind.
#[allow(clippy::too_many_arguments)]
pub async fn recall_with(
    db: &DatabaseConnection,
    user_id: i32,
    present: &Audience,
    message: &str,
    cues: Option<&Cues>,
    limit: usize,
    thorough: usize,
    priming: &Priming,
    breadth: f64,
) -> Result<(Recalled, Priming), anyhow::Error> {
    let limit = match cues {
        Some(cues) if cues.thorough => thorough.max(limit),
        _ => limit,
    };
    let (first, next) = store::recall_remembered_split(
        db,
        user_id,
        present,
        Some(message),
        limit,
        priming,
        breadth,
    )
    .await?;
    let Some(cues) = cues.filter(|cues| !cues.cues.is_empty()) else {
        return Ok((first, next));
    };
    let mut tries = vec![first.named];
    for cue in &cues.cues {
        let (found, _) = store::recall_remembered_split(
            db,
            user_id,
            present,
            Some(cue),
            limit,
            &Priming::default(),
            0.0,
        )
        .await?;
        tries.push(found.named);
    }
    let room = limit.saturating_sub(first.brought_to_mind.len()).max(1);
    Ok((
        Recalled {
            named: merged(&tries, room),
            brought_to_mind: first.brought_to_mind,
        },
        next,
    ))
}
