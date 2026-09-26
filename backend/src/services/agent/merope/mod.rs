//! Merope: site persona, per-addressee state, hidden proactive speech.

mod appraisal;
pub mod bits;
pub mod chat_remember;
pub mod curiosity;
pub mod doing;
pub mod gates;
pub mod hearing;
pub mod ingest;
pub(crate) mod inner;
pub mod life;
pub mod motion;
pub mod motion_local;
pub mod motion_preview;
pub mod onboarding_ai;
pub mod onboarding_prompts;
pub mod outfit_overlay;
pub mod playing;
mod priming;
pub mod reach;
pub mod report_dna;
pub mod self_state;
pub mod soup;
pub mod speaking_prompts;
pub mod state;
pub mod store;
pub mod strangers;
pub mod threads;
pub mod views;
pub mod wander;

pub use chat_remember::spawn_chat_remember;
pub use curiosity::spawn_curiosity;
pub use ingest::{
    allow_existing_notify, is_enabled, spawn as spawn_ingest, spawn_diary, spawn_presence,
    tick_speak_intents,
};
pub use motion::{
    MotionContext, MotionPhase, PerformanceDirective, direct_motion, local_directive,
    refine_motion, resolve_round_motion_style,
};
pub use myriad_merope::RigStateSummary;
pub use outfit_overlay::{apply_model_wear_directive, chat_wardrobe_section, overlay_outfit_id};
pub use store::{
    JsonDocumentUpdate, PersonaContractUpdate, PortraitUpdate, acquire_avatar_generation,
    acquire_portrait_generation, avatar_generation_is_pending, clear_persona_on,
    complete_avatar_generation, complete_portrait_generation, credit_music_listening,
    generation_inputs_changed, get_or_create_state, get_persona, get_persona_on, insert_diary,
    insert_proactive, latest_diary, list_diary_from_sources, normalize_persona_fields,
    portrait_generation_is_pending, promote_activity, recent_proactive, release_avatar_generation,
    release_portrait_generation, rewrite_persona_media_urls, set_activity, set_dnd_schedule,
    set_do_not_disturb, sticker_avatar_asset_id, update_affect, upsert_persona_on,
};

/// Logged-in users only. Guests use negative ids; heartbeat is `SYSTEM_USER_ID` (0).
pub fn is_logged_in_addressee(user_id: i32) -> bool {
    user_id > 0
}

pub async fn mark_activity(db: &sea_orm::DatabaseConnection, user_id: i32, activity: &str) {
    if !is_logged_in_addressee(user_id) {
        return;
    }
    if !is_enabled().await {
        return;
    }
    let executing = crate::services::agent::run_hub::user_executing_run_count(user_id).await;
    if activity == "idle" && executing > 1 {
        return;
    }
    if executing > 1 {
        let _ = promote_activity(db, user_id, activity).await;
    } else {
        let _ = set_activity(db, user_id, activity).await;
    }
}

use crate::models::entities::agent_persona;

/// Soul text for user-facing speech: site persona when the flag is on, else SOUL.md.
pub async fn resolve_speaking_soul() -> Option<String> {
    let enabled = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved();
    if enabled {
        if let Ok(db) = crate::services::process_db::database() {
            if let Ok(Some(persona)) = get_persona(&db).await {
                if let Some(text) = format_persona(&persona) {
                    return Some(text);
                }
            }
        }
    }
    crate::services::agent::identity::get_identity()
        .await
        .and_then(|id| id.soul)
}

pub fn refuse_new_task_message(mood_before: Option<f64>) -> Option<String> {
    if mood_before.is_some_and(is_extremely_low) {
        Some(
            "I'm in a very low mood and don't want to take on anything new. Let's just talk."
                .to_string(),
        )
    } else {
        None
    }
}

