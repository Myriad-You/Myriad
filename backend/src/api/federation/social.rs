use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::error::status_json_to_http;
use crate::extract;
use crate::federation;

/// 小请求体联邦端点的上限（64 KiB）。
///
/// 这些端点原本用 `to_bytes(req.into_body(), 64 * 1024)` 自己封顶。改用 `Json<T>`
/// 提取器后上限由 layer 决定，若不显式加这一层就会退回到 AUTHENTICATED_BODY_LIMIT ——
/// 一个只需要几百字节 JSON 的端点没有理由缓冲满额 body。
/// `?limit=` 查询参数。
///
/// 用 `Option<String>` 而不是 `Option<i64>` 是为了保持原有的宽松语义：
/// 手写解析对 `limit=abc` / `limit=` 是静默回落到默认值，而 `Option<i64>`
/// 会让 serde 直接拒绝成 400。行为改变不该夹带在重构里。
#[derive(serde::Deserialize)]
pub(crate) struct LimitQuery {
    pub(crate) limit: Option<String>,
    /// Optional status filter: pending | delivering | delivered | dead
    pub(crate) status: Option<String>,
}

impl LimitQuery {
    pub(crate) fn or(&self, default: i64) -> i64 {
        self.limit
            .as_deref()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(default)
    }

    pub(crate) fn status_filter(&self) -> Option<&str> {
        self.status
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}

/// `?limit=&cancelled_only=` 查询参数。
///
/// `cancelled_only` 保持原有的宽松真值解析（`1|true|yes|on`，其余一律 false）。
/// 换成 `Option<bool>` 会让 serde 只认 `true`/`false`，把 `?cancelled_only=1`
/// 变成 400 —— 前端正在用的写法。
#[derive(serde::Deserialize)]
pub(crate) struct PurgeDeadQuery {
    pub(crate) limit: Option<String>,
    pub(crate) cancelled_only: Option<String>,
}

impl PurgeDeadQuery {
    pub(crate) fn limit_or(&self, default: i64) -> i64 {
        self.limit
            .as_deref()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(default)
    }

    pub(crate) fn cancelled_only(&self) -> bool {
        self.cancelled_only
            .as_deref()
            .map(|s| matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false)
    }
}

/// 分页与过滤查询参数（消息列表、房间文件列表共用）。
///
/// 全部字段用 `Option<String>`：手写解析对 `limit=abc` 是静默忽略、回落到
/// 「不限制」，而 `Option<i64>` 会让 serde 直接拒成 400。`filter` / `q` 本就是
/// 字符串，`list_room_files` 才用得到。
#[derive(serde::Deserialize)]
pub(crate) struct ListQuery {
    pub(crate) before: Option<String>,
    pub(crate) limit: Option<String>,
    pub(crate) filter: Option<String>,
    pub(crate) q: Option<String>,
}

impl ListQuery {
    pub(crate) fn before(&self) -> Option<&str> {
        self.before.as_deref()
    }

    /// 解析失败即 `None`（与手写的 `.and_then(|s| s.parse().ok())` 一致）。
    pub(crate) fn limit(&self) -> Option<i64> {
        self.limit.as_deref().and_then(|v| v.parse::<i64>().ok())
    }

    pub(crate) fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    pub(crate) fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }
}

/// 把 `Json<T>` 提取失败翻译成本项目的 JSON 错误体。
///
/// 直接用 `Json<T>` 会让超限 body 拿到 axum 的纯文本 413，丢掉
/// `send_room_message` 原有的那句运维指引（"内联图片上限 ~32 MiB，
/// 更大的走分块传输"）—— 那是用户真正需要看到的下一步动作。
///
/// 体积类拒绝（413）附带 `size_hint`，其余按原状态码返回解析错误详情。
/// Shared by federation HTTP adapters and `federation::limits` extract helpers.
pub fn json_rejection_response(
    rejection: axum::extract::rejection::JsonRejection,
    size_hint: Option<&str>,
) -> Response {
    let status = rejection.status();
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        let mut body = json!({"error": "Request body too large or unreadable"});
        if let Some(hint) = size_hint {
            body["hint"] = json!(hint);
        }
        return (status, Json(body)).into_response();
    }
    (status, Json(json!({"error": rejection.body_text()}))).into_response()
}

pub(crate) const FEDERATION_SMALL_BODY_LIMIT: usize = federation::limits::SMALL_CONTROL_BODY_LIMIT;

// Federation Wrappers

/// POST /api/admin/federation/domain-move
///
/// POST /api/admin/federation/domain-move
///
/// Emit ActivityPub Move for every local user (domain migration). Admin only.
/// `AdminClaims` 取代函数体里的 `federation_admin_required`。
pub async fn admin_federation_domain_move(
    extract::AdminClaims(_claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::move_actor::DomainMoveRequest>,
) -> Response {
    match federation::move_actor::domain_move_all_users(&db, &payload).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, body)) => status_json_to_http((status, body)).into_response(),
    }
}

/// GET /api/federation/identity — 获取当前登录用户的联邦地址
/// 路由已挂 `auth_middleware`，claims 由 `AuthedClaims` 直接取出。
pub(crate) async fn federation_identity(
    extract::Db(db): extract::Db,
    extract::AuthedClaims(claims): extract::AuthedClaims,
) -> Response {
    let identity = federation::actor::get_local_identity(&db, &claims.username).await;
    (StatusCode::OK, Json(identity)).into_response()
}

