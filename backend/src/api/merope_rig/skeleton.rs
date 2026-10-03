//! 主立绘上识别出的骨架：导入时把关节绑到识别出的位置上。

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::{
    ApiResult, internal_error,
    master::{MasterSlot, current_master},
    not_found, require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{image_generation, pose_estimation},
};

/// The worn bust's skeleton.
pub async fn get_skeleton(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    skeleton_of(&db, &claims, MasterSlot::WornBust).await
}

/// A full-body set's skeleton.
pub async fn get_full_body_skeleton(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
) -> ApiResult<Json<Value>> {
    skeleton_of(&db, &claims, MasterSlot::FullBody(&outfit_id)).await
}

async fn skeleton_of(
    db: &DatabaseConnection,
    claims: &Claims,
    slot: MasterSlot<'_>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(claims, db).await?;
    let master = current_master(db, slot)
        .await?
        .ok_or_else(|| not_found("Site portrait is missing"))?;
    let image = image_generation::load_local_reference(&master.asset_id)
        .await
        .map_err(|_| not_found("The master portrait is not available"))?;
    let pose = pose_estimation::estimate(&image.bytes)
        .await
        .map_err(|error| match error {
            pose_estimation::PoseError::Empty | pose_estimation::PoseError::Image(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": "No figure was found in the master portrait",
                    "code": "skeleton_not_found"
                })),
            ),
            other => {
                tracing::warn!(error = %other, "skeleton detection failed");
                internal_error("Skeleton detection is unavailable")
            }
        })?;
    Ok(Json(json!({
        "sourceMasterAssetId": master.asset_id,
        "sourceGenerationFingerprint": master.generation_fingerprint,
        "model": pose.model,
        "width": pose.width,
        "height": pose.height,
        "keypoints": pose.keypoints,
    })))
}
