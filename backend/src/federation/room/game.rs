//! Structured game session messages on federation rooms.
//!
//! Game traffic reuses Room fan-out and membership. The type prefix
//! `game:<tappId>:<protocol>` and a small intent/state envelope keep
//! chat keyword filters from treating chess notation as post text.

use serde_json::Value;

use myriad_tapp_contract::contract_rules::{
    DEFAULT_TAPP_GAME_MESSAGE_BYTES, MAX_TAPP_GAME_MESSAGE_BYTES, MAX_TAPP_GAME_PLAYERS,
    MAX_TAPP_GAME_PROTOCOL_LEN, MIN_TAPP_GAME_PLAYERS,
};

pub const GAME_MESSAGE_PREFIX: &str = "game:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMessageType<'a> {
    pub tapp_id: &'a str,
    pub protocol: &'a str,
}

pub fn is_game_message_type(message_type: &str) -> bool {
    parse_game_message_type(message_type).is_some()
}

pub fn parse_game_message_type(message_type: &str) -> Option<GameMessageType<'_>> {
    let rest = message_type.strip_prefix(GAME_MESSAGE_PREFIX)?;
    let (tapp_id, protocol) = rest.split_once(':')?;
    if !is_tapp_id(tapp_id) || !is_protocol(protocol) {
        return None;
    }
    Some(GameMessageType { tapp_id, protocol })
}

pub fn format_game_message_type(tapp_id: &str, protocol: &str) -> Result<String, String> {
    if !is_tapp_id(tapp_id) {
        return Err("Invalid game tapp id".into());
    }
    if !is_protocol(protocol) {
        return Err("Invalid game protocol".into());
    }
    Ok(format!("{GAME_MESSAGE_PREFIX}{tapp_id}:{protocol}"))
}

pub fn format_share_room_id(room_id: &str, home_server: &str) -> String {
    let home = home_server.trim().trim_end_matches('/');
    if home.is_empty() || room_id.contains('@') {
        room_id.to_string()
    } else {
        format!("{room_id}@{home}")
    }
}

/// Reject malformed game traffic; leave ordinary chat untouched.
pub fn validate_outgoing_game_message(
    message_type: &str,
    payload: &Value,
    encrypt: bool,
) -> Result<(), String> {
    if !message_type.starts_with(GAME_MESSAGE_PREFIX) {
        return Ok(());
    }
    if parse_game_message_type(message_type).is_none() {
        return Err(
            "Game message_type must be game:<tappId>:<protocol> with a safe protocol name".into(),
        );
    }
    if encrypt {
        return Err("Game session messages cannot be E2E-encrypted".into());
    }
    validate_game_payload(payload, DEFAULT_TAPP_GAME_MESSAGE_BYTES as usize)
}

pub fn validate_game_payload(payload: &Value, max_bytes: usize) -> Result<(), String> {
    let encoded = payload.to_string();
    if encoded.len() > max_bytes.min(MAX_TAPP_GAME_MESSAGE_BYTES as usize) {
        return Err(format!(
            "Game payload too large: {} bytes (max {max_bytes})",
            encoded.len()
        ));
    }
    let obj = payload
        .as_object()
        .ok_or_else(|| "Game payload must be a JSON object".to_string())?;
    let kind = obj
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "Game payload.kind is required".to_string())?;
    if kind != "intent" && kind != "state" {
        return Err("Game payload.kind must be intent or state".into());
    }
    let seq = obj
        .get("seq")
        .and_then(Value::as_u64)
        .ok_or_else(|| "Game payload.seq must be an unsigned integer".to_string())?;
    if seq > 1_000_000_000_000 {
        return Err("Game payload.seq is implausibly large".into());
    }
    let nonce = obj
        .get("nonce")
        .and_then(Value::as_str)
        .ok_or_else(|| "Game payload.nonce is required".to_string())?;
    if nonce.is_empty() || nonce.len() > 128 || !nonce.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Game payload.nonce must be 1-128 URL-safe characters".into());
    }
    if !obj.contains_key("body") {
        return Err("Game payload.body is required".into());
    }
    for key in obj.keys() {
        if !matches!(key.as_str(), "kind" | "seq" | "nonce" | "body") {
            return Err(format!("Unknown game payload field: {key}"));
        }
    }
    Ok(())
}