pub async fn maybe_refuse_new_task(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Option<String> {
    if !is_logged_in_addressee(user_id) {
        return None;
    }
    if !crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved()
    {
        return None;
    }
    let state = get_or_create_state(db, user_id).await.ok()?;
    refuse_new_task_message(Some(state.mood))
}

/// Returns the persisted mood transition for this utterance, if Merope applied.
pub async fn note_user_turn(
    db: &sea_orm::DatabaseConnection,
    request: &crate::services::agent::UserRequest,
    utterance_index: u32,
) -> Option<(MoodTransition, chrono::DateTime<chrono::FixedOffset>)> {
    let user_id = request.user_id;
    let text = &request.raw_input;
    if !is_logged_in_addressee(user_id) {
        return None;
    }
    if !crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved()
    {
        return None;
    }
    let (praised, scolded) = detect_mood_cue(text);
    let (previous, saved) = update_affect(db, user_id, true, |affect| {
        apply_user_utterance(affect, utterance_index, praised, scolded);
    })
    .await
    .ok()?;
    let after = store::affect_from_state(&saved);
    if !praised && !scolded && !text.trim().is_empty() {
        appraisal::spawn(db.clone(), request, &saved);
    }
    if !is_extremely_low(previous.mood) && is_extremely_low(after.mood) {
        spawn_ingest(
            user_id,
            "agent.merope.mood_floor",
            "跟这个人的心情掉到了极低",
        );
    }
    let cause = if scolded {
        "user_scold"
    } else if praised {
        "user_praise"
    } else {
        "user_turn"
    };
    let transition = MoodTransition::from_affect(
        &previous,
        &store::affect_from_state(&saved),
        cause,
        saved
            .updated_at
            .with_timezone(&chrono::Utc)
            .timestamp_millis(),
    );
    // Carry the actual persisted input anchor, not a later mood revision.
    Some((transition, saved.last_user_message_at?))
}

/// What surrounded a chat turn, for the calls that follow it: what she said
/// just before, what was on their screen or playing, whether it was a move in
/// a game, and how many images came with it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnContext {
    pub before: Option<String>,
    pub scene: Option<String>,
    pub in_game: bool,
    pub images: usize,
}

pub fn turn_context(request: &crate::services::agent::UserRequest) -> TurnContext {
    let context = request.context.as_ref();
    let before = context
        .and_then(|context| context.conversation_history.as_ref())
        .and_then(|history| {
            history
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
        })
        .map(|message| {
            crate::services::agent::chat_prompt::chat_safe_content(&message.content)
                .chars()
                .take(300)
                .collect::<String>()
        })
        .filter(|line| !line.trim().is_empty());
    let custom = context.and_then(|context| context.custom_data.as_ref());
    let scene = crate::services::agent::chat_prompt::format_chat_scene(
        custom.and_then(|data| data.get("perception")),
        None,
        &request.raw_input,
    );
    TurnContext {
        before,
        scene: (!scene.trim().is_empty()).then(|| scene.chars().take(600).collect()),
        in_game: soup::in_game(request),
        images: context.map(|context| context.images.len()).unwrap_or(0),
    }
}

/// After the persona is deleted: nothing of her stays in memory either, so
/// the next one does not carry on her song, game, state or thoughts.
pub fn forget_in_memory() {
    doing::forget();
    soup::forget();
    inner::forget();
    views::forget();
    priming::forget();
    wander::forget();
}

/// After a chat reply, let her state catch up with the exchange; the next
/// turn starts from it without waiting.
pub fn spawn_inner_after(
    db: sea_orm::DatabaseConnection,
    request: &crate::services::agent::UserRequest,
    reply: &str,
) {
    if is_logged_in_addressee(request.user_id) {
        inner::spawn_after(db, request, reply);
    }
}

/// Chat writes this before the model; Work writes after `plan_for`.
pub async fn note_chat_diary(db: &sea_orm::DatabaseConnection, user_id: i32, text: &str) {
    if !is_logged_in_addressee(user_id) {
        return;
    }
    if !is_enabled().await {
        return;
    }
    maybe_write_chat_diary(db, user_id, text).await;
}

