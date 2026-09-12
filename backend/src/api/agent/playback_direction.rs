use super::*;
use crate::error::HttpError;
use crate::services::agent::run_hub::get_live_run_for_user;
use myriad_error::AppError;

#[derive(Default, Deserialize)]
pub(super) struct Cursor {
    #[serde(default)]
    after: u64,
}

async fn owned_run(
    claims: &Claims,
    db: &DatabaseConnection,
    run_id: &str,
) -> Result<Arc<AgentRun>, HttpError> {
    let user_id = parse_user_id_with_agent_access(claims, db).await?;
    get_live_run_for_user(run_id, user_id).await.ok_or_else(|| {
        HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Live run unavailable")),
        ))
    })
}

pub(super) async fn read(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(run_id): Path<String>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<crate::services::agent::playback_direction::DirectionSnapshot>, HttpError> {
    let run = owned_run(&claims, &db, &run_id).await?;
    Ok(Json(run.playback_direction.read_after(cursor.after).await))
}

pub(super) async fn close(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(run_id): Path<String>,
) -> Result<StatusCode, HttpError> {
    owned_run(&claims, &db, &run_id)
        .await?
        .playback_direction
        .close();
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn observe(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(run_id): Path<String>,
    Json(observation): Json<crate::services::agent::playback_direction::PlaybackObservation>,
) -> Result<StatusCode, HttpError> {
    if observation.upcoming_text.len() > 7_200 || observation.rig.to_string().len() > 16_384 {
        return Ok(StatusCode::PAYLOAD_TOO_LARGE);
    }
    owned_run(&claims, &db, &run_id)
        .await?
        .playback_direction
        .observe(observation);
    Ok(StatusCode::NO_CONTENT)
}
