//! Publish, unpublish, and list local federation content.

use axum::{Json, http::StatusCode};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;

use super::ap_object::{
    StagedFanOut, build_ap_object, deliver_to_local_followers, resolve_audience,
    stage_follower_fan_out, stage_room_peer_fan_out,
};
use super::kind::{ContentKind, REPOST_CONTENT_TYPE};
use super::timeline::{insert_author_timeline, published_fields_from_activity_json};
use super::types::{CreateNoteRequest, PublishRequest, PublishResponse, PublishedItem};
use crate::federation::audience::{FanOutScope, Visibility};
use crate::federation::types::*;

// 核心发布功能

/// 发布本地内容到联邦网络
///
/// 1. 拉取本地内容详情（或构建 freeform Note）
/// 2. 转换为 AP 对象（Note/Article/Application/Collection）
/// 3. 创建 Create Activity
/// 4. 存入 federation_published_content
/// 5. 写入作者时间线（不限 Note）
/// 6. 按 visibility fan-out（Direct/mentioned 不投 followers；Public 另投群邻）
///
/// `idempotency_key`（请求头 `Idempotency-Key`）按用户生效，随已发布行一起存：
/// 提交后响应丢了、客户端带同一个键重试时返回原来那次发布，不再生成第二条
/// `note_{uuid}`；同一个键换了内容是 409。重放不排新的投递，`delivered_queued`
/// 为 0。撤回发布删掉已发布行，键随之失效。
pub async fn publish_content(
    user_id: i32,
    is_admin: bool,
    username: &str,
    db: &DatabaseConnection,
    req: &PublishRequest,
    idempotency_key: Option<&str>,
) -> Result<PublishResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // visibility 必须是明确建模过的取值；未知值由 `parse_visibility` 拒绝。
    let visibility_raw = req.visibility.as_deref().unwrap_or("public");
    let visibility_kind =
        crate::federation::audience::parse_visibility(visibility_raw).map_err(|bad| {
            tracing::warn!(
                user_id,
                visibility = %bad,
                "Publish rejected: unsupported visibility"
            );
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Unsupported visibility",
                    "visibility": bad,
                    "supported": ["public", "followers", "unlisted", "private", "direct", "mentioned"],
                })),
            )
        })?;
    let visibility = visibility_kind.as_str();

    let content_type = req.content_type.trim();
    if content_type.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("content_type required")),
        ));
    }
    let kind: ContentKind = content_type.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Unsupported content type")),
        )
    })?;
    let content_type = kind.as_str();

    let idempotency = match idempotency_key {
        None => None,
        Some(key) if crate::services::ai_task_prepare::validate_idempotency_key(key) => {
            Some((key, publish_fingerprint(content_type, visibility, req)))
        }
        Some(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Invalid Idempotency-Key")),
            ));
        }
    };
    if let Some((key, fingerprint)) = &idempotency {
        if let Some(original) = replay_publish(db, user_id, key, fingerprint).await? {
            return Ok(original);
        }
    }

    let content_id = if kind == ContentKind::Note {
        match req
            .content_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
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
                    Json(AppError::public_json("content_id required")),
                )
            })?;
        id.to_string()
    };

    // 获取内容为 AP 对象
    let ap_object = build_ap_object(
        db,
        user_id,
        username,
        &base_url,
        kind,
        &content_id,
        visibility_kind,
        req.text.as_deref(),
        req.attachments.as_deref(),
        req.in_reply_to.as_deref(),
    )
    .await?;

    // 生成 Activity
    let activity_id = generate_activity_id(&base_url);
    let local_actor = actor_url(&base_url, username);

    let (to, cc) = resolve_audience(visibility_kind, &base_url, username);

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

    // Persist MFP content_type (report/tapp/library/…) for local indexing
    // (Ring gossip filters on object_type = 'library'|'tapp'). The AP object
    // still carries ActivityStreams type (Article/Application/Collection).
    let object_type = content_type.to_string();

    let txn = db.begin().await.map_err(db_err)?;
    // Unique (content_type, content_id) and (user_id, idempotency_key) are the
    // concurrency boundary. Insert the published row first so a conflict
    // cannot leave an orphan Create activity.
    let (stored_key, stored_fingerprint) = match &idempotency {
        Some((key, fingerprint)) => (Some(key.to_string()), Some(fingerprint.clone())),
        None => (None, None),
    };
    match txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_published_content
                   (user_id, content_type, content_id, activity_id, visibility, published_at,
                    idempotency_key, idempotency_fingerprint)
               VALUES ($1, $2, $3, $4, $5, $8::timestamptz, $6, $7)"#,
            [
                user_id.into(),
                content_type.into(),
                content_id.clone().into(),
                activity_id.clone().into(),
                visibility.into(),
                stored_key.into(),
                stored_fingerprint.into(),
                crate::clock::now().into(),
            ],
        ))
        .await
    {
        Ok(_) => {}
        Err(error) if is_unique_violation(&error) => {
            txn.rollback().await.map_err(db_err)?;
            // A concurrent request with the same key committed first: answer
            // with its result (or 409 for a different payload).
            if let Some((key, fingerprint)) = &idempotency {
                if let Some(original) = replay_publish(db, user_id, key, fingerprint).await? {
                    return Ok(original);
                }
            }
            return Err((
                StatusCode::CONFLICT,
                Json(AppError::public_json("Content already published")),
            ));
        }
        Err(error) => return Err(db_err(error)),
    }

    let attachment_urls = ap_attachment_urls(&ap_object);
    let origins = vec![base_url.trim_end_matches('/').to_string()];
    // Attachment URLs come from the request: publish and bind only media the
    // author may manage; another user's draft reads as an invalid attachment.
    let actor = if is_admin {
        crate::services::media::MediaActor::admin(user_id)
    } else {
        crate::services::media::MediaActor::user(user_id)
    }
    .map_err(media_ref_err)?;
    let bound = crate::services::media::bind(
        &txn,
        &crate::services::media::Consumer::federation_activity(activity_id.clone()),
        &crate::services::media::Citations::urls(&origins, &attachment_urls, |index| {
            format!("attachment:{index}")
        }),
        crate::services::media::Authority::Actor(&actor),
        crate::services::media::Unresolved::Reject,
    )
    .await
    .map_err(|error| match error {
        crate::services::media::MediaError::Missing => (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid attachment URL")),
        ),
        other => media_ref_err(other),
    })?;
    let mut activity_json = activity_json;
    rewrite_activity_attachment_urls(
        &mut activity_json,
        base_url.trim_end_matches('/'),
        &origins,
        bound.urls(),
        &txn,
    )
    .await
    .map_err(media_ref_err)?;
    let act_db_id = insert_local_activity(
        &txn,
        user_id,
        &activity_id,
        "Create",
        Some(&object_type),
        activity_json.clone(),
    )
    .await
    .map_err(db_err)?;
    insert_author_timeline(
        &txn,
        user_id,
        &activity_id,
        "Create",
        &object_type,
        &activity_json,
    )
    .await?;
    // 扇出意图随内容同一事务落进投递队列：提交成功即由投递 worker 送达，
    // 提交失败则什么都不留，重试不会产生重复帖子。
    let staged = stage_fan_out(&txn, &base_url, user_id, act_db_id, visibility_kind, true)
        .await
        .map_err(db_err)?;
    txn.commit().await.map_err(db_err)?;

    // 同实例粉丝走提交后的本地捷径；逐个尽力而为，不影响已提交的发布结果。
    let delivered_queued = staged.queued
        + deliver_to_local_followers(db, &staged.local_followers, &activity_json).await;

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

