//! Writing to someone first, the way a friend would.
//!
//! Now and then she thinks of someone who is not with her: something they
//! told her was coming up has come (see `threads`), or it has been a while
//! since they talked. Where her words go depends on where they are:
//! - on the site with her panel open: nothing; she is right there, and what
//!   is on her mind comes up when they talk;
//! - on the site elsewhere: a notification with her line, opening her panel;
//! - away, paired in a chat app where she can write first: a message there,
//!   and their answer carries on in the same chat;
//! - away otherwise: the same notification, waiting on the site.
//!
//! Cheap gates come first, and they are what keep her from being a nuisance:
//! they have not asked her not to (「别主动找我」, with its hours), it is not
//! night on the site's clock, she has not written to them first in the last
//! day, they are not in the middle of talking with her, and there is a
//! reason. Then the judgment model decides, as her, whether she
//! would; most of the time she would not. If she would, she writes it in her
//! own voice with their recent talk in front of her, and a thread she wrote
//! about is taken up.

use std::time::Duration;

use chrono::{DateTime, Timelike, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::threads;
use myriad_merope::reach::{
    JUDGE_SCHEMA, MISSED_UNTIL_DAYS, Reason, Route, as_text, awake, judge_schema, judge_system,
    parse_judged, reason, route, writing_first,
};
#[cfg(test)]
use serde_json::Value;

pub const EVENT_KEY: &str = "agent.merope.reach_out";
/// At most once a day each, first words only.
const AT_MOST_EVERY_MINUTES: i64 = 20 * 60;
/// People written to in one pass, at most.
const PER_PASS: usize = 3;
/// The last times she wrote to someone first that she has in mind, and how
/// far back.
const FIRST_WORDS_SHOWN: i64 = 4;
const FIRST_WORDS_WITHIN_DAYS: i64 = 30;
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// One pass over everyone she could write to first: whoever she has
/// talked with in private lately, and whoever is paired in a chat app.
pub async fn tick(db: DatabaseConnection) {
    if !super::is_enabled().await || !awake(chrono::Local::now().hour()) {
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
}

/// Whom she wrote to first and when, in this run: what keeps her from
/// writing again even if keeping the line failed.
static WROTE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<i32, DateTime<Utc>>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn wrote_lately(user_id: i32, now: DateTime<Utc>) -> bool {
    WROTE.lock().is_ok_and(|wrote| {
        wrote
            .get(&user_id)
            .is_some_and(|at| now - *at < chrono::Duration::minutes(AT_MOST_EVERY_MINUTES))
    })
}

/// They have not asked her not to, and she has not written first lately.
async fn quiet_enough(db: &DatabaseConnection, user_id: i32) -> bool {
    if wrote_lately(user_id, Utc::now()) {
        return false;
    }
    let Ok(state) = super::get_or_create_state(db, user_id).await else {
        return false;
    };
    if super::effective_do_not_disturb(&state) {
        return false;
    }
    !super::store::recently_spoke_event(db, user_id, EVENT_KEY, AT_MOST_EVERY_MINUTES)
        .await
        .unwrap_or(true)
}

/// People who have talked with her in private lately.
async fn talked_lately(db: &DatabaseConnection) -> Vec<i32> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT DISTINCT s.user_id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE s.user_id > 0 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
           AND m.role = 'user' AND m.created_at > NOW() - make_interval(days => $1)",
        [(MISSED_UNTIL_DAYS as i32).into()],
    ))
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(|row| row.try_get::<i32>("", "user_id").ok())
    .collect()
}

/// When they last said anything to her in private, anywhere.
async fn last_talk(db: &DatabaseConnection, user_id: i32) -> Option<DateTime<Utc>> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT max(m.created_at) AS at FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL AND m.role = 'user'",
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

async fn reach_out(db: &DatabaseConnection, user_id: i32, here: Route) -> bool {
    let now = Utc::now();
    let threads = threads::open(db, user_id).await;
    let last = last_talk(db, user_id).await;
    let to_tell = super::doing::would_tell(db, last).await;
    let Some(reason) = reason(&threads, last, to_tell, now) else {
        return false;
    };
    let talk = recent_talk(db, user_id).await;
    let Some(about) = would_write(db, user_id, &reason, &talk).await else {
        return false;
    };
    let Some(line) = compose(db, user_id, &about, &talk).await else {
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
    if let Ok(mut wrote) = WROTE.lock() {
        wrote.insert(user_id, Utc::now());
    }
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
    let remembered = super::store::recall_remembered(db, user_id, None, 5)
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
    let input = json!({
        "name": super::resolve_addressee_label(db, user_id).await,
        "localTime": chrono::Local::now().format("%A %H:%M").to_string(),
        "daysSinceYouTalked": reason.days_since,
        "dueNow": reason.due.iter().map(|thread| json!({"about": thread.about, "then": thread.then})).collect::<Vec<_>>(),
        "remembered": remembered,
        "whatTheyAreToYou": super::bits::us(db, user_id).await.map(|us| us.now),
        "yourLastFirstWords": myriad_merope::others::first_words_view(&first_words),
        "yourOwnTime": {
            "now": super::doing::now_text(Utc::now()),
            "wouldTell": reason.to_tell,
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

async fn compose(
    db: &DatabaseConnection,
    user_id: i32,
    about: &str,
    talk: &[crate::services::agent::ConversationMessage],
) -> Option<String> {
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let mut sections = super::speaking_prompt_to_reach(db, user_id, about).await;
    sections.push(writing_first(about));
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
    writing_first(about)
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
        assert!(writing_first("考试").contains("no actions or descriptions in brackets"));
        assert!(judge_system("你是小灯。").contains("Most of the time, no"));
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
