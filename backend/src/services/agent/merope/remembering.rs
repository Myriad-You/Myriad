//! Trying to remember before answering (see `myriad_merope::remembering`).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
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

/// Past this she answers from what their words alone brought back. Only
/// words that reach back to something before wait for it at all.
const THINK_WITHIN: Duration = Duration::from_secs(6);
/// What she goes through when answering needs all of it.
pub const THOROUGH: usize = 24;
/// Not going through all of it, the lines each try brings of its own
/// before the rest are ranked together, so each thing their words ask
/// about has a few; and what a turn asking of several goes through, at
/// most. One cue beside their words is the turn's eight as ever.
const EACH_TRY: usize = 4;
const SEVERAL: usize = 16;

/// What each try to remember came to since the site started: cues, none,
/// an answer that does not read, no answer.
#[cfg(not(test))]
fn tries<T>(of: impl FnOnce(&[AtomicU64; 4]) -> T) -> T {
    static TRIES: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
    of(&TRIES)
}
/// Each test's own.
#[cfg(test)]
fn tries<T>(of: impl FnOnce(&[AtomicU64; 4]) -> T) -> T {
    thread_local!(static TRIES: [AtomicU64; 4] = const { [const { AtomicU64::new(0) }; 4] });
    TRIES.with(of)
}

/// [`tries`] so far, for her status. Counts only.
pub(crate) fn counts() -> serde_json::Value {
    let [cues, empty, unreadable, unanswered] =
        tries(|all| all.each_ref().map(|one| one.load(Relaxed)));
    serde_json::json!({
        "cues": cues, "empty": empty, "unreadable": unreadable, "unanswered": unanswered,
    })
}

/// What to remember, asked of `voice`. Its schema is in the prompt alone:
/// held to it by the provider, 53 of 58 messages with something to look for
/// got no cues, and 0 of 58 so (2026-11-24).
fn asking(voice: Voice, owner: i32) -> call::Ask {
    call::Ask::new(voice, owner, "remembering").shape_in_prompt()
}

/// What she would try to remember before answering `message`; billed to
/// `owner`.
async fn think(owner: i32, message: String) -> Option<Cues> {
    let today = super::clock::today().format("%Y-%m-%d").to_string();
    let raw = asking(Voice::Judge, owner)
        .within(THINK_WITHIN)
        .json_raw(
            &system(),
            &input(&message, None, &today),
            SCHEMA_NAME,
            &schema(),
        )
        .await;
    let cues = raw.as_ref().ok().and_then(|raw| parse(raw));
    let came_to = match (&raw, &cues) {
        (Err(_), _) => 3,
        (Ok(_), None) => 2,
        (Ok(_), Some(cues)) => usize::from(cues.cues.is_empty()),
    };
    tries(|all| all[came_to].fetch_add(1, Relaxed));
    cues
}

type Thinking = Shared<BoxFuture<'static, Option<Cues>>>;

/// Trying to remember that began as a message came, by person and where
/// (the same person may be talking to her in private and in a group at
/// once): the message, the thinking, and whether it opened a conversation.
static THINKING: LazyLock<Mutex<HashMap<(i32, String), (String, Thinking, bool)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const PEOPLE_KEPT: usize = 4096;

/// Her persona cleared: what is being thought of for the last her goes.
pub(super) fn forget() {
    if let Ok(mut thinking) = THINKING.lock() {
        thinking.clear();
    }
    if let Ok(mut heard) = LAST_HEARD.lock() {
        heard.clear();
    }
}

/// When each person last wrote to her, by where.
static LAST_HEARD: LazyLock<Mutex<HashMap<(i32, String), std::time::Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Coming back after this long, they start talking anew: what each has been
/// up to comes to mind, as it does when friends meet again.
const OPENS_AFTER: Duration = Duration::from_secs(3 * 3600);

