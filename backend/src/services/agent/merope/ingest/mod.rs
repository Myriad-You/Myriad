//! Named-event speech.
//!
//! Chain (observation never decides):
//! 1. Page inbound writes live presence (`POST /agent/presence`). A revival
//!    onto the page may spawn the greeting *event*.
//! 2. Named events call `spawn` → `ingest` (produce): diary + speak intent.
//! 3. `tick_speak_intents` (redeem) turns an intent into a sentence.
//! 4. Delivery is two independent channels: `live_speech` (face) when on the
//!    page; notification when not looking at the panel.
//!
//! Produce does not write a sentence. Redeem does not call `consider_event`.

mod delivery_claim;
mod in_person;
mod produce;
mod redeem;
mod told;

use sea_orm::DatabaseConnection;

use super::gates::IngestSight;
use super::is_logged_in_addressee;
use super::store::{insert_remembered_from, insert_remembered_if_new, latest_open_session};
use super::{activity_is_busy, current_activity, effective_do_not_disturb};
use crate::models::entities::agent_addressee_state;
use crate::services::agent::run_hub;

pub use myriad_merope::diary::{compact_summary, redact_event_text};
pub use produce::{ingest, spawn, spawn_presence};
pub use told::{Told, spawn_told};
#[cfg(test)]
pub(crate) use produce::apply_task_mood;
pub use redeem::tick_speak_intents;
pub(crate) use in_person::{InPerson, in_person, say_first_on_app};
#[cfg(test)]
pub(crate) use in_person::say_on_app;

const SAME_EVENT_MINUTES: i64 = 15;

pub async fn is_enabled() -> bool {
    #[cfg(test)]
    if let Some(away) = crate::services::channel_group::g0::away() {
        return !away;
    }
    // She is not on the site (moving, living on another end, moved out):
    // nothing of her runs here, as with her switch off (H3e-9).
    if !crate::services::persona_sync::lease::place().here() {
        return false;
    }
    crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved()
}

pub async fn latest_session_id_for(user_id: i32) -> Option<String> {
    let db = crate::services::process_db::database().ok()?;
    latest_open_session(&db, user_id)
        .await
        .ok()
        .flatten()
        .map(|(id, _)| id)
}

pub async fn allow_existing_notify(user_id: i32) -> bool {
    if !is_logged_in_addressee(user_id) {
        return true;
    }
    if !is_enabled().await {
        return true;
    }
    // Looking at the Agent panel: existing producers skip every Agent
    // notification, including progress. Speech still goes through ingest → face.
    // On the page with the panel closed still notifies. Do-not-disturb gates
    // speech in `decide_ingest`, not here.
    !crate::services::agent::consciousness::live_presence_panel_open(user_id)
}

pub(crate) async fn current_sight(
    user_id: i32,
    state: &agent_addressee_state::Model,
) -> IngestSight {
    // Here on any surface: on the Mac app as much as on a page.
    let here = crate::services::agent::consciousness::owner_here(user_id);
    IngestSight {
        on_page: here.present,
        panel_open: here.panel_open,
        executing: run_hub::user_has_executing_run(user_id).await,
        working: activity_is_busy(current_activity(state)),
        do_not_disturb: effective_do_not_disturb(state),
    }
}

pub(crate) fn log_skip(user_id: i32, event_key: &str, reason: &'static str) {
    tracing::info!(user_id, event_key, reason, "[Merope] ingest skipped");
}

pub fn is_trivial_line(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.chars().count() < 2 || trimmed.starts_with('{')
}

/// Compact a candidate persona-memory fact and skip empty or duplicate text.
///
/// `existing` is already-stored remember content for this addressee. Comparison
/// uses the same compact form ingest writes, so ledger rows are not involved.
pub fn persona_remember_insert(candidate: &str, existing: &[String]) -> Option<String> {
    let compact = compact_summary(candidate);
    if compact.is_empty() {
        return None;
    }
    let duplicate = existing.iter().any(|fact| compact_summary(fact) == compact);
    if duplicate { None } else { Some(compact) }
}

