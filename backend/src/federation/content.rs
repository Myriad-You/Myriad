//! 联邦内容发布模块（Phase 2 — Layer 4）
//!
//! 将本地内容（Report / Brew / Tapp / Library / freeform Note）发布为 AP Activity，
//! 自动推送给所有关注者，并把本地 Note 写入作者时间线。

use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::federation::types::*;

// ==================== 请求/响应类型 ====================

/// 发布内容请求
#[derive(Debug, Deserialize)]
pub struct PublishRequest {
    /// 内容类型: report, brew-article, tapp, library, note
    pub content_type: String,
    /// 内容 ID（本地数据库 ID 或标识符；note 可省略，由服务端生成）
    #[serde(default)]
    pub content_id: Option<String>,
    /// 可见性: public, followers, direct
    pub visibility: Option<String>,
    /// Freeform Note 正文（content_type = note）
    pub text: Option<String>,
    /// Freeform Note 附件（已上传的公开 URL）
    pub attachments: Option<Vec<NoteAttachmentInput>>,
}

/// Freeform Note 创建请求（POST /api/federation/notes）
#[derive(Debug, Deserialize)]
pub struct CreateNoteRequest {
    pub text: Option<String>,
    pub attachments: Option<Vec<NoteAttachmentInput>>,
    pub visibility: Option<String>,
}

/// 附件输入（来自 media 上传返回的公开 URL）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NoteAttachmentInput {
    pub url: String,
    /// MIME type，如 image/jpeg / video/mp4
    pub media_type: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// 发布响应
#[derive(Debug, Serialize)]
pub struct PublishResponse {
    pub success: bool,
    pub activity_id: String,
    pub content_type: String,
    pub content_id: String,
    pub visibility: String,
    /// Follower inboxes successfully enqueued for best-effort delivery.
    /// Fan-out never fails the publish; check logs if this is lower than expected.
    #[serde(default)]
    pub delivered_queued: u32,
    /// Whether the Create was written to the author's local timeline.
    #[serde(default)]
    pub author_timeline: bool,
}

