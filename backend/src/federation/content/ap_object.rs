//! ActivityPub object construction, audience addressing, and delivery fan-out.

use axum::{Json, http::StatusCode};
use myriad_error::AppError;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::kind::ContentKind;
use super::media::{attachment_url_rejection_reason, classify_media_mime};
use super::types::NoteAttachmentInput;
use crate::federation::audience::Visibility;
use crate::federation::limits::NOTE_ATTACHMENT_COUNT_LIMIT as MAX_NOTE_ATTACHMENTS;
use crate::federation::limits::NOTE_TEXT_CHAR_LIMIT as MAX_NOTE_TEXT_CHARS;
use crate::federation::types::*;

// 内容 → AP 对象转换

/// 根据内容类型构建对应的 AP 对象
#[allow(clippy::too_many_arguments)]
pub(super) async fn build_ap_object(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
    base_url: &str,
    kind: ContentKind,
    content_id: &str,
    visibility: Visibility,
    note_text: Option<&str>,
    note_attachments: Option<&[NoteAttachmentInput]>,
    in_reply_to: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let local_actor = actor_url(base_url, username);
    let (to, cc) = resolve_audience(visibility, base_url, username);

    match kind {
        ContentKind::Note => {
            let text = note_text.unwrap_or("").trim();
            let attachments = note_attachments.unwrap_or(&[]);
            if text.is_empty() && attachments.is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(AppError::public_json(
                        "Note requires text and/or attachments",
                    )),
                ));
            }
            if text.chars().count() > MAX_NOTE_TEXT_CHARS {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("Note text too long (max {} chars)", MAX_NOTE_TEXT_CHARS),
                        "code": "note_too_long",
                    })),
                ));
            }
            if attachments.len() > MAX_NOTE_ATTACHMENTS {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("Too many attachments (max {})", MAX_NOTE_ATTACHMENTS),
                        "code": "note_too_many_attachments",
                    })),
                ));
            }

            let mut ap_attachments = Vec::new();
            for att in attachments {
                let mime = att.media_type.split(';').next().unwrap_or("").trim();
                let kind = classify_media_mime(&mime.to_ascii_lowercase()).ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(AppError::public_json("Unsupported attachment type")),
                    )
                })?;
                if let Some(reason) = attachment_url_rejection_reason(base_url, att.url.trim()) {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": reason,
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

            let mut note = json!({
                "type": "Note",
                "id": kind.object_url(base_url, content_id),
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
                "mfp:contentType": kind.as_str(),
                "mfp:contentId": content_id,
            });
            if let Some(parent) = in_reply_to.map(str::trim).filter(|s| !s.is_empty()) {
                note["inReplyTo"] = json!(parent);
            }
            Ok(note)
        }
        ContentKind::Report => {
            // 单平台报告 → AP Article
            let report_id: i32 = content_id.parse().unwrap_or(0);
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
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
            let name = title
                .clone()
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| format!("{} report", platform));

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
            // report_id, summary, platform, content_preview
            // Also expose mfp:* for ActivityPub-style clients. Do not send full report JSON.
            Ok(json!({
                "type": "Article",
                "id": kind.object_url(base_url, &report_id.to_string()),
                "attributedTo": &local_actor,
                "name": &name,
                "summary": &summary,
                "content": &content_text,
                "mediaType": "text/html",
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "mfp:contentType": kind.as_str(),
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
        ContentKind::PhantasiArticle => {
            // Phantasi 文章 → AP Article
            let item_id: i32 = content_id.parse().unwrap_or(0);
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT bi.id, bi.title, bi.content, bi.link, bi.author,
                              bs.name AS source_name
                       FROM phantasi_items bi
                       LEFT JOIN phantasi_sources bs ON bs.id = bi.source_id
                       WHERE bi.id = $1 AND bs.user_id = $2"#,
                    [item_id.into(), user_id.into()],
                ))
                .await
                .map_err(db_err)?
                .ok_or_else(|| not_found("Phantasi article not found"))?;

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
                "id": kind.object_url(base_url, &item_id.to_string()),
                "attributedTo": &local_actor,
                "name": &title,
                "content": format!("<p>{}</p>", &summary_text),
                "mediaType": "text/html",
                "url": url,
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "mfp:contentType": kind.as_str(),
                "mfp:contentId": content_id,
                "mfp:source": source_name,
                "mfp:author": author,
            }))
        }
        ContentKind::Tapp => {
            // Tapp 应用 → AP Application。仅发布清单元数据，不发布代码包。
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
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

            Ok(json!({
                "type": "Application",
                "id": kind.object_url(base_url, &tapp_id),
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
                "mfp:contentType": kind.as_str(),
                "mfp:contentId": tapp_id,
                "mfp:version": version,
                "mfp:author": author,
                "mfp:manifest": manifest,
            }))
        }
        ContentKind::Library => {
            // Library 发布：content_id = platform_metadata.id（平台收藏快照）
            // 或 platform 名（取该用户该平台最新一条 metadata）。
            // 无独立 library_items 表；数据来自 platform_metadata.raw_data 摘要。
            let (meta_id, platform_name, raw): (i32, String, serde_json::Value) =
                if let Ok(id) = content_id.parse::<i32>() {
                    let row = db
                        .query_one_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"SELECT id, platform_name, raw_data
                               FROM platform_metadata
                               WHERE id = $1 AND user_id = $2"#,
                            [id.into(), user_id.into()],
                        ))
                        .await
                        .map_err(db_err)?
                        .ok_or_else(|| not_found("Library metadata not found"))?;
                    (
                        row_positive_id(&row, "id")
                            .map_err(|error| db_err(sea_orm::DbErr::Custom(error)))?,
                        row.try_get::<String>("", "platform_name")
                            .unwrap_or_default(),
                        row.try_get::<serde_json::Value>("", "raw_data")
                            .unwrap_or(json!({})),
                    )
                } else {
                    let platform = content_id.trim();
                    if platform.is_empty() {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(AppError::public_json(
                                "library content_id must be platform_metadata id or platform name",
                            )),
                        ));
                    }
                    let row = db
                        .query_one_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"SELECT id, platform_name, raw_data
                               FROM platform_metadata
                               WHERE user_id = $1 AND lower(platform_name) = lower($2)
                               ORDER BY fetched_at DESC NULLS LAST, id DESC
                               LIMIT 1"#,
                            [user_id.into(), platform.into()],
                        ))
                        .await
                        .map_err(db_err)?
                        .ok_or_else(|| {
                            not_found(&format!("No library metadata for platform '{}'", platform))
                        })?;
                    (
                        row_positive_id(&row, "id")
                            .map_err(|error| db_err(sea_orm::DbErr::Custom(error)))?,
                        row.try_get::<String>("", "platform_name")
                            .unwrap_or_else(|_| platform.to_string()),
                        row.try_get::<serde_json::Value>("", "raw_data")
                            .unwrap_or(json!({})),
                    )
                };

            let (item_count, sample_titles) = summarize_library_raw(&raw);
            let name = format!("{} library", platform_name);
            let summary = if item_count > 0 {
                format!(
                    "{} items on {}{}",
                    item_count,
                    platform_name,
                    if sample_titles.is_empty() {
                        String::new()
                    } else {
                        format!(" — e.g. {}", sample_titles.join(", "))
                    }
                )
            } else {
                format!("Library snapshot from {}", platform_name)
            };

            Ok(json!({
                "type": "Collection",
                "id": kind.object_url(base_url, &meta_id.to_string()),
                "attributedTo": &local_actor,
                "name": &name,
                "summary": &summary,
                "totalItems": item_count,
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "mfp:contentType": kind.as_str(),
                "mfp:contentId": content_id,
                "mfp:platform": &platform_name,
                "mfp:metadataId": meta_id,
                "mfp:sampleTitles": sample_titles,
                "platform": &platform_name,
                "item_count": item_count,
            }))
        }
    }
}