/// `event` is what happened, kept as where she gathered it from; `origin`
/// is the device that told her, when a child end did.
pub(crate) async fn persist_persona_remember(
    db: &DatabaseConnection,
    user_id: i32,
    candidate: Option<&str>,
    event: &str,
    origin: Option<&str>,
) {
    let Some(candidate) = candidate else {
        return;
    };
    let written = match origin {
        None => insert_remembered_if_new(db, user_id, candidate, Some(event)).await,
        Some(origin) => insert_remembered_from(db, user_id, candidate, Some(event), origin).await,
    };
    if let Err(error) = written {
        tracing::debug!(%error, user_id, "[Merope] persona memory write skipped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_remember_insert_skips_empty_and_duplicate_compact_text() {
        assert_eq!(persona_remember_insert("{\"token\":\"abc\"}", &[]), None);
        assert_eq!(persona_remember_insert("   ", &[]), None);
        let kept = persona_remember_insert("  晚上想打独立游戏  ", &[]).unwrap();
        assert_eq!(kept, "晚上想打独立游戏");
        assert_eq!(
            persona_remember_insert("晚上想打独立游戏", &[kept.clone()]),
            None
        );
        assert_eq!(
            persona_remember_insert("  晚上想打独立游戏  ", &["晚上想打独立游戏".into()]),
            None
        );
        assert_eq!(
            persona_remember_insert("早上喝美式", &["晚上想打独立游戏".into()]).as_deref(),
            Some("早上喝美式")
        );
    }

    #[test]
    fn json_shaped_speech_is_trivial() {
        assert!(is_trivial_line("{\"line\":\"hi\"}"));
        assert!(!is_trivial_line("刚才那件事做成了。"));
    }

    #[test]
    fn speak_memory_fact_is_kept_when_spoken_line_would_be_skipped() {
        let speech = "晚上好。";
        let memory = "晚上想打独立游戏";
        assert!(!is_trivial_line(speech));
        assert_eq!(speech.trim(), "晚上好。");
        assert_eq!(
            persona_remember_insert(memory, &[]).as_deref(),
            Some("晚上想打独立游戏")
        );
        assert_eq!(
            persona_remember_insert(memory, &["晚上想打独立游戏".into()]),
            None
        );
    }

    #[test]
    fn persist_remember_writes_persona_memory_not_event_ledger() {
        let src = include_str!("mod.rs");
        assert!(src.contains("insert_remembered_if_new(db, user_id, candidate, Some(event))"));
        assert!(!src.contains("insert_diary(db, user_id, memory, \"event\")"));
        assert!(!src.contains("insert_diary(db, user_id, &memory, \"event\")"));
    }

    /// Someone on the Mac app is here for what she says and whether she
    /// notifies, as on a page (an isolated schema; skipped without a test
    /// database).
    #[tokio::test]
    async fn the_owner_on_the_app_is_in_sight() {
        use crate::services::agent::consciousness::{
            forget_native_presence, remember_native_presence,
        };
        use sea_orm::ConnectionTrait;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let user = 1_930_778;
        fixture
            .db
            .execute_unprepared(&format!(
                "INSERT INTO users (id, username) VALUES ({user}, 'mac{user}')"
            ))
            .await
            .unwrap();
        let state = super::super::get_or_create_state(&fixture.db, user)
            .await
            .unwrap();
        let away = current_sight(user, &state).await;
        assert!(!away.on_page && !away.panel_open);
        let valuable = "agent.merope.mood_floor";
        assert!(super::super::gates::decide_ingest(valuable, &away).notify);

        let on_the_app = |panel: bool| serde_json::json!({ "presence": { "pageVisible": true, "panelVisible": panel } });
        remember_native_presence(user, "dev_sight", &on_the_app(true));
        let looking = current_sight(user, &state).await;
        assert!(looking.on_page && looking.panel_open);
        let decision = super::super::gates::decide_ingest(valuable, &looking);
        assert!(!decision.notify && decision.live, "{decision:?}");
        // Speaking first stays quiet while her panel is open on the app.
        assert!(crate::services::agent::consciousness::live_presence_panel_open(user));

        remember_native_presence(user, "dev_sight", &on_the_app(false));
        let here = current_sight(user, &state).await;
        assert!(here.on_page && !here.panel_open);
        assert!(super::super::gates::decide_ingest(valuable, &here).notify);
        assert!(crate::services::agent::consciousness::present_users().contains(&user));

        forget_native_presence(user, "dev_sight");
        assert_eq!(current_sight(user, &state).await, away);
        fixture.close().await;
    }

    #[test]
    fn produce_and_redeem_live_in_separate_files() {
        let produce = include_str!("produce.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let redeem = include_str!("redeem.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(produce.contains("enqueue_speak_intent"));
        assert!(!produce.contains("insert_proactive"));
        assert!(!produce.contains("emit_speech_notification"));
        assert!(!produce.contains("direct_motion"));
        assert!(redeem.contains("insert_proactive"));
        assert!(redeem.contains("emit_speech_notification"));
        assert!(redeem.contains("emit_live_speech"));
        assert!(redeem.contains("direct_motion"));
        assert!(!redeem.contains("enqueue_speak_intent"));
        assert!(!redeem.contains("consider_event"));
    }
}
