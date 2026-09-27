//! AI-assisted SEO / GEO copy over HTTP: the settings form's "draft for me".
//! The drafting is `services::seo_copy`.

use axum::{Json, extract::State};
use sea_orm::DatabaseConnection;

use crate::error::HttpError;
use crate::services::seo_copy::{
    GenerateSiteSeoRequest, GenerateSiteSeoResponse, generate_site_seo_copy_with_db,
};
use myriad_error::AppError;

/// POST /api/seo/generate-copy — admin/authenticated; uses site AI config.
pub async fn generate_site_seo_copy(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<GenerateSiteSeoRequest>,
) -> Result<Json<GenerateSiteSeoResponse>, HttpError> {
    Ok(Json(
        generate_site_seo_copy_with_db(&db, payload)
            .await
            .map_err(|message| HttpError(AppError::bad_request(message)))?,
    ))
}