/// POST /api/federation/keys/rotate — explicit federation key rotation
///
/// 路由已挂 auth_middleware。
///
/// 这里用 `Bytes` 而不是 `Json<Value>`：原实现是
/// `from_slice(..).unwrap_or(json!({}))`，空 body / 畸形 JSON 会落到「缺少
/// confirm」这条**带操作指引**的 400；换成 `Json` 提取器会先被 axum 拒成一条
/// 通用错误，用户看不到"需要 {\"confirm\": true}"这句提示。
pub(crate) async fn federation_keys_rotate(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    body_bytes: axum::body::Bytes,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or(json!({}));
    if !federation::actor::rotation_confirm_accepted(&body) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Key rotation requires {\"confirm\": true}",
                "hint": "This permanently replaces your federation signing key. Peers must re-fetch your actor document."
            })),
        )
            .into_response();
    }

    match federation::actor::rotate_user_federation_keys(&db, user_id, &claims.username).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(e) => {
            tracing::error!(
                user_id = user_id,
                username = %claims.username,
                error = %e,
                "Federation key rotation failed"
            );
            (StatusCode::INTERNAL_SERVER_ERROR, {
                let msg = if e.contains("DB error") || e.contains("Database error") {
                    tracing::error!(error = %e, "Federation API database failure");
                    "Database error".to_string()
                } else {
                    e
                };
                Json(json!({"error": msg}))
            })
                .into_response()
        }
    }
}

/// POST /api/federation/follow — 关注远程用户
/// 路由已挂 auth_middleware；claims / body / db 走提取器。
/// body 上限仍由路由的 DefaultBodyLimit 决定（与改造前一致）。
pub(crate) async fn federation_follow(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::follow::FollowRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::follow::follow_remote(user_id, &claims.username, &db, &payload.target).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// POST /api/federation/unfollow — 取消关注远程用户
/// 路由已挂 auth_middleware；claims / body / db 走提取器。
/// body 上限仍由路由的 DefaultBodyLimit 决定（与改造前一致）。
pub(crate) async fn federation_unfollow(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::follow::FollowRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::follow::unfollow_remote(user_id, &claims.username, &db, &payload.target).await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// GET /api/federation/following — 获取我关注的远程用户列表
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_following_list(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match get_follow_list(&db, user_id, "outgoing").await {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            let msg = if e.contains("DB error") || e.contains("Database error") {
                tracing::error!(error = %e, "Federation API database failure");
                "Database error".to_string()
            } else {
                e
            };
            Json(json!({"error": msg}))
        })
            .into_response(),
    }
}

/// GET /api/federation/followers — 获取关注我的远程用户列表
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_followers_list(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match get_follow_list(&db, user_id, "incoming").await {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            let msg = if e.contains("DB error") || e.contains("Database error") {
                tracing::error!(error = %e, "Federation API database failure");
                "Database error".to_string()
            } else {
                e
            };
            Json(json!({"error": msg}))
        })
            .into_response(),
    }
}

/// GET /api/federation/timeline — 获取联邦时间线
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_timeline(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match get_federation_timeline(&db, user_id).await {
        Ok(timeline) => (StatusCode::OK, Json(timeline)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            let msg = if e.contains("DB error") || e.contains("Database error") {
                tracing::error!(error = %e, "Federation API database failure");
                "Database error".to_string()
            } else {
                e
            };
            Json(json!({"error": msg}))
        })
            .into_response(),
    }
}

// Phase 2: Content Publishing Wrappers