/// When `owner` last wrote at `venue` before this message, when she does not
/// have it in mind (after a restart): from the conversation kept, so a talk
/// going on is not taken for one starting anew.
pub async fn recall_last_heard(db: &DatabaseConnection, owner: i32, venue: &str) {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let known = LAST_HEARD
        .lock()
        .is_ok_and(|heard| heard.contains_key(&(owner, venue.to_string())));
    if known || owner <= 0 {
        return;
    }
    let (clause, value): (&str, sea_orm::Value) = match venue.strip_prefix("group:") {
        Some(group) => ("s.context->>'venue' = $2", group.into()),
        None => (
            "(s.context->>'venue') IS NULL AND $2::text IS NULL",
            Option::<String>::None.into(),
        ),
    };
    // The message now being answered is already kept: the one before it.
    let sql = format!(
        "SELECT m.created_at FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE (s.context->>'past') IS NULL AND s.user_id = $1 AND m.role = 'user' AND s.context->>'mode' = 'chat' AND {clause} \
         ORDER BY m.created_at DESC OFFSET 1 LIMIT 1"
    );
    let Ok(Some(row)) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            [owner.into(), value],
        ))
        .await
    else {
        return;
    };
    let Ok(at) = row.try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at") else {
        return;
    };
    let ago = (crate::clock::now() - at.with_timezone(&chrono::Utc))
        .to_std()
        .unwrap_or_default();
    if let (Some(then), Ok(mut heard)) = (
        std::time::Instant::now().checked_sub(ago),
        LAST_HEARD.lock(),
    ) {
        heard.entry((owner, venue.to_string())).or_insert(then);
    }
}

/// Whether a message from `owner` at `venue` now starts talking anew, and
/// note that they wrote.
/// A simulated day later: when they last wrote is read again from what was
/// kept, as after a restart.
#[cfg(test)]
pub(crate) fn forget_heard(owner: i32, venue: &str) {
    if let Ok(mut heard) = LAST_HEARD.lock() {
        heard.remove(&(owner, venue.to_string()));
    }
}

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
    /// Their words ask about her, as they read.
    pub about_her: bool,
}

impl Attention {
    /// Whether her own life comes to mind this turn: when the talk is about
    /// her, or they are only now starting to talk again.
    pub fn her_life(&self) -> bool {
        self.opening || self.about_her || self.cues.as_ref().is_some_and(|cues| cues.about_you)
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
        all.insert(
            (owner, venue.to_string()),
            (message.to_string(), thinking, opening),
        );
    }
}

/// What she has in mind before answering `message`: what she tried to
/// remember (begun when it came, or now if it was not), and whether it
/// opened a conversation (unknown counts as opening).
///
/// Only when their words reach back to something before does she wait to
/// recall (it takes a couple of seconds); otherwise she answers from what
/// their words bring back at once, and what she was thinking of is used if
/// it is already there.
pub async fn cues(owner: i32, venue: &str, message: &str) -> Attention {
    let reaching_back = myriad_merope::remembering::reaches_back(message);
    let about_her = myriad_merope::remembering::about_her(message);
    let begun = THINKING
        .lock()
        .ok()
        .and_then(|mut all| all.remove(&(owner, venue.to_string())))
        .filter(|(said, _, _)| said == message);
    match begun {
        Some((_, thinking, opening)) => Attention {
            cues: if reaching_back {
                tokio::time::timeout(THINK_WITHIN, thinking)
                    .await
                    .ok()
                    .flatten()
            } else {
                thinking.now_or_never().flatten()
            },
            opening,
            about_her,
        },
        None => Attention {
            cues: if reaching_back {
                think(owner, message.to_string()).await
            } else {
                None
            },
            opening: true,
            about_her,
        },
    }
}

