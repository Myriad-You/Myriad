//! Her speaking prompt for one turn: who she is talking to, what she knows
//! and feels about them, what she has been doing, read from storage and laid
//! out as prompt sections. The sections' wording lives in
//! `myriad_merope::speaking`; this module only gathers what fills them.

use super::*;

mod sections;

pub fn format_addressee_label(
    user_id: i32,
    display_name: Option<&str>,
    username: Option<&str>,
) -> String {
    if !is_logged_in_addressee(user_id) {
        return "Guest".to_string();
    }
    display_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| username.map(str::trim).filter(|name| !name.is_empty()))
        .map(str::to_string)
        .unwrap_or_else(|| format!("User#{user_id}"))
}

pub async fn resolve_addressee_label(db: &sea_orm::DatabaseConnection, user_id: i32) -> String {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SeaValue};

    let Ok(Some(row)) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT display_name, username FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
    else {
        return format_addressee_label(user_id, None, None);
    };
    let display_name = row
        .try_get::<Option<String>>("", "display_name")
        .ok()
        .flatten();
    let username = row.try_get::<Option<String>>("", "username").ok().flatten();
    format_addressee_label(user_id, display_name.as_deref(), username.as_deref())
}

/// Her first words the last few times this person came back to her in
/// private after a while (their message following three hours apart),
/// oldest first: (how long ago, what she said). As of now as this turn is
/// put together.
pub async fn her_openers(db: &sea_orm::DatabaseConnection, user_id: i32) -> Vec<(String, String)> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    const SHOWN: i64 = 4;
    let now = super::clock::now();
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "WITH t AS (SELECT m.role, m.content, m.created_at, lag(m.role) OVER w AS r1, \
               lag(m.created_at) OVER w AS a1, lag(m.created_at, 2) OVER w AS a2 \
               FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
               WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
                 AND NOT s.archived AND m.created_at < $2 \
               WINDOW w AS (ORDER BY m.created_at)) \
             SELECT content, created_at FROM t WHERE role = 'assistant' AND r1 = 'user' \
               AND a2 IS NOT NULL AND a1 - a2 >= interval '3 hours' \
             ORDER BY created_at DESC LIMIT $3",
            [user_id.into(), now.fixed_offset().into(), SHOWN.into()],
        ))
        .await
        .unwrap_or_default();
    let mut openers: Vec<(String, String)> = rows
        .iter()
        .filter_map(|row| {
            let content: String = row.try_get("", "content").ok()?;
            let at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "created_at").ok()?;
            Some((
                myriad_merope::doing::ago_text(now - at.with_timezone(&chrono::Utc)),
                content,
            ))
        })
        .collect();
    openers.reverse();
    openers
}

/// When this person first wrote to her and on how many different days they
/// have written, from everything they sent her anywhere. Looked up at most
/// every few minutes per person.
pub(super) async fn acquaintance(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Option<(Option<chrono::DateTime<chrono::Utc>>, u32)> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SeaValue};
    type Known = (Option<chrono::DateTime<chrono::Utc>>, u32);
    static KNOWN: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<i32, (std::time::Instant, Known)>>,
    > = std::sync::LazyLock::new(Default::default);
    const FRESH_FOR: std::time::Duration = std::time::Duration::from_secs(10 * 60);
    const PEOPLE_KEPT: usize = 4096;
    if let Some((_, known)) = KNOWN
        .lock()
        .ok()?
        .get(&user_id)
        .filter(|(at, _)| at.elapsed() < FRESH_FOR)
    {
        return Some(*known);
    }
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT MIN(m.created_at) AS first, \
                    COUNT(DISTINCT (m.created_at AT TIME ZONE 'UTC')::date) AS days \
             FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND m.role = 'user'",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .ok()??;
    let first = row
        .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "first")
        .ok()
        .flatten()
        .map(|first| first.with_timezone(&chrono::Utc));
    let days = row
        .try_get::<i64>("", "days")
        .ok()
        .and_then(|days| u32::try_from(days).ok())
        .unwrap_or(0);
    let known = (first, days);
    if let Ok(mut cache) = KNOWN.lock() {
        if cache.len() >= PEOPLE_KEPT {
            cache.retain(|_, (at, _)| at.elapsed() < FRESH_FOR);
        }
        cache.insert(user_id, (std::time::Instant::now(), known));
    }
    Some(known)
}