const CHAT_DIARY_MIN_CHARS: usize = 8;
const CHAT_DIARY_GAP_MINUTES: i64 = 20;

pub fn should_write_chat_diary(text: &str, last_chat_age_minutes: Option<i64>) -> bool {
    let summary = crate::services::agent::merope::ingest::compact_summary(text);
    if summary.chars().count() < CHAT_DIARY_MIN_CHARS {
        return false;
    }
    last_chat_age_minutes.is_none_or(|age| age >= CHAT_DIARY_GAP_MINUTES)
}

async fn maybe_write_chat_diary(db: &sea_orm::DatabaseConnection, user_id: i32, text: &str) {
    let last_age = match latest_diary(db, user_id, store::DIARY_SOURCE_CHAT).await {
        Ok(Some(last)) => {
            Some((chrono::Utc::now() - last.created_at.with_timezone(&chrono::Utc)).num_minutes())
        }
        Ok(None) => None,
        Err(_) => return,
    };
    if !should_write_chat_diary(text, last_age) {
        return;
    }
    let summary = crate::services::agent::merope::ingest::compact_summary(text);
    let _ = insert_diary(db, user_id, &summary, store::DIARY_SOURCE_CHAT).await;
}

/// Public face: 人设 off → Agent (product). Empty 人设 name → Arael.
pub fn public_persona_name(is_enabled: bool, stored_name: Option<&str>) -> String {
    if !is_enabled {
        return "Agent".to_string();
    }
    stored_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Arael")
        .to_string()
}

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