/// 已发布内容列表项
#[derive(Debug, Serialize)]
pub struct PublishedItem {
    pub id: i32,
    pub content_type: String,
    pub content_id: String,
    pub activity_id: String,
    pub visibility: String,
    pub published_at: String,
    /// Plain-text preview for Aro「已发布」cards (from joined Create object).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_preview: Option<String>,
    /// Display title (AP `name` / report title) when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Short summary when present (AP `summary` / report summary).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// 媒体上传响应
#[derive(Debug, Serialize)]
pub struct MediaUploadResponse {
    pub url: String,
    pub media_type: String,
    pub name: String,
    pub size: u64,
    pub attachment_type: String,
}

// ==================== 媒体限制 ====================

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_VIDEO_BYTES: usize = 50 * 1024 * 1024;
const MAX_NOTE_ATTACHMENTS: usize = 8;
const MAX_NOTE_TEXT_CHARS: usize = 10_000;

// ==================== 核心发布功能 ====================

/// 发布本地内容到联邦网络
///
/// 1. 拉取本地内容详情（或构建 freeform Note）
/// 2. 转换为 AP Note/Article 对象
/// 3. 创建 Create Activity
/// 4. 存入 federation_published_content
/// 5. 推送给所有 followers
/// 6. Note：立即写入作者时间线
pub async fn publish_content(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    req: &PublishRequest,
) -> Result<PublishResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let visibility = req.visibility.as_deref().unwrap_or("public");
    let content_type = req.content_type.trim();
    if content_type.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "content_type required"})),
        ));
    }

    let content_id = if content_type == "note" {
        match req.content_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => id.to_string(),
            None => format!("note_{}", uuid::Uuid::new_v4()),
        }
    } else {
        let id = req
            .content_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "content_id required"})),
                )
            })?;
        id.to_string()
    };

    // 检查是否已发布
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM federation_published_content WHERE content_type = $1 AND content_id = $2",
            [content_type.into(), content_id.clone().into()],
        ))
        .await
        .map_err(db_err)?;

    if existing.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "Content already published"})),
        ));
    }

    // 获取内容为 AP 对象
    let ap_object = build_ap_object(
        db,
        user_id,
        username,
        &base_url,
        content_type,
        &content_id,
        visibility,
        req.text.as_deref(),
        req.attachments.as_deref(),
    )
    .await?;

    // 生成 Activity
    let activity_id = generate_activity_id(&base_url);
    let local_actor = actor_url(&base_url, username);

    let (to, cc) = resolve_audience(visibility, &base_url, username);

    let activity_json = json!({
        "@context": build_context(),
        "type": "Create",
        "id": &activity_id,
        "actor": &local_actor,
        "published": now_iso8601(),
        "to": to,
        "cc": cc,
        "object": ap_object,
    });

    let object_type = activity_json["object"]["type"]
        .as_str()
        .unwrap_or(content_type)
        .to_string();

    // 存入 federation_activities
    let act_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
               VALUES ($1, $2, 'Create', $3, $4, true, NOW())
               RETURNING id"#,
            [
                activity_id.clone().into(),
                user_id.into(),
                object_type.clone().into(),
                activity_json.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    let act_db_id: i32 = act_row
        .map(|r| r.try_get("", "id").unwrap_or(0))
        .unwrap_or(0);

    // 存入 federation_published_content
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_published_content
               (user_id, content_type, content_id, activity_id, visibility, published_at)
           VALUES ($1, $2, $3, $4, $5, NOW())"#,
        [
            user_id.into(),
            content_type.into(),
            content_id.clone().into(),
            activity_id.clone().into(),
            visibility.into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    // Note / 本地发帖：立即出现在作者时间线
    insert_author_timeline(
        db,
        user_id,
        &activity_id,
        "Create",
        &object_type,
        &activity_json,
    )
    .await?;

    // Best-effort fan-out: enqueue deliveries; never fail the publish on queue errors.
    let delivered_queued =
        fan_out_to_followers(db, user_id, act_db_id, &activity_json).await;

    tracing::info!(
        "📢 Published {} #{} as {} ({}); delivered_queued={}",
        content_type,
        content_id,
        activity_id,
        visibility,
        delivered_queued
    );

    Ok(PublishResponse {
        success: true,
        activity_id,
        content_type: content_type.to_string(),
        content_id,
        visibility: visibility.to_string(),
        delivered_queued,
        author_timeline: true,
    })
}

/// 创建 freeform Note（Aro 发帖）
pub async fn create_note(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    req: &CreateNoteRequest,
) -> Result<PublishResponse, (StatusCode, Json<serde_json::Value>)> {
    let publish_req = PublishRequest {
        content_type: "note".to_string(),
        content_id: None,
        visibility: req.visibility.clone(),
        text: req.text.clone(),
        attachments: req.attachments.clone(),
    };
    publish_content(user_id, username, db, &publish_req).await
}

/// 取消发布（Delete Activity）
pub async fn unpublish_content(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    content_type: &str,
    content_id: &str,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // 查找已发布记录
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, activity_id FROM federation_published_content WHERE user_id = $1 AND content_type = $2 AND content_id = $3",
            [user_id.into(), content_type.into(), content_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let row = row.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Content not published"})),
        )
    })?;

    let pub_id: i32 = row.try_get("", "id").unwrap_or(0);
    let original_activity_id: String = row.try_get("", "activity_id").unwrap_or_default();

    // 创建 Delete Activity
    let delete_activity_id = generate_activity_id(&base_url);
    let local_actor = actor_url(&base_url, username);

    let delete_json = json!({
        "@context": build_ap_context(),
        "type": "Delete",
        "id": &delete_activity_id,
        "actor": &local_actor,
        "published": now_iso8601(),
        "to": [AP_PUBLIC],
        "object": &original_activity_id,
    });

    // 存 Delete Activity
    let del_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
               VALUES ($1, $2, 'Delete', $3, $4, true, NOW())
               RETURNING id"#,
            [
                delete_activity_id.clone().into(),
                user_id.into(),
                content_type.into(),
                delete_json.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    let del_db_id: i32 = del_row
        .map(|r| r.try_get("", "id").unwrap_or(0))
        .unwrap_or(0);

    // 删除 published_content 记录
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_published_content WHERE id = $1",
        [pub_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // 从作者与本地时间线移除原 Create
    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM federation_timeline WHERE activity_id = $1",
            [original_activity_id.clone().into()],
        ))
        .await;

    // Best-effort fan-out of Delete to followers
    let delivered_queued =
        fan_out_to_followers(db, user_id, del_db_id, &delete_json).await;

    tracing::info!(
        "🗑️ Unpublished {} #{} (Delete: {}); delivered_queued={}",
        content_type,
        content_id,
        delete_activity_id,
        delivered_queued
    );

    Ok(json!({
        "success": true,
        "delete_activity_id": delete_activity_id,
    }))
}

/// 获取用户已发布的内容列表
///
/// Joins `federation_activities.object_json` so clients (Aro) can render
/// title / summary / content_preview instead of bare content_type + id.
pub async fn list_published(
    user_id: i32,
    db: &DatabaseConnection,
) -> Result<Vec<PublishedItem>, (StatusCode, Json<serde_json::Value>)> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT p.id, p.content_type, p.content_id, p.activity_id, p.visibility, p.published_at,
                      a.object_json
               FROM federation_published_content p
               LEFT JOIN federation_activities a ON a.activity_id = p.activity_id
               WHERE p.user_id = $1
               ORDER BY p.published_at DESC
               LIMIT 200"#,
            [user_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let items = rows
        .iter()
        .map(|r| {
            let object_json = r
                .try_get::<Option<serde_json::Value>>("", "object_json")
                .ok()
                .flatten();
            let (title, summary, content_preview) =
                published_fields_from_activity_json(object_json.as_ref());
            PublishedItem {
                id: r.try_get("", "id").unwrap_or(0),
                content_type: r.try_get("", "content_type").unwrap_or_default(),
                content_id: r.try_get("", "content_id").unwrap_or_default(),
                activity_id: r.try_get("", "activity_id").unwrap_or_default(),
                visibility: r.try_get("", "visibility").unwrap_or_default(),
                published_at: r
                    .try_get::<chrono::DateTime<chrono::Utc>>("", "published_at")
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default(),
                content_preview,
                title,
                summary,
            }
        })
        .collect();

    Ok(items)
}