// Fan-out / Timeline

/// 不在调用方事务里的扇出：把活动投给全部已接受的粉丝，逐个尽力而为。
///
/// 给「自身写入已经提交、扇出只是通知」的调用方用（密钥轮换后的
/// Update(Person)）。坏数据的粉丝（空 inbox、列解码失败）记日志跳过，
/// 同实例粉丝某一个投递失败也只记日志 —— 一个坏粉丝不再让其余粉丝收不到。
/// 只有读粉丝列表或写投递队列的数据库错误返回 `Err`：已经排上的行保留，
/// `(activity_id, target_inbox)` 唯一约束让重试不重复。
///
/// 需要「写入与扇出同生共死」的调用方（发布 / 撤回、转发 / 取消转发）不用它，而是在
/// 自己的事务里调 [`stage_follower_fan_out`]、提交后调
/// [`deliver_to_local_followers`]。返回排上队的远端行数加成功的本地投递数。
///
/// Same-instance followers (inbox under our `base_url`) get the activity through
/// [`deliver_activity_locally`](crate::federation::inbox::deliver_activity_locally),
/// the same dispatch a remote inbox runs — HTTP delivery to localhost / private
/// hosts is refused by the delivery worker. Remote rows are sent by
/// `delivery::process_delivery_queue_detailed` (`delivery::spawn_delivery_worker`).
pub(crate) async fn fan_out_to_followers(
    db: &DatabaseConnection,
    user_id: i32,
    activity_db_id: i32,
    activity_json: &serde_json::Value,
) -> Result<u32, String> {
    let base_url = get_base_url().await;
    let staged = stage_follower_fan_out(db, &base_url, user_id, activity_db_id)
        .await
        .map_err(|e| {
            format!("Fan-out failed for user {user_id} activity_db_id={activity_db_id}: {e}")
        })?;
    let local_delivered =
        deliver_to_local_followers(db, &staged.local_followers, activity_json).await;
    tracing::info!(
        user_id,
        activity_db_id,
        queued = staged.queued,
        local_delivered,
        local_followers = staged.local_followers.len(),
        skipped = staged.skipped,
        "Fan-out to followers finished"
    );
    Ok(staged.queued + local_delivered)
}

