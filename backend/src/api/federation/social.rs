use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use myriad_error::AppError;
use serde_json::json;

use crate::error::status_json_to_http;
use crate::extract;
use crate::federation;

/// 查询参数 `limit` / `status`。
/// `limit` 用 `Option<String>`：`limit=abc` / `limit=` 静默回落到默认值，不用
/// `Option<i64>`（serde 会直接 400）。
#[derive(serde::Deserialize)]
pub(crate) struct LimitQuery {
    pub(crate) limit: Option<String>,
    /// Optional status filter: pending | delivering | delivered | failed | dead
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

fn federation_user_error(context: &'static str, error: impl std::fmt::Display) -> String {
    let detail = error.to_string();
    tracing::error!(error = %detail, context, "federation request failed");
    let lower = detail.to_ascii_lowercase();
    if lower.contains("database error")
        || lower.contains("db error")
        || lower.contains("relation ")
        || lower.contains("does not exist")
        || lower.contains("duplicate key")
    {
        return format!("Failed to {context}");
    }
    if detail.starts_with("Failed to ")
        && detail != "Failed to list follows"
        && detail != "Failed to load timeline"
    {
        return detail;
    }
    if detail.len() > 160 || detail.starts_with('{') {
        return format!("Failed to {context}");
    }
    format!("Failed to {context}: {detail}")
}

fn federation_store_response(context: &'static str, error: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AppError::public_json(federation_user_error(context, error))),
    )
        .into_response()
}

/// `?limit=&cancelled_only=` 查询参数。
///
/// `cancelled_only` 认 `1|true|yes|on`，其余一律 false。
/// `Option<bool>` 只认 `true`/`false`，`1`/`yes`/`on` 会 400。
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
/// 全部字段用 `Option<String>`：`limit=abc` 解析失败 → `None`。
/// 下游 `get_messages` / `get_room_messages`：`unwrap_or(50).min(200)`；`list_room_files`：`unwrap_or(50).clamp(1, 200)`。
/// `filter` / `q` 是字符串，`list_room_files` 才用得到。
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
/// 直接用 `Json<T>` 会让超限 body 拿到 axum 的纯文本 413，丢掉分块传输指引。
/// HTTP 上限是路由上的 `live_*_body_limit`。413 附带调用方给的 `size_hint`；
/// 其余按原状态码返回解析错误详情。
pub fn json_rejection_response(
    rejection: axum::extract::rejection::JsonRejection,
    size_hint: Option<&str>,
) -> Response {
    let status = rejection.status();
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        let mut body = AppError::public_json("Request body too large or unreadable");
        if let Some(hint) = size_hint {
            body["hint"] = json!(hint);
        }
        return (status, Json(body)).into_response();
    }
    (status, Json(AppError::public_json("Invalid JSON body"))).into_response()
}

// Federation Wrappers

/// POST /api/admin/federation/domain-move
///
/// Emit ActivityPub Move for every local user (domain migration). Admin only.
/// 管理员校验由 `AdminClaims` 承担。
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
/// 用 `Bytes` 而不是 `Json<Value>`：`from_slice(..).unwrap_or(json!({}))`
/// 让空 body / 畸形 JSON 落到「缺少 confirm」这条带操作指引的 400；
/// `Json` 提取器会先被 axum 拒成通用错误，看不到 "{\"confirm\": true}"。
pub(crate) async fn federation_keys_rotate(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    body_bytes: axum::body::Bytes,
) -> Response {
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
        Err(e) => federation_store_response("rotate federation keys", e),
    }
}

