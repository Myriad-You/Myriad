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
mod produce;
mod redeem;

use sea_orm::DatabaseConnection;

use super::gates::IngestSight;
use super::is_logged_in_addressee;
use super::store::{insert_diary, insert_remembered_if_new, latest_open_session};
use super::{activity_is_busy, current_activity, effective_do_not_disturb};
use crate::models::entities::agent_addressee_state;
use crate::services::agent::consciousness::last_live_presence;
use crate::services::agent::run_hub;

pub use produce::{
    ingest, spawn, spawn_diary, spawn_presence, stable_consciousness_event_id, work_outcome_parent,
};
pub use redeem::tick_speak_intents;

const SAME_EVENT_MINUTES: i64 = 15;

pub async fn is_enabled() -> bool {
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
    let live = last_live_presence(user_id);
    IngestSight {
        on_page: live.page_visible,
        panel_open: live.panel_visible,
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

pub(crate) async fn persist_persona_remember(
    db: &DatabaseConnection,
    user_id: i32,
    candidate: Option<&str>,
) {
    let Some(candidate) = candidate else {
        return;
    };
    if let Err(error) = insert_remembered_if_new(db, user_id, candidate).await {
        tracing::debug!(%error, user_id, "[Merope] persona memory write skipped");
    }
}

pub fn compact_summary(summary: &str) -> String {
    redact_event_text(summary)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

pub fn redact_event_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return String::new();
    }
    let mut skip_next = false;
    let mut out = Vec::new();
    for token in trimmed.split_whitespace() {
        if skip_next {
            skip_next = false;
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if lower == "bearer" || lower.starts_with("bearer") {
            skip_next = lower == "bearer";
            continue;
        }
        if let Some(cleaned) = redact_token(token) {
            out.push(cleaned);
        }
    }
    out.join(" ")
}

fn redact_token(token: &str) -> Option<String> {
    let stripped = if let Some(scheme) = token.find("://") {
        let after_scheme = &token[scheme + 3..];
        if let Some(query) = after_scheme.find('?') {
            token[..scheme + 3 + query].to_string()
        } else {
            token.to_string()
        }
    } else {
        token.to_string()
    };
    let lower = stripped.to_ascii_lowercase();
    if lower.contains("api_key=")
        || lower.contains("access_token=")
        || lower.contains("refresh_token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.starts_with("bearer")
    {
        return None;
    }
    Some(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_summary_drops_json_and_secret_shaped_tokens() {
        assert!(compact_summary("{\"token\":\"abc\"}").is_empty());
        assert_eq!(
            compact_summary("Steam 刷新失败 https://api.example/sync?access_token=abcd"),
            "Steam 刷新失败 https://api.example/sync"
        );
        assert_eq!(compact_summary("抓取失败 Bearer eyJhbGciOi"), "抓取失败");
        assert_eq!(compact_summary("  Steam  解锁了成就  "), "Steam 解锁了成就");
    }

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
        assert!(src.contains("insert_remembered_if_new(db, user_id, candidate)"));
        assert!(!src.contains("insert_diary(db, user_id, memory, \"event\")"));
        assert!(!src.contains("insert_diary(db, user_id, &memory, \"event\")"));
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