pub fn validate_room_game_config(
    tapp_id: &str,
    protocol: &str,
    max_players: Option<i32>,
    max_message_bytes: Option<i32>,
) -> Result<(), String> {
    if !is_tapp_id(tapp_id) {
        return Err("Invalid game tapp id".into());
    }
    if !is_protocol(protocol) {
        return Err("Invalid game protocol".into());
    }
    if let Some(players) = max_players {
        if players < MIN_TAPP_GAME_PLAYERS as i32 || players > MAX_TAPP_GAME_PLAYERS as i32 {
            return Err(format!(
                "game.max_players must be {MIN_TAPP_GAME_PLAYERS}-{MAX_TAPP_GAME_PLAYERS}"
            ));
        }
    }
    if let Some(bytes) = max_message_bytes {
        if bytes < 1024 || bytes > MAX_TAPP_GAME_MESSAGE_BYTES as i32 {
            return Err(format!(
                "game.max_message_bytes must be 1024-{MAX_TAPP_GAME_MESSAGE_BYTES}"
            ));
        }
    }
    Ok(())
}

pub fn activity_is_structured_game_message(activity: &Value) -> bool {
    let top = activity.get("type").and_then(Value::as_str).unwrap_or("");
    let object_type = activity
        .pointer("/object/type")
        .and_then(Value::as_str)
        .unwrap_or("");
    if top != "myriad:RoomMessage" && object_type != "myriad:RoomMessage" {
        return false;
    }
    let message_type = activity
        .pointer("/object/messageType")
        .and_then(Value::as_str)
        .or_else(|| {
            activity
                .pointer("/object/message_type")
                .and_then(Value::as_str)
        })
        .unwrap_or("");
    if !is_game_message_type(message_type) {
        return false;
    }
    let Some(payload) = activity.pointer("/object/payload") else {
        return false;
    };
    validate_game_payload(payload, DEFAULT_TAPP_GAME_MESSAGE_BYTES as usize).is_ok()
}

fn is_tapp_id(value: &str) -> bool {
    let len = value.len();
    (1..=128).contains(&len)
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        && value.contains('.')
}

fn is_protocol(value: &str) -> bool {
    let len = value.len();
    (1..=MAX_TAPP_GAME_PROTOCOL_LEN).contains(&len)
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_and_rejects_message_types() {
        assert!(parse_game_message_type("game:com.example.chess:v1").is_some());
        assert!(parse_game_message_type("text").is_none());
        assert!(parse_game_message_type("game:not-an-id:v1").is_none());
        assert!(parse_game_message_type("game:com.example.chess:V1").is_none());
    }

    #[test]
    fn validates_intent_envelope() {
        let payload = json!({
            "kind": "intent",
            "seq": 3,
            "nonce": "n1",
            "body": {"action": "place", "row": 7}
        });
        assert!(validate_outgoing_game_message("game:com.example.chess:v1", &payload, false).is_ok());
        assert!(validate_outgoing_game_message("game:com.example.chess:v1", &payload, true).is_err());
        assert!(validate_outgoing_game_message("text", &payload, false).is_ok());
    }

    #[test]
    fn rejects_unknown_fields_and_keyword_shaped_chat() {
        let extra = json!({"kind":"state","seq":1,"nonce":"a","body":{},"cheat":true});
        assert!(validate_game_payload(&extra, 1024).is_err());
        assert!(!activity_is_structured_game_message(&json!({"type":"Note","content":"bomb"})));
        assert!(activity_is_structured_game_message(&json!({
            "type": "myriad:RoomMessage",
            "object": {
                "type": "myriad:RoomMessage",
                "messageType": "game:com.example.chess:v1",
                "payload": {"kind":"intent","seq":1,"nonce":"n","body":{}}
            }
        })));
        assert!(
            !activity_is_structured_game_message(&json!({
                "type": "Create",
                "content": "buy spam now",
                "object": {
                    "type": "Note",
                    "content": "buy spam now",
                    "messageType": "game:com.example.chess:v1"
                }
            })),
            "ordinary notes must not skip filters by spoofing messageType"
        );
    }

    #[test]
    fn share_id_joins_home_server() {
        assert_eq!(
            format_share_room_id("rm_abc", "peer.example:8443"),
            "rm_abc@peer.example:8443"
        );
        assert_eq!(format_share_room_id("rm_abc@peer.example", "ignored"), "rm_abc@peer.example");
    }
}
