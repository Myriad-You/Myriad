//! Shared Phantasi error mapping. Storage details stay in server logs.

use axum::{Json, http::StatusCode};
use myriad_error::AppError;

use crate::error::HttpError;

pub(crate) fn phantasi_http_err(status: StatusCode, error: impl Into<String>) -> HttpError {
    HttpError::from((status, Json(AppError::fail_json(error))))
}

pub(crate) fn phantasi_store_http(
    context: &'static str,
    error: impl std::fmt::Display,
) -> HttpError {
    tracing::error!(%error, context, "phantasi store failed");
    phantasi_http_err(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to {context}"),
    )
}