/// 在发布事务内落下的扇出意图：远端投递行已写进 `federation_delivery_queue`，
/// 同实例粉丝留给提交后的进程内投递。
///
/// 投递队列本身就是持久化的扇出意图 —— 与内容、Create/Delete 活动同一个事务
/// 提交，提交成功即由投递 worker 负责送达；提交失败则一行都不留，客户端重试
/// 不会产生重复帖子，也不会有「帖子已落库但没排上投递」的半成品。
#[derive(Debug, Default)]
pub(crate) struct StagedFanOut {
    /// 本事务写入的远端投递行数。
    pub queued: u32,
    /// 需要在提交后进程内投递的同实例粉丝用户名。
    pub local_followers: Vec<String>,
    /// 因数据坏掉而跳过的粉丝数（空 inbox、列解码失败）。
    pub skipped: u32,
}

/// 单个粉丝行的去向。坏数据只影响它自己，不连累其他粉丝。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FollowerRoute {
    Local(String),
    Remote { inbox: String, domain: String },
    Skip(&'static str),
}

pub(crate) fn route_follower(
    base_url: &str,
    inbox: Option<&str>,
    domain: Option<&str>,
    actor: Option<&str>,
) -> FollowerRoute {
    let inbox = inbox.map(str::trim).unwrap_or("");
    let actor = actor.map(str::trim).unwrap_or("");
    let local = local_username_from_inbox_url(base_url, inbox).or_else(|| {
        (!actor.is_empty())
            .then(|| local_username_from_actor_url(base_url, actor))
            .flatten()
    });
    if let Some(username) = local {
        return FollowerRoute::Local(username);
    }
    if inbox.is_empty() {
        return FollowerRoute::Skip("empty inbox_url");
    }
    let Some(domain) = domain else {
        return FollowerRoute::Skip("undecodable domain");
    };
    FollowerRoute::Remote {
        inbox: inbox.to_string(),
        domain: domain.to_string(),
    }
}

