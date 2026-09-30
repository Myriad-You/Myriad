//! Her speaking prompt for one turn: who she is talking to, what she knows
//! and feels about them, what she has been doing, read from storage and laid
//! out as prompt sections. The sections' wording lives in
//! `myriad_merope::speaking`; this module only gathers what fills them.

use super::*;

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

/// How her lines to this person differ from theirs to her (see
/// `myriad_merope::contrast`), from their latest messages anywhere and her
/// latest answers. Looked up at most every few minutes per person.
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
             WHERE s.user_id = $1 AND m.role IN ('user', 'assistant') \
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

/// How her last talks with others left her (see
/// `myriad_merope::speaking::format_carried_section`): each one's last
/// feeling off even, and how many hours ago, for talks in the last hours.
async fn carried(db: &sea_orm::DatabaseConnection, user_id: i32) -> Vec<(f64, f64)> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SeaValue};
    const WITHIN_HOURS: i32 = 6;
    let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT emotion, (EXTRACT(EPOCH FROM (now() - emotion_settled_at)) / 3600.0)::float8 AS hours \
             FROM agent_addressee_state \
             WHERE user_id <> $1 AND emotion_settled_at > now() - make_interval(hours => $2)",
            vec![SeaValue::Int(Some(user_id)), SeaValue::Int(Some(WITHIN_HOURS))],
        ))
        .await
    else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let emotion = row.try_get::<f64>("", "emotion").ok()?;
            let hours = row.try_get::<f64>("", "hours").ok()?;
            Some((emotion - myriad_merope::affect::ORIGIN, hours))
        })
        .collect()
}

/// Prompt sections for whoever this turn is speaking to. Empty when Merope is off.
pub async fn speaking_prompt(user_id: i32) -> Vec<String> {
    speaking_prompt_with_query(user_id, None).await
}

pub async fn speaking_prompt_with_query(user_id: i32, query: Option<&str>) -> Vec<String> {
    let present = crate::services::agent::memory::unified::Audience::private(user_id);
    speaking_prompt_for_turn(user_id, query, &present).await
}

