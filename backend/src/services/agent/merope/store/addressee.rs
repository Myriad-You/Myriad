//! Her state with each person: affect, appraisal, music credit, do-not-disturb and activity.

use super::*;

pub const MUSIC_MOOD_CREDIT_COOLDOWN_SECS: i64 = 30 * 60;

#[derive(Debug)]
pub struct MusicMoodCredit {
    pub before: Affect,
    pub state: agent_addressee_state::Model,
    pub credited: bool,
    pub next_credit_in_seconds: i64,
}

pub fn affect_baseline_from_persona_lookup(
    lookup: Result<Option<(Option<serde_json::Value>, String)>, anyhow::Error>,
) -> Result<AffectBaseline, anyhow::Error> {
    match lookup {
        Ok(Some((persona_json, personality))) => {
            Ok(persona_affect_baseline(persona_json.as_ref(), &personality))
        }
        Ok(None) => Ok(AffectBaseline::default()),
        Err(error) => Err(error),
    }
}

pub async fn load_affect_baseline<C>(db: &C) -> Result<AffectBaseline, anyhow::Error>
where
    C: ConnectionTrait,
{
    let lookup = get_persona_on(db)
        .await
        .map(|persona| persona.map(|row| (row.persona_json, row.personality)));
    affect_baseline_from_persona_lookup(lookup)
}

pub fn affect_from_state(state: &agent_addressee_state::Model) -> Affect {
    Affect {
        mood: state.mood,
        arousal: state.arousal,
        emotion: state.emotion,
        emotion_arousal: state.emotion_arousal,
    }
}

pub(super) fn hours_since(at: chrono::DateTime<chrono::FixedOffset>) -> f64 {
    let secs = (Utc::now() - at.with_timezone(&Utc)).num_seconds();
    (secs.max(0) as f64) / 3600.0
}

/// Overlay mood/emotion regression in memory. Does not write; activity staleness uses `activity_updated_at`.
pub(super) fn overlay_settled(
    mut state: agent_addressee_state::Model,
    base: AffectBaseline,
) -> agent_addressee_state::Model {
    let settled = settle(
        affect_from_state(&state),
        base,
        hours_since(state.mood_settled_at),
        hours_since(state.emotion_settled_at),
    );
    state.mood = settled.mood;
    state.arousal = settled.arousal;
    state.emotion = settled.emotion;
    state.emotion_arousal = settled.emotion_arousal;
    state
}

pub async fn get_or_create_state<C>(
    db: &C,
    user_id: i32,
) -> Result<agent_addressee_state::Model, anyhow::Error>
where
    C: ConnectionTrait,
{
    let base = load_affect_baseline(db).await?;
    if let Some(existing) = agent_addressee_state::Entity::find_by_id(user_id)
        .one(db)
        .await?
    {
        return Ok(overlay_settled(existing, base));
    }
    let rest = Affect::at_rest(base);
    let now = Utc::now().into();
    let active = agent_addressee_state::ActiveModel {
        user_id: Set(user_id),
        mood: Set(rest.mood),
        arousal: Set(rest.arousal),
        emotion: Set(rest.emotion),
        emotion_arousal: Set(rest.emotion_arousal),
        activity: Set("idle".to_string()),
        activity_updated_at: Set(now),
        do_not_disturb: Set(false),
        dnd_start_minute: Set(None),
        dnd_end_minute: Set(None),
        last_user_message_at: Set(None),
        last_proactive_at: Set(None),
        music_mood_credited_at: Set(None),
        mood_settled_at: Set(now),
        emotion_settled_at: Set(now),
        updated_at: Set(now),
    };
    match active.insert(db).await {
        Ok(model) => Ok(model),
        Err(err) if is_unique_conflict(&err) => {
            let existing = agent_addressee_state::Entity::find_by_id(user_id)
                .one(db)
                .await?
                .ok_or_else(|| anyhow::Error::from(err))?;
            Ok(overlay_settled(existing, base))
        }
        Err(err) => Err(err.into()),
    }
}