/// 在调用方事务内为全部已接受的粉丝排队投递（发布 / 撤回、转发 / 取消转发用；
/// [`fan_out_to_followers`] 在自动提交连接上也走它）。
///
/// 逐个粉丝尽力而为：坏数据的粉丝记日志跳过，其余照常排队。数据库错误原样
/// 返回 —— 事务里任何一条语句失败都会让整个事务作废，此时调用方回滚，
/// 内容和活动也一起不落库。
pub(crate) async fn stage_follower_fan_out(
    txn: &impl ConnectionTrait,
    base_url: &str,
    user_id: i32,
    activity_db_id: i32,
) -> Result<StagedFanOut, sea_orm::DbErr> {
    let followers = txn
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ra.inbox_url, ra.domain, ra.actor_url
               FROM federation_follows f
               JOIN federation_remote_actors ra ON ra.id = f.remote_actor_id
               WHERE f.user_id = $1 AND f.direction = 'incoming' AND f.status = 'accepted'"#,
            [user_id.into()],
        ))
        .await?;

    let mut staged = StagedFanOut::default();
    for row in followers {
        let inbox = row
            .try_get::<Option<String>>("", "inbox_url")
            .ok()
            .flatten();
        let domain = row.try_get::<Option<String>>("", "domain").ok().flatten();
        let actor = row
            .try_get::<Option<String>>("", "actor_url")
            .ok()
            .flatten();
        match route_follower(
            base_url,
            inbox.as_deref(),
            domain.as_deref(),
            actor.as_deref(),
        ) {
            FollowerRoute::Local(username) => staged.local_followers.push(username),
            FollowerRoute::Remote { inbox, domain } => {
                staged.queued += enqueue_delivery(txn, activity_db_id, &inbox, &domain).await?;
            }
            FollowerRoute::Skip(reason) => {
                staged.skipped += 1;
                tracing::warn!(
                    user_id,
                    activity_db_id,
                    follower = actor.as_deref().unwrap_or(""),
                    reason,
                    "Fan-out skipped an unusable follower"
                );
            }
        }
    }
    Ok(staged)
}

/// 在调用方事务内把一条公开活动排队投给群邻实例（见 `federation::room_peers`）。
///
/// 与粉丝扇出并行、不互斥：同一个实例既是粉丝又是群邻时，
/// `(activity_id, target_inbox)` 唯一约束把重复投递吃掉，收方只收到一份。
///
/// 只对 `Visibility::Public` 调用 —— followers / direct 的收件人是明确的，
/// 群邻不在其中，往那边投等于把非公开内容广播给没被寻址的实例。
pub(super) async fn stage_room_peer_fan_out(
    txn: &impl ConnectionTrait,
    base_url: &str,
    activity_db_id: i32,
) -> Result<u32, sea_orm::DbErr> {
    let peers = crate::federation::room_peers::room_peer_inboxes(txn).await?;
    // 本地 actor 的帖子对本实例用户已经可见（federation_activities 就是查询源），
    // 同域目标只会让投递线程对自己发一次 HTTP。
    let local_domain = crate::federation::types::extract_domain(base_url)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut queued = 0u32;
    for peer in peers {
        if !local_domain.is_empty() && peer.domain == local_domain {
            continue;
        }
        queued += enqueue_delivery(txn, activity_db_id, &peer.inbox_url, &peer.domain).await?;
    }
    Ok(queued)
}

async fn enqueue_delivery(
    txn: &impl ConnectionTrait,
    activity_db_id: i32,
    inbox: &str,
    domain: &str,
) -> Result<u32, sea_orm::DbErr> {
    let result = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_delivery_queue
                   (activity_id, target_inbox, target_domain, status, created_at)
               VALUES ($1, $2, $3, 'pending', NOW())
               ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
            [activity_db_id.into(), inbox.into(), domain.into()],
        ))
        .await?;
    Ok(result.rows_affected() as u32)
}

/// 提交后把活动进程内投给同实例粉丝，逐个尽力而为。
///
/// 投递 worker 拒绝往本机 / 私网发 HTTP，所以同实例粉丝只能走进程内投递；
/// 它与远端收件箱走同一套分发（回执、事务、Delete / Undo 语义都一致）。
/// 某个粉丝失败只记日志，不影响其他粉丝，也不影响已经提交的发布结果。
/// 返回成功投递的人数。
pub(crate) async fn deliver_to_local_followers(
    db: &DatabaseConnection,
    usernames: &[String],
    activity_json: &serde_json::Value,
) -> u32 {
    let mut delivered = 0u32;
    for username in usernames {
        match crate::federation::inbox::deliver_activity_locally(db, username, activity_json).await
        {
            Ok(()) => delivered += 1,
            Err(error) => tracing::error!(
                username = %username,
                activity = activity_json["id"].as_str().unwrap_or(""),
                %error,
                "Local fan-out failed for one follower"
            ),
        }
    }
    delivered
}