/// POST /api/federation/follow — 关注远程用户
/// 路由已挂 auth_middleware；claims / body / db 走提取器。
/// body 上限由路由的 `live_authenticated_body_limit`（默认 `AUTHENTICATED_BODY_LIMIT` 24 MiB）决定。
pub(crate) async fn federation_follow(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::follow::FollowRequest>,
) -> Response {
    match federation::follow::follow_remote(user_id, &claims.username, &db, &payload.target).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// POST /api/federation/unfollow — 取消关注远程用户
/// 路由已挂 auth_middleware；claims / body / db 走提取器。
/// body 上限由路由的 `live_authenticated_body_limit`（默认 `AUTHENTICATED_BODY_LIMIT` 24 MiB）决定。
pub(crate) async fn federation_unfollow(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::follow::FollowRequest>,
) -> Response {
    match federation::follow::unfollow_remote(user_id, &claims.username, &db, &payload.target).await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// GET /api/federation/following — 获取我关注的远程用户列表
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_following_list(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
) -> Response {
    match get_follow_list(&db, user_id, "outgoing").await {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => federation_store_response("list following", e),
    }
}

/// GET /api/federation/followers — 获取关注我的远程用户列表
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_followers_list(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
) -> Response {
    match get_follow_list(&db, user_id, "incoming").await {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => federation_store_response("list followers", e),
    }
}

/// GET /api/federation/timeline — 获取联邦时间线
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_timeline(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
) -> Response {
    match get_federation_timeline(&db, user_id).await {
        Ok(timeline) => (StatusCode::OK, Json(timeline)).into_response(),
        Err(e) => federation_store_response("load timeline", e),
    }
}

// Content Publishing Wrappers

/// POST /api/federation/publish — 发布内容到联邦网络
/// 路由已挂 auth_middleware；claims / body / db 走提取器。
/// body 上限由路由的 `live_authenticated_body_limit`（默认 `AUTHENTICATED_BODY_LIMIT` 24 MiB）决定。
pub(crate) async fn federation_publish(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::content::PublishRequest>,
) -> Response {
    match federation::content::publish_content(user_id, &claims.username, &db, &payload).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims / body / db 全部走提取器。
pub(crate) async fn federation_like(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::ObjectIdRequest>,
) -> Response {
    match federation::interactions::like_object(user_id, &claims.username, &db, &payload.object_id)
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims / body / db 全部走提取器。
pub(crate) async fn federation_unlike(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::ObjectIdRequest>,
) -> Response {
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
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::ObjectIdRequest>,
) -> Response {
    match federation::interactions::bookmark_object(user_id, &db, &payload.object_id).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims / body / db 全部走提取器。
pub(crate) async fn federation_unbookmark(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::ObjectIdRequest>,
) -> Response {
    match federation::interactions::unbookmark_object(user_id, &db, &payload.object_id).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_bookmarks_list(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
) -> Response {
    match federation::interactions::list_bookmarks(user_id, &db).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims / body / db 全部走提取器。
pub(crate) async fn federation_announce(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::AnnounceRequest>,
) -> Response {
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
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::interactions::ObjectIdRequest>,
) -> Response {
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
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Query(q): axum::extract::Query<federation::interactions::GetObjectQuery>,
) -> Response {
    match federation::interactions::get_object(user_id, &db, &q.id).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// POST /api/federation/notes — 创建 freeform Note（文本 + 附件）
/// 路由已挂 auth_middleware；body 上限由 `live_authenticated_body_limit`（默认 24 MiB）决定。
pub(crate) async fn federation_create_note(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::content::CreateNoteRequest>,
) -> Response {
    match federation::content::create_note(user_id, &claims.username, &db, &payload).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；`Multipart` 是 axum 自带的提取器，
/// 提取失败（非 multipart/form-data）由它自己返回 400。
pub(crate) async fn federation_media_upload(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    mut multipart: axum::extract::Multipart,
) -> Response {

    let mut file_bytes: Option<axum::body::Bytes> = None;
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
            Ok(b) => file_bytes = Some(b),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(AppError::public_json("Failed to read file field")),
                )
                    .into_response();
            }
        }
        break;
    }

    let Some(bytes) = file_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Missing multipart field 'file'")),
        )
            .into_response();
    };

    match persist_federation_upload(&db, user_id, &filename, &mime, bytes).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

async fn persist_federation_upload(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    filename: &str,
    mime: &str,
    bytes: axum::body::Bytes,
) -> Result<crate::federation::content::MediaUploadResponse, (StatusCode, Json<serde_json::Value>)>
{
    let mime = mime
        .split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase();
    let attachment_type = crate::federation::content::classify_media_mime(&mime)
        .ok_or_else(|| {
            (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(json!({
                    "error": "Unsupported media type",
                    "allowed": ["image/jpeg","image/png","image/gif","image/webp","video/mp4","video/webm","video/quicktime"]
                })),
            )
        })?;
    let max = if attachment_type == "Image" {
        crate::services::memory_profile::note_image_limit()
    } else {
        crate::services::memory_profile::note_video_limit()
    };
    let actor = crate::services::media::MediaActor::user(user_id).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error.to_string(), "code": error.code() })),
        )
    })?;
    let ctx = crate::services::media::MediaContext::user(
        actor,
        crate::services::media::MediaSource::Upload,
    )
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error.to_string(), "code": error.code() })),
        )
    })?;
    let (asset, _) =
        crate::services::media::MediaService::from_data_paths(crate::services::data_paths::paths())
            .persist_ready_bytes(
                db,
                ctx,
                crate::services::media::NewMediaBytes {
                    bytes,
                    claimed_mime: mime.clone(),
                    filename: filename.to_string(),
                    max_bytes: max,
                    derived_from_id: None,
                    exposure: crate::services::media::MediaExposure::Private,
                },
            )
            .await
            .map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": error.to_string(), "code": error.code() })),
                )
            })?;
    let base = crate::federation::types::get_base_url()
        .await
        .trim_end_matches('/')
        .to_string();
    let path = asset.catalog_url();
    Ok(crate::federation::content::MediaUploadResponse {
        url: format!("{base}{path}"),
        media_type: asset.mime,
        name: asset.name,
        size: asset.size as u64,
        attachment_type: attachment_type.to_string(),
    })
}

