//! Per-chat-session memory of a temporary wardrobe overlay.
//!
//! Chat can point the live face at another saved set. It does not write
//! persona, the worn outfit, or the live rig pointer.

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;
use sea_orm::DatabaseConnection;

use super::store::get_persona;
use myriad_merope::{
    looks_from_visual_profile, resolve_wear_directive, wardrobe_look, worn_outfit_id,
    OverlayDecision, WearDirective, DEFAULT_WARDROBE_ID,
};

static OVERLAYS: Lazy<Mutex<HashMap<(i32, String), String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn overlay_outfit_id(user_id: i32, session_id: &str) -> Option<String> {
    OVERLAYS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&(user_id, session_id.to_string()))
        .cloned()
}

pub fn clear_overlay(user_id: i32, session_id: &str) {
    OVERLAYS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&(user_id, session_id.to_string()));
}

fn set_overlay(user_id: i32, session_id: &str, outfit_id: &str) {
    OVERLAYS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert((user_id, session_id.to_string()), outfit_id.to_string());
}

/// Apply Lite's wardrobe choice. `Some` means the showing outfit changed;
/// `outfit_id` is `None` when the overlay was cleared back to the worn set.
pub async fn apply_model_wear_directive(
    db: &DatabaseConnection,
    user_id: i32,
    session_id: &str,
    directive: &WearDirective,
) -> Option<Option<String>> {
    if session_id.is_empty() {
        return None;
    }
    if !super::is_enabled().await {
        return None;
    }
    let Ok(Some(persona)) = get_persona(db).await else {
        return None;
    };
    let profile = persona.visual_profile.as_ref()?;
    let looks = looks_from_visual_profile(profile);
    if looks.is_empty() {
        return None;
    }
    let worn = worn_outfit_id(profile).unwrap_or(DEFAULT_WARDROBE_ID);
    let current = live_overlay(user_id, session_id, &looks);
    match resolve_wear_directive(directive, &looks, worn, current.as_deref()) {
        OverlayDecision::Unchanged => {
            tracing::debug!(user_id, session_id, "chat outfit overlay unchanged");
            None
        }
        OverlayDecision::Clear => {
            tracing::info!(user_id, session_id, "chat outfit overlay cleared");
            clear_overlay(user_id, session_id);
            Some(None)
        }
        OverlayDecision::Wear(id) => {
            tracing::info!(
                user_id,
                session_id,
                outfit_id = %id,
                "chat outfit overlay wear"
            );
            set_overlay(user_id, session_id, id);
            Some(Some(id.to_string()))
        }
    }
}

pub async fn chat_wardrobe_section(
    db: &DatabaseConnection,
    user_id: i32,
    session_id: &str,
) -> Option<String> {
    if !super::is_enabled().await {
        return None;
    }
    let Ok(Some(persona)) = get_persona(db).await else {
        return None;
    };
    let profile = persona.visual_profile.as_ref()?;
    let looks = looks_from_visual_profile(profile);
    let worn = worn_outfit_id(profile).unwrap_or(DEFAULT_WARDROBE_ID);
    let overlay = live_overlay(user_id, session_id, &looks);
    myriad_merope::format_chat_wardrobe_section(&looks, worn, overlay.as_deref())
}

fn live_overlay(
    user_id: i32,
    session_id: &str,
    looks: &[myriad_merope::WardrobeLook],
) -> Option<String> {
    let current = overlay_outfit_id(user_id, session_id)?;
    if wardrobe_look(looks, &current).is_some_and(|look| look.playable()) {
        Some(current)
    } else {
        clear_overlay(user_id, session_id);
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn overlay_memory_does_not_write_persona_or_the_live_pointer() {
        let source = include_str!("outfit_overlay.rs");
        let prod = source.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("resolve_wear_directive"));
        assert!(prod.contains("apply_model_wear_directive"));
        assert!(!prod.contains("upsert_persona"));
        assert!(!prod.contains("persist_active_asset"));
        assert!(!prod.contains("activeOutfitId"));
        assert!(!prod.contains("PortraitUpdate"));
        assert!(!prod.contains("put_persona"));
    }

    #[test]
    fn chat_turns_apply_lite_wear_after_it_speaks() {
        let process = include_str!("../process_chat.rs");
        assert!(process.contains("peel_chat_live_reply"));
        assert!(process.contains("wear_directive_after_reply"));
        assert!(process.contains("apply_model_wear_directive"));
        let chat = process
            .split("pub(super) async fn process_chat_with_progress(")
            .nth(1)
            .expect("streaming chat branch");
        let chat = chat.split("\nfn ").next().unwrap();
        assert!(chat.contains("stream_strict_lite_chat_response"));
        assert!(chat.contains("publish_model_outfit_overlay"));
        assert!(chat.contains("chat_reply_with_overlay"));
        assert!(
            chat.find("stream_strict_lite_chat_response").unwrap()
                < chat.find("publish_model_outfit_overlay").unwrap()
        );
        assert!(!chat.contains("upsert_persona"));
        assert!(!chat.contains("persist_active_asset"));
        let prompt = include_str!("../confirmation_and_tasks/chat_stream.rs");
        let prompt_fn = prompt
            .split("async fn chat_response_prompt")
            .nth(1)
            .and_then(|rest| rest.split("async fn ").next())
            .unwrap();
        assert!(prompt_fn.contains("chat_wardrobe_section"));
        assert!(prompt_fn.contains("AgentInteractionMode::Chat"));
        assert!(!prompt_fn.contains("upsert_persona"));
        assert!(prompt.contains("spawn_model_outfit_overlay"));
        assert!(prompt.contains("spawn_chat_music_control"));
        assert!(prompt.contains("WearStreamFilter"));
        assert!(prompt_fn.contains("format_chat_player_section"));
    }
}