pub use speaking_prompts::{
    addressee_speaking_section, format_activity_section, format_bits_section,
    format_brought_to_mind_section, format_curious_section, format_doing_section,
    format_emotion_section, format_found_out_section, format_inner_moment_ago_section,
    format_mood_section, format_on_your_mind_section, format_own_days_section, format_persona,
    format_playing_section, format_recent_section, format_remembered_section, format_since_section,
    format_views_section, group_speaking_section, guest_speaking_section, mood_tone_instruction,
};

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
const RECENT_SPEAKING_DIARY_SOURCES: &[&str] = &[store::DIARY_SOURCE_CHAT];

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
    let Ok(state) = get_or_create_state(db, user_id).await else {
        return sections;
    };
    let myself = self_state::current(db).await;
    // Only a chat turn (it has the person's words) carries its train of
    // thought to the next turn; other readers see memory without moving it.
    let remembered = match turn {
        Turn::Chat(words) | Turn::Event(words) => store::recall_remembered_split(
            db,
            user_id,
            present,
            Some(words),
            REMEMBERED_PROMPT_LIMIT,
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
    if let Some(block) = format_activity_section(current_activity(&state)) {
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
    sections.push(format_mood_section(state.mood, state.arousal));
    // Her state after the last exchange already weighs how she has been and
    // how her day went; the raw facts would say it twice.
    let compiled = match turn {
        Turn::Chat(_) => inner::current(user_id, present),
        _ => None,
    };
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
        if let Some(block) = format_own_days_section(&life::recent_days(db, OWN_DAYS_LIMIT).await) {
            sections.push(block);
        }
        // Her own time is about public things, so any audience may hear it.
        let words = match turn {
            Turn::Chat(words) | Turn::Event(words) => Some(words),
            Turn::Plain => None,
        };
        let lately = doing::recalled(db, words, DOING_RECENT, DOING_RELATED).await;
        let now = doing::current().map(|doing| doing::now_line(&doing, chrono::Utc::now()));
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
        // What she meant to come back to with them: private, never in a group.
        if !group {
            if let Some(block) =
                threads::section(&threads::open(db, user_id).await, chrono::Utc::now())
            {
                sections.push(block);
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

pub fn has_custom_persona(persona: &agent_persona::Model) -> bool {
    format_persona(persona).is_some()
}

pub use gates::{IngestDecision, IngestSight, decide_ingest, is_valuable_event};
pub use state::{
    ACTIVITY_STALE_SECS, Affect, AffectBaseline, DEFAULT_AROUSAL, DEFAULT_MOOD, MOOD_FLOOR,
    MUSIC_LISTENING_MIN_SECS, MoodTransition, ORIGIN, apply_task_outcome, apply_user_utterance,
    clamp_mood, detect_mood_cue, effective_activity, is_extremely_low, mood_band,
};

/// The activity to act on, with a stale one read as idle.
pub fn current_activity(state: &crate::models::entities::agent_addressee_state::Model) -> &str {
    let age =
        (chrono::Utc::now() - state.activity_updated_at.with_timezone(&chrono::Utc)).num_seconds();
    effective_activity(&state.activity, age)
}

pub fn activity_is_busy(activity: &str) -> bool {
    matches!(activity, "thinking" | "talking" | "working")
}

pub fn parse_clock_minute(raw: &str) -> Option<i32> {
    let raw = raw.trim();
    let (hour, minute) = raw.split_once(':')?;
    let hour: i32 = hour.parse().ok()?;
    let minute: i32 = minute.parse().ok()?;
    if (0..24).contains(&hour) && (0..60).contains(&minute) {
        Some(hour * 60 + minute)
    } else {
        None
    }
}

pub fn format_clock_minute(minute: i32) -> Option<String> {
    if !(0..1440).contains(&minute) {
        return None;
    }
    Some(format!("{:02}:{:02}", minute / 60, minute % 60))
}

pub fn minute_in_window(now: i32, start: i32, end: i32) -> bool {
    if start == end {
        return false;
    }
    if start < end {
        now >= start && now < end
    } else {
        now >= start || now < end
    }
}

pub fn effective_do_not_disturb(
    state: &crate::models::entities::agent_addressee_state::Model,
) -> bool {
    if state.do_not_disturb {
        return true;
    }
    match (state.dnd_start_minute, state.dnd_end_minute) {
        (Some(start), Some(end)) if (0..1440).contains(&start) && (0..1440).contains(&end) => {
            use chrono::Timelike;
            let now = chrono::Local::now();
            let minute = (now.hour() * 60 + now.minute()) as i32;
            minute_in_window(minute, start, end)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{activity_is_busy, minute_in_window, parse_clock_minute, should_write_chat_diary};

    #[test]
    fn every_live_agent_activity_blocks_proactive_speech() {
        assert!(activity_is_busy("thinking"));
        assert!(activity_is_busy("talking"));
        assert!(activity_is_busy("working"));
        assert!(!activity_is_busy("idle"));
    }

    #[test]
    fn chat_diary_skips_short_and_recent_turns() {
        assert!(!should_write_chat_diary("嗯", None));
        assert!(should_write_chat_diary("今天晚上想打会独立游戏", None));
        assert!(!should_write_chat_diary("今天晚上想打会独立游戏", Some(5)));
        assert!(should_write_chat_diary("今天晚上想打会独立游戏", Some(20)));
    }

    #[test]
    fn addressee_label_prefers_display_name() {
        assert_eq!(
            super::format_addressee_label(7, Some("  瞳  "), Some("hitomi")),
            "瞳"
        );
        assert_eq!(
            super::format_addressee_label(7, Some("   "), Some("hitomi")),
            "hitomi"
        );
        assert_eq!(super::format_addressee_label(7, None, None), "User#7");
        assert_eq!(
            super::format_addressee_label(-12, Some("瞳"), None),
            "Guest"
        );
        assert_eq!(super::public_persona_name(false, Some("瞳")), "Agent");
        assert_eq!(super::public_persona_name(true, Some("  瞳  ")), "瞳");
        assert_eq!(super::public_persona_name(true, Some("   ")), "Arael");
        assert_eq!(super::public_persona_name(true, None), "Arael");
        assert!(!super::is_logged_in_addressee(0));
        assert!(!super::is_logged_in_addressee(-1));
        assert!(super::is_logged_in_addressee(1));
    }

    #[test]
    fn dnd_window_covers_same_day_and_overnight() {
        assert_eq!(parse_clock_minute("22:30"), Some(22 * 60 + 30));
        assert!(minute_in_window(23 * 60, 22 * 60, 7 * 60));
        assert!(minute_in_window(6 * 60, 22 * 60, 7 * 60));
        assert!(!minute_in_window(12 * 60, 22 * 60, 7 * 60));
        assert!(minute_in_window(13 * 60, 12 * 60, 14 * 60));
        assert!(!minute_in_window(14 * 60, 12 * 60, 14 * 60));
        assert!(!minute_in_window(12 * 60, 12 * 60, 12 * 60));
    }

    #[test]
    fn empty_persona_is_not_custom() {
        let blank = crate::models::entities::agent_persona::Model {
            id: "site".into(),
            name: "  ".into(),
            personality: String::new(),
            persona_json: None,
            visual_profile: None,
            portrait_asset_id: None,
            portrait_generation: None,
            avatar_asset_id: None,
            avatar_generation: None,
            updated_by: None,
            updated_at: chrono::Utc::now().into(),
        };
        assert!(super::format_persona(&blank).is_none());
        assert!(!super::has_custom_persona(&blank));
        let named = crate::models::entities::agent_persona::Model {
            name: "瞳".into(),
            ..blank.clone()
        };
        let named_text = super::format_persona(&named).unwrap();
        assert!(named_text.starts_with("You are 瞳."));
        assert!(named_text.contains(super::speaking_prompts::PERSONA_SPEAKING_CONTRACT));
        assert!(super::has_custom_persona(&named));
    }

    #[test]
    fn persona_fields_trim_whitespace() {
        let (name, personality) =
            crate::services::agent::merope::store::normalize_persona_fields("  瞳  ", "  认真  ");
        assert_eq!(name, "瞳");
        assert_eq!(personality, "认真");
        let (empty, _) =
            crate::services::agent::merope::store::normalize_persona_fields(" \t ", "");
        assert!(empty.is_empty());
    }

    #[test]
    fn mood_tone_stays_quiet_about_the_number() {
        assert!(super::refuse_new_task_message(Some(10.0)).is_some());
        assert!(super::refuse_new_task_message(Some(10.1)).is_none());
        assert!(super::refuse_new_task_message(None).is_none());
        assert!(
            super::speaking_prompts::PERSONA_SPEAKING_CONTRACT.contains("Do not name the mood")
        );
        assert!(super::mood_tone_instruction(8.0, 48.0).contains("very low"));
        assert!(super::mood_tone_instruction(30.0, 40.0).contains("a bit low"));
        assert!(super::mood_tone_instruction(30.0, 70.0).contains("on edge"));
        assert!(super::mood_tone_instruction(90.0, 48.0).contains("at ease"));
        assert!(super::mood_tone_instruction(90.0, 70.0).contains("bright"));
        let section = super::format_mood_section(72.4, 48.0);
        assert!(!section.contains("72/100"));
        assert!(!section.contains("72.4"));
    }

    #[test]
    fn diary_section_skips_empty_and_compacts() {
        assert!(super::format_recent_section(&[]).is_none());
        let block = super::format_remembered_section(&["今天晚上想打独立游戏".into()]).unwrap();
        assert!(block.contains("## About this person"));
        assert!(block.contains("facts you kept"));
        assert!(block.contains("- 今天晚上想打独立游戏"));
        assert!(
            super::format_recent_section(&["Steam 解锁了成就".into()])
                .unwrap()
                .contains("## Recently")
        );
    }

    #[test]
    fn speaking_recent_omits_event_diary_and_keeps_remembered() {
        assert_eq!(
            super::RECENT_SPEAKING_DIARY_SOURCES,
            &[super::store::DIARY_SOURCE_CHAT]
        );
        assert!(!super::RECENT_SPEAKING_DIARY_SOURCES.contains(&super::store::DIARY_SOURCE_EVENT));
        let remembered = super::format_remembered_section(&["晚上想打独立游戏".into()]).unwrap();
        let event_line = "正在收尾一篇文章，还差最后一段";
        let prompt = super::speaking_prompt_plain(&[remembered]);
        assert!(prompt.contains("## About this person"));
        assert!(prompt.contains("晚上想打独立游戏"));
        assert!(!prompt.contains(event_line));
        let recent_src = include_str!("mod.rs")
            .split("async fn speaking_prompt_from_db")
            .nth(1)
            .and_then(|rest| rest.split("pub fn speaking_prompt_plain").next())
            .unwrap();
        assert!(recent_src.contains("RECENT_SPEAKING_DIARY_SOURCES"));
        assert!(recent_src.contains("format_remembered_section"));
        assert!(!recent_src.contains("DIARY_SOURCE_EVENT"));
    }

    /// One diary table is safe only while every read names its source.
    ///
    /// `remember` holds facts the user stated; `event` and `chat` hold
    /// summaries the platform wrote about them. An unscoped "latest row" would
    /// let one arrive where the other is expected, which is the only way the
    /// shared table could actually hurt — so the query cannot express it.
    #[test]
    fn every_diary_read_names_its_source() {
        let store = include_str!("store.rs");
        assert!(
            !store.contains("source: Option<&str>"),
            "latest_diary accepts an unscoped read again"
        );
        for signature in [
            "pub async fn latest_diary(",
            "pub async fn list_diary_from_sources(",
        ] {
            let body = store
                .split(signature)
                .nth(1)
                .unwrap_or_else(|| panic!("{signature} is gone"));
            assert!(
                body.contains("Column::Source"),
                "{signature} no longer filters by source"
            );
        }
    }

    #[test]
    fn what_she_said_on_her_own_sits_in_the_history_by_time() {
        use crate::services::agent::ConversationMessage;
        let at = |minute: u32| {
            chrono::DateTime::parse_from_rfc3339(&format!("2026-09-25T10:{minute:02}:00Z"))
                .unwrap()
                .with_timezone(&chrono::Utc)
        };
        let message = |role: &str, content: &str, minute: u32| ConversationMessage {
            role: role.into(),
            content: content.into(),
            created_at: Some(at(minute).to_rfc3339()),
        };
        let history = vec![
            message("user", "早", 0),
            message("assistant", "早啊", 1),
            message("user", "我去忙了", 2),
        ];
        let said = [
            (at(20), "周报理好了，放资料库了".to_string()),
            (at(1), "早啊".to_string()),
        ];
        let merged = super::merge_said_unprompted(&history, &said);
        let lines: Vec<(&str, &str)> = merged
            .iter()
            .map(|message| (message.role.as_str(), message.content.as_str()))
            .collect();
        assert_eq!(
            lines,
            vec![
                ("user", "早"),
                ("assistant", "早啊"),
                ("user", "我去忙了"),
                ("assistant", "周报理好了，放资料库了"),
            ],
            "by time, and never twice"
        );
        let untimed = vec![ConversationMessage {
            role: "user".into(),
            content: "在吗".into(),
            created_at: None,
        }];
        let merged = super::merge_said_unprompted(&untimed, &said[..1]);
        assert_eq!(merged.last().unwrap().content, "周报理好了，放资料库了");
    }

    #[test]
    fn guest_and_addressee_sections_name_the_other_person() {
        assert!(super::guest_speaking_section().contains("guest"));
        assert!(super::addressee_speaking_section("瞳").contains("speaking to 瞳"));
        assert_eq!(
            super::speaking_prompt_plain(&[
                super::addressee_speaking_section("瞳"),
                super::format_mood_section(70.0, 48.0)
            ]),
            format!(
                "{}\n\n{}",
                super::addressee_speaking_section("瞳"),
                super::format_mood_section(70.0, 48.0)
            )
        );
    }
}
