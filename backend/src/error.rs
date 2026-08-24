//! Axum adapter for the shared [`myriad_error::AppError`].
//!
//! Domain / services depend only on `myriad-error`. This module is the sole place
//! that couples that type to Axum's `IntoResponse`.
//!
//! We use a local newtype [`HttpError`] because Rust's orphan rules forbid
//! `impl IntoResponse for AppError` (both the trait and the type are foreign).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use myriad_error::AppError;

/// Local wrapper so handlers can `return Err(HttpError(...))` / `.into_response()`.
#[derive(Debug)]
pub struct HttpError(pub AppError);

impl From<AppError> for HttpError {
    fn from(err: AppError) -> Self {
        Self(err)
    }
}

impl From<HttpError> for AppError {
    fn from(err: HttpError) -> Self {
        err.0
    }
}

/// Convert a shared [`AppError`] into an Axum response.
pub fn app_error_response(err: AppError) -> Response {
    let status =
        StatusCode::from_u16(err.status_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(err.to_json())).into_response()
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        app_error_response(self.0)
    }
}

/// Convenience: map any `Display` failure into a redacted internal error.
#[allow(dead_code)] // used as handlers adopt AppError gradually
pub fn internal_from_display(e: impl std::fmt::Display) -> AppError {
    AppError::internal("internal error").with_message(e.to_string())
}

/// Bridge legacy `(StatusCode, Json<Value>)` handler errors into [`HttpError`].
///
/// Used while routes migrate onto `AppError` one path at a time.
pub fn status_json_to_http(err: (StatusCode, axum::Json<serde_json::Value>)) -> HttpError {
    let (status, axum::Json(v)) = err;
    let label = v
        .get("error")
        .and_then(|x| x.as_str())
        .unwrap_or("error")
        .to_string();
    let mut app = AppError::from_status_u16(status.as_u16(), label);
    if let Some(m) = v.get("message").and_then(|x| x.as_str()) {
        app = app.with_message(m);
    }
    if let Some(h) = v.get("hint").and_then(|x| x.as_str()) {
        app = app.with_hint(h);
    }
    if let Some(c) = v.get("code").and_then(|x| x.as_str()) {
        app = app.with_code(c);
    }
    HttpError(app)
}

/// Allows `?` on legacy `(StatusCode, Json<_>)` errors inside `Result<_, HttpError>` handlers.
impl From<(StatusCode, axum::Json<serde_json::Value>)> for HttpError {
    fn from(err: (StatusCode, axum::Json<serde_json::Value>)) -> Self {
        status_json_to_http(err)
    }
}

/// Bridge bare `StatusCode` handler errors (common on platform-proxy routes).
impl From<StatusCode> for HttpError {
    fn from(status: StatusCode) -> Self {
        let label = status.canonical_reason().unwrap_or("error").to_string();
        HttpError(AppError::from_status_u16(status.as_u16(), label))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn into_response_uses_status_and_json_error_field() {
        let resp = HttpError(AppError::conflict("Admin account already exists")).into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"], "Admin account already exists");
    }

    #[tokio::test]
    async fn response_redacts_secrets_in_message() {
        let resp = HttpError(
            AppError::bad_gateway("upstream")
                .with_message("Authorization: Bearer supersecrettoken99"),
        )
        .into_response();
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let s = String::from_utf8_lossy(&bytes);
        assert!(!s.contains("supersecrettoken99"), "leaked: {s}");
        assert!(s.contains("[REDACTED]"), "got: {s}");
    }

    #[tokio::test]
    async fn app_error_response_matches_http_error() {
        let err = AppError::not_found("gone");
        let a = app_error_response(err.clone());
        let b = HttpError(err).into_response();
        assert_eq!(a.status(), b.status());
        assert_eq!(a.status(), StatusCode::NOT_FOUND);
    }

    /// Federation write handlers bridge legacy domain `(StatusCode, Json)` via
    /// [`status_json_to_http`] — this drives that real path.
    #[tokio::test]
    async fn status_json_to_http_preserves_status_label_and_hint() {
        let err = status_json_to_http((
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": "Key rotation requires confirm",
                "hint": "pass {\"confirm\": true}",
                "message": "rotation aborted",
            })),
        ));
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"], "Key rotation requires confirm");
        assert_eq!(v["hint"], "pass {\"confirm\": true}");
        assert_eq!(v["message"], "rotation aborted");
    }

    #[tokio::test]
    async fn status_json_to_http_preserves_machine_code() {
        let err = status_json_to_http((
            StatusCode::BAD_GATEWAY,
            axum::Json(serde_json::json!({
                "error": "Failed to suggest a name",
                "code": "name_suggest_failed",
                "message": "provider timed out",
            })),
        ));
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"], "Failed to suggest a name");
        assert_eq!(v["code"], "name_suggest_failed");
        assert_eq!(v["message"], "provider timed out");
    }

    #[tokio::test]
    async fn status_json_to_http_preserves_error_field_for_write_paths() {
        // Write-path bridge used by setup/config/proxy migrations: legacy
        // (StatusCode, Json) errors must become HttpError with the same public `error`.
        let legacy = (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "error": "Setup already completed",
                "message": "Admin exists"
            })),
        );
        let resp = status_json_to_http(legacy).into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"], "Setup already completed");
        assert_eq!(v["message"], "Admin exists");
    }
}