/// How often they talk with her (see `myriad_merope::rhythm`), from the
/// days they wrote to her anywhere; none until anything is usual. Looked up
/// at most every few minutes per person.
pub(super) async fn talk_rhythm(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Option<myriad_merope::rhythm::Rhythm> {
    use myriad_merope::rhythm::Rhythm;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    static KNOWN: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<i32, (std::time::Instant, Option<Rhythm>)>>,
    > = std::sync::LazyLock::new(Default::default);
    const FRESH_FOR: std::time::Duration = std::time::Duration::from_secs(10 * 60);
    const PEOPLE_KEPT: usize = 4096;
    // Enough for the gaps weighed and eight weeks of talking every day.
    const DAYS: i64 = 64;
    if let Some((_, known)) = KNOWN
        .lock()
        .ok()?
        .get(&user_id)
        .filter(|(at, _)| at.elapsed() < FRESH_FOR)
    {
        return *known;
    }
    let first = acquaintance(db, user_id).await?.0?.date_naive();
    let days: Vec<chrono::NaiveDate> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT day FROM (SELECT DISTINCT (m.created_at AT TIME ZONE 'UTC')::date AS day \
             FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND m.role = 'user') talked ORDER BY day DESC LIMIT $2",
            [user_id.into(), DAYS.into()],
        ))
        .await
        .ok()?
        .iter()
        .filter_map(|row| row.try_get::<chrono::NaiveDate>("", "day").ok())
        .collect();
    let known = myriad_merope::rhythm::rhythm(&days, first, crate::clock::now().date_naive());
    if let Ok(mut cache) = KNOWN.lock() {
        if cache.len() >= PEOPLE_KEPT {
            cache.retain(|_, (at, _)| at.elapsed() < FRESH_FOR);
        }
        cache.insert(user_id, (std::time::Instant::now(), known));
    }
    known
}

/// How her lines to this person differ from theirs to her (see
/// `myriad_merope::contrast`), from their latest chat messages anywhere and
/// her latest answers (not work: a report is not how she talks). Looked up
/// at most every few minutes per person.
async fn how_she_differs_with(db: &sea_orm::DatabaseConnection, user_id: i32) -> Option<String> {
    use myriad_merope::contrast::{Counts, describe, overused};
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SeaValue};
    static TOLD: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<i32, (std::time::Instant, Option<String>)>>,
    > = std::sync::LazyLock::new(Default::default);
    const FRESH_FOR: std::time::Duration = std::time::Duration::from_secs(10 * 60);
    const PEOPLE_KEPT: usize = 4096;
    const LATEST: i32 = 400;
    if let Some((_, told)) = TOLD
        .lock()
        .ok()?
        .get(&user_id)
        .filter(|(at, _)| at.elapsed() < FRESH_FOR)
    {
        return told.clone();
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.role, m.content FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
               AND m.role IN ('user', 'assistant') \
             ORDER BY m.created_at DESC LIMIT $2",
            vec![SeaValue::Int(Some(user_id)), SeaValue::Int(Some(LATEST))],
        ))
        .await
        .ok()?;
    let (mut theirs, mut hers) = (Counts::default(), Counts::default());
    for row in &rows {
        let (Ok(role), Ok(content)) = (
            row.try_get::<String>("", "role"),
            row.try_get::<String>("", "content"),
        ) else {
            continue;
        };
        // Each line of hers went as a message of its own.
        let counts = if role == "user" {
            &mut theirs
        } else {
            &mut hers
        };
        for line in content.lines() {
            counts.add(line);
        }
    }
    let told = describe(&overused(&hers, &theirs));
    if let Ok(mut cache) = TOLD.lock() {
        if cache.len() >= PEOPLE_KEPT {
            cache.retain(|_, (at, _)| at.elapsed() < FRESH_FOR);
        }
        cache.insert(user_id, (std::time::Instant::now(), told.clone()));
    }
    told
}