/// POST /api/federation/publish — 发布内容到联邦网络
/// 路由已挂 auth_middleware；claims / body / db 走提取器。
/// body 上限仍由路由的 DefaultBodyLimit 决定（与改造前一致）。
pub(crate) async fn federation_publish(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::content::PublishRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::content::publish_content(user_id, &claims.username, &db, &payload).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims / body / db 全部走提取器。
pub(crate) async fn federation_like(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::ObjectIdRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::interactions::like_object(user_id, &claims.username, &db, &payload.object_id)
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims / body / db 全部走提取器。
pub(crate) async fn federation_unlike(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::ObjectIdRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::interactions::unlike_object(
        user_id,
        &claims.username,
        &db,
        &payload.object_id,
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims / body / db 全部走提取器。
pub(crate) async fn federation_bookmark(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::ObjectIdRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::interactions::bookmark_object(user_id, &db, &payload.object_id).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims / body / db 全部走提取器。
pub(crate) async fn federation_unbookmark(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::ObjectIdRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::interactions::unbookmark_object(user_id, &db, &payload.object_id).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_bookmarks_list(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::interactions::list_bookmarks(user_id, &db).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims / body / db 全部走提取器。
pub(crate) async fn federation_announce(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::AnnounceRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    let content = payload.content.as_deref().unwrap_or("");
    match federation::interactions::announce_object(
        user_id,
        &claims.username,
        &db,
        &payload.object_id,
        content,
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims / body / db 全部走提取器。
pub(crate) async fn federation_unannounce(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::ObjectIdRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::interactions::unannounce_object(
        user_id,
        &claims.username,
        &db,
        &payload.object_id,
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// GET /api/federation/objects?id= — resolve a public object for quote click-through.
/// Does not require following the author (local DB + optional remote public fetch).
pub(crate) async fn federation_get_object(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Query(q): axum::extract::Query<federation::interactions::GetObjectQuery>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::interactions::get_object(user_id, &db, &q.id).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// POST /api/federation/notes — 创建 freeform Note（文本 + 附件）
/// 路由已挂 auth_middleware；body 上限仍由路由的 DefaultBodyLimit 决定。
pub(crate) async fn federation_create_note(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::content::CreateNoteRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::content::create_note(user_id, &claims.username, &db, &payload).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；`Multipart` 是 axum 自带的提取器，
/// 提取失败（非 multipart/form-data）由它自己返回 400。
pub(crate) async fn federation_media_upload(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);

    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename = "upload.bin".to_string();
    let mut mime = "application/octet-stream".to_string();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name != "file" {
            continue;
        }
        if let Some(fname) = field.file_name() {
            filename = fname.to_string();
        }
        if let Some(ct) = field.content_type() {
            mime = ct.to_string();
        }
        match field.bytes().await {
            Ok(b) => file_bytes = Some(b.to_vec()),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "Failed to read file field"})),
                )
                    .into_response()
            }
        }
        break;
    }

    let Some(bytes) = file_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing multipart field 'file'"})),
        )
            .into_response();
    };

    match federation::content::store_federation_media(user_id, &filename, &mime, &bytes).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；body 上限仍由路由的 DefaultBodyLimit 决定。
pub(crate) async fn federation_unpublish(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let content_type = payload["content_type"].as_str().unwrap_or("").trim();
    let content_id = payload["content_id"].as_str().unwrap_or("").trim();
    let activity_id = payload["activity_id"].as_str().unwrap_or("").trim();
    let has_activity = !activity_id.is_empty();
    let has_content = !content_id.is_empty(); // content_type optional when activity_id or inferable
    if !has_activity && !has_content {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Provide activity_id, or content_type + content_id"
            })),
        )
            .into_response();
    }
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::content::unpublish_content(
        user_id,
        &claims.username,
        &db,
        if content_type.is_empty() {
            None
        } else {
            Some(content_type)
        },
        if content_id.is_empty() {
            None
        } else {
            Some(content_id)
        },
        if activity_id.is_empty() {
            None
        } else {
            Some(activity_id)
        },
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// GET /api/federation/published — 获取已发布内容列表
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_published_list(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::content::list_published(user_id, &db).await {
        Ok(items) => (
            StatusCode::OK,
            Json(json!({"items": items, "total": items.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

// Phase 3: Channel Wrapper Functions

/// 路由已挂 auth_middleware；body 上限仍由路由的 DefaultBodyLimit 决定。
pub(crate) async fn federation_create_channel(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::channel::CreateChannelRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::channel::create_channel(user_id, &claims.username, &db, &payload).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// Channel 列表
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_list_channels(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::channel::list_channels(user_id, &claims.username, &db).await {
        Ok(channels) => (
            StatusCode::OK,
            Json(json!({"channels": channels, "total": channels.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// Channel 详情
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_get_channel(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::channel::get_channel(user_id, &channel_id, &db).await {
        Ok(detail) => (StatusCode::OK, Json(serde_json::to_value(detail).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 关闭 Channel
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_close_channel(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::channel::close_channel(user_id, &claims.username, &channel_id, &db).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 删除已关闭的 Channel（本地硬删除）
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_delete_channel(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::channel::delete_channel(user_id, &channel_id, &db).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 接受 Channel
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_accept_channel(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::channel::accept_channel(user_id, &claims.username, &channel_id, &db).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 发起 Channel E2E 密钥交换
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_e2e_key_exchange(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::channel::initiate_e2e_key_exchange(
        user_id,
        &claims.username,
        &channel_id,
        &db,
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明 `{channel_id}`；body 上限来自 federation 路由层。
pub(crate) async fn federation_send_message(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
    payload: Result<
        Json<federation::channel::SendMessageRequest>,
        axum::extract::rejection::JsonRejection,
    >,
) -> Response {
    let Json(parsed) = match payload {
        Ok(v) => v,
        Err(e) => {
            return json_rejection_response(
                e,
                Some("Inline payloads max ~4 MiB (MESSAGE_PAYLOAD_LIMIT); larger files use chunked transfer"),
            )
        }
    };

    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::channel::send_message(user_id, &claims.username, &channel_id, &db, &parsed)
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明 `{channel_id}`；分页参数走 `Query<ListQuery>`。
pub(crate) async fn federation_get_messages(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::channel::get_messages(user_id, &channel_id, &db, q.before(), q.limit()).await
    {
        Ok(messages) => (
            StatusCode::OK,
            Json(json!({"messages": messages, "total": messages.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

// Phase 4: Room 多方通信 Wrapper

/// 路由通过 `DefaultBodyLimit::max(FEDERATION_SMALL_BODY_LIMIT)` 保留原有的
/// 64 KiB 上限 —— 换成 Json 提取器后若不显式加这层，端点会退回到
/// 路由级的 AUTHENTICATED_BODY_LIMIT，等于放大可缓冲的请求体。
pub(crate) async fn federation_create_room(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(parsed): Json<federation::room::CreateRoomRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::create_room(user_id, &claims.username, &db, &parsed).await {
        Ok(detail) => (StatusCode::OK, Json(json!(detail))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_list_rooms(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::list_rooms(user_id, &claims.username, &db).await {
        Ok(rooms) => (
            StatusCode::OK,
            Json(json!({"rooms": rooms, "total": rooms.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；claims / path / db 全部走提取器，
/// 不再手工 strip_prefix 重解析 URI。
pub(crate) async fn federation_get_room(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::get_room(user_id, &claims.username, &room_id, &db).await {
        Ok(detail) => (StatusCode::OK, Json(json!(detail))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数并挂了 auth_middleware；
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 64 KiB）。
pub(crate) async fn federation_update_room(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    Json(parsed): Json<federation::room::UpdateRoomRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::update_room(user_id, &claims.username, &room_id, &db, &parsed).await {
        Ok(detail) => (StatusCode::OK, Json(json!(detail))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；claims / path / db 全部走提取器，
/// 不再手工 strip_prefix 重解析 URI。
pub(crate) async fn federation_delete_room(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::delete_room(user_id, &claims.username, &room_id, &db).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_get_room_members(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::get_members(user_id, &claims.username, &room_id, &db).await {
        Ok(members) => (
            StatusCode::OK,
            Json(json!({"members": members, "total": members.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数并挂了 auth_middleware；
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 64 KiB）。
pub(crate) async fn federation_invite_room_member(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    Json(parsed): Json<federation::room::InviteMemberRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::invite_member(user_id, &claims.username, &room_id, &db, &parsed).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由声明的是 `{room_id}/members/{actor}`。
///
/// `Path<(String, String)>` 会对每段做百分号解码，与原先手工
/// `urlencoding::decode(actor)` 等价 —— actor 是完整 URL，必然带编码。
pub(crate) async fn federation_remove_room_member(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path((room_id, target_actor)): axum::extract::Path<(String, String)>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::remove_member(user_id, &claims.username, &room_id, &target_actor, &db)
        .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_leave_room(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::leave_room(user_id, &claims.username, &room_id, &db).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// POST /api/federation/rooms/{room_id}/accept — accept pending room invite
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_accept_room_invite(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::accept_room_invite(user_id, &claims.username, &room_id, &db).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// POST /api/federation/rooms/{room_id}/reject — reject pending room invite
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_reject_room_invite(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::reject_room_invite(user_id, &claims.username, &room_id, &db).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明 `{room_id}`。
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 64 KiB）。
pub(crate) async fn federation_transfer_room_ownership(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    Json(parsed): Json<serde_json::Value>,
) -> Response {
    // `new_owner` 是规范键名，`actor` 是老客户端的写法 —— 兜底保留。
    let new_owner = parsed
        .get("new_owner")
        .or_else(|| parsed.get("actor"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::transfer_room_ownership(
        user_id,
        &claims.username,
        &room_id,
        &new_owner,
        &db,
    )
    .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 发起 Room E2E 密钥发布
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_room_e2e_key_exchange(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::initiate_e2e_key_exchange(user_id, &claims.username, &room_id, &db)
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// PUT /api/federation/rooms/{room_id}/members/{actor}/role — owner sets admin|member
pub(crate) async fn federation_set_room_member_role(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path((room_id, actor)): axum::extract::Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    let role = body
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Path may be percent-encoded actor URL
    let actor = urlencoding::decode(&actor)
        .map(|s| s.into_owned())
        .unwrap_or(actor);
    match federation::room::set_member_role(user_id, &claims.username, &room_id, &actor, &role, &db)
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// POST /api/federation/rooms/{room_id}/stickers — share sticker into group pack
pub(crate) async fn federation_add_room_sticker(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    Json(req): Json<federation::room::AddRoomStickerRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::add_room_sticker(user_id, &claims.username, &room_id, req, &db).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// DELETE /api/federation/rooms/{room_id}/stickers/{sticker_id}
pub(crate) async fn federation_remove_room_sticker(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path((room_id, sticker_id)): axum::extract::Path<(String, String)>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::remove_room_sticker(
        user_id,
        &claims.username,
        &room_id,
        &sticker_id,
        &db,
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明 `{room_id}`；body 上限来自 federation 路由层（见 `federation::limits`）。
///
/// 用 `Result<Json<T>, JsonRejection>` 而不是裸 `Json<T>`：超限时要保住原有的
/// 413 + 分块传输指引，而不是 axum 的纯文本拒绝。
pub(crate) async fn federation_send_room_message(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    payload: Result<
        Json<federation::room::SendRoomMessageRequest>,
        axum::extract::rejection::JsonRejection,
    >,
) -> Response {
    let Json(parsed) = match payload {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                room_id = %room_id,
                error = %e,
                "[Room] send message body rejected (likely over DefaultBodyLimit)"
            );
            return json_rejection_response(
                e,
                Some("Inline payloads max ~4 MiB (MESSAGE_PAYLOAD_LIMIT); larger files use chunked transfer"),
            );
        }
    };

    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::send_room_message(user_id, &claims.username, &room_id, &db, &parsed)
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(json!(resp))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明 `{room_id}`；分页参数走 `Query<ListQuery>`。
pub(crate) async fn federation_get_room_messages(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::get_room_messages(
        user_id,
        &claims.username,
        &room_id,
        &db,
        q.before(),
        q.limit(),
    )
    .await
    {
        Ok(messages) => (
            StatusCode::OK,
            Json(json!({"messages": messages, "total": messages.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由声明的是 `{room_id}/messages/{message_id}/pin`。
/// `Path<(String, String)>` 对每段做百分号解码，与原先手工
/// `urlencoding::decode(message_id)` 等价。
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 16 KiB）。
pub(crate) async fn federation_pin_room_message(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path((room_id, message_id)): axum::extract::Path<(String, String)>,
    Json(parsed): Json<federation::room::PinRoomMessageRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::pin_room_message(
        user_id,
        &claims.username,
        &room_id,
        &message_id,
        &db,
        &parsed,
    )
    .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

// Phase 5: Ring 去中心化环网

/// `AdminClaims` 取代函数体里的 `federation_admin_required` —— 这些 ring 端点的
/// 路由只有 router 级 `auth_middleware`（普通登录），管理员校验必须留在这里。
/// 写进签名后，路由被挪动或重挂中间件也带不走它。
pub(crate) async fn federation_create_ring(
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    Json(create_req): Json<federation::ring::CreateRingRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::ring::create_ring(user_id, &db, &create_req).await {
        Ok(ring) => (StatusCode::CREATED, Json(json!(ring))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

pub(crate) async fn federation_list_rings(extract::Db(db): extract::Db) -> Response {
    match federation::ring::list_rings(&db).await {
        Ok(rings) => (
            StatusCode::OK,
            Json(json!({"rings": rings, "total": rings.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；不再手工解析 URI。
pub(crate) async fn federation_get_ring(
    extract::Db(db): extract::Db,
    axum::extract::Path(ring_id): axum::extract::Path<String>,
) -> Response {
    match federation::ring::get_ring(&ring_id, &db).await {
        Ok(ring) => (StatusCode::OK, Json(json!(ring))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 见 [`federation_create_ring`]：管理员校验由 `AdminClaims` 承担。
pub(crate) async fn federation_leave_ring(
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(ring_id): axum::extract::Path<String>,
) -> Response {
    match federation::ring::leave_ring(&ring_id, &claims.username, &db).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；不再手工解析 URI。
pub(crate) async fn federation_get_ring_peers(
    extract::Db(db): extract::Db,
    axum::extract::Path(ring_id): axum::extract::Path<String>,
) -> Response {
    match federation::ring::get_peers(&ring_id, &db).await {
        Ok(peers) => (
            StatusCode::OK,
            Json(json!({"peers": peers, "total": peers.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 见 [`federation_create_ring`]：管理员校验由 `AdminClaims` 承担。
pub(crate) async fn federation_add_ring_peer(
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(ring_id): axum::extract::Path<String>,
    Json(add_req): Json<federation::ring::AddPeerRequest>,
) -> Response {
    match federation::ring::add_peer(&ring_id, &claims.username, &db, &add_req).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 见 [`federation_create_ring`]：管理员校验由 `AdminClaims` 承担。
///
/// `Path<(String, String)>` 会对每段做百分号解码，与原先手工
/// `urlencoding::decode(peer)` 等价 —— peer 是完整 Actor URL，必然带编码。
pub(crate) async fn federation_remove_ring_peer(
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path((ring_id, peer_url)): axum::extract::Path<(String, String)>,
) -> Response {
    match federation::ring::remove_peer(&ring_id, &peer_url, &claims.username, &db).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 见 [`federation_create_ring`]：管理员校验由 `AdminClaims` 承担。
pub(crate) async fn federation_trigger_ring_sync(
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(ring_id): axum::extract::Path<String>,
) -> Response {
    match federation::ring::trigger_sync(&ring_id, &claims.username, &db).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

// Phase 5 补全: Trust 策略管理

/// GET /api/federation/delivery/stats — user delivery queue counters
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_delivery_stats(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::delivery::delivery_stats_for_user(&db, user_id).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            let msg = if e.contains("DB error") || e.contains("Database error") {
                tracing::error!(error = %e, "Federation API database failure");
                "Database error".to_string()
            } else {
                e
            };
            Json(json!({"error": msg}))
        })
            .into_response(),
    }
}

/// POST /api/federation/delivery/{id}/retry — requeue a dead/stuck item
/// 路由已声明该数值路径参数；`Path<i32>` 取代手工 strip + parse。
pub(crate) async fn federation_retry_delivery(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(queue_id): axum::extract::Path<i32>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::delivery::retry_delivery_item(&db, user_id, queue_id).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// POST /api/federation/delivery/{id}/cancel — cancel pending/delivering item
/// 路由已声明该数值路径参数；`Path<i32>` 取代手工 strip + parse。
pub(crate) async fn federation_cancel_delivery(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(queue_id): axum::extract::Path<i32>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::delivery::cancel_delivery_item(&db, user_id, queue_id).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 路由已挂 auth_middleware；`Query<LimitQuery>` 取代手工切 query 串。
pub(crate) async fn federation_retry_all_dead_delivery(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Query(q): axum::extract::Query<LimitQuery>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::delivery::retry_all_dead_for_user(&db, user_id, q.or(50)).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 路由已挂 auth_middleware；`Query<LimitQuery>` 取代手工切 query 串。
pub(crate) async fn federation_cancel_all_pending_delivery(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Query(q): axum::extract::Query<LimitQuery>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::delivery::cancel_all_pending_for_user(&db, user_id, q.or(100)).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// DELETE /api/federation/delivery/{id} — purge a dead queue row (user-owned dismiss)
/// 路由已声明该数值路径参数；`Path<i32>` 取代手工 strip + parse。
pub(crate) async fn federation_dismiss_delivery(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(queue_id): axum::extract::Path<i32>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    {
        match federation::delivery::dismiss_delivery_item(&db, user_id, queue_id).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
        }
    }
}

/// 路由已挂 auth_middleware；`Query<PurgeDeadQuery>` 保留原有的宽松真值解析。
pub(crate) async fn federation_purge_dead_delivery(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Query(q): axum::extract::Query<PurgeDeadQuery>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::delivery::purge_dead_for_user(
        &db,
        user_id,
        q.limit_or(100),
        q.cancelled_only(),
    )
    .await
    {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// POST /api/federation/rooms/{room_id}/join — self-join open/public rooms
///
/// 路由已声明 `{room_id}`。
///
/// body 是**可选**的（`{"home_server": "…"}`，空 body 合法），所以用
/// `Option<Json<T>>` 而不是 `Json<T>` —— 后者会把「不带 body 加入房间」
/// 这个正常用法拒成 400。
///
/// 与原实现有一处**有意的差异**：畸形 JSON 原先被 `unwrap_or_default()` 静默
/// 吞掉，现在会返回 400。用户写错 `home_server` 时显式报错优于静默忽略。
/// 行为已由 `federation::limits` 里的 `optional_json_distinguishes_absent_from_malformed`
/// 实测锁定。
pub(crate) async fn federation_join_room(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    body: Option<Json<federation::room::JoinRoomRequest>>,
) -> Response {
    let join_req = body.map(|Json(v)| v).unwrap_or_default();
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::room::join_room(user_id, &claims.username, &room_id, &db, Some(&join_req))
        .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// GET /api/federation/public/rooms/{room_id} — unauthenticated public room card
/// 路由已声明该路径参数；不再手工解析 URI。
pub async fn federation_get_public_room(
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    match federation::room::get_public_room(&room_id, &db).await {
        Ok(info) => (StatusCode::OK, Json(json!(info))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；`Query<LimitQuery>` 取代手工 form_urlencoded 解析。
pub(crate) async fn federation_list_delivery(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Query(q): axum::extract::Query<LimitQuery>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::delivery::list_delivery_for_user_filtered(
        &db,
        user_id,
        q.or(30),
        q.status_filter(),
    )
    .await
    {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            let msg = if e.contains("DB error") || e.contains("Database error") {
                tracing::error!(error = %e, "Federation API database failure");
                "Database error".to_string()
            } else {
                e
            };
            Json(json!({"error": msg}))
        })
            .into_response(),
    }
}

pub(crate) async fn federation_get_trust_policy(extract::Db(db): extract::Db) -> Response {
    match federation::trust::get_policy(&db).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 见 [`federation_create_ring`]：管理员校验由 `AdminClaims` 承担。
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 64 KiB）。
pub(crate) async fn federation_update_trust_policy(
    extract::AdminClaims(_claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let min_trust = payload
        .get("min_trust_level")
        .and_then(|v| v.as_i64())
        .map(|n| n as i16);
    let allowed_domains = payload.get("allowed_domains").and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
    });
    let auto_discover = payload.get("auto_discover").and_then(|v| v.as_bool());
    // Prefer nested rate_limit { max_requests_per_window, window_seconds, trusted_multiplier }
    // with flat keys as fallback for older clients.
    let rate_obj = payload.get("rate_limit");
    let rate_max = rate_obj
        .and_then(|r| r.get("max_requests_per_window"))
        .or_else(|| payload.get("rate_max_requests"))
        .and_then(|v| v.as_i64());
    let rate_window = rate_obj
        .and_then(|r| r.get("window_seconds"))
        .or_else(|| payload.get("rate_window_seconds"))
        .and_then(|v| v.as_i64());
    let rate_mul = rate_obj
        .and_then(|r| r.get("trusted_multiplier"))
        .or_else(|| payload.get("rate_trusted_multiplier"))
        .and_then(|v| v.as_i64());
    match federation::trust::update_policy(
        &db,
        min_trust,
        allowed_domains,
        auto_discover,
        rate_max,
        rate_window,
        rate_mul,
    )
    .await
    {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

pub(crate) async fn federation_list_instances(extract::Db(db): extract::Db) -> Response {
    match federation::trust::list_instances(&db).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 路由已挂 admin_middleware；`AuthedClaims` 保留原 wrapper 的 401 行为。
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 64 KiB）。
pub(crate) async fn federation_update_instance_trust(
    extract::AuthedClaims(_claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let domain = match payload.get("domain").and_then(|v| v.as_str()) {
        Some(d) => d.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "domain required"})),
            )
                .into_response()
        }
    };
    let level = payload
        .get("trust_level")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i16;
    match federation::trust::update_instance_trust(&db, &domain, level).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

pub(crate) async fn federation_list_content_filters(extract::Db(db): extract::Db) -> Response {
    match federation::trust::list_content_filters(&db).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 路由已挂 admin_middleware；`AuthedClaims` 保留原 wrapper 的 401 行为。
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 64 KiB）。
pub(crate) async fn federation_create_content_filter(
    extract::AuthedClaims(_claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let filter_type = payload
        .get("filter_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let value = payload
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let enabled = payload
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    match federation::trust::create_content_filter(&db, &name, &filter_type, &value, enabled).await
    {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 路由已声明该数值路径参数并挂了 admin_middleware；
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 64 KiB）。
pub(crate) async fn federation_update_content_filter(
    extract::AuthedClaims(_claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(id): axum::extract::Path<i32>,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let name = payload.get("name").and_then(|v| v.as_str());
    let filter_type = payload.get("filter_type").and_then(|v| v.as_str());
    let value = payload.get("value").and_then(|v| v.as_str());
    let enabled = payload.get("enabled").and_then(|v| v.as_bool());
    match federation::trust::update_content_filter(&db, id, name, filter_type, value, enabled).await
    {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 路由已声明该数值路径参数并挂了 admin_middleware。
pub(crate) async fn federation_delete_content_filter(
    extract::AdminClaims(_claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(id): axum::extract::Path<i32>,
) -> Response {
    match federation::trust::delete_content_filter(&db, id).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 路由已挂 admin_middleware；`AuthedClaims` 保留原 wrapper 的 401 行为。
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 64 KiB）。
pub(crate) async fn federation_toggle_instance_block(
    extract::AuthedClaims(_claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let domain = match payload.get("domain").and_then(|v| v.as_str()) {
        Some(d) => d.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "domain required"})),
            )
                .into_response()
        }
    };
    let block = payload
        .get("block")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    match federation::trust::toggle_instance_block(&db, &domain, block).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

// Phase 5 补全: 文件传输

/// 路由已声明该路径参数并挂了 auth_middleware；
/// body 上限由路由的 `FEDERATION_SMALL_BODY_LIMIT` 层提供（原为内联 64 KiB）。
pub(crate) async fn federation_initiate_transfer(
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
    Json(transfer_req): Json<federation::file_transfer::InitTransferRequest>,
) -> Response {
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    match federation::file_transfer::initiate_transfer(
        user_id,
        &claims.username,
        &channel_id,
        &db,
        &transfer_req,
    )
    .await
    {
        Ok(t) => (StatusCode::CREATED, Json(json!(t))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

pub(crate) async fn get_follow_list(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    direction: &str,
) -> Result<serde_json::Value, String> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ra.actor_url, ra.username, ra.domain, ra.display_name,
                      ra.avatar_url, f.status, f.created_at
               FROM federation_follows f
               JOIN federation_remote_actors ra ON ra.id = f.remote_actor_id
               WHERE f.user_id = $1 AND f.direction = $2
               ORDER BY f.created_at DESC"#,
            [user_id.into(), direction.into()],
        ))
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            "Database error".to_string()
        })?;

    let list: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            json!({
                "actor_url": r.try_get::<String>("", "actor_url").unwrap_or_default(),
                // Nullable columns must use Option — try_get::<String> fails on NULL
                "username": r.try_get::<Option<String>>("", "username").ok().flatten(),
                "domain": r.try_get::<String>("", "domain").unwrap_or_default(),
                "display_name": r.try_get::<Option<String>>("", "display_name").ok().flatten(),
                "avatar_url": r.try_get::<Option<String>>("", "avatar_url").ok().flatten(),
                "status": r.try_get::<String>("", "status").unwrap_or_default(),
            })
        })
        .collect();

    Ok(json!({"items": list, "total": list.len()}))
}

/// 查询联邦时间线
pub(crate) async fn get_federation_timeline(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Result<serde_json::Value, String> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let base_url = federation::types::get_base_url().await;
    let base = base_url.trim_end_matches('/');
    let local_domain = federation::types::extract_domain(&base_url).unwrap_or_default();
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                r#"SELECT t.activity_id, t.activity_type, t.object_type,
                      t.content_preview, t.content_json, t.is_read, t.received_at,
                      ra.actor_url AS remote_actor_url,
                      ra.username AS remote_username,
                      ra.domain AS remote_domain,
                      ra.display_name AS remote_display_name,
                      ra.avatar_url AS remote_avatar_url,
                      author.username AS author_username,
                      author.display_name AS author_display_name,
                      peer.username AS peer_username,
                      peer.display_name AS peer_display_name,
                      CASE
                          WHEN ra.id IS NOT NULL THEN
                              CASE
                                  WHEN peer.username IS NOT NULL
                                       AND {peer_has_avatar}
                                  THEN $2 || '/users/' || peer.username || '/avatar'
                                  ELSE NULL
                              END
                          ELSE
                              CASE
                                  WHEN author.username IS NOT NULL
                                       AND {author_has_avatar}
                                  THEN $2 || '/users/' || author.username || '/avatar'
                                  ELSE NULL
                              END
                      END AS local_avatar_proxy
               FROM federation_timeline t
               LEFT JOIN federation_remote_actors ra ON ra.id = t.remote_actor_id
               -- Self-authored rows only — do NOT join viewer profile onto remote posts.
               LEFT JOIN users author ON ra.id IS NULL AND author.id = t.user_id
               -- Same-instance remote_actor stubs → local user profile enrichment.
               LEFT JOIN users peer ON ra.id IS NOT NULL
                   AND ra.username IS NOT NULL
                   AND peer.username = ra.username
                   AND (
                       ra.actor_url LIKE ($2 || '/users/%')
                       OR ra.domain = $3
                   )
               WHERE t.user_id = $1
                 AND (t.activity_type IS NULL OR t.activity_type <> 'Like')
               ORDER BY t.received_at DESC
               LIMIT 50"#,
                peer_has_avatar = crate::services::avatar::avatar_presence_expr("peer"),
                author_has_avatar = crate::services::avatar::avatar_presence_expr("author")
            ),
            [user_id.into(), base.into(), local_domain.clone().into()],
        ))
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            "Database error".to_string()
        })?;

    let mut items: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let received_at = r
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "received_at")
                .ok()
                .map(|t| t.to_rfc3339());
            let remote_actor_url = r
                .try_get::<Option<String>>("", "remote_actor_url")
                .ok()
                .flatten()
                .filter(|s| !s.is_empty());
            let content_json = r
                .try_get::<Option<serde_json::Value>>("", "content_json")
                .ok()
                .flatten();
            let local_avatar_proxy = r
                .try_get::<Option<String>>("", "local_avatar_proxy")
                .ok()
                .flatten()
                .filter(|s| !s.is_empty());
            // Prefer the post author's remote_actor; never the viewer's profile.
            let actor = if let Some(url) = remote_actor_url {
                let remote_display = r
                    .try_get::<Option<String>>("", "remote_display_name")
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty());
                let peer_display = r
                    .try_get::<Option<String>>("", "peer_display_name")
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty());
                let remote_username = r
                    .try_get::<Option<String>>("", "remote_username")
                    .ok()
                    .flatten();
                let peer_username = r
                    .try_get::<Option<String>>("", "peer_username")
                    .ok()
                    .flatten();
                let remote_avatar = r
                    .try_get::<Option<String>>("", "remote_avatar_url")
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty());
                json!({
                    "actor_url": url,
                    "username": remote_username.clone().or(peer_username.clone()),
                    "domain": r.try_get::<Option<String>>("", "remote_domain").ok().flatten(),
                    "display_name": remote_display
                        .or(peer_display)
                        .or(remote_username)
                        .or(peer_username),
                    "avatar_url": remote_avatar.or(local_avatar_proxy),
                })
            } else {
                let local_username = r
                    .try_get::<Option<String>>("", "author_username")
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let local_display = r
                    .try_get::<Option<String>>("", "author_display_name")
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty());
                let actor_url = if local_username.is_empty() {
                    String::new()
                } else {
                    federation::types::actor_url(&base_url, &local_username)
                };
                let domain = federation::types::extract_domain(&base_url);
                json!({
                    "actor_url": actor_url,
                    "username": local_username,
                    "domain": domain,
                    "display_name": local_display,
                    "avatar_url": local_avatar_proxy,
                    "is_local": true,
                })
            };
            let object_id = content_json
                .as_ref()
                .and_then(federation::interactions::extract_object_id);
            json!({
                "activity_id": r.try_get::<String>("", "activity_id").unwrap_or_default(),
                "activity_type": r.try_get::<Option<String>>("", "activity_type").ok().flatten(),
                "object_type": r.try_get::<Option<String>>("", "object_type").ok().flatten(),
                "content_preview": r.try_get::<Option<String>>("", "content_preview").ok().flatten(),
                "content_json": content_json,
                "object_id": object_id,
                "is_read": r.try_get::<bool>("", "is_read").unwrap_or(false),
                // Frontend (Aro) expects created_at / timestamp for timeAgo()
                "created_at": received_at.clone(),
                "received_at": received_at,
                "actor": actor,
            })
        })
        .collect();

    // Enrich like/bookmark/announce/reply counts and me-flags
    let object_ids: Vec<String> = items
        .iter()
        .filter_map(|it| {
            it.get("object_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    if let Ok(stats_map) =
        federation::interactions::interaction_stats_for_objects(db, user_id, &object_ids).await
    {
        for item in &mut items {
            if let Some(oid) = item
                .get("object_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
            {
                if let Some(st) = stats_map.get(&oid) {
                    if let Some(obj) = item.as_object_mut() {
                        obj.insert("liked_by_me".into(), json!(st.liked_by_me));
                        obj.insert("bookmarked_by_me".into(), json!(st.bookmarked_by_me));
                        obj.insert("announced_by_me".into(), json!(st.announced_by_me));
                        obj.insert("like_count".into(), json!(st.like_count));
                        obj.insert("bookmark_count".into(), json!(st.bookmark_count));
                        obj.insert("announce_count".into(), json!(st.announce_count));
                        obj.insert("reply_count".into(), json!(st.reply_count));
                        obj.insert("is_bookmarked".into(), json!(st.bookmarked_by_me));
                    }
                }
            }
        }
    }

    Ok(json!({"items": items, "total": items.len()}))
}
