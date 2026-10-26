use chrono::Utc;
use sea_orm::DatabaseConnection;

use crate::services::agent::{AgentInteractionMode, get_user_permissions, merope, run_hub};

use super::{IntentStore, SaidUnasked, SelfSnapshot};

/// Her lines said unasked that a decision sees: as many as her line itself
/// is shown (`redeem::compose_line`).
pub(crate) const SAID_LATELY: i64 = 6;

/// What she said to them lately unasked, newest first, as she is shown it
/// before deciding to say anything again (and before saying it).
pub(crate) async fn said_unasked_lately(db: &DatabaseConnection, user_id: i32) -> Vec<SaidUnasked> {
    let now = crate::clock::now();
    merope::store::said_lately(db, user_id, SAID_LATELY)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(at, line, they_wrote_since)| SaidUnasked {
            minutes_ago: (now - at).num_minutes().max(0),
            line: myriad_merope::diary::compact_summary(&line),
            they_wrote_since,
        })
        .filter(|said| !said.line.is_empty())
        .collect()
}

/// Capture trusted state immediately before an autonomy decision.
///
/// Permission names come only from the runtime grant filter. No host secret,
/// installation credential, declared permission, or approved permission is
/// copied into this snapshot.
pub async fn capture_self_snapshot(
    db: &DatabaseConnection,
    user_id: i32,
    interaction_mode: AgentInteractionMode,
    memory_query: &str,
) -> Result<SelfSnapshot, anyhow::Error> {
    let enabled = merope::is_enabled().await;
    let persona = merope::get_persona(db).await?;
    let state = merope::get_or_create_state(db, user_id).await?;
    let mut granted_permissions = get_user_permissions(db, user_id)
        .await
        .into_iter()
        .collect::<Vec<_>>();
    granted_permissions.sort_unstable();

    let remembered = crate::services::agent::memory::unified::quietly(
        merope::store::recall_remembered(db, user_id, Some(memory_query), 8),
    )
    .await
    .unwrap_or_default();

    Ok(SelfSnapshot {
        persona_name: merope::public_persona_name(
            enabled,
            persona.as_ref().map(|value| value.name.as_str()),
        ),
        addressee_user_id: user_id,
        interaction_mode,
        mood: merope::clamp_mood(state.mood),
        activity: merope::current_activity(&state).to_string(),
        do_not_disturb: merope::effective_do_not_disturb(&state),
        has_active_work: run_hub::user_has_executing_run(user_id).await,
        granted_permissions,
        recent_intents: IntentStore::new(db.clone()).recent(user_id, 8).await?,
        remembered,
        captured_at: crate::clock::now(),
        // Her senses follow them: the app's when that is where they are.
        live: super::presence::senses_presence(user_id),
        attention: super::attention::last_attention(user_id),
        myself: Some(merope::self_state::current(db).await.facts_view()),
        minutes_since_you_spoke_unasked: state.last_proactive_at.map(|at| {
            ((crate::clock::now() - at.with_timezone(&Utc))
                .num_seconds()
                .max(0) as f64
                / 60.0
                * 10.0)
                .round()
                / 10.0
        }),
        said_unasked_lately: said_unasked_lately(db, user_id).await,
        addressee_name: Some(merope::resolve_addressee_label(db, user_id).await)
            .filter(|name| !name.trim().is_empty()),
        inner: merope::inner::current(
            user_id,
            &crate::services::agent::memory::unified::Audience::private(user_id),
        ),
        macs_for_work: crate::services::agent::link::macs(user_id)
            .into_iter()
            .map(|mac| mac.name)
            .collect(),
    })
}
