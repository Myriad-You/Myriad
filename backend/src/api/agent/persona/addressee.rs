//! Per-person endpoints: do-not-disturb, what she is doing, and music listened together.

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicListeningRequest {
    pub listened_seconds: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutAddresseeRequest {
    pub do_not_disturb: Option<bool>,
    pub dnd_start: Option<String>,
    pub dnd_end: Option<String>,
}

pub(super) fn parse_schedule(
    start: Option<&str>,
    end: Option<&str>,
) -> Result<(Option<i32>, Option<i32>), HttpError> {
    let start = start.map(str::trim).filter(|value| !value.is_empty());
    let end = end.map(str::trim).filter(|value| !value.is_empty());
    match (start, end) {
        (None, None) => Ok((None, None)),
        (Some(start), Some(end)) => {
            let start = merope::parse_clock_minute(start).ok_or_else(|| {
                HttpError::from((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Invalid do-not-disturb start time",
                        "code": "dnd_schedule_invalid"
                    })),
                ))
            })?;
            let end = merope::parse_clock_minute(end).ok_or_else(|| {
                HttpError::from((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Invalid do-not-disturb end time",
                        "code": "dnd_schedule_invalid"
                    })),
                ))
            })?;
            Ok((Some(start), Some(end)))
        }
        _ => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Set both start and end, or clear both",
                "code": "dnd_schedule_incomplete"
            })),
        ))),
    }
}

/// PUT /api/agent/addressee — current speaker only. Never another person's state.
pub async fn put_addressee(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<PutAddresseeRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    if !merope::is_logged_in_addressee(user_id) {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Guests cannot change addressee state",
                "code": "login_required"
            })),
        )));
    }
    let mut state = if let Some(do_not_disturb) = body.do_not_disturb {
        merope::set_do_not_disturb(&db, user_id, do_not_disturb)
            .await
            .map_err(|error| persona_store_http("save addressee", error))?
    } else {
        merope::get_or_create_state(&db, user_id)
            .await
            .map_err(|error| persona_store_http("load addressee", error))?
    };
    if body.dnd_start.is_some() || body.dnd_end.is_some() {
        let (start, end) = parse_schedule(body.dnd_start.as_deref(), body.dnd_end.as_deref())?;
        state = merope::set_dnd_schedule(&db, user_id, start, end)
            .await
            .map_err(|error| persona_store_http("save quiet-hours", error))?;
    }
    Ok(Json(json!({
        "mood": state.mood,
        "arousal": state.arousal,
        "activity": merope::current_activity(&state),
        "doNotDisturb": state.do_not_disturb,
        "doNotDisturbActive": merope::effective_do_not_disturb(&state),
        "dndStart": state.dnd_start_minute.and_then(merope::format_clock_minute),
        "dndEnd": state.dnd_end_minute.and_then(merope::format_clock_minute),
    })))
}

/// POST /api/agent/addressee/music-listening — credit a meaningful block of
/// actual playback. The client reports time, never a mood delta; the backend
/// owns the effect, ceiling and cross-process cooldown.
/// What she is doing on her own right now, for someone who may join her.
/// Why she picked it stays hers.
pub async fn get_doing(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    parse_user_id_with_agent_access(&claims, &db).await?;
    Ok(Json(json!({
        "doing": merope::api::doing::current(),
        // Lazing about is something she is doing too.
        "lazing": merope::api::doing::lazing().map(|lazing| json!({
            "kind": lazing.kind,
            "started": lazing.started,
            "ends": lazing.ends,
        })),
        "now": chrono::Utc::now(),
    })))
}

pub async fn post_music_listening(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<MusicListeningRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    if !merope::is_logged_in_addressee(user_id) {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Guests cannot change addressee mood",
                "code": "login_required"
            })),
        )));
    }
    if body.listened_seconds < merope::MUSIC_LISTENING_MIN_SECS {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Listening block is too short",
                "code": "music_listening_too_short",
                "minimumSeconds": merope::MUSIC_LISTENING_MIN_SECS,
            })),
        )));
    }

    let credit = merope::credit_music_listening(&db, user_id, body.listened_seconds)
        .await
        .map_err(|error| persona_store_http("credit music listening", error))?;
    let after = merope::api::store::affect_from_state(&credit.state);
    let mood = merope::MoodTransition::from_affect(
        &credit.before,
        &after,
        "music_listening",
        credit
            .state
            .updated_at
            .with_timezone(&chrono::Utc)
            .timestamp_millis(),
    );
    Ok(Json(json!({
        "credited": credit.credited,
        "nextCreditInSeconds": credit.next_credit_in_seconds,
        "mood": mood,
        "activity": merope::current_activity(&credit.state),
    })))
}
