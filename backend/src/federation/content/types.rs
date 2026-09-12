//! Request/response DTOs for federation content publish and media upload.

use serde::{Deserialize, Serialize};

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
    /// Parent object id for replies (AP `inReplyTo`). Accepts camelCase alias.
    #[serde(default, alias = "inReplyTo")]
    pub in_reply_to: Option<String>,
}

/// Freeform Note 创建请求（POST /api/federation/notes）
#[derive(Debug, Deserialize)]
pub struct CreateNoteRequest {
    pub text: Option<String>,
    pub attachments: Option<Vec<NoteAttachmentInput>>,
    pub visibility: Option<String>,
    /// Parent object id for replies (AP `inReplyTo`). Accepts camelCase alias.
    #[serde(default, alias = "inReplyTo")]
    pub in_reply_to: Option<String>,
}

/// 附件输入（来自 media 上传返回的公开 URL）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NoteAttachmentInput {
    pub url: String,
    /// MIME type，如 image/jpeg / video/mp4
    #[serde(alias = "mediaType")]
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
    /// Best-effort fan-out count (followers + room peers on Public; includes local timeline delivery).
    /// Fan-out never fails the publish; check logs if this is lower than expected.
    #[serde(default)]
    pub delivered_queued: u32,
    /// Whether the Create was written to the author's local timeline.
    #[serde(default)]
    pub author_timeline: bool,
}

/// Media attachment preview for Aro「已发布」cards (from joined Create object).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PublishedAttachment {
    pub url: String,
    /// MIME type (image/jpeg, video/mp4, …). Serialize-only; AP `mediaType` is read in timeline.rs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// AP attachment type (`Image` / `Video`).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub attachment_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Note Image/Video attachments so Aro can render media on 已发布 cards.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<PublishedAttachment>,
    /// Full AP object (Create envelope unwrapped) so quote-reposts can show nested
    /// mfp:quotedObject and open original posts without a second fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_json: Option<serde_json::Value>,
    /// Canonical object id when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
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