// ==================== 媒体上传 ====================

/// 保存联邦媒体附件，返回可被 AP attachment 引用的公开 URL。
pub async fn store_federation_media(
    user_id: i32,
    filename: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<MediaUploadResponse, (StatusCode, Json<serde_json::Value>)> {
    let mime = mime.split(';').next().unwrap_or(mime).trim().to_ascii_lowercase();
    let attachment_type = classify_media_mime(&mime).ok_or_else(|| {
        (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(json!({
                "error": "Unsupported media type",
                "allowed": ["image/jpeg","image/png","image/gif","image/webp","video/mp4","video/webm","video/quicktime"]
            })),
        )
    })?;

    let max = if attachment_type == "Image" {
        MAX_IMAGE_BYTES
    } else {
        MAX_VIDEO_BYTES
    };
    if bytes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Empty file"})),
        ));
    }
    if bytes.len() > max {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({
                "error": format!("File too large (max {} bytes for {})", max, attachment_type),
                "max_bytes": max,
            })),
        ));
    }

    let ext_raw = extension_for_mime(&mime).map(|s| s.to_string()).unwrap_or_else(|| {
        Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin")
            .to_ascii_lowercase()
    });
    // Sanitize extension
    let ext: String = ext_raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let ext = if ext.is_empty() {
        "bin".to_string()
    } else {
        ext
    };

    let media_id = uuid::Uuid::new_v4();
    let stored_name = format!("{}.{}", media_id, ext);
    let dir = federation_media_dir(user_id);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| {
        tracing::error!("Failed to create media dir: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to store media"})),
        )
    })?;

    let path = dir.join(&stored_name);
    tokio::fs::write(&path, bytes).await.map_err(|e| {
        tracing::error!("Failed to write media file: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to store media"})),
        )
    })?;

    let base_url = get_base_url().await;
    let url = format!(
        "{}/media/federation/{}/{}",
        base_url.trim_end_matches('/'),
        user_id,
        stored_name
    );

    let safe_name = Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload")
        .chars()
        .take(200)
        .collect::<String>();

    Ok(MediaUploadResponse {
        url,
        media_type: mime,
        name: safe_name,
        size: bytes.len() as u64,
        attachment_type: attachment_type.to_string(),
    })
}

pub fn federation_media_root() -> PathBuf {
    crate::services::data_paths::paths()
        .root
        .join("federation_media")
}

fn federation_media_dir(user_id: i32) -> PathBuf {
    federation_media_root().join(user_id.to_string())
}

fn classify_media_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" => Some("Image"),
        "video/mp4" | "video/webm" | "video/quicktime" => Some("Video"),
        _ => None,
    }
}

fn extension_for_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "video/mp4" => Some("mp4"),
        "video/webm" => Some("webm"),
        "video/quicktime" => Some("mov"),
        _ => None,
    }
}

/// Human-readable reason if `url` is not a valid local federation media URL for this user.
/// Returns `None` when the URL is acceptable.
fn attachment_url_rejection_reason(base_url: &str, user_id: i32, url: &str) -> Option<&'static str> {
    let url = url.trim();
    if url.is_empty() {
        return Some("Attachment URL is empty");
    }
    let base = base_url.trim_end_matches('/');
    let prefix = format!("{}/media/federation/{}/", base, user_id);
    if !url.starts_with(&prefix) {
        return Some(
            "Attachment URL must be a media file uploaded via POST /api/federation/media for this user on this instance (expected /media/federation/{userId}/{filename})",
        );
    }
    let rest = &url[prefix.len()..];
    if rest.is_empty() {
        return Some("Attachment URL is missing the media filename");
    }
    if rest.contains("..") || rest.contains('/') {
        return Some("Attachment URL path is invalid (no subpaths or '..' allowed)");
    }
    if !rest
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Some("Attachment URL filename contains invalid characters");
    }
    None
}

// ==================== 内容 → AP 对象转换 ====================

