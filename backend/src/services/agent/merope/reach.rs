//! Writing to someone first, the way a friend would.
//!
//! Now and then she thinks of someone who is not with her: something they
//! told her was coming up has come (see `threads`), something of hers she
//! wanted to do with them, show or ask them, something of her own that meets
//! them (they asked for something like it, or are into it), a turtle soup
//! she made they have not tried, or days apart. What only got to her is not
//! a reason to write: nobody texts someone out of the blue about that; it
//! comes up when they next talk.
//! Where her words go depends on where they are:
//! - on the site with her panel open: nothing; she is right there, and what
//!   is on her mind comes up when they talk;
//! - on the site elsewhere: a notification with her line, opening her panel;
//! - away, paired in a chat app where she can write first: a message there,
//!   and their answer carries on in the same chat;
//! - away otherwise: the same notification, waiting on the site.
//!
//! Only a few things are gates: they have not asked her not to (「别主动找
//!我」, with its hours), she is awake by her own night (see `timing`), they
//! are not in the middle of talking with her, and there is a reason. How
//! often she writes is not capped: the judgment model decides, as her, with
//! the facts a friend would weigh (how long since they talked, what they
//! are to her, when she last wrote first and whether they answered); most of
//! the time she would not. The same reason is put to her once, in this run:
//! after a restart it may be put again, with her last first words in front
//! of her. If she would, she writes it in her own voice with their recent
//! talk in front of her, and a thread of theirs she wrote about is taken up.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::threads;
use myriad_merope::reach::{
    JUDGE_SCHEMA, Reason, Route, THINKS_BACK_DAYS, as_text, judge_schema, judge_system,
    parse_judged, reason, route, writing_first,
};
#[cfg(test)]
use serde_json::Value;

pub const EVENT_KEY: &str = "agent.merope.reach_out";
/// People written to in one pass, at most.
const PER_PASS: usize = 3;
/// The last times she wrote to someone first that she has in mind, and how
/// far back.
const FIRST_WORDS_SHOWN: i64 = 4;
const FIRST_WORDS_WITHIN_DAYS: i64 = 30;
/// Her last messages to them when she wrote first, shown as she writes.
const WROTE_BEFORE_SHOWN: u64 = 3;
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// One pass over everyone she could write to first, while she is awake:
/// whoever she has talked with in private lately, and whoever is paired in
/// a chat app.
pub async fn tick(db: DatabaseConnection) {
    if !super::is_enabled().await || super::timing::asleep_now().is_some() {
        return;
    }
    let mut people = crate::services::channel_work::reachable_people(&db).await;
    people.extend(talked_lately(&db).await);
    people.sort_unstable();
    people.dedup();
    let mut written = 0;
    for user_id in people {
        if written >= PER_PASS {
            break;
        }
        let here = route(
            crate::services::agent::consciousness::live_presence_panel_open(user_id),
            crate::services::agent::consciousness::present_users().contains(&user_id),
        );
        if here == Route::Stay || !quiet_enough(&db, user_id).await {
            continue;
        }
        if reach_out(&db, user_id, here).await {
            written += 1;
        }
    }
    // What she meant to come back to in a group, once it is due.
    threads::come_back_in_groups(&db).await;
}

/// The reason she was last asked about, by person, in this run: the same
/// reason is not asked about again until something in it changes.
static ASKED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<i32, String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Whether she has been asked about this already; marks it asked.
fn asked_already(user_id: i32, reason: &Reason) -> bool {
    let key = reason.key();
    let Ok(mut asked) = ASKED.lock() else {
        return true;
    };
    if asked.get(&user_id) == Some(&key) {
        return true;
    }
    if asked.len() >= 4096 {
        asked.clear();
    }
    asked.insert(user_id, key);
    false
}

/// They have not asked her not to be disturbed. When she last wrote first
/// and whether they answered are hers to weigh (`yourLastFirstWords`).
async fn quiet_enough(db: &DatabaseConnection, user_id: i32) -> bool {
    let Ok(state) = super::get_or_create_state(db, user_id).await else {
        return false;
    };
    !super::effective_do_not_disturb(&state)
}

/// People who have talked with her in private lately.
async fn talked_lately(db: &DatabaseConnection) -> Vec<i32> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT DISTINCT s.user_id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE s.user_id > 0 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
           AND m.role = 'user' AND m.created_at > NOW() - make_interval(days => $1)",
        [(THINKS_BACK_DAYS as i32).into()],
    ))
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(|row| row.try_get::<i32>("", "user_id").ok())
    .collect()
}

/// When they last said anything to her, anywhere: in private or in a group
/// they are in with her (yesterday in the group is not "days without a
/// word").
async fn last_talk(db: &DatabaseConnection, user_id: i32) -> Option<DateTime<Utc>> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT max(m.created_at) AS at FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' AND m.role = 'user'",
        [user_id.into()],
    ))
    .await
    .ok()
    .flatten()?
    .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "at")
    .ok()
    .flatten()
    .map(|at| at.with_timezone(&Utc))
}

