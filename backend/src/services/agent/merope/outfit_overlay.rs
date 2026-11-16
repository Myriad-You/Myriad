//! Per-chat-session memory of a temporary wardrobe overlay.
//!
//! Chat can point the live face at another saved set. It does not write
//! persona, the worn outfit, or the live rig pointer.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::store::get_persona;
use myriad_merope::{
    DEFAULT_WARDROBE_ID, OverlayDecision, WearDirective, looks_from_visual_profile,
    resolve_wear_directive, wardrobe_look, worn_outfit_id,
};

/// Session state is durable; reading it does not retain a process-wide entry.
/// It never updates persona, the worn outfit, or the active rig pointer.
pub async fn overlay_outfit_id(
    db: &DatabaseConnection,
    user_id: i32,
    session_id: &str,
) -> Option<String> {
    if session_id.is_empty() {
        return None;
    }
    match db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT context->>'chat_outfit_overlay' AS outfit_id FROM agent_sessions WHERE id = $1 AND user_id = $2 AND archived = FALSE",
        [session_id.into(), user_id.into()],
    )).await {
        Ok(Some(row)) => row.try_get::<Option<String>>("", "outfit_id").ok().flatten(),
        Ok(None) => None,
        Err(error) => { tracing::warn!(%error, "Failed to read chat outfit overlay"); None }
    }
}

async fn set_overlay(
    db: &DatabaseConnection,
    user_id: i32,
    session_id: &str,
    outfit_id: Option<&str>,
) -> bool {
    let statement = match outfit_id {
        Some(id) => Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_sessions SET context = jsonb_set(COALESCE(context::jsonb, '{}'::jsonb), '{chat_outfit_overlay}', to_jsonb($3::text)) WHERE id = $1 AND user_id = $2 AND archived = FALSE",
            vec![session_id.into(), user_id.into(), id.into()],
        ),
        None => Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_sessions SET context = COALESCE(context::jsonb, '{}'::jsonb) - 'chat_outfit_overlay' WHERE id = $1 AND user_id = $2 AND archived = FALSE",
            vec![session_id.into(), user_id.into()],
        ),
    };
    match db.execute_raw(statement).await {
        Ok(result) => result.rows_affected() == 1,
        Err(error) => {
            tracing::warn!(%error, "Failed to save chat outfit overlay");
            false
        }
    }
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
    let current = live_overlay(db, user_id, session_id, &looks).await;
    match resolve_wear_directive(directive, &looks, worn, current.as_deref()) {
        OverlayDecision::Unchanged => {
            tracing::debug!(user_id, session_id, "chat outfit overlay unchanged");
            None
        }
        OverlayDecision::Clear => {
            tracing::info!(user_id, session_id, "chat outfit overlay cleared");
            set_overlay(db, user_id, session_id, None)
                .await
                .then_some(None)
        }
        OverlayDecision::Wear(id) => {
            tracing::info!(
                user_id,
                session_id,
                outfit_id = %id,
                "chat outfit overlay wear"
            );
            set_overlay(db, user_id, session_id, Some(id))
                .await
                .then(|| Some(id.to_string()))
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
    let overlay = live_overlay(db, user_id, session_id, &looks).await;
    myriad_merope::format_chat_wardrobe_section(&looks, worn, overlay.as_deref())
}

async fn live_overlay(
    db: &DatabaseConnection,
    user_id: i32,
    session_id: &str,
    looks: &[myriad_merope::WardrobeLook],
) -> Option<String> {
    let current = overlay_outfit_id(db, user_id, session_id).await?;
    if wardrobe_look(looks, &current).is_some_and(|look| look.playable()) {
        Some(current)
    } else {
        set_overlay(db, user_id, session_id, None).await;
        None
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn durable_overlay_survives_session_churn_and_preserves_ownership_and_context() {
        use super::*;
        let Some(url) = crate::db::test_url(&["MYRIAD_MEMORY_TEST_DATABASE_URL"]) else {
            return;
        };
        let mut options = sea_orm::ConnectOptions::new(url);
        options.max_connections(1);
        let db = sea_orm::Database::connect(options).await.unwrap();
        db.execute_raw(Statement::from_string(DatabaseBackend::Postgres,
            "CREATE TEMP TABLE agent_sessions (id TEXT PRIMARY KEY, user_id INTEGER NOT NULL, context JSON, archived BOOLEAN NOT NULL DEFAULT FALSE)")).await.unwrap();
        db.execute_raw(Statement::from_string(DatabaseBackend::Postgres,
            r#"INSERT INTO agent_sessions VALUES ('active', 7, '{"mode":"chat","other":123}', FALSE), ('archived', 7, '{}', TRUE)"#)).await.unwrap();
        assert!(set_overlay(&db, 7, "active", Some("look-a")).await);
        assert!(!set_overlay(&db, 8, "active", Some("unauthorized")).await);
        assert!(!set_overlay(&db, 7, "archived", Some("look-a")).await);
        assert!(overlay_outfit_id(&db, 8, "active").await.is_none());
        db.execute_raw(Statement::from_string(DatabaseBackend::Postgres,
            "INSERT INTO agent_sessions SELECT 'churn-' || n::text, 7, '{}'::jsonb, FALSE FROM generate_series(1, 2000) n")).await.unwrap();
        assert_eq!(
            overlay_outfit_id(&db, 7, "active").await.as_deref(),
            Some("look-a")
        );
        assert!(set_overlay(&db, 7, "active", None).await);
        assert!(overlay_outfit_id(&db, 7, "active").await.is_none());
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT context FROM agent_sessions WHERE id = 'active'",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.try_get::<serde_json::Value>("", "context").unwrap(),
            serde_json::json!({"mode":"chat", "other":123})
        );
        assert!(set_overlay(&db, 7, "active", Some("look-b")).await);
        db.execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "UPDATE agent_sessions SET archived = TRUE WHERE id = 'active'",
        ))
        .await
        .unwrap();
        assert!(overlay_outfit_id(&db, 7, "active").await.is_none());
    }

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