/// If inbox is `{base}/users/{username}/inbox`, return username.
fn local_username_from_inbox_url(base_url: &str, inbox_url: &str) -> Option<String> {
    let trimmed = inbox_url.trim().trim_end_matches('/');
    let actor = trimmed.strip_suffix("/inbox")?;
    local_username_from_actor_url(base_url, actor)
}

// 辅助函数

/// 解析观众列表。
///
/// 每个 [`Visibility`] 分支都必须产出**非空**的 `to`。
///
/// `Direct` 目前没有可表达的收件人字段（`PublishRequest` 不带 recipients），
/// 所以自寻址给作者本人：语义上等于"仅自己可见"，且绝不 fan-out。
pub(super) fn resolve_audience(
    visibility: Visibility,
    base_url: &str,
    username: &str,
) -> (Vec<String>, Vec<String>) {
    match visibility {
        Visibility::Public => (
            vec![AP_PUBLIC.to_string()],
            vec![followers_url(base_url, username)],
        ),
        Visibility::Followers => (vec![followers_url(base_url, username)], vec![]),
        Visibility::Direct => (vec![actor_url(base_url, username)], vec![]),
    }
}

/// Best-effort summary of platform_metadata.raw_data for library publish.
/// Returns (approx item count, up to 5 sample titles).
fn summarize_library_raw(raw: &serde_json::Value) -> (usize, Vec<String>) {
    let mut titles: Vec<String> = Vec::new();
    let mut count = 0usize;

    fn push_title(titles: &mut Vec<String>, v: &serde_json::Value) {
        if titles.len() >= 5 {
            return;
        }
        let t = v
            .get("title")
            .or_else(|| v.get("name"))
            .or_else(|| v.get("subject_title"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        if !t.is_empty() && !titles.iter().any(|e| e == t) {
            titles.push(t.chars().take(80).collect());
        }
    }

    fn walk(v: &serde_json::Value, count: &mut usize, titles: &mut Vec<String>) {
        match v {
            serde_json::Value::Array(arr) => {
                // Treat top-level-ish arrays of objects as item lists
                let object_items: Vec<&serde_json::Value> =
                    arr.iter().filter(|x| x.is_object()).collect();
                if !object_items.is_empty() {
                    *count += object_items.len();
                    for item in object_items {
                        // nested subject/node common in MAL/Bangumi
                        if let Some(node) = item.get("node").or_else(|| item.get("subject")) {
                            push_title(titles, node);
                        } else {
                            push_title(titles, item);
                        }
                    }
                } else {
                    for x in arr {
                        walk(x, count, titles);
                    }
                }
            }
            serde_json::Value::Object(map) => {
                // Prefer known collection keys
                for key in [
                    "games",
                    "anime",
                    "manga",
                    "music",
                    "collection",
                    "items",
                    "data",
                    "list",
                ] {
                    if let Some(inner) = map.get(key) {
                        walk(inner, count, titles);
                    }
                }
                // If still empty, shallow-walk remaining arrays once
                if *count == 0 {
                    for (k, inner) in map {
                        if matches!(
                            k.as_str(),
                            "games"
                                | "anime"
                                | "manga"
                                | "music"
                                | "collection"
                                | "items"
                                | "data"
                                | "list"
                        ) {
                            continue;
                        }
                        if inner.is_array() {
                            walk(inner, count, titles);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    walk(raw, &mut count, &mut titles);
    (count, titles)
}

/// 从报告 JSON 中提取纯文本摘要（chat / mfp snapshot 用）
fn extract_report_summary_plain(report_json: &serde_json::Value) -> String {
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
        return "<p>Data report</p>".to_string();
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

fn not_found(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(AppError::public_json(msg)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn escape_html_basic() {
        assert_eq!(escape_html("a<b>&c"), "a&lt;b&gt;&amp;c");
    }

    #[test]
    fn extract_report_summary_plain_from_summary_and_insights() {
        let with_summary =
            json!({"summary": "活跃开发者", "insights": ["ignored when summary present"]});
        assert_eq!(extract_report_summary_plain(&with_summary), "活跃开发者");

        let with_insights = json!({"insights": ["首条洞察", "第二条"]});
        assert_eq!(extract_report_summary_plain(&with_insights), "首条洞察");

        assert_eq!(extract_report_summary_plain(&json!({})), "");
    }

    #[test]
    fn extract_report_summary_html_escapes_and_falls_back() {
        let xss = json!({"summary": "a<b>&c"});
        assert_eq!(extract_report_summary(&xss), "<p>a&lt;b&gt;&amp;c</p>");
        assert_eq!(extract_report_summary(&json!({})), "<p>Data report</p>");
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

    #[test]
    fn resolve_audience_public_and_followers() {
        let base = "https://myriad.example";
        let (to, cc) = resolve_audience(Visibility::Public, base, "alice");
        assert!(to.iter().any(|u| u.contains("Public") || u == AP_PUBLIC));
        assert!(cc.iter().any(|u| u.ends_with("/users/alice/followers")));
        let (to2, cc2) = resolve_audience(Visibility::Followers, base, "alice");
        assert!(to2.iter().any(|u| u.ends_with("/users/alice/followers")));
        assert!(cc2.is_empty());
        // Direct 自寻址给作者本人，绝不留空 to。
        let (to3, cc3) = resolve_audience(Visibility::Direct, base, "alice");
        assert_eq!(to3, vec!["https://myriad.example/users/alice".to_string()]);
        assert!(cc3.is_empty());
    }

    #[test]
    fn local_username_from_inbox_url_rejects_wrong_host() {
        let base = "https://myriad.example";
        assert_eq!(
            local_username_from_inbox_url(base, "https://myriad.example/users/alice/inbox"),
            Some("alice".into())
        );
        assert_eq!(
            local_username_from_inbox_url(base, "https://evil.example/users/alice/inbox"),
            None
        );
    }

    #[test]
    fn w175_resolve_audience_matrix() {
        let base = "https://myriad.example";
        let (to, cc) = resolve_audience(Visibility::Public, base, "alice");
        assert!(to.iter().any(|u| u == AP_PUBLIC || u.contains("Public")));
        assert!(cc.iter().any(|u| u.ends_with("/users/alice/followers")));
        let (to_f, cc_f) = resolve_audience(Visibility::Followers, base, "bob");
        assert!(to_f.iter().any(|u| u.ends_with("/users/bob/followers")));
        assert!(cc_f.is_empty());
        let (to_d, cc_d) = resolve_audience(Visibility::Direct, base, "carol");
        assert_eq!(to_d, vec!["https://myriad.example/users/carol".to_string()]);
        assert!(cc_d.is_empty());
    }

    #[test]
    fn w175_local_username_from_inbox_host() {
        let base = "https://myriad.example";
        assert_eq!(
            local_username_from_inbox_url(base, "https://myriad.example/users/alice/inbox"),
            Some("alice".into())
        );
        assert_eq!(
            local_username_from_inbox_url(base, "https://evil.example/users/alice/inbox"),
            None
        );
    }

    #[test]
    fn route_follower_isolates_bad_rows() {
        let base = "https://myriad.example";
        assert_eq!(
            route_follower(
                base,
                Some("https://r.example/users/a/inbox"),
                Some("r.example"),
                Some("https://r.example/users/a"),
            ),
            FollowerRoute::Remote {
                inbox: "https://r.example/users/a/inbox".into(),
                domain: "r.example".into(),
            }
        );
        assert_eq!(
            route_follower(
                base,
                Some("https://myriad.example/users/bob/inbox"),
                None,
                None
            ),
            FollowerRoute::Local("bob".into())
        );
        // 本地粉丝即使 inbox 缺失，也能凭 actor URL 走本地捷径。
        assert_eq!(
            route_follower(
                base,
                Some(""),
                None,
                Some("https://myriad.example/users/bob")
            ),
            FollowerRoute::Local("bob".into())
        );
        assert!(matches!(
            route_follower(
                base,
                Some("  "),
                Some("r.example"),
                Some("https://r.example/a")
            ),
            FollowerRoute::Skip(_)
        ));
        assert!(matches!(
            route_follower(base, None, Some("r.example"), None),
            FollowerRoute::Skip(_)
        ));
        assert!(matches!(
            route_follower(base, Some("https://r.example/inbox"), None, None),
            FollowerRoute::Skip(_)
        ));
    }

    /// 发布 / 撤回用的扇出：坏粉丝只跳过自己，数据库错误才让事务作废；
    /// 本地捷径在提交后逐个尽力，不返回错误。
    #[test]
    fn staged_fan_out_is_best_effort_per_recipient() {
        let src = include_str!("ap_object.rs");
        let section = |start: &str, end: &str| {
            src.split(start)
                .nth(1)
                .and_then(|rest| rest.split(end).next())
                .expect(start)
                .to_string()
        };
        let stage = section(
            "pub(crate) async fn stage_follower_fan_out",
            "pub(super) async fn stage_room_peer_fan_out",
        );
        assert!(stage.contains("txn: &impl ConnectionTrait"));
        assert!(stage.contains("FollowerRoute::Skip(reason)"));
        assert!(!stage.contains("return Err"));
        let local = section(
            "pub(crate) async fn deliver_to_local_followers",
            "/// If inbox is",
        );
        assert!(local.contains(") -> u32 {"));
        assert!(!local.contains('?'));
    }

    /// 提交后扇出与事务内扇出走同一套逐粉丝路由：坏粉丝只跳过自己，
    /// 数据库错误才返回 `Err`。
    #[test]
    fn fan_out_to_followers_is_best_effort_per_recipient() {
        let src = include_str!("ap_object.rs");
        let fan = src
            .split("pub(crate) async fn fan_out_to_followers")
            .nth(1)
            .and_then(|rest| rest.split("pub(crate) struct StagedFanOut").next())
            .expect("fan_out_to_followers");
        assert!(fan.contains("Result<u32, String>"));
        assert!(fan.contains("stage_follower_fan_out(db"));
        assert!(fan.contains("deliver_to_local_followers(db"));
        assert!(!fan.contains("return Err"));
    }

    /// 密钥轮换走的提交后扇出：空 inbox 的坏远端、已不存在的本地用户都只
    /// 跳过自己，正常远端照常排队，本地 bob 照常收到。
    #[tokio::test]
    async fn fan_out_to_followers_skips_bad_followers() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        let base = get_base_url().await;
        db.execute_unprepared(&format!(
            r#"
            INSERT INTO users (id, username) VALUES (1, 'alice'), (2, 'bob');
            INSERT INTO federation_remote_actors (id, actor_url, domain, inbox_url) VALUES
                (11, 'https://good.example/users/g', 'good.example', 'https://good.example/users/g/inbox'),
                (12, 'https://bad.example/users/b', 'bad.example', ''),
                (13, '{base}/users/bob', 'local', '{base}/users/bob/inbox'),
                (14, '{base}/users/ghost', 'local', '{base}/users/ghost/inbox');
            INSERT INTO federation_follows (user_id, remote_actor_id, direction, status) VALUES
                (1, 11, 'incoming', 'accepted'), (1, 12, 'incoming', 'accepted'),
                (1, 13, 'incoming', 'accepted'), (1, 14, 'incoming', 'accepted');
            "#
        ))
        .await
        .unwrap();
        let activity_id = format!("{base}/activities/fan-out-test");
        let create = json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "Create",
            "id": &activity_id,
            "actor": format!("{base}/users/alice"),
            "to": [AP_PUBLIC],
            "object": {
                "type": "Note",
                "id": format!("{base}/notes/fan-out-test"),
                "attributedTo": format!("{base}/users/alice"),
                "content": "hi",
                "to": [AP_PUBLIC],
            },
        });
        let act_db_id =
            insert_local_activity(db, 1, &activity_id, "Create", Some("Note"), create.clone())
                .await
                .unwrap();

        let delivered = fan_out_to_followers(db, 1, act_db_id, &create)
            .await
            .expect("bad followers must not fail the fan-out");
        assert_eq!(delivered, 2, "good remote queued + bob delivered locally");
        let queued: Vec<String> = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT target_inbox FROM federation_delivery_queue WHERE activity_id = $1",
                [act_db_id.into()],
            ))
            .await
            .unwrap()
            .iter()
            .map(|row| row.try_get("", "target_inbox").unwrap())
            .collect();
        assert_eq!(
            queued,
            vec!["https://good.example/users/g/inbox".to_string()]
        );
        let bob_rows = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT COUNT(*) AS n FROM federation_timeline WHERE user_id = 2 AND activity_id = $1",
                [activity_id.into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "n")
            .unwrap();
        assert_eq!(bob_rows, 1);

        fixture.close().await;
    }
}
