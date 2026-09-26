//! The owner's Apply on a drafted SEO review, over HTTP. Reviewing, drafting
//! and saving are `services::seo_review`.

use axum::{Json, extract::State};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

use crate::error::HttpError;
use crate::services::seo_review::apply_site_seo_fields;
use myriad_error::AppError;

#[derive(Debug, Deserialize, Default)]
pub struct ApplySiteSeoRequest {
    #[serde(default)]
    pub site_description: Option<String>,
    #[serde(default)]
    pub site_keywords: Option<String>,
    #[serde(default)]
    pub site_ai_intro: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApplySiteSeoResponse {
    pub ok: bool,
    pub saved: Vec<String>,
}

/// POST /api/seo/apply-copy — admin; writes branding copy only.
pub async fn apply_site_seo_copy(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<ApplySiteSeoRequest>,
) -> Result<Json<ApplySiteSeoResponse>, HttpError> {
    match apply_site_seo_fields(
        &db,
        payload.site_description.as_deref(),
        payload.site_keywords.as_deref(),
        payload.site_ai_intro.as_deref(),
    )
    .await
    {
        Ok(saved) => Ok(Json(ApplySiteSeoResponse { ok: true, saved })),
        Err(message) if message.starts_with("Provide ") => {
            Err(HttpError(AppError::bad_request(message)))
        }
        Err(message) => Err(HttpError(AppError::internal(message))),
    }
}