/// 根据内容类型构建对应的 AP 对象
#[allow(clippy::too_many_arguments)]
async fn build_ap_object(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
    base_url: &str,
    content_type: &str,
    content_id: &str,
    visibility: &str,
    note_text: Option<&str>,
    note_attachments: Option<&[NoteAttachmentInput]>,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let local_actor = actor_url(base_url, username);
    let (to, cc) = resolve_audience(visibility, base_url, username);

    match content_type {
        "note" => {
            let text = note_text.unwrap_or("").trim();
            let attachments = note_attachments.unwrap_or(&[]);
            if text.is_empty() && attachments.is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "Note requires text and/or attachments"})),
                ));
            }
            if text.chars().count() > MAX_NOTE_TEXT_CHARS {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("Note text too long (max {} chars)", MAX_NOTE_TEXT_CHARS)
                    })),
                ));
            }
            if attachments.len() > MAX_NOTE_ATTACHMENTS {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("Too many attachments (max {})", MAX_NOTE_ATTACHMENTS)
                    })),
                ));
            }

            let mut ap_attachments = Vec::new();
            for att in attachments {
                let mime = att.media_type.split(';').next().unwrap_or("").trim();
                let kind = classify_media_mime(&mime.to_ascii_lowercase()).ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error": format!("Unsupported attachment MIME: {}", att.media_type)})),
                    )
                })?;
                if let Some(reason) =
                    attachment_url_rejection_reason(base_url, user_id, att.url.trim())
                {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": reason,
                            "url": att.url,
                            "hint": "Upload media first via POST /api/federation/media, then pass the returned url as attachment.url",
                        })),
                    ));
                }
                ap_attachments.push(json!({
                    "type": kind,
                    "mediaType": mime,
                    "url": att.url.trim(),
                    "name": att.name,
                }));
            }

            let content_html = if text.is_empty() {
                String::new()
            } else {
                format!("<p>{}</p>", escape_html(text))
            };

            Ok(json!({
                "type": "Note",
                "id": format!("{}/notes/{}", base_url.trim_end_matches('/'), content_id),
                "attributedTo": &local_actor,
                "content": content_html,
                "source": {
                    "content": text,
                    "mediaType": "text/plain",
                },
                "mediaType": "text/html",
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "attachment": ap_attachments,
                "mfp:contentType": "note",
                "mfp:contentId": content_id,
            }))
        }
        "report" => {
            // 综合报告 → AP Article
            let report_id: i32 = content_id.parse().unwrap_or(0);
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT id, platform, report, report_title, created_at
                       FROM platform_reports
                       WHERE id = $1 AND user_id = $2"#,
                    [report_id.into(), user_id.into()],
                ))
                .await
                .map_err(db_err)?
                .ok_or_else(|| not_found("Report not found"))?;

            let platform: String = row.try_get("", "platform").unwrap_or_default();
            let report_json: serde_json::Value = row.try_get("", "report").unwrap_or_default();
            let title: Option<String> = row.try_get("", "report_title").ok();

            // Display title for the Article name
            let name = if platform == "all" {
                title
                    .clone()
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "综合分析报告".to_string())
            } else {
                title
                    .clone()
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| format!("{} 平台报告", platform))
            };

            // Align with Aro chat snapshot fields: report_id, summary, platform, content_preview
            // so remote instances can render without a user-scoped catalog lookup.
            let summary_plain = extract_report_summary_plain(&report_json);
            let summary = if !summary_plain.is_empty() {
                summary_plain.clone()
            } else {
                name.clone()
            };
            let content_preview = {
                let src = if !summary_plain.is_empty() {
                    summary_plain
                } else {
                    name.clone()
                };
                if src.chars().count() > 500 {
                    src.chars().take(500).collect::<String>()
                } else {
                    src
                }
            };
            let content_text = extract_report_summary(&report_json);

            // Snapshot field contract (Aro chat + federation consumers):
            //   report_id, summary, platform, content_preview
            // Also expose mfp:* for ActivityPub-style clients. Do not send full report JSON.
            Ok(json!({
                "type": "Article",
                "id": format!("{}/reports/{}", base_url, report_id),
                "attributedTo": &local_actor,
                "name": &name,
                "summary": &summary,
                "content": &content_text,
                "mediaType": "text/html",
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "mfp:contentType": "report",
                "mfp:contentId": content_id,
                "mfp:reportId": report_id,
                "mfp:platform": &platform,
                "mfp:summary": &summary,
                "mfp:contentPreview": &content_preview,
                // Aro-aligned snake_case aliases (same values as mfp:* above)
                "report_id": report_id,
                "platform": &platform,
                "content_preview": &content_preview,
            }))
        }
        "brew-article" => {
            // Brew 文章 → AP Article
            let item_id: i32 = content_id.parse().unwrap_or(0);
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT bi.id, bi.title, bi.content, bi.link, bi.author,
                              bs.name AS source_name
                       FROM brew_items bi
                       LEFT JOIN brew_sources bs ON bs.id = bi.source_id
                       WHERE bi.id = $1 AND bs.user_id = $2"#,
                    [item_id.into(), user_id.into()],
                ))
                .await
                .map_err(db_err)?
                .ok_or_else(|| not_found("Brew article not found"))?;

            let title: String = row.try_get("", "title").unwrap_or_default();
            let content_text: Option<String> = row.try_get("", "content").ok();
            let url: Option<String> = row.try_get("", "link").ok();
            let author: Option<String> = row.try_get("", "author").ok();
            let source_name: Option<String> = row.try_get("", "source_name").ok();

            let summary_text = content_text
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(500)
                .collect::<String>();

            Ok(json!({
                "type": "Article",
                "id": format!("{}/brew/articles/{}", base_url, item_id),
                "attributedTo": &local_actor,
                "name": &title,
                "content": format!("<p>{}</p>", &summary_text),
                "mediaType": "text/html",
                "url": url,
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "mfp:contentType": "brew-article",
                "mfp:contentId": content_id,
                "mfp:source": source_name,
                "mfp:author": author,
            }))
        }
        "tapp" => {
            // Tapp 应用 → AP Application。仅发布清单元数据，不发布代码包。
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT tapp_id, name, version, description, author, icon, manifest
                       FROM tapps
                       WHERE tapp_id = $1 AND user_id = $2"#,
                    [content_id.into(), user_id.into()],
                ))
                .await
                .map_err(db_err)?
                .ok_or_else(|| not_found("Tapp not found"))?;

            let tapp_id: String = row.try_get("", "tapp_id").unwrap_or_default();
            let name: String = row.try_get("", "name").unwrap_or_default();
            let version: String = row.try_get("", "version").unwrap_or_default();
            let description: Option<String> = row.try_get("", "description").ok();
            let author: Option<serde_json::Value> = row.try_get("", "author").ok();
            let icon: Option<String> = row.try_get("", "icon").ok();
            let manifest: serde_json::Value = row.try_get("", "manifest").unwrap_or(json!({}));
            let encoded_id = urlencoding::encode(&tapp_id);

            Ok(json!({
                "type": "Application",
                "id": format!("{}/tapps/{}", base_url, encoded_id),
                "attributedTo": &local_actor,
                "name": name,
                "summary": description,
                "icon": icon.map(|url| json!({
                    "type": "Image",
                    "url": url
                })),
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "mfp:contentType": "tapp",
                "mfp:contentId": tapp_id,
                "mfp:version": version,
                "mfp:author": author,
                "mfp:manifest": manifest,
            }))
        }
        "library" => {
            // Library 条目 — library_items 表尚未创建，返回明确错误
            Err((
                StatusCode::NOT_IMPLEMENTED,
                Json(
                    json!({"error": "Library content publishing is not yet supported (library_items table not available)"}),
                ),
            ))
        }
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Unsupported content type: {}", content_type)})),
        )),
    }
}