/// Her talks with others since she got up (see `myriad_merope::elsewhere`),
/// as the section that tells them. In a group, where people outside the
/// community may be reading, only her talks in other groups, by the group's
/// name; how today has left her is part of her own state there as
/// everywhere (`self_state`). Read as times, kinds and counts only: no
/// message text, person's name or what a sore spot was about is selected
/// here, so none of it can reach the prompt this goes into.
pub(super) async fn since_she_got_up(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
) -> Option<String> {
    use myriad_merope::elsewhere::{self, Cause, Facing, Heard, Moved, Place, Stung};
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let now = super::clock::now();
    let since = now - chrono::Duration::minutes(i64::from(super::timing::her_day().since_up));
    let facing = Facing {
        person: user_id,
        group: present.group_id(),
    };
    let at = |row: &sea_orm::QueryResult, column: &str| {
        row.try_get::<chrono::DateTime<chrono::FixedOffset>>("", column)
            .ok()
            .map(|at| at.with_timezone(&chrono::Utc))
    };
    let moved: Vec<Moved> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id, at, cause, (after->>'emotion')::float8 AS emotion \
             FROM agent_affect_events WHERE at >= $1 AND user_id <> $2 ORDER BY at",
            [since.fixed_offset().into(), user_id.into()],
        ))
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            Some(Moved {
                person: row.try_get("", "user_id").ok()?,
                at: at(row, "at")?,
                cause: Cause::parse(&row.try_get::<String>("", "cause").ok()?),
                felt_after: row.try_get::<f64>("", "emotion").ok()? - myriad_merope::affect::ORIGIN,
            })
        })
        .collect();
    let mut people: Vec<i32> = moved.iter().map(|moved| moved.person).collect();
    people.sort_unstable();
    people.dedup();
    let told =
        |talks: &[elsewhere::Talk]| (!present.is_group()).then(|| elsewhere::section(talks, now));
    if people.is_empty() {
        return told(&[]);
    }
    let heard: Vec<Heard> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT s.user_id, s.context->>'mode' AS mode, s.context->>'venue' AS venue, \
                    m.created_at \
             FROM agent_sessions s JOIN agent_messages m ON m.session_id = s.id \
             WHERE s.user_id = ANY($1) AND m.role = 'user' AND m.created_at >= $2",
            [people.clone().into(), since.fixed_offset().into()],
        ))
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            let venue = row.try_get::<Option<String>>("", "venue").ok().flatten();
            let mode = row.try_get::<Option<String>>("", "mode").ok().flatten();
            Some(Heard {
                person: row.try_get("", "user_id").ok()?,
                place: match (venue, mode.as_deref()) {
                    (Some(venue), _) => Place::Group(venue),
                    (None, Some("chat")) => Place::Private,
                    (None, _) => Place::Work,
                },
                at: at(row, "created_at")?,
            })
        })
        .collect();
    let mut stung = Vec::new();
    let mut first_met = Vec::new();
    for &person in &people {
        stung.extend(
            super::sore::open_all(db, person)
                .await
                .into_iter()
                .map(|sore| Stung {
                    person,
                    place: match sore.venue.strip_prefix("group:") {
                        Some(group) => Place::Group(group.to_string()),
                        None => Place::Private,
                    },
                    weight: sore.weight,
                    since: sore.since,
                    mended: sore.mended,
                }),
        );
        if acquaintance(db, person)
            .await
            .and_then(|(first, _)| first)
            .is_some_and(|first| first >= since)
        {
            first_met.push(person);
        }
    }
    let placed = elsewhere::talks_placed(&heard, &moved, &stung, &first_met, since, facing);
    // In a group, only her talks in other groups, by their names: a private
    // talk never reaches a group.
    if present.is_group() {
        let mut named = Vec::new();
        for (place, talk) in placed {
            let name = match &place {
                Place::Group(venue) => crate::services::channel_group::title(venue).await,
                Place::Private | Place::Work => None,
            };
            named.push((name, talk));
        }
        return elsewhere::section_in_group(&named, now);
    }
    told(&placed.into_iter().map(|(_, talk)| talk).collect::<Vec<_>>())
}

/// Prompt sections for whoever this turn is speaking to. Empty when Merope is off.
pub async fn speaking_prompt(user_id: i32) -> Vec<String> {
    speaking_prompt_with_query(user_id, None).await
}

pub async fn speaking_prompt_with_query(user_id: i32, query: Option<&str>) -> Vec<String> {
    let present = crate::services::agent::memory::unified::Audience::private(user_id);
    speaking_prompt_for_turn(user_id, query, &present, None).await
}