/// 同一个幂等键下「同一次发布」的指纹：规范化后的类型与可见性加上请求里决定
/// 发布内容的字段。服务端生成的 note id 不在里面，重试才会得到同一个指纹。
fn publish_fingerprint(content_type: &str, visibility: &str, req: &PublishRequest) -> String {
    use sha2::{Digest, Sha256};
    let canonical = json!({
        "content_type": content_type,
        "content_id": req.content_id.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        "visibility": visibility,
        "text": req.text,
        "attachments": req.attachments,
        "in_reply_to": req.in_reply_to,
    });
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

/// 这个用户用 `key` 发布过的结果；换了内容是 409。
async fn replay_publish(
    db: &DatabaseConnection,
    user_id: i32,
    key: &str,
    fingerprint: &str,
) -> Result<Option<PublishResponse>, (StatusCode, Json<serde_json::Value>)> {
    let Some(row) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT content_type, content_id, activity_id, visibility, idempotency_fingerprint
               FROM federation_published_content
               WHERE user_id = $1 AND idempotency_key = $2"#,
            [user_id.into(), key.into()],
        ))
        .await
        .map_err(db_err)?
    else {
        return Ok(None);
    };
    let stored: Option<String> = row.try_get("", "idempotency_fingerprint").map_err(db_err)?;
    if stored.as_deref() != Some(fingerprint) {
        return Err((
            StatusCode::CONFLICT,
            Json(AppError::public_json(
                "Idempotency-Key was already used for a different request",
            )),
        ));
    }
    Ok(Some(PublishResponse {
        success: true,
        activity_id: row.try_get("", "activity_id").map_err(db_err)?,
        content_type: row.try_get("", "content_type").map_err(db_err)?,
        content_id: row.try_get("", "content_id").map_err(db_err)?,
        visibility: row.try_get("", "visibility").map_err(db_err)?,
        delivered_queued: 0,
        author_timeline: true,
    }))
}