/// Scroll back through the private chat with `user_id` for what `query`
/// names, as a person checks what was said: the few messages that name
/// most of it, each cut to the part that does, oldest first; `asked` (what
/// they just said) is not what she is looking for, and neither is the rest
/// of the talk still in front of her: found again, it would read as
/// something said before. Their groups' talk is the groups', not looked
/// through here.
pub async fn look_back(
    db: &DatabaseConnection,
    user_id: i32,
    query: &str,
    asked: &str,
    found: usize,
) -> Vec<(String, Who, String)> {
    const SHOWN_CHARS: usize = 600;
    let in_front = in_front(db, user_id).await;
    // The lines a chat turn may look through (see `chat_lines`): not a
    // group's, not a piece of work, not an archived talk, nothing they took
    // back. Oldest first, as the chat reads.
    let lines: Vec<super::chat_lines::Line> = super::chat_lines::private_lines(db, user_id, 3000)
        .await
        .into_iter()
        .filter(|line| !in_front.contains(&line.id) && line.content.trim() != asked.trim())
        .collect();
    // Which of them a later fact took the place of: said where it is shown.
    let replaced: Vec<bool> = lines.iter().map(|line| line.replaced).collect();
    let chat: Vec<(chrono::DateTime<chrono::FixedOffset>, bool, String)> = lines
        .into_iter()
        .map(|line| (line.at, line.hers, line.content))
        .collect();
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
    // Of what she said, the one that most names it is read whole: asked what
    // exactly she gave (the fifth bottle, the seventh job), a person reads
    // their own long message through, not the few lines that matched. Her
    // other messages found are read through too, only less of a long one:
    // a list she gave cut after its second item hides what came last.
    let whole = named.first().and_then(|(_, index)| {
        [*index, index + 1]
            .into_iter()
            .find(|index| chat.get(*index).is_some_and(|(_, hers, _)| *hers))
    });
    let by_words: Vec<(String, Who, String)> = shown
        .into_iter()
        .map(|index| {
            let (at, hers, content) = &chat[index];
            let text = if whole == Some(index) {
                her_reply(content, query, READ_WHOLE_CHARS)
            } else if *hers {
                her_reply(content, query, HERS_CHARS)
            } else if myriad_merope::remembering::overlap(query, content) == 0 {
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
            let day = super::chat_lines::day(at);
            (super::chat_lines::found_on(day, replaced[index]), who, text)
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

/// One of her messages read through, up to `most`: whole when it fits;
/// longer, what it opens with, the lines that name `query` and how it
/// ends, with what lies between left out, as a person skims their own long
/// reply: a list cut at its tail would lose its last items, the ones most
/// often asked after.
pub(in crate::services::agent::merope) fn her_reply(
    content: &str,
    query: &str,
    most: usize,
) -> String {
    const AROUND: usize = 2;
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let whole = lines.join(" / ");
    if whole.chars().count() <= most {
        return whole;
    }
    let best = lines
        .iter()
        .enumerate()
        .max_by_key(|(index, line)| {
            (
                myriad_merope::remembering::overlap(query, line),
                std::cmp::Reverse(*index),
            )
        })
        .map_or(0, |(index, _)| index);
    let mut kept = vec![false; lines.len()];
    for keep in &mut kept[best.saturating_sub(AROUND)..(best + AROUND + 1).min(lines.len())] {
        *keep = true;
    }
    if skimmed(&lines, &kept).chars().count() > most {
        // Even the lines that name it are too long: its start and its end.
        let chars: Vec<char> = whole.chars().collect();
        let half = most / 2;
        return format!(
            "{} … {}",
            chars[..half].iter().collect::<String>(),
            chars[chars.len() - half..].iter().collect::<String>()
        );
    }
    // From its start and its end in turn, a line at a time, while it fits.
    let (mut from_start, mut start_open, mut end_open) = (true, true, true);
    while start_open || end_open {
        let at_start = start_open && (from_start || !end_open);
        let next = if at_start {
            kept.iter().position(|keep| !keep)
        } else {
            kept.iter().rposition(|keep| !keep)
        };
        let Some(next) = next else {
            break;
        };
        kept[next] = true;
        if skimmed(&lines, &kept).chars().count() > most {
            kept[next] = false;
            if at_start {
                start_open = false;
            } else {
                end_open = false;
            }
        }
        from_start = !from_start;
    }
    skimmed(&lines, &kept)
}

/// The kept lines of a message, " / " between lines read on, " … " where
/// lines were left out.
fn skimmed(lines: &[&str], kept: &[bool]) -> String {
    let mut out = String::new();
    let mut left_out = false;
    for (line, keep) in lines.iter().zip(kept) {
        if !keep {
            left_out = true;
            continue;
        }
        if out.is_empty() {
            if left_out {
                out.push_str("… ");
            }
        } else {
            out.push_str(if left_out { " … " } else { " / " });
        }
        out.push_str(line);
        left_out = false;
    }
    if left_out {
        out.push_str(" …");
    }
    out
}

/// Her other messages found looking back are read this far at most: a list
/// of a dozen items with a line on each fits.
pub(in crate::services::agent::merope) const HERS_CHARS: usize = 1_500;

/// Her one long message read whole is read this far at most: longer than
/// nearly every reply (the longest in a long chat test is under 5,000).
pub(in crate::services::agent::merope) const READ_WHOLE_CHARS: usize = 4_000;

/// Ids of the private chat messages a chat turn has in front of her: the
/// latest `PRIVATE_CHAT_IN_FRONT` loaded as history, and the message just
/// kept after that history was read. Nothing when it cannot be read.
pub(in crate::services::agent::merope) async fn in_front(
    db: &DatabaseConnection,
    user_id: i32,
) -> std::collections::HashSet<i32> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SeaValue};
    let shown = crate::services::agent::sessions::PRIVATE_CHAT_IN_FRONT as i64 + 1;
    let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE (s.context->>'past') IS NULL AND s.user_id = $1 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
               AND NOT s.archived AND m.role IN ('user', 'assistant') \
             ORDER BY m.created_at DESC LIMIT $2",
            vec![SeaValue::Int(Some(user_id)), SeaValue::BigInt(Some(shown))],
        ))
        .await
    else {
        return Default::default();
    };
    rows.iter()
        .filter_map(|row| row.try_get::<i32>("", "id").ok())
        .collect()
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
    found.sort_by(|left, right| {
        super::chat_lines::day_of(&left.0).cmp(super::chat_lines::day_of(&right.0))
    });
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
    let (mut recalled, next) = ranked_with(
        db, user_id, present, message, cues, limit, thorough, priming, breadth,
    )
    .await?;
    let named = &recalled.named;
    recalled.said.retain(|line, _| named.contains(line));
    Ok((recalled, next))
}

