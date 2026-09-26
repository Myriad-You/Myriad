//! Touch on her avatar over HTTP: who may, how often, and the bounds of a
//! summary. What the touch is and how she reacts is `merope::touch`.
use super::*;
use crate::services::agent::merope::touch::{TouchSummary, completion_summary};
use crate::{error::HttpError, services::agent::merope};
use std::{collections::HashMap, sync::Mutex, time::Instant};

// Deliberately process-local, like live presence. Multiple replicas need a
// shared ephemeral limiter before deploying this endpoint across replicas.
static RECENT: once_cell::sync::Lazy<Mutex<HashMap<i32, Instant>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));
static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
static COMPLETED: once_cell::sync::Lazy<Mutex<HashMap<i32, Instant>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

/// Completion is a separate ingest event (`agent.merope.touch`), not a continuation of appraise.
pub async fn complete(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<TouchSummary>,
) -> Result<StatusCode, HttpError> {
    let user_id = super::parse_user_id_with_agent_access(&claims, &db).await?;
    if !merope::is_enabled().await || !merope::is_logged_in_addressee(user_id) {
        return Ok(StatusCode::NO_CONTENT);
    }
    if !(1200..=600_000).contains(&body.duration_ms) || !(1..=8).contains(&body.repeat_count) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid touch summary")),
        )));
    }
    let live = crate::services::agent::consciousness::last_live_presence(user_id);
    if !live.page_visible || !live.face_visible || live.speaking {
        return Ok(StatusCode::NO_CONTENT);
    }
    {
        let mut recent = COMPLETED.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        recent.retain(|_, at| now.duration_since(*at) < Duration::from_secs(30));
        if recent.contains_key(&user_id) || recent.len() >= 4096 {
            return Ok(StatusCode::NO_CONTENT);
        }
        recent.insert(user_id, now);
    }
    merope::spawn_ingest(user_id, "agent.merope.touch", completion_summary(&body));
    Ok(StatusCode::NO_CONTENT)
}

fn admit(recent: &mut HashMap<i32, Instant>, user: i32, now: Instant) -> bool {
    recent.retain(|_, at| now.duration_since(*at) < Duration::from_secs(60));
    if recent
        .get(&user)
        .is_some_and(|at| now.duration_since(*at) < Duration::from_secs(5))
        || recent.len() >= 4096
    {
        return false;
    }
    recent.insert(user, now);
    true
}

pub async fn appraise(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<TouchSummary>,
) -> Result<Json<Value>, HttpError> {
    let user_id = super::parse_user_id_with_agent_access(&claims, &db).await?;
    let none = || Json(json!({ "reaction": null }));
    if !merope::is_enabled().await || !merope::is_logged_in_addressee(user_id) {
        return Ok(none());
    }
    if !(120..=600_000).contains(&body.duration_ms) || !(1..=8).contains(&body.repeat_count) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid touch summary")),
        )));
    }
    let Ok(_slot) = SLOTS.try_acquire() else {
        return Ok(none());
    };
    if !admit(
        &mut RECENT.lock().unwrap_or_else(|e| e.into_inner()),
        user_id,
        Instant::now(),
    ) {
        return Ok(none());
    }
    // Bound the entire operation, including provider resolution and state reads.
    let decision = tokio::time::timeout(
        Duration::from_secs(3),
        merope::touch::appraise(&db, user_id, &body),
    )
    .await
    .ok()
    .flatten();
    Ok(decision.map(|d| Json(json!(d))).unwrap_or_else(none))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_is_per_user_and_expires() {
        let mut recent = HashMap::new();
        let now = Instant::now();
        assert!(admit(&mut recent, 1, now));
        assert!(!admit(&mut recent, 1, now + Duration::from_secs(4)));
        assert!(admit(&mut recent, 2, now + Duration::from_secs(4)));
        assert!(admit(&mut recent, 1, now + Duration::from_secs(5)));
        assert!(admit(&mut recent, 3, now + Duration::from_secs(66)));
        assert_eq!(recent.len(), 1);
    }
}
use myriad_error::AppError;