/// 在调用方事务内按 visibility 排队扇出：Direct 不投粉丝，Public 另投群邻。
async fn stage_fan_out(
    txn: &impl ConnectionTrait,
    base_url: &str,
    user_id: i32,
    activity_db_id: i32,
    visibility: Visibility,
    room_peers: bool,
) -> Result<StagedFanOut, sea_orm::DbErr> {
    // Direct 走 ExplicitRecipientsOnly —— 没有收件人就一个 inbox 都不投。
    let mut staged = match crate::federation::audience::fan_out_scope(visibility) {
        FanOutScope::AllFollowers => {
            stage_follower_fan_out(txn, base_url, user_id, activity_db_id).await?
        }
        FanOutScope::ExplicitRecipientsOnly => {
            tracing::info!(
                user_id,
                visibility = visibility.as_str(),
                "Skipping follower fan-out for non-broadcast visibility"
            );
            StagedFanOut::default()
        }
    };
    // 群邻实例扇出：只有 Public 走这条。`Followers` 虽然也 fan-out，但收件人是
    // 粉丝集合，不是 Public —— 投给群邻会把只给粉丝看的内容送出寻址范围。
    if room_peers && visibility == Visibility::Public {
        staged.queued += stage_room_peer_fan_out(txn, base_url, activity_db_id).await?;
    }
    if staged.skipped > 0 {
        tracing::warn!(
            user_id,
            activity_db_id,
            skipped = staged.skipped,
            "Fan-out skipped unusable followers"
        );
    }
    Ok(staged)
}

/// 创建 freeform Note（Aro 发帖）
pub async fn create_note(
    user_id: i32,
    is_admin: bool,
    username: &str,
    db: &DatabaseConnection,
    req: &CreateNoteRequest,
    idempotency_key: Option<&str>,
) -> Result<PublishResponse, (StatusCode, Json<serde_json::Value>)> {
    let publish_req = PublishRequest {
        content_type: ContentKind::Note.as_str().to_string(),
        content_id: None,
        visibility: req.visibility.clone(),
        text: req.text.clone(),
        attachments: req.attachments.clone(),
        in_reply_to: req.in_reply_to.clone(),
    };
    publish_content(
        user_id,
        is_admin,
        username,
        db,
        &publish_req,
        idempotency_key,
    )
    .await
}