/// Of `named`, what only filled a try (`filled`) and no try found by what
/// it says (`found`).
fn only_filling(named: &[String], filled: &[String], found: &[String]) -> Vec<String> {
    named
        .iter()
        .filter(|line| filled.contains(line) && !found.contains(line))
        .cloned()
        .collect()
}

/// Of `tries`, the first `each` lines of every try that it found by what
/// they say (`found`), as they stand merged; then the rest as merged, up to
/// `room` in all.
fn shared_out(tries: &[Vec<String>], found: &[String], each: usize, room: usize) -> Vec<String> {
    let ranked = merged(tries, usize::MAX);
    let own: HashSet<&String> = tries
        .iter()
        .flat_map(|tried| tried.iter().filter(|line| found.contains(line)).take(each))
        .collect();
    let mut out: Vec<String> = ranked
        .iter()
        .filter(|line| own.contains(line))
        .take(room)
        .cloned()
        .collect();
    for line in &ranked {
        if out.len() >= room {
            break;
        }
        if !out.contains(line) {
            out.push(line.clone());
        }
    }
    out
}

/// [`recall_with`] before their words are cut to what it named: what their
/// words and each cue found, merged and ranked, each line with the memory
/// it is (`Recalled::ids`); going through all of it, only what something
/// named, and their words beside every line any try found (`said`, for
/// what is added after). Her nodes go through all of a kind their own way
/// (`memory_nodes::recall`).
#[allow(clippy::too_many_arguments)]
pub async fn ranked_with(
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
    let Some(cues) = cues.filter(|cues| cues.thorough || !cues.cues.is_empty()) else {
        return Ok((first, next));
    };
    // What the cues mean, in one request rather than one each.
    crate::services::agent::memory::meaning::warm(&cues.cues).await;
    // Going through all of it, she goes through only what something names:
    // the recent rows filling a try that named nothing do not bear on it,
    // and would take the room of what does.
    let named_of = |recalled: &Recalled| -> Vec<String> {
        if !cues.thorough {
            return recalled.named.clone();
        }
        recalled
            .named
            .iter()
            .filter(|line| !recalled.recent.contains(line))
            .cloned()
            .collect()
    };
    // A line any try found by what it says is found, whatever another try
    // only filled with it: else, many cues each naming one of a kind leave
    // every one of them only filling, and none picks the kind it is of.
    let found_by_words = |recalled: &Recalled| -> Vec<String> {
        recalled
            .named
            .iter()
            .filter(|line| !recalled.recent.contains(line))
            .cloned()
            .collect()
    };
    let mut tries = vec![named_of(&first)];
    let mut found = found_by_words(&first);
    let mut recent = first.recent.clone();
    let mut ids = first.ids.clone();
    let mut said = first.said.clone();
    for cue in &cues.cues {
        let (cue_found, _) = store::recall_remembered_split(
            db,
            user_id,
            present,
            Some(cue),
            limit,
            &Priming::default(),
            0.0,
        )
        .await?;
        tries.push(named_of(&cue_found));
        found.extend(found_by_words(&cue_found));
        recent.extend(cue_found.recent);
        ids.extend(cue_found.ids);
        said.extend(cue_found.said);
    }
    if tries.iter().all(Vec::is_empty) {
        tries = vec![first.named.clone()];
    }
    let named = if cues.thorough {
        merged(
            &tries,
            limit.saturating_sub(first.brought_to_mind.len()).max(1),
        )
    } else {
        let room = (EACH_TRY * tries.len()).clamp(limit, SEVERAL.max(limit));
        shared_out(
            &tries,
            &found,
            EACH_TRY,
            room.saturating_sub(first.brought_to_mind.len()).max(1),
        )
    };
    let recent = only_filling(&named, &recent, &found);
    let named_ids: Vec<String> = named
        .iter()
        .filter_map(|line| ids.iter().find(|(seen, _)| seen == line))
        .map(|(_, id)| id.clone())
        .collect();
    // Going through all of it, their words are kept for every line found:
    // what her nodes add may be one of them.
    if !cues.thorough {
        said.retain(|line, _| named.contains(line));
    }
    Ok((
        Recalled {
            named,
            brought_to_mind: first.brought_to_mind,
            recent,
            ids: ids
                .into_iter()
                .filter(|(_, id)| named_ids.contains(id))
                .collect(),
            said,
        },
        next,
    ))
}