/// `state` is the row the caller already read (settled) under the addressee lock.
pub(super) async fn save_affect_on<C>(
    db: &C,
    state: agent_addressee_state::Model,
    affect: Affect,
    touch_user_message: bool,
    touch_music_credit: bool,
) -> Result<agent_addressee_state::Model, anyhow::Error>
where
    C: ConnectionTrait,
{
    // Revisions travel as milliseconds. Consecutive writes must remain ordered
    // even within one clock tick (and across replicas under the same DB lock).
    let now = Utc::now()
        .max(
            state
                .updated_at
                .max(state.mood_settled_at)
                .with_timezone(&Utc)
                + chrono::Duration::milliseconds(1),
        )
        .into();
    let mut active: agent_addressee_state::ActiveModel = state.into();
    active.mood = Set(clamp(affect.mood));
    active.arousal = Set(clamp(affect.arousal));
    active.emotion = Set(clamp(affect.emotion));
    active.emotion_arousal = Set(clamp(affect.emotion_arousal));
    active.mood_settled_at = Set(now);
    active.emotion_settled_at = Set(now);
    active.updated_at = Set(now);
    if touch_user_message {
        active.last_user_message_at = Set(Some(now));
    }
    if touch_music_credit {
        active.music_mood_credited_at = Set(Some(now));
    }
    Ok(active.update(db).await?)
}

/// Apply one affect delta to the latest row under a per-addressee database
/// lock. Chat turns, async appraisal and task outcomes can arrive from
/// different sessions or backend replicas; serializing the read-modify-write
/// keeps every delta instead of letting the last absolute write erase one.
pub async fn update_affect<F>(
    db: &DatabaseConnection,
    user_id: i32,
    touch_user_message: bool,
    update: F,
) -> Result<(Affect, agent_addressee_state::Model), anyhow::Error>
where
    F: FnOnce(&mut Affect) + Send,
{
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let state = get_or_create_state(&transaction, user_id).await?;
    let before = affect_from_state(&state);
    let mut after = before;
    update(&mut after);
    let saved = save_affect_on(&transaction, state, after, touch_user_message, false).await?;
    transaction.commit().await?;
    Ok((before, saved))
}

/// A delayed interpretation belongs to one persisted input, not whichever
/// input happens to be current when the model finishes. The check and write
/// share the ordinary affect lock; an activity write is not a new input.
pub async fn update_utterance_appraisal<F>(
    db: &DatabaseConnection,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    update: F,
) -> Result<Option<(Affect, agent_addressee_state::Model)>, anyhow::Error>
where
    F: FnOnce(&mut Affect) + Send,
{
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let Some(state) = agent_addressee_state::Entity::find_by_id(user_id)
        .one(&transaction)
        .await?
    else {
        return Ok(None);
    };
    if !appraisal_is_current(state.last_user_message_at, input_at, Utc::now()) {
        return Ok(None);
    }
    let base = load_affect_baseline(&transaction).await?;
    let state = overlay_settled(state, base);
    let before = affect_from_state(&state);
    let mut after = before;
    update(&mut after);
    let saved = save_affect_on(&transaction, state, after, false, false).await?;
    transaction.commit().await?;
    Ok(Some((before, saved)))
}

pub(in crate::services::agent::merope) fn appraisal_is_current(
    latest_input: Option<chrono::DateTime<chrono::FixedOffset>>,
    expected_input: chrono::DateTime<chrono::FixedOffset>,
    now: chrono::DateTime<Utc>,
) -> bool {
    latest_input == Some(expected_input)
        && now.signed_duration_since(expected_input).num_milliseconds() <= 12_000
}