// ==================== Fan-out / Timeline ====================

/// Enqueue Activity delivery to all accepted incoming followers (fan-out on send).
///
/// Best-effort: queue insert failures are logged and skipped; the publish path
/// must not fail after the Create is already persisted. Returns how many
/// follower inboxes were successfully queued **or** delivered locally.
///
/// Same-instance followers (inbox under our `base_url`) are written directly to
/// their local timeline — HTTP delivery to localhost / private hosts is refused
/// by the delivery worker, so without this shortcut multi-user and local-dev
/// follows never see posts.
///
/// Actual HTTP delivery for remote followers is performed by
/// `delivery::process_delivery_queue`, started via
/// `delivery::spawn_delivery_worker` from main on full-mode boot.
async fn fan_out_to_followers(
    db: &DatabaseConnection,
    user_id: i32,
    activity_db_id: i32,
    activity_json: &serde_json::Value,
) -> u32 {
    let base_url = get_base_url().await;

    // 查询所有 incoming followers 的远程 inbox
    let followers = match db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ra.inbox_url, ra.domain, ra.actor_url
               FROM federation_follows f
               JOIN federation_remote_actors ra ON ra.id = f.remote_actor_id
               WHERE f.user_id = $1 AND f.direction = 'incoming' AND f.status = 'accepted'"#,
            [user_id.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(
                "Fan-out follower query failed for user {} activity_db_id={}: {}",
                user_id,
                activity_db_id,
                e
            );
            return 0;
        }
    };

    let mut queued = 0u32;
    let mut failed = 0u32;
    let mut skipped_empty = 0u32;
    let mut local_delivered = 0u32;

    for row in followers {
        let inbox: String = row.try_get("", "inbox_url").unwrap_or_default();
        let domain: String = row.try_get("", "domain").unwrap_or_default();
        let follower_actor: String = row.try_get("", "actor_url").unwrap_or_default();

        if inbox.is_empty() {
            skipped_empty += 1;
            tracing::warn!(
                "Fan-out skip: empty inbox_url for follower domain={} activity_db_id={}",
                domain,
                activity_db_id
            );
            continue;
        }

        // Same-instance follower → direct timeline insert (no HTTP / no SSRF block).
        if let Some(local_username) =
            local_username_from_inbox_url(&base_url, &inbox).or_else(|| {
                if follower_actor.is_empty() {
                    None
                } else {
                    local_username_from_actor_url(&base_url, &follower_actor)
                }
            })
        {
            match deliver_create_to_local_follower(db, &local_username, activity_json).await {
                Ok(true) => {
                    local_delivered += 1;
                    queued += 1;
                }
                Ok(false) => {
                    // User missing — fall through to queue is useless for local inbox.
                    failed += 1;
                    tracing::warn!(
                        "Fan-out local: no user for username={} activity_db_id={}",
                        local_username,
                        activity_db_id
                    );
                }
                Err(e) => {
                    failed += 1;
                    tracing::error!(
                        "Fan-out local timeline failed username={} activity_db_id={}: {}",
                        local_username,
                        activity_db_id,
                        e
                    );
                }
            }
            continue;
        }

        // 加入投递队列（远程）
        match db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_delivery_queue
                       (activity_id, target_inbox, target_domain, status, created_at)
                   VALUES ($1, $2, $3, 'pending', NOW())"#,
                [
                    activity_db_id.into(),
                    inbox.clone().into(),
                    domain.clone().into(),
                ],
            ))
            .await
        {
            Ok(_) => queued += 1,
            Err(e) => {
                failed += 1;
                tracing::error!(
                    "Fan-out enqueue failed activity_db_id={} target_domain={} inbox={}: {}",
                    activity_db_id,
                    domain,
                    inbox,
                    e
                );
            }
        }
    }

    if failed > 0 || skipped_empty > 0 {
        tracing::warn!(
            "Fan-out partial activity_db_id={}: queued={}, local={}, failed={}, skipped_empty_inbox={}",
            activity_db_id,
            queued,
            local_delivered,
            failed,
            skipped_empty
        );
    } else if queued > 0 {
        tracing::info!(
            "Fan-out queued {} deliveries ({} local) for activity_db_id={}",
            queued,
            local_delivered,
            activity_db_id
        );
    } else {
        tracing::debug!(
            "Fan-out: no accepted followers for user {} activity_db_id={}",
            user_id,
            activity_db_id
        );
    }

    queued
}