/// Normalize a content_id that may be a bare id, object URL, or path.
/// Returns (optional content_type hint, bare content_id).
///
/// An object URL maps back through [`ContentKind::from_object_url`], so a
/// percent-encoded Tapp id is decoded to the id the row was stored under. An
/// explicit `content_type` wins over the kind inferred from the URL.
fn normalize_unpublish_target(
    content_type: Option<&str>,
    content_id: &str,
) -> (Option<String>, String) {
    let explicit = content_type
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let raw = content_id.trim();
    // Already bare id (note_uuid / numeric / tapp id)
    if raw.is_empty() || (!raw.contains("://") && !raw.contains('/')) {
        return (explicit, raw.to_string());
    }

    if let Some((kind, id)) = ContentKind::from_object_url(raw) {
        return (explicit.or_else(|| Some(kind.as_str().to_string())), id);
    }
    // Unknown URL family: fall back to its last path segment.
    let path = raw.split(['?', '#']).next().unwrap_or(raw);
    let trailing = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(raw)
        .to_string();
    (explicit, trailing)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UnpublishLookupError {
    None,
    Ambiguous,
}

pub(crate) fn unique_unpublish_row<T>(mut rows: Vec<T>) -> Result<T, UnpublishLookupError> {
    match rows.len() {
        0 => Err(UnpublishLookupError::None),
        1 => Ok(rows.remove(0)),
        _ => Err(UnpublishLookupError::Ambiguous),
    }
}

/// 取消发布（Delete Activity）
///
/// Accepts `activity_id`, or `content_type`+`content_id`, or `content_id` alone
/// (type inferred / looked up). `content_id` may be bare id, object URL, or path.
///
/// 整个撤回在一个事务里：锁住已发布行 → 写 Delete → 删已发布行与时间线 →
/// 释放原活动绑定的附件引用 → 按原受众排队 Delete。并发撤回同一条内容时，
/// 后到的一方等锁后查不到行，得到 404，不会再写第二条 Delete。
pub async fn unpublish_content(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    content_type: Option<&str>,
    content_id: Option<&str>,
    activity_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let activity_id = activity_id.map(str::trim).filter(|s| !s.is_empty());
    let content_id_raw = content_id.map(str::trim).filter(|s| !s.is_empty());

    let txn = db.begin().await.map_err(db_err)?;

    // 查找并锁住已发布记录 — activity_id first, then content_type+content_id (URL-tolerant)
    let row = if let Some(aid) = activity_id {
        txn.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, activity_id, content_type, content_id, visibility FROM federation_published_content WHERE user_id = $1 AND activity_id = $2 FOR UPDATE",
            [user_id.into(), aid.into()],
        ))
        .await
        .map_err(db_err)?
    } else if let Some(cid_raw) = content_id_raw {
        let (ct_opt, bare_id) = normalize_unpublish_target(content_type, cid_raw);
        if bare_id.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("content_id required")),
            ));
        }
        if let Some(ct) = ct_opt.as_deref().filter(|s| !s.is_empty()) {
            // Exact type + id
            let found = txn
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT id, activity_id, content_type, content_id, visibility FROM federation_published_content WHERE user_id = $1 AND content_type = $2 AND content_id = $3 FOR UPDATE",
                    [user_id.into(), ct.into(), bare_id.clone().into()],
                ))
                .await
                .map_err(db_err)?;
            if found.is_some() {
                found
            } else {
                // content_id may have been passed as full object URL while stored bare
                txn.query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT id, activity_id, content_type, content_id, visibility FROM federation_published_content WHERE user_id = $1 AND content_type = $2 AND (content_id = $3 OR content_id = $4) FOR UPDATE",
                    [
                        user_id.into(),
                        ct.into(),
                        bare_id.clone().into(),
                        cid_raw.into(),
                    ],
                ))
                .await
                .map_err(db_err)?
            }
        } else {
            let rows = txn
                .query_all_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT id, activity_id, content_type, content_id, visibility FROM federation_published_content WHERE user_id = $1 AND (content_id = $2 OR content_id = $3) LIMIT 2 FOR UPDATE",
                    [user_id.into(), bare_id.into(), cid_raw.into()],
                ))
                .await
                .map_err(db_err)?;
            match unique_unpublish_row(rows) {
                Ok(row) => Some(row),
                Err(UnpublishLookupError::None) => None,
                Err(UnpublishLookupError::Ambiguous) => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(AppError::public_json(
                            "content_id is ambiguous; provide content_type",
                        )),
                    ));
                }
            }
        }
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Provide activity_id, or content_type + content_id",
            )),
        ));
    };

    let row = row.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Content not published")),
        )
    })?;

    let pub_id = crate::federation::types::row_positive_id(&row, "id")
        .map_err(|error| db_err(sea_orm::DbErr::Custom(error)))?;
    let original_activity_id: String = row.try_get("", "activity_id").map_err(db_err)?;
    let content_type: String = row
        .try_get::<String>("", "content_type")
        .unwrap_or_else(|_| content_type.unwrap_or("").to_string());
    let content_id: String = row
        .try_get::<String>("", "content_id")
        .unwrap_or_else(|_| content_id_raw.unwrap_or("").to_string());
    let stored_visibility: Option<String> = row.try_get("", "visibility").ok().flatten();

    // 撤回转发就是取消转发：转发标记、已发布行、时间线一起清，只写一条 Delete。
    if content_type == REPOST_CONTENT_TYPE {
        let withdrawn = crate::federation::interactions::withdraw_repost(
            &txn,
            &base_url,
            user_id,
            username,
            &original_activity_id,
        )
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Content not published")),
            )
        })?;
        txn.commit().await.map_err(db_err)?;
        let delete_activity_id = withdrawn.activity_id.clone();
        crate::federation::interactions::deliver_withdrawn_repost(db, withdrawn).await;
        return Ok(json!({
            "success": true,
            "delete_activity_id": delete_activity_id,
            "content_type": content_type,
            "content_id": content_id,
            "activity_id": original_activity_id,
        }));
    }

    let audience = unpublish_audience(stored_visibility.as_deref(), &content_type);

    // 创建 Delete Activity —— 寻址与原 Create 一致
    let delete_activity_id = generate_activity_id(&base_url);
    let local_actor = actor_url(&base_url, username);
    let (to, cc) = match audience.addressing {
        Some(visibility) => resolve_audience(visibility, &base_url, username),
        None => (vec![AP_PUBLIC.to_string()], vec![]),
    };

    let delete_json = json!({
        "@context": build_ap_context(),
        "type": "Delete",
        "id": &delete_activity_id,
        "actor": &local_actor,
        "published": now_iso8601(),
        "to": to,
        "cc": cc,
        "object": &original_activity_id,
    });

    // 存 Delete Activity
    let del_db_id = insert_local_activity(
        &txn,
        user_id,
        &delete_activity_id,
        "Delete",
        Some(&content_type),
        delete_json.clone(),
    )
    .await
    .map_err(db_err)?;

    // 删除 published_content 记录
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_published_content WHERE id = $1",
        [pub_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // 从作者与本地时间线移除原 Create
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_timeline WHERE activity_id = $1",
        [original_activity_id.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    // 发布时以原活动为消费者绑定的附件引用随撤回释放，附件才能被删除。
    crate::services::media::bind(
        &txn,
        &crate::services::media::Consumer::federation_activity(original_activity_id.clone()),
        &crate::services::media::Citations::new(),
        crate::services::media::Authority::Site,
        crate::services::media::Unresolved::Skip,
    )
    .await
    .map_err(media_ref_err)?;

    let staged = stage_fan_out(
        &txn,
        &base_url,
        user_id,
        del_db_id,
        audience.fan_out,
        audience.room_peers,
    )
    .await
    .map_err(db_err)?;
    txn.commit().await.map_err(db_err)?;

    let delivered_queued =
        staged.queued + deliver_to_local_followers(db, &staged.local_followers, &delete_json).await;

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
        "content_type": content_type,
        "content_id": content_id,
        "activity_id": original_activity_id,
    }))
}

/// 撤回时 Delete 的受众：与原 Create 相同。
#[derive(Debug, PartialEq, Eq)]
struct UnpublishAudience {
    /// 按哪个 visibility 生成 `to`/`cc`；`None` 是无法识别的历史值，沿用旧的 `to: [Public]`。
    addressing: Option<Visibility>,
    /// 粉丝扇出范围按哪个 visibility 判定。
    fan_out: Visibility,
    /// 原 Create 是否投过群邻。
    room_peers: bool,
}

