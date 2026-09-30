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
use myriad_merope::remembering::{Cues, SCHEMA_NAME, Who, input, merged, parse, schema, system};

/// Messages she reads when scrolling back: for a detail, and when answering
/// needs all of it (the amounts and names memory may have lost are there).
pub const LOOK_BACK: usize = 3;
pub const LOOK_BACK_THOROUGH: usize = 8;

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
/// message, the thinking, and whether it opened a conversation.
static THINKING: LazyLock<Mutex<HashMap<i32, (String, Thinking, bool)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const PEOPLE_KEPT: usize = 4096;

/// When each person last wrote to her, by where.
static LAST_HEARD: LazyLock<Mutex<HashMap<(i32, String), std::time::Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Coming back after this long, they start talking anew: what each has been
/// up to comes to mind, as it does when friends meet again.
const OPENS_AFTER: Duration = Duration::from_secs(3 * 3600);

/// Whether a message from `owner` at `venue` now starts talking anew, and
/// note that they wrote.
fn opens(owner: i32, venue: &str) -> bool {
    let Ok(mut heard) = LAST_HEARD.lock() else {
        return true;
    };
    if heard.len() >= PEOPLE_KEPT {
        heard.clear();
    }
    let now = std::time::Instant::now();
    let last = heard.insert((owner, venue.to_string()), now);
    last.is_none_or(|last| now.duration_since(last) >= OPENS_AFTER)
}

/// What she has in mind as she answers: what she tried to remember, and
/// whether they are only now starting to talk again.
pub struct Attention {
    pub cues: Option<Cues>,
    pub opening: bool,
}

impl Attention {
    /// Whether her own life comes to mind this turn: when the talk is about
    /// her, or they are only now starting to talk again.
    pub fn her_life(&self) -> bool {
        self.opening || self.cues.as_ref().is_some_and(|cues| cues.about_you)
    }
}

/// Start trying to remember as soon as their message comes, alongside how
/// it lands with her, so the reply does not wait for it after.
pub fn begin(owner: i32, venue: &str, message: &str) {
    let opening = opens(owner, venue);
    let thinking: Thinking = think(owner, message.to_string()).boxed().shared();
    let running = thinking.clone();
    let said = [message.to_string()];
    super::background::spawn("remembering", async move {
        // What it means, while she thinks what to look for.
        let (_, cues) = futures::join!(
            crate::services::agent::memory::meaning::warm(&said),
            running
        );
        // And what the cues mean, before recall goes through them.
        if let Some(cues) = cues {
            crate::services::agent::memory::meaning::warm(&cues.cues).await;
        }
    });
    if let Ok(mut all) = THINKING.lock() {
        if all.len() >= PEOPLE_KEPT {
            all.clear();
        }
        all.insert(owner, (message.to_string(), thinking, opening));
    }
}

/// What she has in mind before answering `message`: what she tried to
/// remember (begun when it came, or now if it was not), and whether it
/// opened a conversation (unknown counts as opening).
pub async fn cues(owner: i32, message: &str) -> Attention {
    let begun = THINKING
        .lock()
        .ok()
        .and_then(|mut all| all.remove(&owner))
        .filter(|(said, _, _)| said == message);
    match begun {
        Some((_, thinking, opening)) => Attention {
            cues: tokio::time::timeout(THINK_WITHIN, thinking)
                .await
                .ok()
                .flatten(),
            opening,
        },
        None => Attention {
            cues: think(owner, message.to_string()).await,
            opening: true,
        },
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
    found: usize,
) -> Vec<(String, Who, String)> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SeaValue};
    const LATEST: i32 = 3000;
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
        .take(found)
        .flat_map(|(_, index)| [*index, index + 1])
        .filter(|index| *index < chat.len())
        .collect();
    shown.sort_unstable();
    shown.dedup();
    let by_words: Vec<(String, Who, String)> = shown
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
            let who = if *hers { Who::You } else { Who::They };
            (at.format("%Y-%m-%d").to_string(), who, text)
        })
        .collect();
    // And, as a person does, thinking which day it was and reading that day
    // (see `chat_days`): what the words alone would miss.
    let by_day = super::chat_days::turn_back(
        db,
        super::chat_days::Place::With(user_id),
        user_id,
        query,
        asked,
    )
    .await;
    merge_found(by_words, by_day)
}

/// What was found by words and by day, once each, oldest first.
fn merge_found(
    by_words: Vec<(String, Who, String)>,
    by_day: Vec<(String, Who, String)>,
) -> Vec<(String, Who, String)> {
    let mut found = by_words;
    for (date, who, text) in by_day {
        let key: String = text.trim_start_matches("… ").chars().take(30).collect();
        let known = found.iter().any(|(was_on, _, was)| {
            *was_on == date && was.trim_start_matches("… ").starts_with(key.as_str())
        });
        if !known {
            found.push((date, who, text));
        }
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
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
    // What the cues mean, in one request rather than one each.
    crate::services::agent::memory::meaning::warm(&cues.cues).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_found_by_words_and_by_day_is_there_once_in_order() {
        let found = super::merge_found(
            vec![("2026-09-25".into(), Who::They, "继续上次的海龟汤吧".into())],
            vec![
                ("2026-09-25".into(), Who::They, "继续上次的海龟汤吧".into()),
                ("2026-09-12".into(), Who::You, "… 那盆兰草又蔫了".into()),
            ],
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, "2026-09-12");
    }

    #[test]
    fn her_own_life_comes_to_mind_when_the_moment_asks_for_it() {
        let owner = -9_001;
        assert!(opens(owner, "private"), "first words open a conversation");
        assert!(!opens(owner, "private"), "the next line goes on with it");
        assert!(opens(owner, "group:onebot:1"), "elsewhere is its own talk");
        let about = |about_you, opening| Attention {
            cues: Some(Cues {
                cues: Vec::new(),
                thorough: false,
                look_back: false,
                about_you,
            }),
            opening,
        };
        assert!(!about(false, false).her_life());
        assert!(about(true, false).her_life());
        assert!(about(false, true).her_life());
        assert!(
            !Attention {
                cues: None,
                opening: false
            }
            .her_life()
        );
    }
}