#[cfg(test)]
#[path = "remembering_tests.rs"]
mod asked_tests;

#[cfg(test)]
mod tests {
    /// Her long message is read through to its end; a list she gave is no
    /// longer cut after the lines that matched (the last of the venues she
    /// named was out of sight); past what fits, the middle is left out, not
    /// the end, and the lines that name what is looked for stay.
    #[test]
    fn her_long_message_is_read_whole() {
        let list: String = (1..=30)
            .map(|n| format!("{n}. a bottle to buy, number {n}\n"))
            .collect::<String>()
            + "31. Absinthe";
        let cut = myriad_merope::remembering::excerpt(&list, "bottle", 600);
        assert!(!cut.contains("Absinthe"));
        assert!(
            super::her_reply(&list, "bottle", super::READ_WHOLE_CHARS).ends_with("31. Absinthe")
        );
        let venues = "Sure! Here are some popular venues in Portland that host indie music shows:\n\
                      1. Mississippi Studios\n2. Doug Fir Lounge\n3. Holocene\n4. Wonder Ballroom\n5. Revolution Hall";
        assert!(
            myriad_merope::remembering::excerpt(
                venues,
                "popular venues Portland indie music shows",
                600
            )
            .ends_with(" …")
        );
        assert_eq!(
            super::her_reply(
                venues,
                "popular venues Portland indie music shows",
                super::HERS_CHARS
            ),
            venues
                .lines()
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(" / ")
        );
        // Too long: its start, the line asked about, and its end.
        let long: String = (1..=200)
            .map(|n| format!("{n}. line number {n} of a long reply\n"))
            .collect();
        let long = long.replace("100. line number 100", "100. the Absinthe bottle");
        let skimmed = super::her_reply(&long, "Absinthe", 1_500);
        assert!(
            skimmed.chars().count() <= 1_500,
            "{}",
            skimmed.chars().count()
        );
        assert!(skimmed.starts_with("1. line number 1 of"));
        assert!(skimmed.contains("100. the Absinthe bottle"));
        assert!(skimmed.ends_with("200. line number 200 of a long reply"));
        assert_eq!(skimmed.matches(" … ").count(), 2);
        // One line too long to skim by lines: its start and its end.
        let one = "很长的一句".repeat(2_000);
        let ends = super::her_reply(&one, "长", 600);
        assert!(ends.chars().count() <= 603 && ends.starts_with("很长") && ends.ends_with("一句"));
    }