fn unpublish_audience(stored_visibility: Option<&str>, content_type: &str) -> UnpublishAudience {
    match stored_visibility.map(crate::federation::audience::parse_visibility) {
        Some(Ok(visibility)) => UnpublishAudience {
            addressing: Some(visibility),
            fan_out: visibility,
            // 转发（repost）只投粉丝与原作者，从没投过群邻（见 interactions 的转发路径）。
            room_peers: visibility == Visibility::Public && content_type != REPOST_CONTENT_TYPE,
        },
        // 历史行：维持改动前的行为 —— 投全部粉丝、不投群邻。
        _ => UnpublishAudience {
            addressing: None,
            fan_out: Visibility::Followers,
            room_peers: false,
        },
    }
}

/// 获取用户已发布的内容列表
///
/// Joins `federation_activities.object_json` so clients (Aro) can render
/// title / summary / content_preview / attachments instead of bare content_type + id.
pub async fn list_published(
    user_id: i32,
    db: &DatabaseConnection,
) -> Result<Vec<PublishedItem>, (StatusCode, Json<serde_json::Value>)> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT p.id, p.content_type, p.content_id, p.activity_id, p.visibility, p.published_at,
                      a.object_json
               FROM federation_published_content p
               LEFT JOIN federation_activities a ON a.activity_id = p.activity_id
               WHERE p.user_id = $1
                 AND p.content_type = ANY($2)
               ORDER BY p.published_at DESC
               LIMIT 200"#,
            [user_id.into(), listable_content_types().into()],
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
            let (title, summary, content_preview, attachments) =
                published_fields_from_activity_json(object_json.as_ref());
            // Unwrap Create envelope → object for clients (quote chain / full body).
            let content_obj = object_json.as_ref().map(|root| {
                if root.get("object").map(|o| o.is_object()).unwrap_or(false) {
                    root["object"].clone()
                } else {
                    root.clone()
                }
            });
            let object_id = content_obj
                .as_ref()
                .and_then(crate::federation::interactions::extract_object_id);
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
                attachments,
                content_json: content_obj,
                object_id,
            }
        })
        .collect();

    Ok(items)
}

/// `content_type` values 已发布 lists: every [`ContentKind`] plus quote-reposts.
fn listable_content_types() -> Vec<String> {
    ContentKind::ALL
        .iter()
        .map(|kind| kind.as_str())
        .chain([REPOST_CONTENT_TYPE])
        .map(str::to_string)
        .collect()
}

async fn rewrite_activity_attachment_urls(
    activity: &mut serde_json::Value,
    base: &str,
    origins: &[String],
    published: &std::collections::HashMap<i32, String>,
    txn: &impl sea_orm::ConnectionTrait,
) -> Result<(), crate::services::media::MediaError> {
    let Some(attachments) = activity
        .get_mut("object")
        .and_then(|object| object.get_mut("attachment"))
        .and_then(|value| value.as_array_mut())
    else {
        return Ok(());
    };
    for attachment in attachments {
        let Some(url) = attachment.get("url").and_then(|value| value.as_str()) else {
            continue;
        };
        let path = crate::services::media::cite_local_path(url, origins)
            .unwrap_or_else(|| url.to_string());
        let Some(id) = crate::services::media::resolve_asset_id(txn, &path).await? else {
            continue;
        };
        if let Some(public) = published.get(&id) {
            attachment["url"] = json!(format!("{base}{public}"));
        }
    }
    Ok(())
}

fn ap_attachment_urls(object: &serde_json::Value) -> Vec<String> {
    object
        .get("attachment")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.get("url")
                .and_then(|url| url.as_str())
                .map(str::to_string)
        })
        .collect()
}

