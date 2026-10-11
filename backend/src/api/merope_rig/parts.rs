//! 补找会自己动的部件：对已有的装配包（穿着的半身，或某套全身）在后台重跑一次
//! 部件分析，完成后另存为新包换上。用于这项分析出现之前导入的包，或上次没找成的。

use axum::{
    Extension, Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::{
    ApiResult, bad_request,
    full_body::full_body_set,
    import::{MAX_RIG_ANALYSIS_REFERENCE_BYTES, find_moving_parts, png_validation_error},
    internal_error,
    master::{MasterSlot, master_for, master_from_persona},
    not_found,
    package::{rig_revision_conflict, rig_revision_matches},
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{agent::merope, merope_rig},
};

/// The worn bust's package: its moving parts looked for again.
pub async fn find_site_rig_parts(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let (previous, reference) = read_request(multipart).await?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    if !rig_revision_matches(
        persona.as_ref().and_then(|row| row.visual_profile.as_ref()),
        Some(&previous),
    ) {
        return Err(rig_revision_conflict());
    }
    let master = persona
        .as_ref()
        .and_then(master_from_persona)
        .ok_or_else(|| not_found("Site portrait is missing"))?;
    tokio::spawn(find_moving_parts(
        db, user_id, None, previous, master, reference,
    ));
    Ok((StatusCode::ACCEPTED, Json(json!({ "started": true }))))
}

/// A full-body set's package: its moving parts looked for again.
pub async fn find_full_body_parts(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
    multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let (previous, reference) = read_request(multipart).await?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    let set = full_body_set(persona.as_ref(), &outfit_id)?;
    if set.rig_asset_id.as_deref() != Some(previous.as_str()) {
        return Err(rig_revision_conflict());
    }
    let master = persona
        .as_ref()
        .and_then(|row| master_for(row, MasterSlot::FullBody(&outfit_id)))
        .ok_or_else(|| not_found("The full-body set has no picture"))?;
    tokio::spawn(find_moving_parts(
        db,
        user_id,
        Some(outfit_id),
        previous,
        master,
        reference,
    ));
    Ok((StatusCode::ACCEPTED, Json(json!({ "started": true }))))
}

/// The package the page shows (`assetId`) and that package drawn at rest as
/// the page composes it (`analysisReference`, PNG): what the vision model looks at.
async fn read_request(mut multipart: Multipart) -> ApiResult<(String, Vec<u8>)> {
    let mut asset_id = None;
    let mut reference = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| bad_request("Invalid part analysis request"))?
    {
        match field.name() {
            Some("assetId") if asset_id.is_none() => {
                let text = field
                    .text()
                    .await
                    .map_err(|_| bad_request("Invalid part analysis request"))?;
                asset_id = Some(
                    merope_rig::normalize_asset_id(&text)
                        .ok_or_else(|| bad_request("Invalid rig asset id"))?,
                );
            }
            Some("analysisReference") if reference.is_none() => {
                if field.content_type() != Some("image/png") {
                    return Err(bad_request("Rig analysis reference must be a PNG image"));
                }
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| bad_request("Invalid part analysis request"))?;
                if bytes.len() > MAX_RIG_ANALYSIS_REFERENCE_BYTES {
                    return Err(bad_request("Rig analysis reference exceeds 10 MB"));
                }
                reference = Some(bytes.to_vec());
            }
            _ => {
                return Err(bad_request(
                    "Part analysis request has an unsupported field",
                ));
            }
        }
    }
    let asset_id = asset_id.ok_or_else(|| bad_request("Part analysis request has no asset id"))?;
    let reference =
        reference.ok_or_else(|| bad_request("Part analysis request has no reference"))?;
    let (reference, _) = merope_rig::validate_png_dimensions(reference)
        .await
        .map_err(png_validation_error)?;
    Ok((asset_id, reference))
}