/// 路由已挂 auth_middleware；body 上限由 `live_authenticated_body_limit`（默认 24 MiB）决定。
pub(crate) async fn federation_unpublish(
    extract::DurableUserId(user_id): extract::DurableUserId,
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
            Json(AppError::public_json(
                "Provide activity_id, or content_type + content_id",
            )),
        )
            .into_response();
    }
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
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
) -> Response {
    match federation::content::list_published(user_id, &db).await {
        Ok(items) => (
            StatusCode::OK,
            Json(json!({"items": items, "total": items.len()})),
        )
            .into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

// Channel Wrapper Functions

/// 路由已挂 auth_middleware；body 上限由 `live_authenticated_body_limit`（默认 24 MiB）决定。
pub(crate) async fn federation_create_channel(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<federation::channel::CreateChannelRequest>,
) -> Response {
    match federation::channel::create_channel(user_id, &claims.username, &db, &payload).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// Channel 列表
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_list_channels(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
) -> Response {
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
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
    match federation::channel::get_channel(user_id, &channel_id, &db).await {
        Ok(detail) => (StatusCode::OK, Json(serde_json::to_value(detail).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 关闭 Channel
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_close_channel(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
    match federation::channel::close_channel(user_id, &claims.username, &channel_id, &db).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 删除已关闭的 Channel（本地硬删除）
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_delete_channel(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
    match federation::channel::delete_channel(user_id, &channel_id, &db).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 接受 Channel
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_accept_channel(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
    match federation::channel::accept_channel(user_id, &claims.username, &channel_id, &db).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 发起 Channel E2E 密钥交换
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_e2e_key_exchange(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
) -> Response {
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

/// 路由已声明 `{channel_id}`；body 上限由 `live_authenticated_body_limit`（默认 24 MiB）决定。
pub(crate) async fn federation_send_message(
    extract::DurableUserId(user_id): extract::DurableUserId,
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
                Some(
                    "Inline payloads max ~4 MiB (MESSAGE_PAYLOAD_LIMIT); larger files use chunked transfer",
                ),
            );
        }
    };

    match federation::channel::send_message(user_id, &claims.username, &channel_id, &db, &parsed)
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明 `{channel_id}`；分页参数走 `Query<ListQuery>`。
pub(crate) async fn federation_get_messages(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> Response {
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

// Room 多方通信 Wrapper

/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
/// 不加这层会退回到 AUTHENTICATED_BODY_LIMIT。
pub(crate) async fn federation_create_room(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    Json(parsed): Json<federation::room::CreateRoomRequest>,
) -> Response {
    match federation::room::create_room(user_id, &claims.username, &db, &parsed).await {
        Ok(detail) => (StatusCode::OK, Json(json!(detail))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_list_rooms(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
) -> Response {
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
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    match federation::room::get_room(user_id, &claims.username, &room_id, &db).await {
        Ok(detail) => (StatusCode::OK, Json(json!(detail))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数并挂了 auth_middleware；
/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
pub(crate) async fn federation_update_room(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    Json(parsed): Json<federation::room::UpdateRoomRequest>,
) -> Response {
    match federation::room::update_room(user_id, &claims.username, &room_id, &db, &parsed).await {
        Ok(detail) => (StatusCode::OK, Json(json!(detail))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；claims / path / db 全部走提取器，
/// 不再手工 strip_prefix 重解析 URI。
pub(crate) async fn federation_delete_room(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    match federation::room::delete_room(user_id, &claims.username, &room_id, &db).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_get_room_members(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
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
/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
pub(crate) async fn federation_invite_room_member(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    Json(parsed): Json<federation::room::InviteMemberRequest>,
) -> Response {
    match federation::room::invite_member(user_id, &claims.username, &room_id, &db, &parsed).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由声明的是 `{room_id}/members/{actor}`。
/// `Path<(String, String)>` 会对每段做百分号解码；actor 是完整 URL，必然带编码。
pub(crate) async fn federation_remove_room_member(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path((room_id, target_actor)): axum::extract::Path<(String, String)>,
) -> Response {
    match federation::room::remove_member(user_id, &claims.username, &room_id, &target_actor, &db)
        .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_leave_room(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    match federation::room::leave_room(user_id, &claims.username, &room_id, &db).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// POST /api/federation/rooms/{room_id}/accept — accept pending room invite
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_accept_room_invite(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    match federation::room::accept_room_invite(user_id, &claims.username, &room_id, &db).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// POST /api/federation/rooms/{room_id}/reject — reject pending room invite
/// 路由已声明该路径参数；claims / path / db 走提取器，不再手工解析 URI。
pub(crate) async fn federation_reject_room_invite(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    match federation::room::reject_room_invite(user_id, &claims.username, &room_id, &db).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明 `{room_id}`。
/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
pub(crate) async fn federation_transfer_room_ownership(
    extract::DurableUserId(user_id): extract::DurableUserId,
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
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Response {
    match federation::room::initiate_e2e_key_exchange(user_id, &claims.username, &room_id, &db)
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// PUT /api/federation/rooms/{room_id}/members/{actor}/role — owner sets admin|member
pub(crate) async fn federation_set_room_member_role(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path((room_id, actor)): axum::extract::Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let role = body
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
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
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    Json(req): Json<federation::room::AddRoomStickerRequest>,
) -> Response {
    match federation::room::add_room_sticker(user_id, &claims.username, &room_id, req, &db).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// DELETE /api/federation/rooms/{room_id}/stickers/{sticker_id}
pub(crate) async fn federation_remove_room_sticker(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path((room_id, sticker_id)): axum::extract::Path<(String, String)>,
) -> Response {
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

/// 路由已声明 `{room_id}`；body 上限由 `live_authenticated_body_limit`（默认 24 MiB）决定。
///
/// 用 `Result<Json<T>, JsonRejection>` 而不是裸 `Json<T>`：超限时保住
/// 413 + 分块传输指引，而不是 axum 的纯文本拒绝。
pub(crate) async fn federation_send_room_message(
    extract::DurableUserId(user_id): extract::DurableUserId,
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
                "[Room] send message body rejected (likely over live_authenticated_body_limit / MESSAGE_PAYLOAD_LIMIT)"
            );
            return json_rejection_response(
                e,
                Some(
                    "Inline payloads max ~4 MiB (MESSAGE_PAYLOAD_LIMIT); larger files use chunked transfer",
                ),
            );
        }
    };

    match federation::room::send_room_message(user_id, &claims.username, &room_id, &db, &parsed)
        .await
    {
        Ok(resp) => (StatusCode::OK, Json(json!(resp))).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 路由已声明 `{room_id}`；分页参数走 `Query<ListQuery>`。
pub(crate) async fn federation_get_room_messages(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> Response {
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
/// `Path<(String, String)>` 对每段做百分号解码。
/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
pub(crate) async fn federation_pin_room_message(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path((room_id, message_id)): axum::extract::Path<(String, String)>,
    Json(parsed): Json<federation::room::PinRoomMessageRequest>,
) -> Response {
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

// Ring 去中心化环网

/// 管理员校验由 `AdminClaims` 承担。这些 ring 端点的路由只有 router 级
/// `auth_middleware`（普通登录）；写进签名后，路由被挪动或重挂中间件也带不走它。
pub(crate) async fn federation_create_ring(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AdminClaims(_): extract::AdminClaims,
    extract::Db(db): extract::Db,
    Json(create_req): Json<federation::ring::CreateRingRequest>,
) -> Response {
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
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(ring_id): axum::extract::Path<String>,
) -> Response {
    match federation::ring::leave_ring(&ring_id, user_id, &claims.username, &db).await {
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
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(ring_id): axum::extract::Path<String>,
    Json(add_req): Json<federation::ring::AddPeerRequest>,
) -> Response {
    match federation::ring::add_peer(&ring_id, user_id, &claims.username, &db, &add_req).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 见 [`federation_create_ring`]：管理员校验由 `AdminClaims` 承担。
///
/// `Path<(String, String)>` 会对每段做百分号解码；peer 是完整 Actor URL，必然带编码。
pub(crate) async fn federation_remove_ring_peer(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path((ring_id, peer_url)): axum::extract::Path<(String, String)>,
) -> Response {
    match federation::ring::remove_peer(&ring_id, &peer_url, user_id, &claims.username, &db).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

/// 见 [`federation_create_ring`]：管理员校验由 `AdminClaims` 承担。
pub(crate) async fn federation_trigger_ring_sync(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(ring_id): axum::extract::Path<String>,
) -> Response {
    match federation::ring::trigger_sync(&ring_id, user_id, &claims.username, &db).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, json)) => status_json_to_http((status, json)).into_response(),
    }
}

// Trust 策略管理

/// GET /api/federation/delivery/stats — user delivery queue counters
/// 路由已挂 auth_middleware；claims 由 AuthedClaims 提取。
pub(crate) async fn federation_delivery_stats(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
) -> Response {
    match federation::delivery::delivery_stats_for_user(&db, user_id).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => federation_store_response("load delivery stats", e),
    }
}

/// POST /api/federation/delivery/{id}/retry — requeue a dead/stuck item
/// 路径参数走 `Path<i32>`。
pub(crate) async fn federation_retry_delivery(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Path(queue_id): axum::extract::Path<i32>,
) -> Response {
    match federation::delivery::retry_delivery_item(&db, user_id, queue_id).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// POST /api/federation/delivery/{id}/cancel — cancel pending/delivering item
/// 路径参数走 `Path<i32>`。
pub(crate) async fn federation_cancel_delivery(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Path(queue_id): axum::extract::Path<i32>,
) -> Response {
    match federation::delivery::cancel_delivery_item(&db, user_id, queue_id).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 查询参数走 `Query<LimitQuery>`。
pub(crate) async fn federation_retry_all_dead_delivery(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Query(q): axum::extract::Query<LimitQuery>,
) -> Response {
    match federation::delivery::retry_all_dead_for_user(&db, user_id, q.or(50)).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 查询参数走 `Query<LimitQuery>`。
pub(crate) async fn federation_cancel_all_pending_delivery(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Query(q): axum::extract::Query<LimitQuery>,
) -> Response {
    match federation::delivery::cancel_all_pending_for_user(&db, user_id, q.or(100)).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// DELETE /api/federation/delivery/{id} — purge a dead queue row (user-owned dismiss)
/// 路径参数走 `Path<i32>`。
pub(crate) async fn federation_dismiss_delivery(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Path(queue_id): axum::extract::Path<i32>,
) -> Response {
    {
        match federation::delivery::dismiss_delivery_item(&db, user_id, queue_id).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
        }
    }
}

/// 路由已挂 auth_middleware；`Query<PurgeDeadQuery>` 认 `1|true|yes|on`。
pub(crate) async fn federation_purge_dead_delivery(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Query(q): axum::extract::Query<PurgeDeadQuery>,
) -> Response {
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
/// 畸形 JSON 返回 400（`optional_json_distinguishes_absent_from_malformed`）。
pub(crate) async fn federation_join_room(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    body: Option<Json<federation::room::JoinRoomRequest>>,
) -> Response {
    let join_req = body.map(|Json(v)| v).unwrap_or_default();
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

/// 查询参数走 `Query<LimitQuery>`。
pub(crate) async fn federation_list_delivery(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::Db(db): extract::Db,
    axum::extract::Query(q): axum::extract::Query<LimitQuery>,
) -> Response {
    match federation::delivery::list_delivery_for_user_filtered(
        &db,
        user_id,
        q.or(30),
        q.status_filter(),
    )
    .await
    {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => federation_store_response("list delivery", e),
    }
}

pub(crate) async fn federation_get_trust_policy(extract::Db(db): extract::Db) -> Response {
    match federation::trust::get_policy(&db).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((status, v)) => status_json_to_http((status, Json(v))).into_response(),
    }
}

/// 见 [`federation_create_ring`]：管理员校验由 `AdminClaims` 承担。
/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
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
    // 优先 nested `rate_limit`；否则读顶层 `rate_max_requests` / `rate_window_seconds` / `rate_trusted_multiplier`。
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

/// 路由已挂 admin_middleware；`AuthedClaims` 无凭证 → 401。
/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
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
                Json(AppError::public_json("domain required")),
            )
                .into_response();
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

/// 路由已挂 admin_middleware；`AuthedClaims` 无凭证 → 401。
/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
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
/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
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

/// 路由已挂 admin_middleware；`AuthedClaims` 无凭证 → 401。
/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
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
                Json(AppError::public_json("domain required")),
            )
                .into_response();
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

// 文件传输

/// 路由已声明该路径参数并挂了 auth_middleware；
/// body 上限由路由的 `live_small_control_body_limit`（`SMALL_CONTROL_BODY_LIMIT` 256 KiB）。
pub(crate) async fn federation_initiate_transfer(
    extract::DurableUserId(user_id): extract::DurableUserId,
    extract::AuthedClaims(claims): extract::AuthedClaims,
    extract::Db(db): extract::Db,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
    Json(transfer_req): Json<federation::file_transfer::InitTransferRequest>,
) -> Response {
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
        .map_err(|error| federation_user_error("list follows", error))?;

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
        .map_err(|error| federation_user_error("load timeline", error))?;

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
                // created_at 与 received_at 同值
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
    // `is_bookmarked` is derived from the authoritative interaction rows; a stats
    // failure fails the timeline rather than returning unknown bookmark state.
    let stats_map =
        federation::interactions::interaction_stats_for_objects(db, user_id, &object_ids)
            .await
            .map_err(|error| federation_user_error("load timeline", error))?;
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

    Ok(json!({"items": items, "total": items.len()}))
}

#[cfg(test)]
mod tests {
    use super::federation_user_error;

    #[test]
    fn federation_user_error_names_step_and_drops_sql() {
        let following = federation_user_error(
            "list following",
            r#"DB error: relation "federation_follows" does not exist"#,
        );
        assert_eq!(following, "Failed to list following");
        assert!(!following.contains("federation_follows"));

        let rotate = federation_user_error("rotate federation keys", "Database error");
        assert_eq!(rotate, "Failed to rotate federation keys");
        assert_ne!(following, rotate);

        let confirm = federation_user_error(
            "rotate federation keys",
            "Key rotation requires {\"confirm\": true}",
        );
        assert!(confirm.contains("confirm"));
        assert_ne!(confirm, rotate);
    }

    #[test]
    fn federation_handlers_take_boundary_subject() {
        let src = include_str!("social.rs");
        let production = src.split("#[cfg(test)]").next().expect("production");
        for handler in [
            "federation_leave_ring",
            "federation_add_ring_peer",
            "federation_remove_ring_peer",
            "federation_trigger_ring_sync",
        ] {
            let body = production
                .split(&format!("pub(crate) async fn {handler}"))
                .nth(1)
                .and_then(|rest| rest.split("pub(crate) async fn").next())
                .unwrap_or("");
            assert!(body.contains("extract::DurableUserId(user_id)"), "{handler}");
        }
        for file in [production, include_str!("rooms_and_router.rs")] {
            let production = file.split("#[cfg(test)]").next().expect("production");
            assert!(!production.contains("claims.sub"));
            assert!(!production.contains("positive_user_id"));
        }
    }

    #[tokio::test]
    async fn durable_user_extractor_rejects_guest_zero_and_missing_subject() {
        use crate::extract::DurableUserId;
        use crate::middleware::auth::{Claims, mint_session_claims};
        use axum::extract::FromRequestParts;
        use axum::http::StatusCode;

        async fn extract(claims: Option<Claims>) -> Result<i32, StatusCode> {
            let (mut parts, ()) = axum::http::Request::new(()).into_parts();
            if let Some(claims) = claims {
                parts.extensions.insert(claims);
            }
            DurableUserId::from_request_parts(&mut parts, &())
                .await
                .map(|DurableUserId(id)| id)
                .map_err(|(status, _)| status)
        }
        let minted = |id| Some(mint_session_claims(id, "u", false, false, 0));

        assert_eq!(extract(minted(7)).await, Ok(7));
        assert_eq!(extract(minted(0)).await, Err(StatusCode::FORBIDDEN));
        assert_eq!(extract(minted(-42)).await, Err(StatusCode::FORBIDDEN));
        // Claims that never passed the auth boundary carry no typed subject,
        // even with a valid-looking `sub`: never re-parsed, fail closed.
        let mut unbound = mint_session_claims(7, "u", false, false, 0);
        unbound.subject = None;
        assert_eq!(extract(Some(unbound)).await, Err(StatusCode::UNAUTHORIZED));
        assert_eq!(extract(None).await, Err(StatusCode::UNAUTHORIZED));
    }
}