/// Their recent private talk with her, oldest first, as she would read it.
async fn recent_talk(
    db: &DatabaseConnection,
    user_id: i32,
) -> Vec<crate::services::agent::ConversationMessage> {
    let Some((session, _)) = super::store::latest_open_session(db, user_id)
        .await
        .ok()
        .flatten()
    else {
        return Vec::new();
    };
    crate::services::agent::sessions::load_session_history(db, &session, 8, true)
        .await
        .unwrap_or_default()
}

/// Whether she would write to them first now, and if so why, about what,
/// and her line.
async fn first_words(db: &DatabaseConnection, user_id: i32) -> Option<(Reason, String, String)> {
    let now = super::clock::now();
    let threads = threads::open(db, user_id).await;
    let last = last_talk(db, user_id).await;
    let to_tell = super::doing::would_tell(db, last).await;
    // A puzzle she made that they have not played.
    let table = myriad_merope::soup::Table::Private { user_id }.record_id();
    let puzzle = super::making::untried_at(db, &table).await;
    let to_try = puzzle
        .as_ref()
        .map(|made| format!("{} ({})", made.surface, made.how_it_went()));
    let reason = reason(&threads, last, to_tell, to_try, now)?;
    if asked_already(user_id, &reason) {
        return None;
    }
    let talk = recent_talk(db, user_id).await;
    let about = would_write(db, user_id, &reason, &talk).await?;
    let own = puzzle.as_ref().map(|made| made.surface.as_str());
    let last_talked = last.map(|last| myriad_merope::doing::ago_text(now - last));
    let line = compose(db, user_id, &about, own, &talk, last_talked.as_deref()).await?;
    Some((reason, about, line))
}

#[cfg(test)]
pub(super) async fn first_words_now(
    db: &DatabaseConnection,
    user_id: i32,
) -> Option<(Reason, String, String)> {
    first_words(db, user_id).await
}

async fn reach_out(db: &DatabaseConnection, user_id: i32, here: Route) -> bool {
    let Some((reason, _, line)) = first_words(db, user_id).await else {
        return false;
    };
    let went = match here {
        Route::Stay => None,
        Route::Site => notify_on_site(db, user_id, &line).await.then_some("site"),
        Route::Away => match crate::services::channel_work::say_first(db, user_id, &line).await {
            Some(platform) => Some(platform.slug()),
            None => notify_on_site(db, user_id, &line).await.then_some("site"),
        },
    };
    let Some(went) = went else {
        return false;
    };
    if let Err(error) =
        super::store::insert_proactive(db, user_id, &line, Some(EVENT_KEY), true).await
    {
        tracing::warn!(%error, user_id, "[Merope] wrote to someone first but could not keep the line");
    }
    let taken: Vec<String> = reason.due.iter().map(|thread| thread.id.clone()).collect();
    threads::close(db, user_id, &taken, "reached_out").await;
    tracing::info!(user_id, went, "[Merope] wrote to someone first");
    true
}

/// Her line as a notification on the site: shown now if they are there,
/// waiting for them if not; opening it opens her panel. It is one of theirs
/// to switch off (「她想找你说话」).
async fn notify_on_site(db: &DatabaseConnection, user_id: i32, line: &str) -> bool {
    use crate::services::agent::notification_preferences::{
        ACTION_OPEN_AGENT, NotificationEventKey,
    };
    use crate::services::agent::notifications::{
        Notification, NotificationPriority, NotificationType, get_notification_manager,
    };
    let Some(manager) = get_notification_manager() else {
        return false;
    };
    let title = super::store::get_persona(db)
        .await
        .ok()
        .flatten()
        .map(|persona| persona.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Merope".to_string());
    let session_id = super::store::latest_open_session(db, user_id)
        .await
        .ok()
        .flatten()
        .map(|(id, _)| id);
    let notification = Notification::new(
        user_id,
        NotificationType::SystemInfo,
        NotificationPriority::Normal,
        title,
        line,
    )
    .with_event(
        NotificationEventKey::MeropeReachOut,
        json!({ "action": ACTION_OPEN_AGENT, "session_id": session_id }),
    );
    manager.notify(notification).await
}

async fn would_write(
    db: &DatabaseConnection,
    user_id: i32,
    reason: &Reason,
    talk: &[crate::services::agent::ConversationMessage],
) -> Option<String> {
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let remembered = crate::services::agent::memory::unified::quietly(
        super::store::recall_remembered(db, user_id, None, 5),
    )
    .await
    .unwrap_or_default();
    let now = Utc::now();
    let first_words: Vec<(String, Option<bool>)> = super::store::first_words(
        db,
        EVENT_KEY,
        Some(user_id),
        now - chrono::Duration::days(FIRST_WORDS_WITHIN_DAYS),
        myriad_merope::others::ANSWERED_WITHIN_HOURS,
        FIRST_WORDS_SHOWN,
    )
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(_, at, answered)| (myriad_merope::doing::ago_text(now - at), answered))
    .collect();
    let soup_games = match reason.to_try {
        Some(_) => soup_games_with(db, user_id).await,
        None => 0,
    };
    let input = json!({
        "name": super::resolve_addressee_label(db, user_id).await,
        "localTime": chrono::Local::now().format("%A %H:%M").to_string(),
        "daysSinceYouTalked": reason.days_since,
        "dueNow": reason.due.iter().map(|thread| json!({"about": thread.about, "then": thread.then})).collect::<Vec<_>>(),
        "youWantedTo": reason.wished.iter().map(|thread| json!({"about": thread.about, "then": thread.then})).collect::<Vec<_>>(),
        "remembered": remembered,
        "whatTheyAreToYou": super::bits::us(db, user_id).await.map(|us| us.now),
        "yourLastFirstWords": myriad_merope::others::first_words_view(&first_words),
        "yourOwnTime": {
            "now": super::doing::now_text(Utc::now()),
            "wouldTell": reason.to_tell,
            "yourPuzzle": reason.to_try.as_ref().map(|puzzle| json!({
                "puzzle": puzzle,
                "gamesOfTurtleSoupWithYou": soup_games,
            })),
        },
        "recentTalk": talk.iter().rev().take(6).rev()
            .map(|message| format!("{}: {}", if message.role == "user" { "they" } else { "you" }, message.content.chars().take(200).collect::<String>()))
            .collect::<Vec<_>>(),
    })
    .to_string();
    let raw = super::call::Ask::new(super::call::Voice::Judge, user_id, "reach_judge")
        .within(CALL_TIMEOUT)
        .json_raw(&judge_system(&soul), &input, JUDGE_SCHEMA, &judge_schema())
        .await
        .ok()?;
    parse_judged(&raw).flatten()
}

