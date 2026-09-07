// Federation file transfer handlers and authenticated router surface.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::json;

use crate::api;
use crate::error::status_json_to_http;
use crate::extract;
use crate::federation;
use crate::middleware;

use super::social::*;

/// 路由已声明该路径参数；不再手工解析 URI。
async fn federation_list_transfers(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
    match federation::file_transfer::list_transfers(
        &channel_id,
        claims.sub.parse().unwrap_or(0),
        &db,
    )
    .await
    {
        Ok(transfers) => (
            StatusCode::OK,
            Json(json!({"transfers": transfers, "total": transfers.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数并挂了 auth_middleware；
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 64 KiB）。
async fn federation_initiate_room_transfer(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    Json(transfer_req): Json<federation::file_transfer::InitTransferRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::file_transfer::initiate_room_transfer(
        user_id,
        &claims.username,
        &room_id,
        &db,
        &transfer_req,
    )
    .await
    {
        Ok(t) => (StatusCode::CREATED, Json(json!(t))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；不再手工解析 URI。
async fn federation_list_room_transfers(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    match federation::file_transfer::list_room_transfers(
        &room_id,
        claims.sub.parse().unwrap_or(0),
        &claims.username,
        &db,
    )
    .await
    {
        Ok(transfers) => (
            StatusCode::OK,
            Json(json!({"transfers": transfers, "total": transfers.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明 `{room_id}`；before/limit/filter/q 全部走 `Query<ListQuery>`。
async fn federation_list_room_files(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::list_room_files(
        user_id,
        &claims.username,
        &room_id,
        &db,
        q.before(),
        q.limit(),
        q.filter(),
        q.q(),
    )
    .await
    {
        Ok(result) => (StatusCode::OK, Json(json!(result))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；不再手工解析 URI。
async fn federation_get_transfer(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(transfer_id): axum::extract::Path<String>,
) -> Response {
    match federation::file_transfer::get_transfer(
        &transfer_id,
        claims.sub.parse().unwrap_or(0),
        &claims.username,
        &db,
    )
    .await
    {
        Ok(t) => (StatusCode::OK, Json(json!(t))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明 `{transfer_id}`。
///
/// 只把 claims/path/db 的样板换成提取器；下面的流式响应与 RFC 5987 文件名
/// 处理原样保留 —— 这条路径返回的是文件流而不是 JSON。
async fn federation_download_transfer(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(transfer_id): axum::extract::Path<String>,
) -> Response {
    use axum::body::Body;
    use axum::http::header::{HeaderValue, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE};
    use tokio::io::AsyncReadExt;
    use tokio_stream::wrappers::ReceiverStream;

    let file = match federation::file_transfer::open_transfer_file(
        &transfer_id,
        claims.sub.parse().unwrap_or(0),
        &claims.username,
        &db,
    )
    .await
    {
        Ok(f) => f,
        Err((status, json)) => return (status, json).into_response(),
    };

    // RFC 5987 filename* for non-ASCII; ASCII fallback for legacy clients.
    let ascii_name: String = file
        .filename
        .chars()
        .map(|c| {
            if c.is_ascii() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let ascii_name = if ascii_name.trim_matches('_').is_empty() {
        "download".to_string()
    } else {
        ascii_name
    };
    let encoded = urlencoding::encode(&file.filename);
    let disposition = format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        ascii_name, encoded
    );

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(4);
    let disk_path = file.path.clone();
    tokio::spawn(async move {
        let mut f = match tokio::fs::File::open(&disk_path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = tx.send(Err(e)).await;
                return;
            }
        };
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match f.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(Ok(buf[..n].to_vec())).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            }
        }
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    let mut res = Response::new(body);
    *res.status_mut() = StatusCode::OK;
    let headers = res.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&file.mime_type) {
        headers.insert(CONTENT_TYPE, v);
    } else {
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
    }
    if let Ok(v) = HeaderValue::from_str(&disposition) {
        headers.insert(CONTENT_DISPOSITION, v);
    }
    if let Ok(v) = HeaderValue::from_str(&file.file_size.to_string()) {
        headers.insert(CONTENT_LENGTH, v);
    }
    res
}

/// 路由已声明 `{transfer_id}`。
///
/// 块内容是 base64（4/3 膨胀），上限由路由的 `TRANSFER_CHUNK_BODY_LIMIT` 层
/// 提供 —— 见 `federation::limits`，那里有编译期断言保证它容得下一整块。
async fn federation_upload_chunk(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(transfer_id): axum::extract::Path<String>,
    chunk_req: Result<
        Json<federation::file_transfer::UploadChunkRequest>,
        axum::extract::rejection::JsonRejection,
    >,
) -> Response {
    let Json(chunk_req) = match chunk_req {
        Ok(v) => v,
        Err(e) => {
            return json_rejection_response(e, Some("Chunk payload exceeds the per-chunk limit"))
        }
    };

    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::file_transfer::upload_chunk(
        user_id,
        &claims.username,
        &transfer_id,
        &db,
        &chunk_req,
    )
    .await
    {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
async fn federation_cancel_transfer(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(transfer_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::file_transfer::cancel_transfer(user_id, &claims.username, &transfer_id, &db)
        .await
    {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// Start unified server with all routes (middleware controls access based on mode)
/// Authenticated `/api/federation/*` API surface.
///
/// Tapp host attribution and authentication are applied once at router level
/// instead of per route: `auth_middleware` runs first (outermost), then
/// `federation_host_attribution`, so grant-bearing requests are enforced on
/// every federation route — including ones added later — instead of silently
/// executing with host identity when a per-route layer is forgotten
/// (fail-closed). Admin-only trust management keeps its extra admin gate.
pub fn router(app_state: crate::state::AppState) -> axum::Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    let main_router = Router::<crate::state::AppState>::new()
        .route("/api/federation/identity", get(federation_identity))
        .route("/api/federation/keys/rotate", post(federation_keys_rotate))
        .route("/api/federation/follow", post(federation_follow))
        .route("/api/federation/unfollow", post(federation_unfollow))
        .route("/api/federation/following", get(federation_following_list))
        .route("/api/federation/followers", get(federation_followers_list))
        .route("/api/federation/timeline", get(federation_timeline))
        .route("/api/federation/publish", post(federation_publish))
        .route("/api/federation/notes", post(federation_create_note))
        .route("/api/federation/like", post(federation_like))
        .route("/api/federation/unlike", post(federation_unlike))
        .route("/api/federation/bookmark", post(federation_bookmark))
        .route("/api/federation/unbookmark", post(federation_unbookmark))
        .route("/api/federation/bookmarks", get(federation_bookmarks_list))
        .route("/api/federation/announce", post(federation_announce))
        .route("/api/federation/unannounce", post(federation_unannounce))
        .route("/api/federation/objects", get(federation_get_object))
        .route("/api/federation/unpublish", post(federation_unpublish))
        .route("/api/federation/published", get(federation_published_list))
        .route(
            "/api/federation/channels",
            get(federation_list_channels).post(federation_create_channel),
        )
        .route(
            "/api/federation/channels/{channel_id}",
            get(federation_get_channel).delete(federation_delete_channel),
        )
        .route(
            "/api/federation/channels/{channel_id}/close",
            post(federation_close_channel),
        )
        .route(
            "/api/federation/channels/{channel_id}/accept",
            post(federation_accept_channel),
        )
        .route(
            "/api/federation/channels/{channel_id}/e2e/key-exchange",
            post(federation_e2e_key_exchange),
        )
        .route(
            "/api/federation/channels/{channel_id}/messages",
            get(federation_get_messages).post(federation_send_message),
        )
        .route(
            "/api/federation/channels/{channel_id}/ws-ticket",
            post(api::tapp_runtime::mint_channel_ws_ticket),
        )
        .route(
            "/api/federation/channels/{channel_id}/ws",
            get(federation::ws_gateway::channel_websocket),
        )
        .route(
            "/api/federation/rooms",
            get(federation_list_rooms)
                .post(federation_create_room)
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route(
            "/api/federation/rooms/{room_id}",
            get(federation_get_room)
                .put(federation_update_room)
                .delete(federation_delete_room)
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route(
            "/api/federation/rooms/{room_id}/members",
            get(federation_get_room_members),
        )
        .route(
            "/api/federation/rooms/{room_id}/invite",
            post(federation_invite_room_member).layer(axum::middleware::from_fn(
                crate::federation::limits::live_small_control_body_limit,
            )),
        )
        .route(
            "/api/federation/rooms/{room_id}/accept",
            post(federation_accept_room_invite),
        )
        .route(
            "/api/federation/rooms/{room_id}/reject",
            post(federation_reject_room_invite),
        )
        .route(
            "/api/federation/rooms/{room_id}/join",
            post(federation_join_room).layer(axum::middleware::from_fn(
                crate::federation::limits::live_small_control_body_limit,
            )),
        )
        .route(
            "/api/federation/rooms/{room_id}/members/{actor}",
            delete(federation_remove_room_member),
        )
        .route(
            "/api/federation/rooms/{room_id}/members/{actor}/role",
            put(federation_set_room_member_role).layer(axum::middleware::from_fn(
                crate::federation::limits::live_small_control_body_limit,
            )),
        )
        .route(
            "/api/federation/rooms/{room_id}/leave",
            post(federation_leave_room),
        )
        .route(
            "/api/federation/rooms/{room_id}/transfer-ownership",
            post(federation_transfer_room_ownership).layer(axum::middleware::from_fn(
                crate::federation::limits::live_small_control_body_limit,
            )),
        )
        .route(
            "/api/federation/rooms/{room_id}/messages",
            get(federation_get_room_messages).post(federation_send_room_message),
        )
        .route(
            "/api/federation/rooms/{room_id}/e2e/key-exchange",
            post(federation_room_e2e_key_exchange),
        )
        .route(
            "/api/federation/rooms/{room_id}/stickers",
            post(federation_add_room_sticker).layer(axum::middleware::from_fn(
                crate::federation::limits::live_small_control_body_limit,
            )),
        )
        .route(
            "/api/federation/rooms/{room_id}/stickers/{sticker_id}",
            delete(federation_remove_room_sticker),
        )
        .route(
            "/api/federation/rooms/{room_id}/messages/{message_id}/pin",
            post(federation_pin_room_message).layer(axum::middleware::from_fn(
                crate::federation::limits::live_small_control_body_limit,
            )),
        )
        .route(
            "/api/federation/rooms/{room_id}/ws-ticket",
            post(api::tapp_runtime::mint_room_ws_ticket),
        )
        .route(
            "/api/federation/rooms/{room_id}/ws",
            get(federation::ws_gateway::room_websocket),
        )
        .route(
            "/api/federation/rings",
            get(federation_list_rings)
                .post(federation_create_ring)
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route("/api/federation/rings/{ring_id}", get(federation_get_ring))
        .route(
            "/api/federation/rings/{ring_id}/leave",
            post(federation_leave_ring),
        )
        .route(
            "/api/federation/rings/{ring_id}/peers",
            get(federation_get_ring_peers)
                .post(federation_add_ring_peer)
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route(
            "/api/federation/rings/{ring_id}/peers/{peer}",
            delete(federation_remove_ring_peer),
        )
        .route(
            "/api/federation/rings/{ring_id}/sync",
            post(federation_trigger_ring_sync),
        )
        .route(
            "/api/federation/delivery/stats",
            get(federation_delivery_stats),
        )
        .route("/api/federation/delivery", get(federation_list_delivery))
        .route(
            "/api/federation/delivery/retry-dead",
            post(federation_retry_all_dead_delivery),
        )
        .route(
            "/api/federation/delivery/cancel-pending",
            post(federation_cancel_all_pending_delivery),
        )
        .route(
            "/api/federation/delivery/purge-dead",
            post(federation_purge_dead_delivery),
        )
        .route(
            "/api/federation/delivery/{id}/retry",
            post(federation_retry_delivery),
        )
        .route(
            "/api/federation/delivery/{id}/cancel",
            post(federation_cancel_delivery),
        )
        .route(
            "/api/federation/delivery/{id}",
            delete(federation_dismiss_delivery),
        )
        .route(
            "/api/federation/trust/policy",
            get(federation_get_trust_policy)
                .put(federation_update_trust_policy)
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route(
            "/api/federation/trust/instances",
            get(federation_list_instances),
        )
        .route(
            "/api/federation/trust/update",
            post(federation_update_instance_trust)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                ))
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route(
            "/api/federation/trust/block",
            post(federation_toggle_instance_block)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                ))
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route(
            "/api/federation/trust/filters",
            get(federation_list_content_filters)
                .post(federation_create_content_filter)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                ))
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route(
            "/api/federation/trust/filters/{id}",
            axum::routing::put(federation_update_content_filter)
                .delete(federation_delete_content_filter)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                ))
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route(
            "/api/federation/channels/{channel_id}/transfers",
            get(federation_list_transfers)
                .post(federation_initiate_transfer)
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route(
            "/api/federation/rooms/{room_id}/transfers",
            get(federation_list_room_transfers)
                .post(federation_initiate_room_transfer)
                .layer(axum::middleware::from_fn(
                    crate::federation::limits::live_small_control_body_limit,
                )),
        )
        .route(
            "/api/federation/rooms/{room_id}/files",
            get(federation_list_room_files),
        )
        .route(
            "/api/federation/transfers/{transfer_id}",
            get(federation_get_transfer),
        )
        .route(
            "/api/federation/transfers/{transfer_id}/content",
            get(federation_download_transfer),
        )
        .route(
            "/api/federation/transfers/{transfer_id}/chunks",
            post(federation_upload_chunk).layer(axum::middleware::from_fn(
                crate::federation::limits::live_transfer_chunk_body_limit,
            )),
        )
        .route(
            "/api/federation/transfers/{transfer_id}/cancel",
            post(federation_cancel_transfer),
        )
        // 已认证本地用户提交的联邦写操作（follow/channel/room/message、
        // base64 文件分块、Tapp 包快照）。上限见 federation::limits —— 那里
        // 同时约束「本端发得出的东西本端必须收得进」。
        .layer(axum::middleware::from_fn(
            crate::federation::limits::live_authenticated_body_limit,
        ))
        .route_layer(from_fn_with_state(
            app_state.clone(),
            api::tapp_runtime::federation_host_attribution,
        ))
        .route_layer(from_fn_with_state(
            app_state.clone(),
            middleware::auth::auth_middleware,
        ));

    // Freeform Note 媒体：图片/视频上限见 federation::limits::{note_image_limit,
    // note_video_limit}；路由层取二者中较大者再留信封余量。
    let media_router = Router::<crate::state::AppState>::new()
        .route("/api/federation/media", post(federation_media_upload))
        .layer(axum::middleware::from_fn(
            crate::federation::limits::live_note_media_body_limit,
        ))
        .route_layer(from_fn_with_state(
            app_state.clone(),
            api::tapp_runtime::federation_host_attribution,
        ))
        .route_layer(from_fn_with_state(
            app_state.clone(),
            middleware::auth::auth_middleware,
        ));

    main_router.merge(media_router)
}

#[cfg(test)]
mod query_parse_tests {
    use super::{LimitQuery, PurgeDeadQuery};

    /// `?limit=` 的宽松解析必须保持不变。
    #[test]
    fn limit_query_falls_back_instead_of_rejecting() {
        let malformed = LimitQuery {
            limit: Some("abc".into()),
            status: None,
        };
        assert_eq!(malformed.or(50), 50);

        let empty = LimitQuery {
            limit: Some(String::new()),
            status: None,
        };
        assert_eq!(empty.or(100), 100);

        let absent = LimitQuery {
            limit: None,
            status: None,
        };
        assert_eq!(absent.or(7), 7);

        let valid = LimitQuery {
            limit: Some("25".into()),
            status: None,
        };
        assert_eq!(valid.or(50), 25);

        let negative = LimitQuery {
            limit: Some("-1".into()),
            status: None,
        };
        assert_eq!(negative.or(50), -1);
    }

    #[test]
    fn purge_dead_query_accepts_loose_truthy_values() {
        let t = |v: &str| {
            PurgeDeadQuery {
                limit: None,
                cancelled_only: Some(v.into()),
            }
            .cancelled_only()
        };

        for truthy in ["1", "true", "TRUE", "yes", "on", "On"] {
            assert!(t(truthy), "{truthy} should be truthy");
        }
        for falsy in ["0", "false", "no", "off", "", "garbage"] {
            assert!(!t(falsy), "{falsy} should be falsy");
        }

        assert!(!PurgeDeadQuery {
            limit: None,
            cancelled_only: None,
        }
        .cancelled_only());

        assert_eq!(
            PurgeDeadQuery {
                limit: Some("abc".into()),
                cancelled_only: None,
            }
            .limit_or(100),
            100
        );
    }
}
