//! Myriad Federation Protocol (MFP) inbox activity handlers.

use axum::{Json, http::StatusCode};
use myriad_error::AppError;
use sea_orm::ConnectionTrait;
use serde_json::json;

use super::{PostCommit, inbox_err};
use crate::federation::file_transfer::InboundChunkError;

/// Map a classified FileChunk failure. Details stay in logs; the peer sees a
/// stable label, and 4xx vs retryable 5xx follows the classification.
fn file_chunk_error(error: InboundChunkError) -> (StatusCode, Json<serde_json::Value>) {
    let status = error.status();
    if status.is_server_error() {
        tracing::error!(detail = %error.detail(), %status, "FileChunk handling failed");
    } else {
        tracing::warn!(detail = %error.detail(), %status, "FileChunk rejected");
    }
    let body = match error {
        InboundChunkError::Invalid(_) => AppError::public_json("Invalid file chunk"),
        InboundChunkError::Forbidden(_) => AppError::public_json("Access denied"),
        InboundChunkError::Closed(_) => AppError::public_json("Target is closed"),
        InboundChunkError::Conflict(_) => {
            AppError::public_json("Chunk conflicts with stored data")
        }
        InboundChunkError::NotReady(_) => {
            json!({"error": "Activity not ready", "retry": true})
        }
        InboundChunkError::Busy(_) => {
            json!({"error": "Transfer chunk budget exhausted; retry later", "retry": true})
        }
        InboundChunkError::Internal(_) => AppError::public_json("Inbox processing failed"),
    };
    (status, Json(body))
}

pub(crate) fn ensure_allowed_mfp_type(
    activity_type: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if ALLOWED_MFP_TYPES.contains(&activity_type) {
        Ok(())
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            Json(
                AppError::bad_request(format!("Unknown MFP activity type: {activity_type}"))
                    .with_code("unknown_activity")
                    .to_json(),
            ),
        ))
    }
}

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
///
/// `db` must be the caller's transaction for FileTransfer: its advisory lock
/// and progress update commit together with the receipt. Live-UI notices are
/// queued on `post_commit` for the caller to run after that commit.
pub(crate) async fn handle_mfp_activity(
    db: &impl ConnectionTrait,
    _local_user_id: Option<i32>,
    actor_url_str: &str,
    activity_type: &str,
    activity: &serde_json::Value,
    pool: &sea_orm::DatabaseConnection,
    post_commit: &mut PostCommit,
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
            let chunk = activity
                .get("object")
                .filter(|o| o.get("type").and_then(|v| v.as_str()) == Some("myriad:FileChunk"));
            let notice = if let Some(object) = chunk {
                crate::federation::file_transfer::handle_file_chunk(db, pool, actor_url_str, object)
                    .await
                    .map_err(file_chunk_error)?
            } else {
                crate::federation::file_transfer::handle_file_transfer(db, actor_url_str, activity)
                    .await
                    .map_err(|e| inbox_err("FileTransfer handling failed", e))?
            };
            post_commit.push(notice);
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
        _ => ensure_allowed_mfp_type(activity_type).map(|()| StatusCode::ACCEPTED),
    }
}

#[cfg(test)]
mod tests {
    /// `ALLOWED_MFP_TYPES` 与 `match activity_type` 必须一一对应。
    /// 缺臂会落到 `_` → 400。对源码断言，因为类型系统表达不了。
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
             so they would be signature-verified then fail closed: {undispatched:?}"
        );
    }

    #[test]
    fn file_chunk_errors_keep_retry_class_and_hide_details() {
        use crate::federation::file_transfer::InboundChunkError;
        use axum::http::StatusCode;
        let (status, body) = super::file_chunk_error(InboundChunkError::NotReady(
            "Transfer ft_x not yet present".into(),
        ));
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.0["retry"], true);
        let (status, body) = super::file_chunk_error(InboundChunkError::Internal(
            "No space left on device: /srv/data/federation/transfers/ft_x/f.part".into(),
        ));
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!body.0.to_string().contains("/srv/data"));
        let (status, _) =
            super::file_chunk_error(InboundChunkError::Forbidden("sender mismatch".into()));
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = super::file_chunk_error(InboundChunkError::Closed("cancelled".into()));
        assert_eq!(status, StatusCode::GONE);
        let (status, _) =
            super::file_chunk_error(InboundChunkError::Conflict("bytes differ".into()));
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[test]
    fn unknown_mfp_type_is_bad_request_not_accepted() {
        let err = super::ensure_allowed_mfp_type("myriad:NotAThing").unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::BAD_REQUEST);
        let shared = include_str!("receive.rs")
            .split("pub async fn post_shared_inbox")
            .nth(1)
            .expect("post_shared_inbox");
        assert!(shared.contains("ensure_allowed_mfp_type"));
    }
}