/// A turn in a group chat (`venue` such as `telegram:-100123`), answering
/// `user_id`. Others outside the community may be reading: only what this
/// group heard is said, never anyone's private matters.
pub async fn speaking_prompt_in_group(user_id: i32, query: &str, venue: &str) -> Vec<String> {
    let present = crate::services::agent::memory::unified::Audience::group(venue, user_id);
    speaking_prompt_for_turn(user_id, Some(query), &present).await
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
    speaking_prompt_from_db(&db, user_id, turn, present).await
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
    speaking_prompt_from_db(db, user_id, Turn::Event(summary), &present).await
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
    speaking_prompt_from_db(db, user_id, Turn::Event(about), &present).await
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
/// Her own days a conversation carries, most recent last.
const OWN_DAYS_LIMIT: u64 = 3;
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
    let since = chrono::Utc::now() - chrono::Duration::hours(SAID_UNPROMPTED_WITHIN_HOURS);
    let said: Vec<(chrono::DateTime<chrono::Utc>, String)> =
        store::recent_proactive(db, user_id, SAID_UNPROMPTED_LIMIT)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|line| {
                (
                    line.created_at.with_timezone(&chrono::Utc),
                    ingest::compact_summary(&line.content),
                )
            })
            .filter(|(at, line)| *at >= since && !line.is_empty())
            .collect();
    merge_said_unprompted(history, &said)
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
) -> Vec<String> {
    // In a group, people outside the community may be reading: nothing
    // private to anyone — the person's diary, her unprompted lines to them,
    // what was on her mind — is brought in, and memory is what the group heard.
    let group = present.is_group();
    let addressee = resolve_addressee_label(db, user_id).await;
    let mut sections = vec![if group {
        group_speaking_section(&addressee)
    } else {
        addressee_speaking_section(&addressee)
    }];
    if !group && let Some(differs) = how_she_differs_with(db, user_id).await {
        sections.push(differs);
    }
    if !group && let Some(told) = making_sense::told_by_section(db, user_id).await {
        sections.push(told);
    }
    // How long the two of them have known each other is between them.
    if !group && let Some((first, days)) = acquaintance(db, user_id).await {
        sections.push(myriad_merope::speaking::format_acquaintance_section(
            first,
            days,
            chrono::Utc::now(),
        ));
    }
    // Whether it reads like them typing.
    if matches!(turn, Turn::Chat(_))
        && let Some(block) = likeness::section(db, user_id).await
    {
        sections.push(block);
    }
    let Ok(state) = get_or_create_state(db, user_id).await else {
        return sections;
    };
    let myself = self_state::current(db).await;
    // Before answering what they said, she thinks what to try to remember.
    // And, as anyone does, keeps in mind what the moment asks for: her own
    // life when the talk is about her or they are only now starting to talk
    // again, not at every line. Speaking up on her own, it is her own
    // things she brings, so all of it is at hand.
    let (cues, her_life, opening) = match turn {
        Turn::Chat(words) => {
            let attention = remembering::cues(user_id, words).await;
            let her_life = attention.her_life();
            (attention.cues, her_life, attention.opening)
        }
        _ => (None, true, true),
    };
    // Only a chat turn (it has the person's words) carries its train of
    // thought to the next turn; other readers see memory without moving it.
    let remembered = match turn {
        Turn::Chat(words) | Turn::Event(words) => remembering::recall_with(
            db,
            user_id,
            present,
            words,
            cues.as_ref(),
            REMEMBERED_PROMPT_LIMIT,
            remembering::THOROUGH,
            &if group {
                crate::services::agent::memory::unified::Priming::default()
            } else {
                priming::current(user_id)
            },
            myself.recall_breadth(),
        )
        .await
        .map(|(recalled, next)| {
            if matches!(turn, Turn::Chat(_)) && !group {
                priming::keep(user_id, next);
            }
            recalled
        }),
        Turn::Plain => store::recall_remembered(db, user_id, None, REMEMBERED_PROMPT_LIMIT)
            .await
            .map(|named| store::Recalled {
                named,
                brought_to_mind: Vec::new(),
            }),
    };
    if let Ok(recalled) = remembered {
        if let Some(block) = format_remembered_section(&recalled.named) {
            sections.push(block);
        }
        // What their words brought to mind, apart from what they named: the
        // stuff of callbacks and unexpected remarks, hers to use or not.
        if let Some(block) = format_brought_to_mind_section(&recalled.brought_to_mind) {
            sections.push(block);
        }
    }
    // Asked about what was said in detail, or for all of it, she scrolls
    // back through the chat.
    // Asked back to something before, she scrolls back even when she could
    // not think in time what to look for: their words are the query then.
    let reaching_back = |words: &str| {
        cues.as_ref().map_or_else(
            || myriad_merope::remembering::reaches_back(words),
            |cues| cues.look_back || cues.thorough,
        )
    };
    let query_of = |words: &str| {
        std::iter::once(words)
            .chain(
                cues.iter()
                    .flat_map(|cues| cues.cues.iter().map(String::as_str)),
            )
            .collect::<Vec<_>>()
            .join(" ")
    };
    let thorough = cues.as_ref().is_some_and(|cues| cues.thorough);
    if let (false, Turn::Chat(words)) = (group, turn)
        && reaching_back(words)
    {
        let query = query_of(words);
        let found = remembering::look_back(
            db,
            user_id,
            &query,
            words,
            if thorough {
                remembering::LOOK_BACK_THOROUGH
            } else {
                remembering::LOOK_BACK
            },
        )
        .await;
        if let Some(block) = myriad_merope::remembering::looked_back_section(&found) {
            sections.push(block);
        }
    }
    // In a group, the group's own days and talk: nothing private.
    if let (Some(venue), Turn::Chat(words)) = (present.group_id(), turn)
        && reaching_back(words)
    {
        let query = query_of(words);
        let found =
            chat_days::turn_back(db, chat_days::Place::In(venue), user_id, &query, words).await;
        if let Some(block) = myriad_merope::remembering::looked_back_in_group_section(&found) {
            sections.push(block);
        }
    }
    let diary = if group {
        Ok(Vec::new())
    } else {
        list_diary_from_sources(
            db,
            user_id,
            RECENT_SPEAKING_DIARY_SOURCES,
            RECENT_LEDGER_LIMIT,
        )
        .await
    };
    if let Ok(notes) = diary {
        let contents: Vec<String> = notes
            .into_iter()
            .map(|note| ingest::compact_summary(&note.content))
            .filter(|content| !content.is_empty())
            .collect();
        if let Some(block) = format_recent_section(&contents) {
            sections.push(block);
        }
    }
    // What they are doing on the site is theirs: not for a group to hear.
    if !group && let Some(block) = format_activity_section(current_activity(&state)) {
        sections.push(block);
    }
    // What the site's owner is playing, to the owner alone.
    if !group {
        if let Some(block) = playing::now_for(user_id, chrono::Utc::now())
            .and_then(|line| format_playing_section(&line))
        {
            sections.push(block);
        }
    }
    // Her state after the last exchange already weighs how she has been and
    // how her day went; the raw facts would say it twice, and can say it
    // differently.
    let compiled = match turn {
        Turn::Chat(_) => inner::current(user_id, present),
        _ => None,
    };
    if compiled.is_none() {
        sections.push(format_mood_section(state.mood, state.arousal));
        if let Some(block) =
            myriad_merope::speaking::format_carried_section(&carried(db, user_id).await)
        {
            sections.push(block);
        }
    }
    // Her inner state goes last, nearest their words, so it is what she
    // answers from; without it, how the words landed stands here instead.
    let inner_block = compiled
        .as_deref()
        .and_then(format_inner_moment_ago_section);
    if inner_block.is_none() {
        if let Some(block) = format_emotion_section(state.emotion, state.emotion_arousal) {
            sections.push(block);
        }
    }
    if !matches!(turn, Turn::Plain) {
        sections.push(speaking_prompts::format_now_section(chrono::Local::now()));
    }
    if !matches!(turn, Turn::Plain) && compiled.is_none() {
        sections.push(self_state::format_day_section(&myself.facts));
    }
    // How hard she has been going at her own things, when it says something.
    if !matches!(turn, Turn::Plain)
        && let Some(block) = pace::conversation_section(db).await
    {
        sections.push(block);
    }
    if let Turn::Chat(words) | Turn::Event(words) = turn {
        let found = crate::services::agent::memory::unified::recall(
            db,
            user_id,
            present,
            Some(words),
            &[crate::services::agent::memory::unified::MemoryKind::Knowledge],
            FOUND_OUT_LIMIT,
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|note| note.content)
        .collect::<Vec<_>>();
        if let Some(block) = format_found_out_section(&found) {
            sections.push(block);
        }
    }
    if !matches!(turn, Turn::Plain) {
        // Her own life: hers, heard wherever she is, in mind when the moment
        // asks for it.
        if her_life {
            if let Some(block) =
                format_own_days_section(&life::recent_days(db, OWN_DAYS_LIMIT).await)
            {
                sections.push(block);
            }
            if let Some(block) = format_self_story_section(&self_story::current(db).await) {
                sections.push(block);
            }
            if let Some(block) = wants::section(&wants::open(db).await, chrono::Utc::now()) {
                sections.push(block);
            }
        }
        // Her own time is about public things, so any audience may hear it.
        // What she is doing right now is part of any moment; what she did
        // lately, of one about her.
        let words = match turn {
            Turn::Chat(words) | Turn::Event(words) => Some(words),
            Turn::Plain => None,
        };
        let lately = if her_life {
            doing::recalled(db, words, DOING_RECENT, DOING_RELATED).await
        } else {
            Vec::new()
        };
        let now = doing::now_text(chrono::Utc::now());
        if let Some(block) = format_doing_section(now.as_deref(), &lately) {
            sections.push(block);
        }
        // What only she and this person share, or she and this group: each
        // heard only where it grew.
        let shared = match present.group_id() {
            Some(venue) => bits::in_group(db, venue, BITS_LIMIT).await,
            None => bits::between(db, user_id, BITS_LIMIT).await,
        };
        if let Some(block) = format_bits_section(&shared, group) {
            sections.push(block);
        }
        // What the last days in this group were like: the group's own.
        if let Some(venue) = present.group_id() {
            let days = bits::days_in(db, venue, GROUP_DAYS).await;
            if let Some(block) = format_group_days_section(&days) {
                sections.push(block);
            }
        }
        // How she comes across: with them, or in this group, each heard only
        // where it was found.
        let lands = match present.group_id() {
            Some(venue) => bits::lands_in(db, venue).await,
            None => bits::lands_with(db, user_id).await,
        };
        if let Some((lands, since)) = lands
            && let Some(block) = myriad_merope::speaking::format_lands_section(
                &lands,
                since.with_timezone(&chrono::Utc),
                chrono::Utc::now(),
                group,
            )
        {
            sections.push(block);
        }
        // What they are to her: private, never in a group.
        if !group {
            if let Some(us) = bits::us(db, user_id).await {
                if let Some(block) = format_us_section(
                    &us.now,
                    us.since.with_timezone(&chrono::Utc),
                    us.before.as_deref(),
                    us.first
                        .as_ref()
                        .map(|(first, at)| (first.as_str(), at.with_timezone(&chrono::Utc))),
                    chrono::Utc::now(),
                ) {
                    sections.push(block);
                }
            }
        }
        // What still stings with them: in private all of it; in a group,
        // what they did there, and what they did in private only by how
        // much it weighs, never what it was.
        let now = chrono::Utc::now();
        match present.group_id() {
            None => {
                if let Some(block) = sore::section(&sore::open_all(db, user_id).await, now) {
                    sections.push(block);
                }
            }
            Some(venue) => {
                if let Some(block) =
                    sore::section(&sore::open_in_group(db, venue, Some(user_id)).await, now)
                {
                    sections.push(block);
                }
                if let Some(block) = sore::carried_section(&sore::open_all(db, user_id).await, now)
                {
                    sections.push(block);
                }
            }
        }
        // What she meant to come back to with them: private, never in a group;
        // in mind as they start talking again, or when their words touch it.
        if !group {
            let open = threads::open(db, user_id).await;
            let touched = words.is_some_and(|words| {
                open.iter().any(|thread| {
                    myriad_merope::remembering::overlap(
                        words,
                        &format!("{} {}", thread.about, thread.then),
                    ) >= 2
                        || words.contains(thread.about.as_str())
                })
            });
            if opening || touched {
                if let Some(block) = threads::section(&open, chrono::Utc::now()) {
                    sections.push(block);
                }
            }
        }
        // What she thinks of what their words touch: hers, the same whoever asks.
        if let Some(words) = words {
            let views = views::touched(db, words, VIEWS_LIMIT).await;
            if let Some(block) = format_views_section(&views) {
                sections.push(block);
            }
        }
    }
    if matches!(turn, Turn::Chat(_)) {
        // One mouth: what was on her mind belongs to the conversation she is
        // now answering in. What she said on her own is in its history
        // (see `with_said_unprompted`).
        if let Turn::Chat(words) = turn {
            // Something they just named that she knows only a little about.
            // Whether she wants to know more is hers to judge.
            let gap =
                crate::services::agent::memory::unified::curiosity_gap(db, user_id, present, words)
                    .await;
            if let Some(block) = gap
                .ok()
                .flatten()
                .and_then(|(gap, known)| format_curious_section(&gap, known))
            {
                sections.push(block);
            }
        }
        let on_mind = (!group)
            .then(|| crate::services::agent::consciousness::last_attention(user_id))
            .flatten();
        if let Some(segment) = on_mind {
            if let Some(block) = format_on_your_mind_section(&segment.inner) {
                sections.push(block);
            }
        }
    }
    sections.extend(inner_block);
    sections
}

pub fn speaking_prompt_plain(sections: &[String]) -> String {
    sections.join("\n\n")
}