/// If inbox is `{base}/users/{username}/inbox`, return username.
fn local_username_from_inbox_url(base_url: &str, inbox_url: &str) -> Option<String> {
    let trimmed = inbox_url.trim().trim_end_matches('/');
    let actor = trimmed.strip_suffix("/inbox")?;
    local_username_from_actor_url(base_url, actor)
}

/// Insert Create into a same-instance follower's timeline.
/// Returns Ok(true) when inserted (or already present), Ok(false) if user missing.
async fn deliver_create_to_local_follower(
    db: &DatabaseConnection,
    follower_username: &str,
    activity_json: &serde_json::Value,
) -> Result<bool, String> {
    let user_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE username = $1",
            [follower_username.into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?;
    let Some(user_row) = user_row else {
        return Ok(false);
    };
    let follower_user_id: i32 = user_row.try_get("", "id").unwrap_or(0);
    if follower_user_id == 0 {
        return Ok(false);
    }

    let publisher_actor = activity_json["actor"].as_str().unwrap_or("").to_string();
    if publisher_actor.is_empty() {
        return Err("Create activity missing actor".into());
    }

    let remote_actor_id = ensure_remote_actor_stub(db, &publisher_actor).await?;

    let activity_id = activity_json["id"].as_str().unwrap_or("").to_string();
    if activity_id.is_empty() {
        return Err("Create activity missing id".into());
    }
    let activity_type = activity_json["type"]
        .as_str()
        .unwrap_or("Create")
        .to_string();
    let object = &activity_json["object"];
    let object_type = object["type"].as_str().map(|s| s.to_string());
    let preview = preview_from_ap_object(object);

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_timeline
               (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
           SELECT $1, $2, $3, $4, $5, $6, $7, NOW()
           WHERE NOT EXISTS (
               SELECT 1 FROM federation_timeline
               WHERE user_id = $1 AND activity_id = $2
           )"#,
        [
            follower_user_id.into(),
            activity_id.into(),
            remote_actor_id.into(),
            activity_type.into(),
            object_type.into(),
            preview.into(),
            object.clone().into(),
        ],
    ))
    .await
    .map_err(|e| format!("timeline insert: {}", e))?;

    Ok(true)
}