/// Credit one qualified listening block under the same per-addressee database
/// lock used by chat/task affect writes. Persisting the cooldown on the row
/// makes rapid events, multiple browser tabs and multiple backend replicas all
/// converge on one bounded mood change.
pub async fn credit_music_listening(
    db: &DatabaseConnection,
    user_id: i32,
    listened_seconds: u32,
) -> Result<MusicMoodCredit, anyhow::Error> {
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let state = get_or_create_state(&transaction, user_id).await?;
    let before = affect_from_state(&state);
    let now = Utc::now();

    if let Some(last) = state.music_mood_credited_at {
        let elapsed = (now - last.with_timezone(&Utc)).num_seconds().max(0);
        if elapsed < MUSIC_MOOD_CREDIT_COOLDOWN_SECS {
            transaction.commit().await?;
            return Ok(MusicMoodCredit {
                before,
                state,
                credited: false,
                next_credit_in_seconds: MUSIC_MOOD_CREDIT_COOLDOWN_SECS - elapsed,
            });
        }
    }

    let mut after = before;
    apply_music_listening(&mut after, listened_seconds);
    let saved = save_affect_on(&transaction, state, after, false, true).await?;
    transaction.commit().await?;
    Ok(MusicMoodCredit {
        before,
        state: saved,
        credited: true,
        next_credit_in_seconds: MUSIC_MOOD_CREDIT_COOLDOWN_SECS,
    })
}

pub(super) async fn lock_addressee<C>(db: &C, user_id: i32) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock($1, $2)",
        vec![1296388165_i32.into(), user_id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn set_do_not_disturb(
    db: &DatabaseConnection,
    user_id: i32,
    do_not_disturb: bool,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let state = get_or_create_state(db, user_id).await?;
    if state.do_not_disturb == do_not_disturb {
        return Ok(state);
    }
    let mut active: agent_addressee_state::ActiveModel = state.into();
    active.do_not_disturb = Set(do_not_disturb);
    active.updated_at = Set(Utc::now().into());
    Ok(active.update(db).await?)
}

pub async fn set_dnd_schedule(
    db: &DatabaseConnection,
    user_id: i32,
    start_minute: Option<i32>,
    end_minute: Option<i32>,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let state = get_or_create_state(db, user_id).await?;
    if state.dnd_start_minute == start_minute && state.dnd_end_minute == end_minute {
        return Ok(state);
    }
    let mut active: agent_addressee_state::ActiveModel = state.into();
    active.dnd_start_minute = Set(start_minute);
    active.dnd_end_minute = Set(end_minute);
    active.updated_at = Set(Utc::now().into());
    Ok(active.update(db).await?)
}

pub async fn set_activity(
    db: &DatabaseConnection,
    user_id: i32,
    activity: &str,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let state = get_or_create_state(&transaction, user_id).await?;
    if state.activity == activity {
        transaction.commit().await?;
        return Ok(state);
    }
    let mut active: agent_addressee_state::ActiveModel = state.into();
    let now = Utc::now().into();
    active.activity = Set(activity.to_string());
    active.activity_updated_at = Set(now);
    active.updated_at = Set(now);
    let saved = active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(saved)
}

/// Keep the most active phase while more than one run is live. A second run
/// starting its talking phase must not downgrade another run already working.
pub async fn promote_activity(
    db: &DatabaseConnection,
    user_id: i32,
    activity: &str,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let state = get_or_create_state(&transaction, user_id).await?;
    if activity_rank(activity) <= activity_rank(&state.activity) {
        transaction.commit().await?;
        return Ok(state);
    }
    let mut active: agent_addressee_state::ActiveModel = state.into();
    let now = Utc::now().into();
    active.activity = Set(activity.to_string());
    active.activity_updated_at = Set(now);
    active.updated_at = Set(now);
    let saved = active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(saved)
}

pub(super) fn activity_rank(activity: &str) -> u8 {
    match activity {
        "working" => 3,
        "thinking" => 2,
        "talking" => 1,
        _ => 0,
    }
}