/// A turn in a group chat (`venue` such as `telegram:-100123`), answering
/// `user_id`, whom the group knows as `known_as`. Others outside the
/// community may be reading: only what this group heard is said, never
/// anyone's private matters, their name on the site included.
pub async fn speaking_prompt_in_group(
    user_id: i32,
    query: &str,
    venue: &str,
    known_as: Option<&str>,
) -> Vec<String> {
    let present = crate::services::agent::memory::unified::Audience::group(venue, user_id);
    speaking_prompt_for_turn(user_id, Some(query), &present, known_as).await
}

/// Who is present for this request: a group when the server placed the turn
/// in one, otherwise the person alone.
pub fn audience_for(
    request: &crate::services::agent::UserRequest,
) -> crate::services::agent::memory::unified::Audience {
    match request
        .context
        .as_ref()
        .and_then(|context| context.venue.as_deref())
    {
        Some(venue) => {
            crate::services::agent::memory::unified::Audience::group(venue, request.user_id)
        }
        None => crate::services::agent::memory::unified::Audience::private(request.user_id),
    }
}

/// Nothing waits here: this turn's appraisal and her state after it land
/// for the turns that follow, and the speaking model hears the words itself.
async fn speaking_prompt_for_turn(
    user_id: i32,
    query: Option<&str>,
    present: &crate::services::agent::memory::unified::Audience,
    known_as: Option<&str>,
) -> Vec<String> {
    if !is_enabled().await {
        return Vec::new();
    }
    if user_id < 0 {
        return vec![guest_speaking_section()];
    }
    if !is_logged_in_addressee(user_id) {
        return Vec::new();
    }
    let Ok(db) = crate::services::process_db::database() else {
        return vec![addressee_speaking_section(&format_addressee_label(
            user_id, None, None,
        ))];
    };
    let turn = match query.filter(|query| !query.trim().is_empty()) {
        Some(words) => Turn::Chat(words),
        None => Turn::Plain,
    };
    speaking_prompt_from_db(&db, user_id, turn, present, known_as).await
}

/// Sections for speaking up unprompted about `summary`. The same mind as a
/// chat turn: what she knows about them (recalled against what happened), how
/// she feels, how she is. It reads the conversation's train of thought but
/// does not move it; only the person's own words do.
pub async fn speaking_prompt_for_event(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    summary: &str,
) -> Vec<String> {
    if user_id <= 0 || !is_logged_in_addressee(user_id) || !is_enabled().await {
        return Vec::new();
    }
    let present = crate::services::agent::memory::unified::Audience::private(user_id);
    speaking_prompt_from_db(db, user_id, Turn::Event(summary), &present, None).await
}

/// Sections for writing to them first while they are away (see `reach`):
/// the same mind as speaking up, without needing them on the site.
pub async fn speaking_prompt_to_reach(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    about: &str,
) -> Vec<String> {
    if user_id <= 0 || !is_enabled().await {
        return Vec::new();
    }
    let present = crate::services::agent::memory::unified::Audience::private(user_id);
    speaking_prompt_from_db(db, user_id, Turn::Event(about), &present, None).await
}