    use super::*;

    fn lines(prefix: &str, n: usize) -> Vec<String> {
        (0..n).map(|i| format!("{prefix}{i}")).collect()
    }

    #[test]
    fn each_thing_asked_about_has_lines_of_its_own() {
        let tries = vec![lines("m", 8), lines("a", 8), lines("b", 8), lines("c", 8)];
        let found: Vec<String> = tries.concat();
        let room = (EACH_TRY * tries.len()).clamp(8, SEVERAL);
        let named = shared_out(&tries, &found, EACH_TRY, room);
        assert_eq!(named.len(), 16);
        for prefix in ["m", "a", "b", "c"] {
            assert_eq!(
                named.iter().filter(|line| line.starts_with(prefix)).count(),
                4,
                "{prefix}"
            );
        }
    }

    #[test]
    fn one_cue_beside_their_words_is_eight_and_fillers_only_fill() {
        let tries = vec![lines("m", 8), vec!["m0".into(), "x0".into(), "x1".into()]];
        // x1 only filled its try: it has no room of its own.
        let found: Vec<String> = lines("m", 8).into_iter().chain(["x0".into()]).collect();
        let room = (EACH_TRY * tries.len()).clamp(8, SEVERAL);
        let named = shared_out(&tries, &found, EACH_TRY, room);
        assert_eq!(named.len(), 8);
        assert_eq!(named[0], "m0");
        assert!(named.contains(&"x0".to_string()));
        assert!(named[..5].iter().all(|line| line != "x1"));
    }

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

    /// A line whose day says its fact was replaced stays before her
    /// answer to it: lines are put in order by the day, not by what the
    /// day says besides.
    #[test]
    fn a_replaced_line_stays_where_it_was_said_in_its_day() {
        let replaced =
            crate::services::agent::merope::chat_lines::found_on("2026-11-17".into(), true);
        let found = super::merge_found(
            vec![
                (replaced.clone(), Who::They, "我住在北京朝阳".into()),
                ("2026-11-17".into(), Who::You, "朝阳那边挺方便".into()),
            ],
            vec![("2026-11-18".into(), Who::They, "今天下雨了".into())],
        );
        let said: Vec<&str> = found.iter().map(|(_, _, text)| text.as_str()).collect();
        assert_eq!(said, ["我住在北京朝阳", "朝阳那边挺方便", "今天下雨了"]);
        assert_eq!(found[0].0, replaced);
    }

