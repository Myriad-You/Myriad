//! Her state with each person: affect, appraisal, music credit, do-not-disturb and activity.

use super::*;
pub use myriad_persona_sync::AffectImpulse;
use myriad_persona_sync::{affect::Change, entity::AffectAnchorBody};

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

/// Whole seconds from `at` to `now`, in hours, never negative: how far her
/// affect settles between two moments, the same way on every side
/// (`myriad_persona_sync::affect::settle_hours`).
fn hours_between(at: chrono::DateTime<chrono::FixedOffset>, now: chrono::DateTime<Utc>) -> f64 {
    let secs = (now - at.with_timezone(&Utc)).num_seconds();
    (secs.max(0) as f64) / 3600.0
}

/// Overlay mood/emotion regression in memory. Does not write; activity staleness uses `activity_updated_at`.
pub(super) fn overlay_settled(
    state: agent_addressee_state::Model,
    base: AffectBaseline,
) -> agent_addressee_state::Model {
    overlay_settled_at(state, base, super::super::clock::now())
}

/// [`overlay_settled`], settled to `now`.
fn overlay_settled_at(
    mut state: agent_addressee_state::Model,
    base: AffectBaseline,
    now: chrono::DateTime<Utc>,
) -> agent_addressee_state::Model {
    let settled = settle(
        affect_from_state(&state),
        base,
        hours_between(state.mood_settled_at, now),
        hours_between(state.emotion_settled_at, now),
    );
    state.mood = settled.mood;
    state.arousal = settled.arousal;
    state.emotion = settled.emotion;
    state.emotion_arousal = settled.emotion_arousal;
    state
}

/// Where her mood toward this person settles: her usual, lowered by what
/// still stings with them, so a hurt does not fade from her face in days
/// while it still stings in her words.
async fn baseline_toward<C>(db: &C, user_id: i32) -> Result<AffectBaseline, anyhow::Error>
where
    C: ConnectionTrait,
{
    let base = load_affect_baseline(db).await?;
    let weighs = super::super::sore::weighs_on(db, user_id).await?;
    Ok(AffectBaseline {
        mood: (base.mood - weighs).max(0.0),
        ..base
    })
}

pub async fn get_or_create_state<C>(
    db: &C,
    user_id: i32,
) -> Result<agent_addressee_state::Model, anyhow::Error>
where
    C: ConnectionTrait,
{
    let base = baseline_toward(db, user_id).await?;
    Ok(match stored_or_created(db, user_id, base).await? {
        Stored::Existing(existing) => overlay_settled(existing, base),
        Stored::Created(created) => created,
    })
}

/// Her stored state with someone, as written, or a new one at rest.
enum Stored {
    Existing(agent_addressee_state::Model),
    Created(agent_addressee_state::Model),
}

impl Stored {
    fn row(&self) -> &agent_addressee_state::Model {
        match self {
            Stored::Existing(row) | Stored::Created(row) => row,
        }
    }
}

async fn stored_or_created<C>(
    db: &C,
    user_id: i32,
    base: AffectBaseline,
) -> Result<Stored, anyhow::Error>
where
    C: ConnectionTrait,
{
    if let Some(existing) = agent_addressee_state::Entity::find_by_id(user_id)
        .one(db)
        .await?
    {
        return Ok(Stored::Existing(existing));
    }
    let rest = Affect::at_rest(base);
    let now = super::super::clock::now().into();
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
        Ok(model) => Ok(Stored::Created(model)),
        Err(err) if is_unique_conflict(&err) => {
            let existing = agent_addressee_state::Entity::find_by_id(user_id)
                .one(db)
                .await?
                .ok_or_else(|| anyhow::Error::from(err))?;
            Ok(Stored::Existing(existing))
        }
        Err(err) => Err(err.into()),
    }
}

/// The moment a write to `state` is stamped with: now, but always after the
/// row's last write. Revisions travel as milliseconds. Consecutive writes
/// must remain ordered even within one clock tick (and across replicas
/// under the same DB lock).
fn stamp_after(
    state: &agent_addressee_state::Model,
    now: chrono::DateTime<Utc>,
) -> chrono::DateTime<Utc> {
    now.max(
        state
            .updated_at
            .max(state.mood_settled_at)
            .with_timezone(&Utc)
            + chrono::Duration::milliseconds(1),
    )
}

/// `state` is the row the caller already read (settled) under the addressee
/// lock. Writes the row only: every change the site makes goes through
/// [`apply_change_on`], which also records it; tests use this to write
/// behind a waiting change's back.
#[cfg(test)]
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
    let now = stamp_after(&state, super::super::clock::now());
    write_affect_on(
        db,
        state,
        affect,
        now,
        touch_user_message,
        touch_music_credit,
    )
    .await
}