/// How many games of turtle soup they have played with her, as she
/// remembers them.
async fn soup_games_with(db: &DatabaseConnection, user_id: i32) -> i64 {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT count(*) AS games FROM agent_memories \
         WHERE user_id = $1 AND source = 'game' AND invalid_at IS NULL",
        [user_id.into()],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|row| row.try_get::<i64>("", "games").ok())
    .unwrap_or(0)
}

/// Her first words: `own` is a puzzle of hers she might offer them.
async fn compose(
    db: &DatabaseConnection,
    user_id: i32,
    about: &str,
    own: Option<&str>,
    talk: &[crate::services::agent::ConversationMessage],
    last_talked: Option<&str>,
) -> Option<String> {
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let mut sections = super::speaking_prompt_to_reach(db, user_id, about).await;
    sections.extend(own.map(myriad_merope::making::writing_about_own));
    // What she wrote them first the last times, as she would glance up the
    // chat before texting again.
    let now = super::clock::now();
    let before: Vec<(String, String)> =
        super::store::first_words_said(db, EVENT_KEY, user_id, WROTE_BEFORE_SHOWN)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(at, line)| (myriad_merope::doing::ago_text(now - at), line))
            .collect();
    sections.extend(myriad_merope::reach::wrote_before(&before));
    sections.push(writing_first(about, last_talked));
    let prompt = crate::services::agent::chat_prompt::build_chat_lite_prompt_with_perception(
        &soul,
        &sections.join("\n\n"),
        talk,
        "(nothing yet: you are writing first)",
        "",
    );
    let raw = super::call::Ask::new(super::call::Voice::Hers, user_id, "reach_out")
        .within(CALL_TIMEOUT)
        .model()
        .await
        .ok()?
        .say(&prompt)
        .await
        .ok()?;
    as_text(&raw)
}

#[cfg(test)]
pub(crate) fn judge_probe_contract(soul: &str) -> (String, Value) {
    (judge_system(soul), judge_schema())
}

#[cfg(test)]
pub(crate) fn judge_verdict(raw: &str) -> Option<Option<String>> {
    parse_judged(raw)
}

#[cfg(test)]
pub(crate) fn writing_first_section(about: &str) -> String {
    writing_first(about, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn she_writes_a_text_not_a_scene() {
        assert_eq!(
            as_text("（眼睛一亮）考完了没？\n\n[[music:play]]怎么样").as_deref(),
            Some("考完了没？\n怎么样")
        );
        assert!(as_text("（笑）").is_none());
        let first = writing_first("考试", Some("5 days ago"));
        assert!(first.contains("no actions or descriptions in brackets"));
        assert!(first.contains("You last talked 5 days ago"));
        assert!(!writing_first("考试", None).contains("You last talked"));
        assert!(
            judge_system("你是小灯。")
                .contains("Never just to be present, and never to push them.")
        );
        assert_eq!(
            parse_judged(r#"{"reach_out":true,"about":"问考试考得怎么样"}"#),
            Some(Some("问考试考得怎么样".into()))
        );
        assert_eq!(
            parse_judged(r#"{"reach_out":false,"about":null}"#),
            Some(None)
        );
        assert_eq!(parse_judged("嗯"), None);
    }
}
