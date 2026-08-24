//! Shared application error type for HTTP-facing Myriad services.
//!
//! This crate is intentionally free of Axum so `services` / future domain crates
//! can depend on it without pulling the web stack. Backend adapters implement
//! `IntoResponse` in the API layer.

use crate::redact::redact_secrets;
use http::StatusCode;
use serde::Serialize;
use serde_json::{json, Value};

/// Stable JSON body shape returned to clients.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ErrorBody {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Unified application error: status + public `error` string + optional detail.
///
/// All free-form detail is run through [`redact_secrets`] on construction.
#[derive(Debug, Clone)]
pub struct AppError {
    status: StatusCode,
    error: String,
    message: Option<String>,
    hint: Option<String>,
    code: Option<String>,
}

impl AppError {
    /// Stable machine code for well-known public labels.
    pub fn inferred_code(label: &str) -> Option<&'static str> {
        match label.trim() {
            "Database error" | "Database query failed" | "Database not connected" => {
                Some("database_error")
            }
            "Failed to fetch data" => Some("fetch_failed"),
            "Failed to process password" | "Failed to verify password" => Some("password_failed"),
            "Failed to create account" => Some("account_create_failed"),
            "Failed to create session token" | "Failed to refresh session token" => {
                Some("session_failed")
            }
            _ => None,
        }
    }

    /// Build with an explicit status. `error` is the short public label.
    pub fn new(status: StatusCode, error: impl Into<String>) -> Self {
        let error = redact_secrets(&error.into());
        let code = Self::inferred_code(&error).map(str::to_string);
        Self {
            status,
            error,
            message: None,
            hint: None,
            code,
        }
    }

    /// Build from a raw status code (e.g. reqwest/axum status as `u16`).
    /// Invalid codes fall back to 500.
    pub fn from_status_u16(code: u16, error: impl Into<String>) -> Self {
        let status = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        Self::new(status, error)
    }

    pub fn bad_request(error: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error)
    }

    pub fn unauthorized(error: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, error)
    }

    pub fn forbidden(error: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, error)
    }

    pub fn not_found(error: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, error)
    }

    pub fn conflict(error: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, error)
    }

    pub fn service_unavailable(error: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, error)
    }

    pub fn bad_gateway(error: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, error)
    }

    pub fn internal(error: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error)
    }

    /// Attach a redacted detail message (shown to clients when present).
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(redact_secrets(&message.into()));
        self
    }

    /// Attach an operator/user hint (also redacted).
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(redact_secrets(&hint.into()));
        self
    }

    /// Attach a stable machine code for clients to map (also redacted).
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        let code = redact_secrets(&code.into());
        self.code = if code.trim().is_empty() {
            None
        } else {
            Some(code)
        };
        self
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn status_u16(&self) -> u16 {
        self.status.as_u16()
    }

    pub fn error_label(&self) -> &str {
        &self.error
    }

    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    pub fn body(&self) -> ErrorBody {
        ErrorBody {
            error: self.error.clone(),
            message: self.message.clone(),
            hint: self.hint.clone(),
            code: self.code.clone(),
        }
    }

    /// JSON value matching historical `{ "error": "..." }` plus optional fields.
    pub fn to_json(&self) -> Value {
        let mut v = json!({ "error": self.error });
        if let Some(ref m) = self.message {
            v["message"] = json!(m);
        }
        if let Some(ref h) = self.hint {
            v["hint"] = json!(h);
        }
        if let Some(ref c) = self.code {
            v["code"] = json!(c);
        }
        v
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.error, self.status)
    }
}

impl std::error::Error for AppError {}

impl From<AppError> for (StatusCode, Value) {
    fn from(e: AppError) -> Self {
        (e.status(), e.to_json())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_expected_status_codes() {
        assert_eq!(AppError::bad_request("x").status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            AppError::unauthorized("x").status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(AppError::forbidden("x").status(), StatusCode::FORBIDDEN);
        assert_eq!(AppError::not_found("x").status(), StatusCode::NOT_FOUND);
        assert_eq!(AppError::conflict("x").status(), StatusCode::CONFLICT);
        assert_eq!(
            AppError::service_unavailable("x").status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(AppError::bad_gateway("x").status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            AppError::internal("x").status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn body_redacts_bearer_in_message() {
        let e = AppError::internal("upstream failed")
            .with_message("got Authorization: Bearer supersecrettoken99");
        let body = e.body();
        assert_eq!(body.error, "upstream failed");
        let msg = body.message.expect("message");
        assert!(!msg.contains("supersecrettoken99"));
        assert!(msg.contains("[REDACTED]"));
    }

    #[test]
    fn to_json_omits_empty_optional_fields() {
        let e = AppError::not_found("missing");
        let v = e.to_json();
        assert_eq!(v["error"], "missing");
        assert!(v.get("message").is_none());
        assert!(v.get("hint").is_none());
        assert!(v.get("code").is_none());
    }

    #[test]
    fn database_error_label_gets_stable_code() {
        let v = AppError::internal("Database error").to_json();
        assert_eq!(v["error"], "Database error");
        assert_eq!(v["code"], "database_error");
    }

    #[test]
    fn to_json_includes_code_when_set() {
        let e = AppError::bad_gateway("Failed to suggest a name")
            .with_code("name_suggest_failed")
            .with_message("provider timed out");
        let v = e.to_json();
        assert_eq!(v["error"], "Failed to suggest a name");
        assert_eq!(v["code"], "name_suggest_failed");
        assert_eq!(v["message"], "provider timed out");
    }

    #[test]
    fn to_json_includes_hint_when_set() {
        let e = AppError::service_unavailable("updater down").with_hint("set MYRIAD_UPDATER_URL");
        let v = e.to_json();
        assert_eq!(v["hint"], "set MYRIAD_UPDATER_URL");
    }

    #[test]
    fn into_status_value_tuple() {
        let (st, v): (StatusCode, Value) = AppError::conflict("exists").into();
        assert_eq!(st, StatusCode::CONFLICT);
        assert_eq!(v["error"], "exists");
    }
}