/// Why she is about to speak.
#[derive(Clone, Copy)]
enum Turn<'a> {
    /// Answering the person's words.
    Chat(&'a str),
    /// Speaking up about something that happened (untrusted summary).
    Event(&'a str),
    /// Anything else that wears the persona, such as Work.
    Plain,
}

const REMEMBERED_PROMPT_LIMIT: usize = 8;
const RECENT_LEDGER_LIMIT: u64 = 4;
/// Chat diary only. Event diary reaches speaking via Remember, not this ledger.
pub(super) const RECENT_SPEAKING_DIARY_SOURCES: &[&str] = &[store::DIARY_SOURCE_CHAT];

/// Things she looked up on her own that a turn carries.
const FOUND_OUT_LIMIT: usize = 2;
/// Things she heard in groups a group's talk brings back.
const HEARD_LIMIT: usize = 3;
/// Her own days a conversation carries, most recent last.
const OWN_DAYS_LIMIT: u64 = 3;
/// Whose things keep getting to her, when her life comes to mind.
const TASTE_SHOWN: usize = 3;
/// What she did on her own in the last day, and older things their words touch.
const DOING_RECENT: usize = 3;
const DOING_RELATED: usize = 2;
/// Views of her own their words touch.
const VIEWS_LIMIT: usize = 2;
/// Bits between her and them, freshest first.
const BITS_LIMIT: u64 = 3;
/// Days in a group she has in mind when she talks there.
const GROUP_DAYS: u64 = 3;
/// Her own unprompted lines a chat turn should know it said.
const SAID_UNPROMPTED_LIMIT: u64 = 3;
const SAID_UNPROMPTED_WITHIN_HOURS: i64 = 6;

/// The conversation as she lived it: what she said to them on her own is
/// part of it, in its place in time, as her own line. A section beside the
/// history was ignored in testing; a line in the history is not.
pub async fn with_said_unprompted(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    history: &[crate::services::agent::ConversationMessage],
) -> Vec<crate::services::agent::ConversationMessage> {
    if user_id <= 0 || !is_logged_in_addressee(user_id) || !is_enabled().await {
        return history.to_vec();
    }
    let since = super::clock::now() - chrono::Duration::hours(SAID_UNPROMPTED_WITHIN_HOURS);
    let said: Vec<(chrono::DateTime<chrono::Utc>, String)> =
        store::recent_proactive(db, user_id, SAID_UNPROMPTED_LIMIT)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|line| {
                (
                    line.created_at.with_timezone(&chrono::Utc),
                    ingest::compact_summary(&line.content),
                    line.metadata,
                )
            })
            .filter(|(at, line, _)| *at >= since && !line.is_empty())
            // Already in the history as it is: not added twice.
            .filter(|(_, line, _)| {
                !history
                    .iter()
                    .any(|message| message.content.trim() == line.trim())
            })
            .map(|(at, line, metadata)| (at, said_as_heard(line, metadata.as_ref())))
            .collect();
    merge_said_unprompted(history, &said)
}

/// One of her unprompted lines as it reads in the conversation: like a
/// reply of hers, marked when it was cut off while being said aloud.
pub fn said_as_heard(line: String, metadata: Option<&serde_json::Value>) -> String {
    match crate::services::agent::chat_prompt::line_cut_off_marker(metadata) {
        Some(marker) => format!("{line}{marker}"),
        None => line,
    }
}

/// How the one she is talking with types to her, from their lines in the
/// conversation; None while there are too few. Lines without a time are
/// each their own turn.
pub fn their_typing(
    history: &[crate::services::agent::ConversationMessage],
) -> Option<myriad_merope::talk_shape::Shape> {
    let lines: Vec<(&str, i64, &str)> = history
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == "user")
        .map(|(index, message)| {
            let at = message
                .created_at
                .as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.timestamp())
                .unwrap_or(index as i64 * 3600);
            ("them", at, message.content.as_str())
        })
        .collect();
    myriad_merope::talk_shape::room_of(&lines)
}

/// Put her unprompted lines into the history by time. A line already in the
/// history is not added twice; history without timestamps keeps its order
/// and her lines go after it.
pub fn merge_said_unprompted(
    history: &[crate::services::agent::ConversationMessage],
    said: &[(chrono::DateTime<chrono::Utc>, String)],
) -> Vec<crate::services::agent::ConversationMessage> {
    let at = |message: &crate::services::agent::ConversationMessage| {
        message
            .created_at
            .as_deref()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.with_timezone(&chrono::Utc))
    };
    let mut merged = history.to_vec();
    let mut said: Vec<&(chrono::DateTime<chrono::Utc>, String)> = said.iter().collect();
    said.sort_by_key(|(when, _)| *when);
    for (when, line) in said {
        if merged
            .iter()
            .any(|message| message.content.trim() == line.trim())
        {
            continue;
        }
        let position = merged
            .iter()
            .position(|message| at(message).is_some_and(|message_at| message_at > *when))
            .unwrap_or(merged.len());
        merged.insert(
            position,
            crate::services::agent::ConversationMessage {
                role: "assistant".into(),
                content: line.clone(),
                created_at: Some(when.to_rfc3339()),
            },
        );
    }
    merged
}

