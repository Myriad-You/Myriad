//! Shared application error type for HTTP-facing Myriad services.
//!
//! This crate is intentionally free of Axum so `services` / future domain crates
//! can depend on it without pulling the web stack. Backend adapters implement
//! `IntoResponse` in the API layer.

use crate::redact::redact_secrets;
use http::StatusCode;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

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

fn is_resource_not_found(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    lower.ends_with(" not found") && lower != "method not found"
}

fn shared_label_codes() -> &'static HashMap<String, String> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| {
        let spec: Value = serde_json::from_str(include_str!("../../../shared/error_codes.json"))
            .expect("shared/error_codes.json");
        let mut map = HashMap::new();
        if let Some(labels) = spec.get("labels").and_then(Value::as_object) {
            for (label, code) in labels {
                let Some(code) = code.as_str() else { continue };
                map.insert(label.clone(), code.to_string());
                map.insert(label.to_ascii_lowercase(), code.to_string());
            }
        }
        map
    })
}

impl AppError {
    /// Stable machine code for well-known public labels (`shared/error_codes.json`).
    pub fn inferred_code(label: &str) -> Option<&'static str> {
        let trimmed = label.trim();
        let map = shared_label_codes();
        if let Some(code) = map
            .get(trimmed)
            .or_else(|| map.get(&trimmed.to_ascii_lowercase()))
        {
            return Some(code.as_str());
        }
        if is_resource_not_found(trimmed) {
            return Some("not_found");
        }
        None
    }

    /// Build with an explicit status. `error` is the short public label.
    pub fn new(status: StatusCode, error: impl Into<String>) -> Self {
        let error = redact_secrets(&error.into());
        let code = Some(
            Self::inferred_code(&error)
                .unwrap_or("unmapped")
                .to_string(),
        );
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
        v["code"] = json!(self.code.clone().unwrap_or_else(|| "unmapped".into()));
        v
    }

    /// `{error, code}` for handlers that still return raw JSON instead of [`AppError`].
    pub fn public_json(label: impl Into<String>) -> Value {
        let error = redact_secrets(&label.into());
        json!({
            "error": error,
            "code": Self::inferred_code(&error).unwrap_or("unmapped"),
        })
    }

    /// Brew-style `{success: false, error, code?}`.
    pub fn fail_json(label: impl Into<String>) -> Value {
        let mut v = Self::public_json(label);
        v["success"] = json!(false);
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
        assert_eq!(v["code"], "unmapped");
    }

    #[test]
    fn shared_label_codes_cover_inferred_public_labels() {
        let spec: Value =
            serde_json::from_str(include_str!("../../../shared/error_codes.json")).unwrap();
        for (label, code) in spec["labels"].as_object().unwrap() {
            assert_eq!(
                AppError::inferred_code(label.as_str()),
                Some(code.as_str().unwrap()),
                "{label}"
            );
        }
    }

    #[test]
    fn database_error_label_gets_stable_code() {
        let v = AppError::internal("Database error").to_json();
        assert_eq!(v["error"], "Database error");
        assert_eq!(v["code"], "database_error");
    }

    #[test]
    fn generic_http_labels_get_stable_codes() {
        assert_eq!(
            AppError::unauthorized("Unauthorized").to_json()["code"],
            "unauthorized"
        );
        assert_eq!(
            AppError::forbidden("Forbidden").to_json()["code"],
            "forbidden"
        );
        assert_eq!(
            AppError::not_found("Not found").to_json()["code"],
            "not_found"
        );
        assert_eq!(
            AppError::bad_request("Bad request").to_json()["code"],
            "bad_request"
        );
        assert_eq!(AppError::conflict("Conflict").to_json()["code"], "conflict");
        assert_eq!(
            AppError::internal("Internal server error").to_json()["code"],
            "internal_error"
        );
        assert_eq!(
            AppError::service_unavailable("Service unavailable").to_json()["code"],
            "service_unavailable"
        );
        assert_eq!(
            AppError::service_unavailable("Service in configuration mode").to_json()["code"],
            "configuration_mode"
        );
        assert_eq!(
            AppError::forbidden("Setup already completed").to_json()["code"],
            "setup_completed"
        );
        assert_eq!(
            AppError::unauthorized("Setup window closed").to_json()["code"],
            "setup_window_closed"
        );
        assert_eq!(
            AppError::unauthorized("Setup secret required").to_json()["code"],
            "setup_secret_mismatch"
        );
        assert_eq!(
            AppError::service_unavailable("Activity not ready").to_json()["code"],
            "activity_not_ready"
        );
        assert_eq!(
            AppError::forbidden("Access denied").to_json()["code"],
            "forbidden"
        );
        assert_eq!(
            AppError::not_found("User not found").to_json()["code"],
            "not_found"
        );
        assert_eq!(
            AppError::internal("Failed to find Tapp").to_json()["code"],
            "tapp_not_found"
        );
        assert_eq!(
            AppError::not_found("missing").to_json()["code"],
            "unmapped"
        );
        assert_eq!(
            AppError::internal("Method not found").to_json()["code"],
            "unmapped"
        );
        assert_eq!(
            AppError::internal("Failed to update user").to_json()["code"],
            "account_update_failed"
        );
        assert_eq!(
            AppError::internal("Failed to load config").to_json()["code"],
            "config_load_failed"
        );
        assert_eq!(
            AppError::internal("Failed to persist Tapp").to_json()["code"],
            "tapp_save_failed"
        );
        assert_eq!(
            AppError::internal("Failed to save MCP config").to_json()["code"],
            "mcp_config_save_failed"
        );
        let unauthorized = AppError::public_json("Unauthorized");
        assert_eq!(unauthorized["error"], "Unauthorized");
        assert_eq!(unauthorized["code"], "unauthorized");
        let user_missing = AppError::public_json("User not found");
        assert_eq!(user_missing["code"], "not_found");
        let brew = AppError::fail_json("Source not found");
        assert_eq!(brew["success"], false);
        assert_eq!(brew["code"], "not_found");
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
        assert_eq!(v["code"], "unmapped");
    }
}