    /// Each cue naming one book fills with the others: a book some try
    /// found is found, so all of them pick the kind they are of.
    #[test]
    fn a_line_any_try_found_is_never_only_filling() {
        let lines = |all: &[&str]| all.iter().map(|line| line.to_string()).collect::<Vec<_>>();
        let named = lines(&["看完了《三体》", "读完了《挪威的森林》", "养了一只猫"]);
        let filled = lines(&["看完了《三体》", "读完了《挪威的森林》", "养了一只猫"]);
        let found = lines(&["看完了《三体》", "读完了《挪威的森林》"]);
        assert_eq!(
            super::only_filling(&named, &filled, &found),
            lines(&["养了一只猫"])
        );
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
                range: None,
                open: None,
            }),
            opening,
            about_her: false,
        };
        assert!(!about(false, false).her_life());
        assert!(about(true, false).her_life());
        assert!(about(false, true).her_life());
        let words = |about_her| Attention {
            cues: None,
            opening: false,
            about_her,
        };
        assert!(!words(false).her_life());
        // Asked about her, her life comes to mind without waiting on cues.
        assert!(words(true).her_life());
    }

    /// Scrolling back finds what was said before the talk in front of her,
    /// not that talk again.
    #[tokio::test]
    async fn looking_back_skips_the_talk_still_in_front_of_her() {
        use sea_orm::ConnectionTrait;
        let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "look_back").await;
        let db = &schema.db;
        for statement in [
            "INSERT INTO users (id, username) VALUES (51, 'look-back')".to_string(),
            "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
             created_at, last_active_at) VALUES ('s_look', 51, '兰草', '{\"mode\":\"chat\"}', \
             23, false, NOW(), NOW())"
                .to_string(),
            // Said earlier today (so not a day turned back to), then the
            // twenty-one lines in front of her, one naming the same thing.
            "INSERT INTO agent_messages (session_id, role, content, created_at) VALUES \
             ('s_look', 'user', '阳台那盆兰草浇水要隔三天', NOW() - INTERVAL '2 hours')"
                .to_string(),
            format!(
                "INSERT INTO agent_messages (session_id, role, content, created_at) \
                 SELECT 's_look', CASE WHEN n % 2 = 0 THEN 'user' ELSE 'assistant' END, \
                   CASE WHEN n = 5 THEN '兰草浇水我昨天刚做过' ELSE '嗯' || n END, \
                   NOW() - INTERVAL '1 hour' + n * INTERVAL '1 minute' \
                 FROM generate_series(1, {}) n",
                crate::services::agent::sessions::PRIVATE_CHAT_IN_FRONT + 1
            ),
        ] {
            db.execute_unprepared(&statement).await.unwrap();
        }
        let found = look_back(db, 51, "兰草浇水", "", 3).await;
        let texts: Vec<&str> = found.iter().map(|(_, _, text)| text.as_str()).collect();
        assert!(
            texts.iter().any(|text| text.contains("隔三天")),
            "what was said before is found: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains("昨天刚做过")),
            "the talk in front of her is not found again: {texts:?}"
        );
        schema.drop().await;
    }
}

#[cfg(test)]
mod live {
    /// How long thinking what to remember takes, and what comes of it, by
    /// voice: read only, MEROPE_RUNS (default 6).
    #[tokio::test]
    #[ignore = "asks the site's model"]
    async fn how_fast_she_thinks_what_to_remember() {
        let _db = crate::services::agent::semantic_eval::load_configured_lite().await;
        let runs: usize = std::env::var("MEROPE_RUNS")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(6);
        let today = super::super::clock::today().format("%Y-%m-%d").to_string();
        let said = [
            "你好呀",
            "上次那个海龟汤的汤底是什么",
            "我上周说要去面试的那家公司叫什么来着",
        ];
        for (label, voice) in [("judge", super::Voice::Judge), ("hers", super::Voice::Hers)] {
            let mut times = Vec::new();
            for index in 0..runs {
                let message = said[index % said.len()];
                let began = std::time::Instant::now();
                let raw = super::asking(voice, 1)
                    .within(std::time::Duration::from_secs(20))
                    .json_raw(
                        &super::system(),
                        &super::input(message, None, &today),
                        super::SCHEMA_NAME,
                        &super::schema(),
                    )
                    .await;
                let took = began.elapsed().as_secs_f64();
                times.push(took);
                let cues = raw.ok().and_then(|raw| super::parse(&raw));
                println!(
                    "{label} {took:.2}s {message}: {:?}",
                    cues.map(|cues| (cues.cues, cues.look_back))
                );
            }
            times.sort_by(f64::total_cmp);
            println!(
                "{label}: median {:.2}s, slowest {:.2}s",
                times[times.len() / 2],
                times[times.len() - 1]
            );
        }
    }
}
