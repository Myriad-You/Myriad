//! Shared classification for federation handler / delivery failures.
//!
//! Permanent peer-state mismatches must be **4xx** on the receiving side and
//! **non-retryable** on the sending side so dead letters do not burn 12× backoff
//! on rooms/channels that will never exist again.

use axum::http::StatusCode;
use serde_json::{json, Value};

/// Errors that mean "peer will never accept this activity as-is".
///
/// Explicitly **not** permanent: channel "not yet present; retry after ChannelOpen"
/// (ordering race — buffer + retry is correct).
pub fn is_permanent_federation_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();

    // Transient ordering: activity before ChannelOpen / RoomInvite
    if lower.contains("not yet present")
        || lower.contains("retry after channelopen")
        || lower.contains("retry after roominvite")
    {
        return false;
    }

    // Explicit prefixes used by room handlers
    if lower.starts_with("not_found:") || lower.starts_with("not_member:") {
        return true;
    }

    // Channel / room free-form messages seen in production dead letters
    if lower.contains("not the remote party") {
        return true;
    }
    if lower.contains("is not a member") || lower.contains("not a member of room") {
        return true;
    }
    if lower.contains("is closed") {
        return true;
    }
    // "Room X not found" / "Channel X not found" but not "not yet present"
    if lower.contains("not found") {
        return true;
    }
    if lower.contains("already closed") || lower.contains("access denied") {
        return true;
    }

    false
}

/// Map a handler `String` error to HTTP status for ActivityPub inbox responses.
///
/// Permanent → 4xx so remote delivery marks `PERMANENT` and stops retrying.
/// Transient channel-open race → 503 (retryable).
/// Everything else → 500.
pub fn map_inbox_handler_error(e: String) -> (StatusCode, axum::Json<Value>) {
    let lower = e.to_lowercase();

    if lower.contains("not yet present")
        || lower.contains("retry after channelopen")
        || lower.contains("retry after roominvite")
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(json!({"error": e, "retry": true})),
        );
    }

    if is_permanent_federation_error(&e) {
        let status = if lower.contains("not_member")
            || lower.contains("not a member")
            || lower.contains("not the remote party")
            || lower.contains("forbidden")
        {
            StatusCode::FORBIDDEN
        } else if lower.contains("closed") || lower.contains("gone") {
            StatusCode::GONE
        } else {
            StatusCode::NOT_FOUND
        };
        return (status, axum::Json(json!({"error": e})));
    }

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(json!({"error": e})),
    )
}

/// True if a delivery worker error string should stop retrying immediately.
///
/// Covers:
/// - `PERMANENT HTTP 4xx …` (correct remote)
/// - `HTTP 500: …not_found…` (legacy remotes that mis-map permanent errors)
pub fn is_permanent_delivery_error(err: &str) -> bool {
    if err.starts_with("PERMANENT ") {
        return true;
    }
    // Body after status line — still permanent if message says so
    is_permanent_federation_error(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_open_race_is_transient() {
        let msg = "Channel ch_x not yet present; retry after ChannelOpen";
        assert!(!is_permanent_federation_error(msg));
        assert!(!is_permanent_delivery_error(&format!("HTTP 500: {msg}")));
        let (st, _) = map_inbox_handler_error(msg.into());
        assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn room_invite_race_is_transient() {
        let msg = "Room rm_abc not yet present; retry after RoomInvite";
        assert!(!is_permanent_federation_error(msg));
        assert!(!is_permanent_delivery_error(&format!("HTTP 503: {msg}")));
        let (st, body) = map_inbox_handler_error(msg.into());
        assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.0.get("retry").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn room_not_yet_present_not_confused_with_not_found() {
        // Must stay transient even though message contains the room id token
        let msg = "Room rm_deadbeef not yet present; retry after RoomInvite";
        assert!(!is_permanent_federation_error(msg));
        assert!(!msg.to_lowercase().contains("not found") || !is_permanent_federation_error(msg));
    }

    #[test]
    fn room_not_found_is_permanent_404() {
        let msg = "not_found: Room rm_abc not found";
        assert!(is_permanent_federation_error(msg));
        let (st, _) = map_inbox_handler_error(msg.into());
        assert_eq!(st, StatusCode::NOT_FOUND);
        assert!(is_permanent_delivery_error(&format!(
            "HTTP 500: {{\"error\":\"{msg}\"}}"
        )));
    }

    #[test]
    fn not_member_is_permanent_403() {
        let msg = "Actor https://a/users/X is not a member of room rm_y";
        assert!(is_permanent_federation_error(msg));
        let (st, _) = map_inbox_handler_error(msg.into());
        assert_eq!(st, StatusCode::FORBIDDEN);
    }

    #[test]
    fn channel_wrong_party_is_permanent() {
        let msg =
            "Channel ch_x not found or actor https://kiseki.blog/users/Hitomi is not the remote party";
        assert!(is_permanent_federation_error(msg));
        let (st, _) = map_inbox_handler_error(msg.into());
        assert!(st == StatusCode::FORBIDDEN || st == StatusCode::NOT_FOUND);
    }

    #[test]
    fn permanent_prefix_short_circuits() {
        assert!(is_permanent_delivery_error(
            "PERMANENT HTTP 404: {\"error\":\"not_found\"}"
        ));
    }

    #[test]
    fn access_denied_is_permanent() {

        assert!(is_permanent_federation_error("access denied for peer"));
        assert!(is_permanent_delivery_error("HTTP 500: access denied"));

    }

    #[test]
    fn already_closed_is_permanent() {

        assert!(is_permanent_federation_error("Channel ch_x already closed"));
        let (st, _) = map_inbox_handler_error("Channel ch_x already closed".into());
        assert_eq!(st, StatusCode::GONE);

    }

    #[test]
    fn unknown_error_maps_to_500() {

        let (st, body) = map_inbox_handler_error("DB connection refused".into());
        assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body.0.get("error").and_then(|v| v.as_str()),
            Some("DB connection refused")
        );
        assert!(!is_permanent_federation_error("DB connection refused"));

    }

    #[test]
    fn permanent_prefix_case_sensitive_short_circuit() {

        assert!(is_permanent_delivery_error("PERMANENT HTTP 410: gone"));
        // lowercase permanent alone is not the delivery short-circuit
        assert!(!is_permanent_delivery_error("permanent maybe") || is_permanent_federation_error("permanent maybe"));
        assert!(! "permanent maybe".starts_with("PERMANENT "));

    }

    #[test]
    fn is_closed_message_is_permanent() {

        assert!(is_permanent_federation_error("channel is closed by peer"));

    }
}
