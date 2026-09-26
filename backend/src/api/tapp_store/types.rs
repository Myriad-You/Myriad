use axum::Json;
use axum::http::StatusCode;
use serde::Serialize;

use crate::error::HttpError;

/// API 响应
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }
}

/// Domain catalog DTOs (path-stable re-export for handlers / public API).
pub use crate::services::tapp_catalog::{TappDetail, TappListItem};

/// Map store envelope errors onto [`HttpError`] (same status + `error` string).
pub(super) fn api_http_error(status: StatusCode, message: impl Into<String>) -> HttpError {
    HttpError::from((status, Json(myriad_error::AppError::public_json(message))))
}
