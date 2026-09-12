//! Myriad Federation Protocol (MFP) inbox activity handlers.

use axum::{http::StatusCode, Json};
use sea_orm::ConnectionTrait;
use serde_json::json;

use super::inbox_err;

pub(crate) const ALLOWED_MFP_TYPES: &[&str] = &[
    "myriad:ChannelOpen",
    "myriad:ChannelClose",
    "myriad:ChannelAccept",
    "myriad:ChannelMessage",
    "myriad:RoomInvite",
    "myriad:RoomJoin",
    "myriad:RoomLeave",
    "myriad:RoomDissolve",
    "myriad:RoomMessage",
    "myriad:RoomPin",
    "myriad:RoomGovernance",
    "myriad:RingJoin",
    "myriad:RingSync",
    "myriad:RingLeave",
    "myriad:FileTransfer",
    "myriad:KeyExchange",
];

/// 处理 MFP 扩展 Activity（myriad:ChannelOpen, myriad:ChannelMessage, myriad:ChannelClose 等）
pub(crate) async fn handle_mfp_activity(
    db: &impl ConnectionTrait,
    _local_user_id: Option<i32>,
    actor_url_str: &str,
    activity_type: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    tracing::info!(
        "📬 MFP activity received: type={}, actor={}",
        activity_type,
        actor_url_str
    );

    match activity_type {
        "myriad:ChannelOpen" => {
            crate::federation::channel::handle_channel_open(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("ChannelOpen handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:ChannelMessage" => {
            crate::federation::channel::handle_channel_message(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("ChannelMessage handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:ChannelClose" => {
            crate::federation::channel::handle_channel_close(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("ChannelClose handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        // Room
        "myriad:RoomInvite" => {
            crate::federation::room::handle_room_invite(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomInvite handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomMessage" => {
            crate::federation::room::handle_room_message(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomMessage handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomLeave" => {
            crate::federation::room::handle_room_leave(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomLeave handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomDissolve" => {
            crate::federation::room::handle_room_dissolve(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomDissolve handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        // Ring
        "myriad:RingJoin" => {
            crate::federation::ring::handle_ring_join(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RingJoin handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RingSync" => {
            crate::federation::ring::handle_ring_sync(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RingSync handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RingLeave" => {
            crate::federation::ring::handle_ring_leave(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RingLeave handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:FileTransfer" => {
            if activity
                .get("object")
                .and_then(|o| o.get("type"))
                .and_then(|v| v.as_str())
                == Some("myriad:FileChunk")
            {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(AppError::public_json(
                        "FileChunk requires a transactional filesystem outbox",
                    )),
                ));
            }
            crate::federation::file_transfer::handle_file_transfer(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("FileTransfer handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:ChannelAccept" => {
            crate::federation::channel::handle_channel_accept(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("ChannelAccept handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:KeyExchange" => {
            // Channel vs Room：object.room 优先，否则走 Channel
            let is_room = activity
                .get("object")
                .and_then(|o| o.get("room"))
                .and_then(|v| v.as_str())
                .is_some();
            if is_room {
                crate::federation::room::handle_key_exchange(db, actor_url_str, activity)
                    .await
                    .map_err(|e| inbox_err("Room KeyExchange handling failed", e))?;
            } else {
                crate::federation::channel::handle_key_exchange(db, actor_url_str, activity)
                    .await
                    .map_err(|e| inbox_err("Channel KeyExchange handling failed", e))?;
            }
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomJoin" => {
            crate::federation::room::handle_room_join(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomJoin handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomPin" => {
            crate::federation::room::handle_room_pin(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomPin handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomGovernance" => {
            crate::federation::room::handle_room_governance(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomGovernance handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        _ => {
            tracing::info!("Unhandled MFP activity type: {}", activity_type);
            Ok(StatusCode::ACCEPTED)
        }
    }
}

#[cfg(test)]
mod tests {
    /// `ALLOWED_MFP_TYPES` 与 `match activity_type` 必须一一对应。
    /// 缺臂会落到 `_` → 202 然后丢弃。对源码断言，因为类型系统表达不了。
    #[test]
    fn every_allowed_mfp_type_has_a_dispatch_arm() {
        let src = include_str!("mfp.rs");

        let allowlist = src
            .split("const ALLOWED_MFP_TYPES: &[&str] = &[")
            .nth(1)
            .expect("ALLOWED_MFP_TYPES literal moved")
            .split("];")
            .next()
            .unwrap();
        let allowed: Vec<&str> = allowlist
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter_map(|l| l.strip_prefix('"'))
            .filter_map(|l| l.split('"').next())
            .collect();
        assert!(
            allowed.len() >= 15,
            "expected the full MFP allowlist, got {allowed:?}"
        );

        let dispatch = src
            .split("async fn handle_mfp_activity(")
            .nth(1)
            .expect("handle_mfp_activity moved");

        let undispatched: Vec<&&str> = allowed
            .iter()
            .filter(|ty| !dispatch.contains(&format!("\"{ty}\"")))
            .collect();

        assert!(
            undispatched.is_empty(),
            "these MFP types are accepted by the inbox allowlist but have no dispatch arm, \n\
             so they would be signature-verified, answered 202, then silently dropped: {undispatched:?}"
        );
    }
}
use myriad_error::AppError;
