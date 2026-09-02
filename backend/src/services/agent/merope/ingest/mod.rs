//! Named-event speech.
//!
//! Produce turns an event into a speak intent and a diary line. Redeem turns a
//! speak intent into a sentence, maybe a notice. Shared text and gates live
//! here so neither side owns the other.

mod produce;
mod redeem;

use chrono::Utc;
use sea_orm::DatabaseConnection;

use super::gates::is_chatting;
use super::is_logged_in_addressee;
use super::store::{insert_diary, latest_open_session, list_remembered};
use crate::services::agent::run_hub;

pub use produce::{
    ingest, spawn, spawn_diary, spawn_presence, stable_consciousness_event_id, work_outcome_parent,
};
pub use redeem::{fallback_line, may_redeem_speech, tick_speak_intents};

const SAME_EVENT_MINUTES: i64 = 15;

pub async fn is_enabled() -> bool {
    crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved()
}

/// Existing producers keep notifying unless Merope is on and the addressee is mid-conversation.
pub async fn latest_session_id_for(user_id: i32) -> Option<String> {
    let db = crate::services::tapp_registry::database().await.ok()?;
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
    let Ok(db) = crate::services::tapp_registry::database().await else {
        return true;
    };
    // Only the live chat window suppresses these — the addressee is already
    // watching the panel. Do-not-disturb means "don't speak up on your own",
    // not "swallow the failures of work this person asked for", so it is
    // deliberately not consulted here; it gates speech in `decide_ingest`.
    !addressee_is_chatting(&db, user_id).await
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
    if duplicate {
        None
    } else {
        Some(compact)
    }
}

pub(crate) async fn persist_persona_remember(
    db: &DatabaseConnection,
    user_id: i32,
    candidate: Option<&str>,
) {
    let Some(candidate) = candidate else {
        return;
    };
    let existing = match list_remembered(db, user_id, 32).await {
        Ok(notes) => notes
            .into_iter()
            .map(|note| note.content)
            .collect::<Vec<_>>(),
        Err(_) => return,
    };
    let Some(fact) = persona_remember_insert(candidate, &existing) else {
        return;
    };
    let _ = insert_diary(db, user_id, &fact, super::store::DIARY_SOURCE_REMEMBER).await;
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

async fn addressee_is_chatting(db: &DatabaseConnection, user_id: i32) -> bool {
    let last_active = latest_open_session(db, user_id)
        .await
        .ok()
        .flatten()
        .map(|(_, at)| at);
    // A run parked on `waiting_for_input` is the addressee *not* talking: counting
    // it as chatting would suppress the very clarification notice that asks them
    // to come back, so only actively executing runs hold the floor.
    let executing_run = run_hub::user_has_executing_run(user_id).await;
    is_chatting(last_active, executing_run, Utc::now())
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
        assert!(src.contains("DIARY_SOURCE_REMEMBER"));
        assert!(src.contains("persona_remember_insert(candidate, &existing)"));
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
        assert!(redeem.contains("direct_motion"));
        assert!(!redeem.contains("enqueue_speak_intent"));
        assert!(!redeem.contains("consider_event"));
    }
}