async fn speaking_prompt_from_db(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    turn: Turn<'_>,
    present: &crate::services::agent::memory::unified::Audience,
    known_as: Option<&str>,
) -> Vec<String> {
    // In a group, people outside the community may be reading: nothing
    // private to anyone — the person's diary, her unprompted lines to them,
    // what was on her mind — is brought in, and memory is what the group heard.
    // They are who the group knows them as, the name the conversation shows
    // and her @ reaches, not their name on the site.
    let group = present.is_group();
    let addressee = match known_as.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) if group => name.to_string(),
        _ => resolve_addressee_label(db, user_id).await,
    };
    let mut sections = vec![if group {
        group_speaking_section(&addressee)
    } else {
        addressee_speaking_section(&addressee)
    }];
    sections::who_they_are(db, user_id, turn, present, &mut sections).await;
    let Ok(state) = get_or_create_state(db, user_id).await else {
        return sections;
    };
    let myself = self_state::current(db).await;
    let (her_life, opening) =
        sections::recollection(db, user_id, turn, present, &myself, &mut sections).await;
    sections::their_recent(db, user_id, group, &state, &mut sections).await;
    let inner_block =
        sections::her_state(db, user_id, turn, present, &state, &myself, &mut sections).await;
    sections::what_she_found(db, user_id, turn, present, &mut sections).await;
    if !matches!(turn, Turn::Plain) {
        sections::her_own(db, turn, her_life, &mut sections).await;
        sections::between_them(db, user_id, turn, present, opening, &mut sections).await;
    }
    if matches!(turn, Turn::Chat(_)) {
        sections::chat_extras(db, user_id, turn, present, &mut sections).await;
    }
    sections.extend(inner_block);
    sections
}

