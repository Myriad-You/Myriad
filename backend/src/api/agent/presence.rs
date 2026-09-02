//! POST /api/agent/presence — write live presence without a chat turn.
//!
//! Inbound only remembers observation. It must not call `consider_event`.
//! Wake still comes from the named-event list (`is_valuable_event`). This is
//! not a heartbeat and not a second grant source.

use super::*;
use crate::error::HttpError;
use crate::services::agent::consciousness::{
    live_presence_from_custom_data, remember_live_presence,
};
use axum::body::Bytes;
use axum::http::StatusCode;
use serde_json::{json, Value};

fn invalid_presence_payload() -> HttpError {
    HttpError::from((
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "Invalid payload" })),
    ))
}

pub fn parse_presence_body(bytes: &[u8]) -> Result<Value, HttpError> {
    serde_json::from_slice(bytes).map_err(|_| invalid_presence_payload())
}

/// Authenticated presence inbound. Body is JSON; only the live-presence
/// whitelist is kept. 204 on success. When Merope is off the write is a
/// no-op: auth still runs, the body is not parsed, nothing is remembered.
pub async fn post_live_presence(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    body: Bytes,
) -> Result<StatusCode, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    if !crate::services::agent::merope::is_enabled().await {
        return Ok(StatusCode::NO_CONTENT);
    }
    let data = parse_presence_body(&body)?;
    let live = live_presence_from_custom_data(&data);
    remember_live_presence(user_id, live);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn parse_failure_does_not_echo_raw_json() {
        let raw = br#"{"presence":{"__admin":true,"secret":"leak"}"#;
        let err = parse_presence_body(raw).unwrap_err();
        let json = err.0.to_json().to_string();
        assert!(!json.contains("__admin"), "{json}");
        assert!(!json.contains("secret"), "{json}");
        assert!(!json.contains("leak"), "{json}");
        assert_eq!(json, r#"{"error":"Invalid payload"}"#);
        let response = HttpError(err.0).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn disabled_merope_returns_before_parse_or_remember() {
        let src = include_str!("presence.rs");
        let handler = src
            .split("pub async fn post_live_presence")
            .nth(1)
            .and_then(|rest| rest.split("#[cfg(test)]").next())
            .expect("handler");
        let enabled_at = handler.find("is_enabled").expect("gate on merope switch");
        let parse_at = handler
            .find("parse_presence_body")
            .expect("parse after the gate");
        let remember_at = handler
            .find("remember_live_presence")
            .expect("write after the gate");
        assert!(
            enabled_at < parse_at,
            "disabled inbound must not parse the body"
        );
        assert!(
            enabled_at < remember_at,
            "disabled inbound must not remember live presence"
        );
    }
}