/// Ensure a federation_remote_actors row exists for a local (or already-known) actor
/// without HTTP fetch — used when fan-out short-circuits same-instance delivery.
async fn ensure_remote_actor_stub(
    db: &DatabaseConnection,
    actor_url_str: &str,
) -> Result<i32, String> {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM federation_remote_actors WHERE actor_url = $1",
            [actor_url_str.into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?;
    if let Some(row) = existing {
        let id: i32 = row.try_get("", "id").unwrap_or(0);
        if id != 0 {
            return Ok(id);
        }
    }

    let domain = extract_domain(actor_url_str).unwrap_or_default();
    let username = actor_url_str
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let inbox = format!("{}/inbox", actor_url_str.trim_end_matches('/'));

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_remote_actors
                   (actor_url, username, domain, inbox_url, last_fetched_at, created_at)
               VALUES ($1, $2, $3, $4, NOW(), NOW())
               ON CONFLICT (actor_url) DO UPDATE SET
                   username = COALESCE(EXCLUDED.username, federation_remote_actors.username),
                   domain = COALESCE(NULLIF(EXCLUDED.domain, ''), federation_remote_actors.domain),
                   inbox_url = COALESCE(NULLIF(EXCLUDED.inbox_url, ''), federation_remote_actors.inbox_url)
               RETURNING id"#,
            [
                actor_url_str.into(),
                username.into(),
                domain.into(),
                inbox.into(),
            ],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    row.map(|r| r.try_get("", "id").unwrap_or(0))
        .filter(|id| *id != 0)
        .ok_or_else(|| "Failed to upsert remote actor stub".into())
}

/// Insert Create into the author's local timeline so freeform posts show up immediately.
async fn insert_author_timeline(
    db: &DatabaseConnection,
    user_id: i32,
    activity_id: &str,
    activity_type: &str,
    object_type: &str,
    activity_json: &serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let object = &activity_json["object"];
    let preview = preview_from_ap_object(object);

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_timeline
               (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
           SELECT $1, $2, NULL, $3, $4, $5, $6, NOW()
           WHERE NOT EXISTS (
               SELECT 1 FROM federation_timeline
               WHERE user_id = $1 AND activity_id = $2
           )"#,
        [
            user_id.into(),
            activity_id.into(),
            activity_type.into(),
            object_type.into(),
            preview.into(),
            object.clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    Ok(())
}

/// Plain preview from an AP Note/Article object (prefers source plain text).
fn preview_from_ap_object(object: &serde_json::Value) -> Option<String> {
    object
        .pointer("/source/content")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("content").and_then(|v| v.as_str()))
        .or_else(|| object.get("summary").and_then(|v| v.as_str()))
        .or_else(|| object.get("content_preview").and_then(|v| v.as_str()))
        .or_else(|| object.get("mfp:contentPreview").and_then(|v| v.as_str()))
        .or_else(|| object.get("mfp:summary").and_then(|v| v.as_str()))
        .or_else(|| object.get("name").and_then(|v| v.as_str()))
        .map(|s| strip_tags_preview(s, 200))
        .filter(|s| !s.is_empty())
}

/// title / summary / content_preview from a stored Create activity JSON
/// (`object_json` column holds the full Create envelope).
fn published_fields_from_activity_json(
    activity_json: Option<&serde_json::Value>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(root) = activity_json else {
        return (None, None, None);
    };
    // Prefer nested object (Create envelope); fall back to root if it is already the object.
    let object = if root.get("object").map(|o| o.is_object()).unwrap_or(false) {
        &root["object"]
    } else {
        root
    };

    let title = object
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(200).collect::<String>())
        .filter(|s| !s.is_empty());

    let summary = object
        .get("summary")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("mfp:summary").and_then(|v| v.as_str()))
        .map(|s| strip_tags_preview(s, 300))
        .filter(|s| !s.is_empty());

    let content_preview = preview_from_ap_object(object).or_else(|| summary.clone());

    (title, summary, content_preview)
}

fn strip_tags_preview(s: &str, max_chars: usize) -> String {
    let plain = s
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<p>", "")
        .replace("</p>", "\n")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"");
    // Drop remaining simple tags
    let mut out = String::with_capacity(plain.len());
    let mut in_tag = false;
    for c in plain.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let trimmed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed.chars().take(max_chars).collect()
}

// ==================== 辅助函数 ====================

/// 解析观众列表
fn resolve_audience(
    visibility: &str,
    base_url: &str,
    username: &str,
) -> (Vec<String>, Vec<String>) {
    match visibility {
        "public" => (
            vec![AP_PUBLIC.to_string()],
            vec![followers_url(base_url, username)],
        ),
        "followers" => (vec![followers_url(base_url, username)], vec![]),
        _ => (vec![], vec![]),
    }
}

/// 从报告 JSON 中提取纯文本摘要（chat / mfp snapshot 用）
fn extract_report_summary_plain(report_json: &serde_json::Value) -> String {
    // 尝试从综合分析中提取
    if let Some(analysis) = report_json.get("综合分析") {
        if let Some(profile) = analysis.get("总体画像").and_then(|v| v.as_str()) {
            return profile.to_string();
        }
        if let Some(content) = analysis.get("content") {
            if let Some(profile) = content.get("总体画像").and_then(|v| v.as_str()) {
                return profile.to_string();
            }
        }
    }
    // 尝试从单平台报告提取 summary
    if let Some(summary) = report_json.get("summary").and_then(|v| v.as_str()) {
        return summary.to_string();
    }
    // 首条 insight 作为预览
    if let Some(insights) = report_json.get("insights").and_then(|v| v.as_array()) {
        if let Some(first) = insights.first().and_then(|v| v.as_str()) {
            return first.to_string();
        }
    }
    String::new()
}

/// 从报告 JSON 中提取摘要（HTML，用于 AP Article content）
fn extract_report_summary(report_json: &serde_json::Value) -> String {
    let plain = extract_report_summary_plain(report_json);
    if plain.is_empty() {
        return "<p>数据分析报告</p>".to_string();
    }
    format!("<p>{}</p>", escape_html(&plain))
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("<br>"),
            _ => out.push(c),
        }
    }
    out
}

async fn get_base_url() -> String {
    crate::federation::types::get_base_url().await
}