async fn write_affect_on<C>(
    db: &C,
    state: agent_addressee_state::Model,
    affect: Affect,
    now: chrono::DateTime<Utc>,
    touch_user_message: bool,
    touch_music_credit: bool,
) -> Result<agent_addressee_state::Model, anyhow::Error>
where
    C: ConnectionTrait,
{
    let now: chrono::DateTime<chrono::FixedOffset> = now.into();
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

/// What moved her and why, as one change of her affect is recorded
/// (`myriad_persona_sync::affect`).
#[derive(Debug, Clone, PartialEq)]
pub struct AffectChange {
    /// `user_turn`, `user_praise`, `user_scold`, `user_appraisal`, `task_ok`,
    /// `task_fail`, `music_listening`.
    pub cause: &'static str,
    pub impulse: AffectImpulse,
}

impl AffectChange {
    /// One turn of their words: praise or scolding lands less the more turns
    /// came before it in the session; otherwise she only settles.
    pub fn utterance(utterance_index: u32, praised: bool, scolded: bool) -> Self {
        Self {
            cause: if scolded {
                "user_scold"
            } else if praised {
                "user_praise"
            } else {
                "user_turn"
            },
            impulse: AffectImpulse::utterance(utterance_index, praised, scolded),
        }
    }

    /// How their words read to her, appraised after the turn.
    pub fn appraisal(appraisal: myriad_merope::affect::Appraisal) -> Self {
        Self {
            cause: "user_appraisal",
            impulse: AffectImpulse::appraisal(appraisal, 1.0),
        }
    }

    /// A task she did for them, done or failed.
    pub fn task(success: bool) -> Self {
        Self {
            cause: if success { "task_ok" } else { "task_fail" },
            impulse: AffectImpulse::task(success),
        }
    }

    fn music(listened_seconds: u32) -> Self {
        Self {
            cause: "music_listening",
            impulse: AffectImpulse::Music {
                listened_seconds: i64::from(listened_seconds),
            },
        }
    }
}

/// Apply one change to the row read under the addressee lock and record it,
/// in the caller's transaction: her state settles toward `base` to the moment
/// the write is stamped with, the impulse moves her, the row is written, and
/// the change is appended to `agent_affect_events` (the first one also
/// anchors replay). Replaying the anchor and the changes gives the row.
async fn apply_change_on<C>(
    db: &C,
    row: agent_addressee_state::Model,
    base: AffectBaseline,
    change: &AffectChange,
    now: chrono::DateTime<Utc>,
    touch_user_message: bool,
    touch_music_credit: bool,
) -> Result<(Affect, agent_addressee_state::Model), anyhow::Error>
where
    C: ConnectionTrait,
{
    let stamp = stamp_after(&row, now);
    let user_id = row.user_id;
    let state = overlay_settled_at(row, base, stamp);
    let before = affect_from_state(&state);
    let mut after = before;
    change.impulse.apply(&mut after);
    let saved = write_affect_on(
        db,
        state,
        after,
        stamp,
        touch_user_message,
        touch_music_credit,
    )
    .await?;
    record_change_on(db, user_id, change, base, before, &saved).await?;
    Ok((before, saved))
}

/// Appends one change, and anchors replay at it when nothing does yet.
async fn record_change_on<C>(
    db: &C,
    user_id: i32,
    change: &AffectChange,
    base: AffectBaseline,
    before: Affect,
    saved: &agent_addressee_state::Model,
) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    let crossed = |affect: Affect| myriad_persona_sync::entity::Affect {
        mood: affect.mood,
        arousal: affect.arousal,
        emotion: affect.emotion,
        emotion_arousal: affect.emotion_arousal,
    };
    let after = affect_from_state(saved);
    let id = Uuid::new_v4().to_string();
    let at = saved.mood_settled_at;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_affect_events (id, user_id, at, cause, impulse, baseline_mood, \
         baseline_arousal, before, after, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::timestamptz)",
        vec![
            id.clone().into(),
            user_id.into(),
            at.into(),
            change.cause.into(),
            serde_json::to_value(&change.impulse)?.into(),
            base.mood.into(),
            base.arousal.into(),
            serde_json::to_value(crossed(before))?.into(),
            serde_json::to_value(crossed(after))?.into(),
            crate::clock::now().into(),
        ],
    ))
    .await?;
    let rev = myriad_persona_sync::hlc::legacy(at.timestamp_micros(), None)
        .map_err(|error| anyhow::anyhow!("anchor revision: {error}"))?
        .to_string();
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_affect_anchor (user_id, mood, arousal, emotion, emotion_arousal, at, \
         through_event, rev) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (user_id) DO NOTHING",
        vec![
            user_id.into(),
            after.mood.into(),
            after.arousal.into(),
            after.emotion.into(),
            after.emotion_arousal.into(),
            at.into(),
            id.into(),
            rev.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Apply one change to the latest row under a per-addressee database lock,
/// and record it. Chat turns, async appraisal and task outcomes can arrive
/// from different sessions or backend replicas; serializing the
/// read-modify-write keeps every change instead of letting the last absolute
/// write erase one.
pub async fn update_affect(
    db: &DatabaseConnection,
    user_id: i32,
    touch_user_message: bool,
    change: AffectChange,
) -> Result<(Affect, agent_addressee_state::Model), anyhow::Error> {
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let base = baseline_toward(&transaction, user_id).await?;
    let now = super::super::clock::now();
    let row = stored_or_created(&transaction, user_id, base).await?;
    let applied = apply_change_on(
        &transaction,
        row.row().clone(),
        base,
        &change,
        now,
        touch_user_message,
        false,
    )
    .await?;
    transaction.commit().await?;
    Ok(applied)
}

/// A delayed interpretation belongs to one persisted input, not whichever
/// input happens to be current when the model finishes. The check and write
/// share the ordinary affect lock; an activity write is not a new input.
pub async fn update_utterance_appraisal(
    db: &DatabaseConnection,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    change: AffectChange,
) -> Result<Option<(Affect, agent_addressee_state::Model)>, anyhow::Error> {
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let Some(state) = agent_addressee_state::Entity::find_by_id(user_id)
        .one(&transaction)
        .await?
    else {
        return Ok(None);
    };
    let now = super::super::clock::now();
    if !appraisal_is_current(state.last_user_message_at, input_at, now) {
        return Ok(None);
    }
    let base = baseline_toward(&transaction, user_id).await?;
    let applied = apply_change_on(&transaction, state, base, &change, now, false, false).await?;
    transaction.commit().await?;
    Ok(Some(applied))
}

/// Where replaying her affect toward someone starts, and the changes recorded
/// after it, `upto` (inclusive) when given; none before her first change.
pub(crate) async fn affect_log_on<C>(
    db: &C,
    user_id: i32,
    upto: Option<chrono::DateTime<Utc>>,
) -> Result<Option<(AffectAnchorBody, Vec<Change>)>, anyhow::Error>
where
    C: ConnectionTrait,
{
    let Some(anchor) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT mood, arousal, emotion, emotion_arousal, at, through_event \
             FROM agent_affect_anchor WHERE user_id = $1",
            vec![user_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let anchor_at: chrono::DateTime<chrono::FixedOffset> = anchor.try_get("", "at")?;
    let anchor = AffectAnchorBody {
        affect: myriad_persona_sync::entity::Affect {
            mood: anchor.try_get("", "mood")?,
            arousal: anchor.try_get("", "arousal")?,
            emotion: anchor.try_get("", "emotion")?,
            emotion_arousal: anchor.try_get("", "emotion_arousal")?,
        },
        at: anchor_at.timestamp_micros(),
        through_event: anchor.try_get("", "through_event")?,
    };
    let upto: Option<chrono::DateTime<chrono::FixedOffset>> = upto.map(|at| at.fixed_offset());
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, at, impulse, baseline_mood, baseline_arousal FROM agent_affect_events \
             WHERE user_id = $1 AND at >= $2 AND ($3::timestamptz IS NULL OR at <= $3) \
             ORDER BY at, id COLLATE \"C\"",
            vec![user_id.into(), anchor_at.into(), upto.into()],
        ))
        .await?;
    let mut changes = Vec::with_capacity(rows.len());
    for row in &rows {
        let at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "at")?;
        changes.push(Change {
            id: row.try_get("", "id")?,
            at: at.timestamp_micros(),
            baseline: myriad_persona_sync::entity::Baseline {
                mood: row.try_get("", "baseline_mood")?,
                arousal: row.try_get("", "baseline_arousal")?,
            },
            impulse: serde_json::from_value(row.try_get::<Value>("", "impulse")?)?,
        });
    }
    Ok(Some((anchor, changes)))
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
    let base = baseline_toward(&transaction, user_id).await?;
    let now = super::super::clock::now();
    let row = stored_or_created(&transaction, user_id, base).await?;

    if let Some(last) = row.row().music_mood_credited_at {
        let elapsed = (now - last.with_timezone(&Utc)).num_seconds().max(0);
        if elapsed < MUSIC_MOOD_CREDIT_COOLDOWN_SECS {
            let state = match row {
                Stored::Existing(existing) => overlay_settled_at(existing, base, now),
                Stored::Created(created) => created,
            };
            transaction.commit().await?;
            return Ok(MusicMoodCredit {
                before: affect_from_state(&state),
                state,
                credited: false,
                next_credit_in_seconds: MUSIC_MOOD_CREDIT_COOLDOWN_SECS - elapsed,
            });
        }
    }

    let (before, saved) = apply_change_on(
        &transaction,
        row.row().clone(),
        base,
        &AffectChange::music(listened_seconds),
        now,
        false,
        true,
    )
    .await?;
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
    active.updated_at = Set(super::super::clock::now().into());
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
    active.updated_at = Set(super::super::clock::now().into());
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
    let now = super::super::clock::now().into();
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
    let now = super::super::clock::now().into();
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