fn media_ref_err(
    error: crate::services::media::MediaError,
) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(%error, "failed to bind federation media references");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "Failed to record media references",
            "code": error.code(),
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpublish_target_maps_object_urls_back_to_stored_ids() {
        let ct = |s: &str| Some(s.to_string());
        assert_eq!(
            normalize_unpublish_target(None, "note_1"),
            (None, "note_1".into())
        );
        assert_eq!(
            normalize_unpublish_target(None, "https://x.example/notes/note_1"),
            (ct("note"), "note_1".into())
        );
        assert_eq!(
            normalize_unpublish_target(None, "/phantasi/articles/8/"),
            (ct("phantasi-article"), "8".into())
        );
        // Tapp 对象 URL 里的 id 是编码过的；已发布行存的是原始 id。
        assert_eq!(
            normalize_unpublish_target(None, "https://x.example/tapps/demo%2Ftapp%20x"),
            (ct("tapp"), "demo/tapp x".into())
        );
        // 显式 content_type 优先；未知 URL 族退回最后一段。
        assert_eq!(
            normalize_unpublish_target(Some("repost"), "https://x.example/notes/n"),
            (ct("repost"), "n".into())
        );
        assert_eq!(
            normalize_unpublish_target(None, "https://x.example/other/z?q=1"),
            (None, "z".into())
        );
    }

    #[test]
    fn published_list_covers_every_kind_and_reposts_only() {
        let listed = listable_content_types();
        for kind in ContentKind::ALL {
            assert!(listed.iter().any(|t| t == kind.as_str()));
        }
        assert!(listed.iter().any(|t| t == REPOST_CONTENT_TYPE));
        assert!(!listed.iter().any(|t| t == "announce"));
        assert_eq!(listed.len(), ContentKind::ALL.len() + 1);
    }

    /// C4 回归：非广播 visibility 必须完全跳过粉丝 fan-out。
    #[test]
    fn non_broadcast_visibility_never_fans_out() {
        use crate::federation::audience::{fan_out_scope, parse_visibility};
        for raw in ["direct", "mentioned"] {
            let v = parse_visibility(raw).expect("modelled visibility");
            assert_eq!(
                fan_out_scope(v),
                FanOutScope::ExplicitRecipientsOnly,
                "{raw}"
            );
        }
        for raw in ["public", "followers", "unlisted", "private"] {
            let v = parse_visibility(raw).expect("modelled visibility");
            assert_eq!(fan_out_scope(v), FanOutScope::AllFollowers, "{raw}");
        }
        // 拼错的 visibility 在 publish_content 入口就会 400，而不是退化成广播
        assert!(parse_visibility("publik").is_err());
    }

    #[test]
    fn unpublish_content_id_only_rejects_ambiguous_matches() {
        assert_eq!(
            unique_unpublish_row::<i32>(vec![]).unwrap_err(),
            UnpublishLookupError::None
        );
        assert_eq!(unique_unpublish_row(vec![7]).unwrap(), 7);
        assert_eq!(
            unique_unpublish_row(vec![1, 2]).unwrap_err(),
            UnpublishLookupError::Ambiguous
        );
    }

    #[test]
    fn publish_uses_one_transaction_and_unique_conflict_as_409() {
        let src = include_str!("publish.rs");
        let publish = src
            .split("pub async fn publish_content")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn create_note").next())
            .expect("publish_content");
        assert!(publish.contains("db.begin()"));
        assert!(publish.contains("is_unique_violation"));
        assert!(publish.contains("Content already published"));
        assert!(publish.contains("txn.commit()"));
        assert!(publish.contains("services::media::bind("));
        assert!(publish.contains("federation_activity"));
        // 扇出在提交前随事务落队列；提交后只剩逐个尽力的本地捷径，
        // 响应不再取决于扇出结果。
        let staged = publish.find("stage_fan_out(&txn").expect("staged fan-out");
        let commit = publish.find("txn.commit()").expect("commit");
        let local = publish.find("deliver_to_local_followers(").expect("local");
        assert!(staged < commit && commit < local);
        assert!(!publish.contains("fan_out_to_followers"));
        assert!(!publish.contains("delivery_enqueue_failed"));
        assert!(
            !publish.contains("SELECT id FROM federation_published_content WHERE content_type")
        );
    }

    fn note(visibility: &str) -> PublishRequest {
        PublishRequest {
            content_type: "note".into(),
            content_id: None,
            visibility: Some(visibility.into()),
            text: Some("hello".into()),
            attachments: None,
            in_reply_to: None,
        }
    }

    async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index::<i64>(0)
            .unwrap()
    }

    /// alice(1) 有四个粉丝：一个正常远端、一个空 inbox 的坏远端、本地 bob、
    /// 已不存在的本地用户 ghost。坏粉丝只跳过自己，发布照常成功。
    async fn seed_followers(db: &DatabaseConnection) -> String {
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
        base
    }

    #[tokio::test]
    async fn publish_stages_fan_out_and_skips_bad_followers() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        seed_followers(db).await;

        let published = publish_content(1, false, "alice", db, &note("public"), None)
            .await
            .expect("one bad follower must not fail the publish");
        // 1 条远端排队 + bob 的本地时间线；空 inbox 与 ghost 被跳过。
        assert_eq!(published.delivered_queued, 2);
        let aid = &published.activity_id;
        assert_eq!(
            count(
                db,
                &format!(
                    "SELECT COUNT(*) FROM federation_delivery_queue q \
                     JOIN federation_activities a ON a.id = q.activity_id \
                     WHERE a.activity_id = '{aid}' AND q.status = 'pending' \
                     AND q.target_inbox = 'https://good.example/users/g/inbox'"
                )
            )
            .await,
            1
        );
        assert_eq!(
            count(
                db,
                "SELECT COUNT(*) FROM federation_delivery_queue WHERE target_inbox = ''"
            )
            .await,
            0
        );
        assert_eq!(
            count(
                db,
                &format!(
                    "SELECT COUNT(*) FROM federation_timeline \
                     WHERE activity_id = '{aid}' AND user_id = 2"
                )
            )
            .await,
            1
        );

        // Direct 不投任何粉丝。
        let direct = publish_content(1, false, "alice", db, &note("direct"), None)
            .await
            .unwrap();
        assert_eq!(direct.delivered_queued, 0);

        fixture.close().await;
    }

    #[test]
    fn unpublish_audience_matches_the_original_create() {
        let public = unpublish_audience(Some("public"), "note");
        assert_eq!(public.addressing, Some(Visibility::Public));
        assert!(public.room_peers);
        // 转发只投过粉丝与原作者，撤回也不投群邻。
        assert!(!unpublish_audience(Some("public"), "repost").room_peers);
        let followers = unpublish_audience(Some("followers"), "note");
        assert_eq!(followers.addressing, Some(Visibility::Followers));
        assert!(!followers.room_peers);
        let direct = unpublish_audience(Some("direct"), "note");
        assert_eq!(direct.fan_out, Visibility::Direct);
        assert!(!direct.room_peers);
        // 无法识别的历史值维持改动前：to Public、投全部粉丝、不投群邻。
        for legacy in [None, Some("weird")] {
            let audience = unpublish_audience(legacy, "note");
            assert_eq!(audience.addressing, None);
            assert_eq!(audience.fan_out, Visibility::Followers);
            assert!(!audience.room_peers);
        }
    }

    #[test]
    fn unpublish_runs_in_one_locked_transaction() {
        let src = include_str!("publish.rs");
        let body = src
            .split("pub async fn unpublish_content")
            .nth(1)
            .and_then(|rest| rest.split("struct UnpublishAudience").next())
            .expect("unpublish_content");
        // 转发走共用的撤回函数，在同一个事务里提交之后再投递。
        let (lookup, rest) = body
            .split_once("if content_type == REPOST_CONTENT_TYPE {")
            .expect("repost branch");
        let (repost, generic) = rest.split_once("let audience").expect("generic path");
        let at = |s: &str, needle: &str| s.find(needle).unwrap_or_else(|| panic!("{needle}"));
        assert!(
            at(repost, "interactions::withdraw_repost(") < at(repost, "txn.commit()")
                && at(repost, "txn.commit()") < at(repost, "deliver_withdrawn_repost(db")
        );
        assert!(at(lookup, "db.begin()") < at(lookup, "FOR UPDATE"));
        let pos = |needle: &str| at(generic, needle);
        let begin = 0;
        let delete = pos("\"Delete\",");
        let drop_row = pos("DELETE FROM federation_published_content");
        let drop_timeline = pos("DELETE FROM federation_timeline");
        let release = pos("Citations::new()");
        let stage = pos("stage_fan_out(");
        let commit = pos("txn.commit()");
        let local = pos("deliver_to_local_followers(");
        assert!(begin < delete && delete < drop_row && drop_row < drop_timeline);
        assert!(drop_timeline < release && release < stage && stage < commit && commit < local);
        assert!(body.contains("Consumer::federation_activity(original_activity_id"));
        // 每条定位已发布行的查询都加行锁，并发撤回的后到者查不到行。
        assert_eq!(
            body.matches("FROM federation_published_content WHERE user_id")
                .count(),
            body.matches("FOR UPDATE\"").count()
        );
        assert!(!body.contains("let _ = db"));
        assert!(!body.contains("[AP_PUBLIC],"));
    }

    #[tokio::test]
    async fn unpublish_is_atomic_audience_scoped_and_releases_media() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        let base = seed_followers(db).await;

        let published = publish_content(1, false, "alice", db, &note("followers"), None)
            .await
            .unwrap();
        let aid = published.activity_id.clone();
        // 模拟发布时绑定的附件引用。
        db.execute_unprepared(&format!(
            "INSERT INTO media_assets (id, kind, url, mime, name, size) \
             VALUES (900, 'upload', '/media/x.png', 'image/png', 'x.png', 0); \
             INSERT INTO media_references \
                 (asset_id, consumer_type, consumer_id, slot, requires_public, created_at) \
             VALUES (900, 'federation_activity', '{aid}', 'attachment:0', true, NOW())"
        ))
        .await
        .unwrap();

        let out = unpublish_content(1, "alice", db, None, None, Some(&aid))
            .await
            .unwrap();
        let delete_id = out["delete_activity_id"].as_str().unwrap().to_string();
        let delete = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                format!(
                    "SELECT id, object_json FROM federation_activities \
                     WHERE activity_id = '{delete_id}'"
                ),
            ))
            .await
            .unwrap()
            .unwrap();
        let delete_db_id: i32 = delete.try_get("", "id").unwrap();
        let json: serde_json::Value = delete.try_get("", "object_json").unwrap();
        // followers-only 帖子的 Delete 寻址给粉丝集合，不是 Public。
        assert_eq!(json["to"], json!([format!("{base}/users/alice/followers")]));
        assert_eq!(json["object"], json!(aid));
        let q = |sql: String| async move { count(db, &sql).await };
        assert_eq!(
            q(format!(
                "SELECT COUNT(*) FROM federation_delivery_queue WHERE activity_id = {delete_db_id}"
            ))
            .await,
            1,
            "only the healthy remote follower is queued"
        );
        assert_eq!(
            q(format!(
                "SELECT COUNT(*) FROM federation_published_content WHERE activity_id = '{aid}'"
            ))
            .await,
            0
        );
        assert_eq!(
            q(format!(
                "SELECT COUNT(*) FROM media_references \
                 WHERE consumer_type = 'federation_activity' AND consumer_id = '{aid}'"
            ))
            .await,
            0,
            "withdrawn post must release its attachment references"
        );

        // 再撤一次：404，不写第二条 Delete。
        let again = unpublish_content(1, "alice", db, None, None, Some(&aid))
            .await
            .unwrap_err();
        assert_eq!(again.0, StatusCode::NOT_FOUND);

        // 并发撤回同一条：恰好一方成功，另一方 404，只有一条 Delete。
        let second = publish_content(1, false, "alice", db, &note("public"), None)
            .await
            .unwrap();
        let sid = second.activity_id.clone();
        let (a, b) = tokio::join!(
            unpublish_content(1, "alice", db, None, None, Some(&sid)),
            unpublish_content(1, "alice", db, None, None, Some(&sid)),
        );
        assert_eq!(a.is_ok() as u8 + b.is_ok() as u8, 1);
        let loser = a.err().or(b.err()).unwrap();
        assert_eq!(loser.0, StatusCode::NOT_FOUND);
        assert_eq!(
            q(format!(
                "SELECT COUNT(*) FROM federation_activities \
                 WHERE activity_type = 'Delete' AND object_json->>'object' = '{sid}'"
            ))
            .await,
            1
        );
        assert_eq!(
            q("SELECT COUNT(*) FROM federation_activities WHERE activity_type = 'Delete'".into())
                .await,
            2
        );

        fixture.close().await;
    }

    /// 同一个键的重试（含并发重试）拿到原来那次发布，只有一条 Note、一条活动、
    /// 一份投递；同一个键换内容是 409；键按用户隔离；非法键 400。
    #[tokio::test]
    async fn publish_with_idempotency_key_replays_the_original() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        seed_followers(db).await;
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (3, 'carol')")
            .await
            .unwrap();

        let first = publish_content(1, false, "alice", db, &note("public"), Some("act-1"))
            .await
            .expect("first publish");
        assert!(first.content_id.starts_with("note_"));
        assert_eq!(first.delivered_queued, 2);
        let retry = publish_content(1, false, "alice", db, &note("public"), Some("act-1"))
            .await
            .expect("retry replays");
        assert_eq!(retry.activity_id, first.activity_id);
        assert_eq!(retry.content_id, first.content_id);
        assert_eq!(retry.visibility, "public");
        assert_eq!(retry.delivered_queued, 0);

        let mut edited = note("public");
        edited.text = Some("hello again".into());
        let reused = publish_content(1, false, "alice", db, &edited, Some("act-1"))
            .await
            .expect_err("same key, different payload");
        assert_eq!(reused.0, StatusCode::CONFLICT);
        let other_visibility =
            publish_content(1, false, "alice", db, &note("followers"), Some("act-1"))
                .await
                .expect_err("visibility is part of the payload");
        assert_eq!(other_visibility.0, StatusCode::CONFLICT);

        let bad = publish_content(1, false, "alice", db, &note("public"), Some("has space"))
            .await
            .expect_err("unsafe key");
        assert_eq!(bad.0, StatusCode::BAD_REQUEST);

        // 另一个用户用同一个键是另一件事。
        let carol = publish_content(3, false, "carol", db, &note("public"), Some("act-1"))
            .await
            .expect("keys are per user");
        assert_ne!(carol.activity_id, first.activity_id);

        // 并发的两次重试：唯一约束挡住后到的，它回放先提交的那次。
        let racer = note("public");
        let (a, b) = tokio::join!(
            publish_content(1, false, "alice", db, &racer, Some("act-2")),
            publish_content(1, false, "alice", db, &racer, Some("act-2")),
        );
        let (a, b) = (a.expect("racer a"), b.expect("racer b"));
        assert_eq!(a.activity_id, b.activity_id);

        assert_eq!(
            count(
                db,
                "SELECT COUNT(*) FROM federation_published_content WHERE user_id = 1"
            )
            .await,
            2
        );
        assert_eq!(
            count(
                db,
                "SELECT COUNT(*) FROM federation_activities \
                 WHERE user_id = 1 AND activity_type = 'Create'"
            )
            .await,
            2
        );
        let first_id = &first.activity_id;
        assert_eq!(
            count(
                db,
                &format!(
                    "SELECT COUNT(*) FROM federation_delivery_queue q \
                     JOIN federation_activities a ON a.id = q.activity_id \
                     WHERE a.activity_id = '{first_id}'"
                )
            )
            .await,
            1
        );

        fixture.close().await;
    }

    #[test]
    fn unpublish_content_id_lookup_uses_all_rows() {
        let src = include_str!("publish.rs");
        assert!(src.contains("query_all_raw"));
        assert!(src.contains("unique_unpublish_row"));
        // Built at runtime so this assertion's own literal is not the needle it
        // forbids (include_str! would otherwise always match it).
        let legacy = ["`LIMIT 2`", " then ", "`query_one_raw`"].concat();
        assert!(!src.contains(&legacy));
    }
}
use myriad_error::AppError;