pub fn speaking_prompt_plain(sections: &[String]) -> String {
    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::memory::unified::Audience;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    async fn user(db: &sea_orm::DatabaseConnection, name: &str) -> i32 {
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (username, display_name) VALUES ($1, $1) RETURNING id",
            [name.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap()
    }

    /// One message `user_id` sent her at `at`, in a session of its own
    /// (`venue` for a group's), and the change it made in her.
    async fn said(
        db: &sea_orm::DatabaseConnection,
        user_id: i32,
        venue: Option<&str>,
        at: chrono::DateTime<chrono::Utc>,
        text: &str,
        cause: &str,
        emotion: f64,
    ) {
        let session = uuid::Uuid::new_v4().to_string();
        let context = match venue {
            Some(venue) => serde_json::json!({ "mode": "chat", "venue": venue }),
            None => serde_json::json!({ "mode": "chat" }),
        };
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
             created_at, last_active_at) VALUES ($1, $2, $3, $4, 1, false, $5, $5)",
            [
                session.clone().into(),
                user_id.into(),
                text.into(),
                context.into(),
                at.fixed_offset().into(),
            ],
        ))
        .await
        .unwrap();
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, created_at) \
             VALUES ($1, 'user', $2, $3)",
            [session.into(), text.into(), at.fixed_offset().into()],
        ))
        .await
        .unwrap();
        let affect = serde_json::json!({
            "mood": 50.0, "arousal": 50.0, "emotion": emotion, "emotionArousal": 50.0
        });
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_affect_events (id, user_id, at, cause, impulse, baseline_mood, \
             baseline_arousal, before, after, created_at) \
             VALUES ($1, $2, $3, $4, '{}'::jsonb, 50, 50, $5, $5, NOW())",
            [
                uuid::Uuid::new_v4().to_string().into(),
                user_id.into(),
                at.fixed_offset().into(),
                cause.into(),
                affect.into(),
            ],
        ))
        .await
        .unwrap();
    }

    /// What one person tells her never crosses to another: the section
    /// built for someone else carries how it went and nothing of what was
    /// said, who said it, where, or what got to her. The one she faces,
    /// the group she is in, and what came before she got up are left out.
    #[tokio::test]
    async fn how_a_talk_went_crosses_and_nothing_said_in_it_does() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "since_she_got_up").await;
        let db = isolated.db.clone();
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-11T15:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let ago = |minutes: i64| now - chrono::Duration::minutes(minutes);
        let ming = user(&db, "ming-secret-handle").await;
        let hong = user(&db, "hong").await;
        let group = "telegram:-1009988";
        said(
            &db,
            ming,
            None,
            ago(50),
            "PLUM-SECRET 我的猫昨天走丢了",
            "user_turn",
            50.0,
        )
        .await;
        said(
            &db,
            ming,
            None,
            ago(40),
            "PLUM-SECRET 你真笨",
            "user_scold",
            20.0,
        )
        .await;
        said(
            &db,
            ming,
            Some(group),
            ago(30),
            "GROUP-SECRET 大家好",
            "user_praise",
            80.0,
        )
        .await;
        // Before she got up today.
        said(
            &db,
            ming,
            None,
            ago(20 * 60),
            "NIGHT-SECRET",
            "user_praise",
            90.0,
        )
        .await;
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, \
             venue, valid_from, created_at, updated_at) \
             VALUES ('sore-1', $1, 'fact', 'SORE-SECRET 他嘲笑我的画', $2, 'agent', 'sore', \
             'private', $3, $3, $3)",
            [
                ming.into(),
                serde_json::json!({ "weight": "hurt", "since": ago(40).to_rfc3339() })
                    .to_string()
                    .into(),
                ago(40).fixed_offset().into(),
            ],
        ))
        .await
        .unwrap();

        let zone = super::super::clock::Zone::named("UTC").unwrap();
        let built = |user_id: i32, present: Audience| {
            let db = db.clone();
            async move {
                super::super::clock::in_zone(
                    zone,
                    super::super::clock::as_of(now, since_she_got_up(&db, user_id, &present)),
                )
                .await
            }
        };

        let for_hong = built(hong, Audience::private(hong)).await.unwrap();
        for secret in ["SECRET", "ming", "猫", "画", "telegram", "-1009988"] {
            assert!(!for_hong.contains(secret), "{secret} crossed: {for_hong}");
        }
        assert!(
            for_hong.contains(
                "- Within the last hour, a private talk, for a while; they scolded you; \
                 something they did got to you; at the time you came away hurting."
            ),
            "{for_hong}"
        );
        assert!(
            for_hong.contains("- Within the last hour, in a group, briefly; they praised you"),
            "{for_hong}"
        );
        assert_eq!(
            for_hong
                .lines()
                .filter(|line| line.starts_with("- "))
                .count(),
            2,
            "nothing from before she got up: {for_hong}"
        );

        // In a group, people outside the community may be reading: only her
        // talks in other groups, never a private one. In the group it all
        // happened in, nothing; how today left her is in her own state there.
        assert_eq!(built(hong, Audience::group(group, hong)).await, None);
        let elsewhere = built(hong, Audience::group("telegram:-1", hong))
            .await
            .unwrap();
        assert!(
            elsewhere.contains("- Within the last hour, in a group, briefly; they praised you"),
            "{elsewhere}"
        );
        for kept in ["private", "scolded", "SECRET", "ming", "猫"] {
            assert!(
                !elsewhere.contains(kept),
                "{kept} reached a group: {elsewhere}"
            );
        }

        // To the one who said it all, nobody else has talked with her.
        assert_eq!(
            built(ming, Audience::private(ming)).await.as_deref(),
            Some("## Since you got up\nNobody else has talked with you since you got up.")
        );
        assert_eq!(built(ming, Audience::group(group, ming)).await, None);
    }

    /// Someone who wrote to her nearly every day, privately and in a group,
    /// and now not for a few days: how often they talk comes from the days
    /// they wrote anywhere, and this time reads as longer than usual. On a
    /// real database (an isolated schema; skipped without a test database).
    #[tokio::test]
    async fn how_often_they_talk_comes_from_the_days_they_wrote() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "talk_rhythm").await;
        let db = isolated.db.clone();
        // Far from the ids other tests' fresh schemas hand out, since what is
        // looked up is kept for a while per person.
        let person = 91_017;
        db.execute_unprepared(&format!(
            "INSERT INTO users (id, username, display_name) VALUES ({person}, 'daily', 'daily')"
        ))
        .await
        .unwrap();
        let now = crate::clock::now();
        for ago in 4..40 {
            let venue = (ago % 3 == 0).then_some("telegram:-1005566");
            let at = now - chrono::Duration::days(ago);
            said(&db, person, venue, at, "早", "user_turn", 50.0).await;
            // Twice on a day is one day of talking.
            said(&db, person, None, at, "晚安", "user_turn", 50.0).await;
        }
        let rhythm = talk_rhythm(&db, person).await.expect("something is usual");
        assert_eq!(rhythm.usual_gap, 1);
        assert_eq!(rhythm.since, 4);
        assert!(rhythm.longer_this_time());
        assert_eq!(rhythm.lately, None, "known for under eight weeks");
        let line = rhythm.while_talking().unwrap();
        assert!(
            line.contains("on most days") && line.contains("4 days ago"),
            "{line}"
        );
        isolated.drop().await;
    }
}