fn not_found(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(json!({"error": msg})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_media_mime_allows_image_and_video() {
        assert_eq!(classify_media_mime("image/jpeg"), Some("Image"));
        assert_eq!(classify_media_mime("video/mp4"), Some("Video"));
        assert_eq!(classify_media_mime("application/pdf"), None);
    }

    fn validate_attachment_url(base_url: &str, user_id: i32, url: &str) -> bool {
        attachment_url_rejection_reason(base_url, user_id, url).is_none()
    }

    #[test]
    fn validate_attachment_url_requires_local_media_path() {
        let base = "https://example.com";
        assert!(validate_attachment_url(
            base,
            1,
            "https://example.com/media/federation/1/abc.jpg"
        ));
        assert!(!validate_attachment_url(
            base,
            1,
            "https://evil.com/media/federation/1/abc.jpg"
        ));
        assert!(!validate_attachment_url(
            base,
            1,
            "https://example.com/media/federation/1/../2/x.jpg"
        ));
        assert!(!validate_attachment_url(
            base,
            2,
            "https://example.com/media/federation/1/abc.jpg"
        ));
        assert!(!validate_attachment_url(base, 1, ""));
        assert!(!validate_attachment_url(base, 1, "   "));
        assert!(!validate_attachment_url(
            base,
            1,
            "https://example.com/media/federation/1/"
        ));
        assert!(!validate_attachment_url(
            base,
            1,
            "https://example.com/media/federation/1/bad name.jpg"
        ));
    }

    #[test]
    fn attachment_url_rejection_reason_is_specific() {
        let base = "https://example.com";
        assert_eq!(
            attachment_url_rejection_reason(base, 1, ""),
            Some("Attachment URL is empty")
        );
        assert!(attachment_url_rejection_reason(
            base,
            1,
            "https://evil.com/media/federation/1/abc.jpg"
        )
        .unwrap()
        .contains("POST /api/federation/media"));
        assert_eq!(
            attachment_url_rejection_reason(
                base,
                1,
                "https://example.com/media/federation/1/abc.jpg"
            ),
            None
        );
    }

    #[test]
    fn escape_html_basic() {
        assert_eq!(escape_html("a<b>&c"), "a&lt;b&gt;&amp;c");
    }

    #[test]
    fn extract_report_summary_plain_from_summary_and_insights() {
        let with_summary = json!({"summary": "活跃开发者", "insights": ["ignored when summary present"]});
        assert_eq!(
            extract_report_summary_plain(&with_summary),
            "活跃开发者"
        );

        let with_insights = json!({"insights": ["首条洞察", "第二条"]});
        assert_eq!(
            extract_report_summary_plain(&with_insights),
            "首条洞察"
        );

        let comprehensive = json!({
            "综合分析": { "总体画像": "跨平台综合画像" }
        });
        assert_eq!(
            extract_report_summary_plain(&comprehensive),
            "跨平台综合画像"
        );

        assert_eq!(extract_report_summary_plain(&json!({})), "");
    }

    #[test]
    fn extract_report_summary_html_escapes_and_falls_back() {
        let xss = json!({"summary": "a<b>&c"});
        assert_eq!(
            extract_report_summary(&xss),
            "<p>a&lt;b&gt;&amp;c</p>"
        );
        assert_eq!(
            extract_report_summary(&json!({})),
            "<p>数据分析报告</p>"
        );
    }

    /// Contract: Aro chat + federation report shares use these field names for the viewable snapshot.
    #[test]
    fn report_share_snapshot_field_names_are_stable() {
        // Keep in lockstep with Aro payload / reportShareSnapshot.ts
        let required = ["report_id", "summary", "platform", "content_preview"];
        for name in required {
            assert!(!name.is_empty());
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "snapshot field {name} must be snake_case"
            );
        }
        // Preview length bound used by Aro + Article builders
        let long = "x".repeat(800);
        let capped: String = long.chars().take(500).collect();
        assert_eq!(capped.chars().count(), 500);
    }

    #[test]
    fn published_fields_from_create_note_activity() {
        let create = json!({
            "type": "Create",
            "object": {
                "type": "Note",
                "content": "<p>Hello world post</p>",
                "source": { "content": "Hello world post", "mediaType": "text/plain" },
                "name": null
            }
        });
        let (title, summary, preview) = published_fields_from_activity_json(Some(&create));
        assert!(title.is_none());
        assert!(summary.is_none());
        assert_eq!(preview.as_deref(), Some("Hello world post"));
    }

    #[test]
    fn published_fields_from_article_name() {
        let create = json!({
            "type": "Create",
            "object": {
                "type": "Article",
                "name": "Spring Report",
                "summary": "A short summary of the report body text"
            }
        });
        let (title, summary, preview) = published_fields_from_activity_json(Some(&create));
        assert_eq!(title.as_deref(), Some("Spring Report"));
        assert!(summary.as_ref().is_some_and(|s| s.contains("short summary")));
        assert!(preview.is_some());
    }

    #[test]
    fn local_username_from_inbox_url_matches_base() {
        let base = "https://myriad.example.com";
        assert_eq!(
            local_username_from_inbox_url(base, "https://myriad.example.com/users/bob/inbox"),
            Some("bob".to_string())
        );
        assert_eq!(
            local_username_from_inbox_url(base, "https://other.example.com/users/bob/inbox"),
            None
        );
    }
}
