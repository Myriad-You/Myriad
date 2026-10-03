//! Outbox 端点
//!
//! 用户的 Outbox — AP 兼容的活动历史
//! `GET /users/{username}/outbox` 返回 OrderedCollection 摘要
//! `GET /users/{username}/outbox?page=1`（第一页）/ `?cursor=…` 返回 OrderedCollectionPage

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use myriad_error::AppError;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::federation::content::ContentKind;
use crate::federation::types::*;

const OUTBOX_PAGE_SIZE: i64 = 20;

/// 公开 Activity/Object 的共同可见性投影。
///
/// `federation_activities` 是**通用**联邦活动表：Follow/Accept、房间邀请、
/// 频道消息、密钥交换、Ring 同步、文件分块都写在这里，且 `is_local = true`。
///
/// fail-closed 投影：只有**同时**满足以下三条的活动才会出现 ——
/// 1. `a.is_local = true`；
/// 2. activity 类型在下面的白名单里；
/// 3. 在 `federation_published_content` 里有一条 `visibility = 'public'` 记录。
///
/// 任何新增的活动类型默认不可见，必须显式登记成公开内容。Outbox 摘要、分页、
/// `/activities/{id}` 以及下方的内容对象解引用都必须复用这段投影，避免某个
/// 路径绕过另一个路径的隐私边界。
const PUBLIC_CONTENT_PROJECTION: &str = r#"
    FROM federation_activities a
    JOIN federation_published_content p ON p.activity_id = a.activity_id
    WHERE a.is_local = true
      AND a.activity_type IN ('Create', 'Announce')
      AND p.visibility = 'public'
"#;

/// Newest public local objects of one content kind, as
/// `(activity_id, object_json)`. Ring gossip and
/// every other re-publisher go through here so that withdrawn or
/// followers-only content never leaves through a side door.
pub(crate) async fn public_local_objects(
    db: &impl ConnectionTrait,
    kind: ContentKind,
    limit: i64,
) -> Result<Vec<(String, Value)>, sea_orm::DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT a.activity_id, a.object_json {PUBLIC_CONTENT_PROJECTION} \
                   AND a.activity_type = 'Create' AND p.content_type = $1 \
                 ORDER BY a.published_at DESC LIMIT $2"
            ),
            [kind.as_str().into(), limit.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            Some((
                row.try_get::<String>("", "activity_id").ok()?,
                row.try_get::<Value>("", "object_json").ok()?,
            ))
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct OutboxQuery {
    /// 只标记「第一页」：`first` 生成 `?page=1`，之后的 `next` 一律用游标。
    /// 值不参与定位，所以任何写法（`?page=1`、Mastodon 的 `?page=true`）都收。
    pub page: Option<String>,
    /// Keyset 游标：`<published_at 微秒>.<activity id>`。
    pub cursor: Option<String>,
}

/// 一页的定位方式。
#[derive(Debug, PartialEq, Eq)]
enum PagePosition {
    /// 从最新一条开始。
    Start,
    /// 严格早于该 `(published_at, id)` 的条目。
    After { published_us: i64, id: i32 },
}

/// 解析 `?cursor=<micros>.<id>`。
///
/// 格式不合法一律当作"从头开始"而不是报错：游标是我们自己生成的不透明串，
/// 远端把它截断或改坏时，给出第一页比返回 400 更符合 AP 的爬取语义。
fn parse_cursor(raw: &str) -> PagePosition {
    let Some((ts, id)) = raw.split_once('.') else {
        return PagePosition::Start;
    };
    match (ts.parse::<i64>(), id.parse::<i32>()) {
        (Ok(published_us), Ok(id)) => PagePosition::After { published_us, id },
        _ => PagePosition::Start,
    }
}

/// 生成下一页游标。
fn encode_cursor(published_us: i64, id: i32) -> String {
    format!("{published_us}.{id}")
}

/// GET /users/{username}/outbox
///
/// - 无参数：返回 `OrderedCollection` 摘要（`totalItems` + `first`）
/// - `?cursor=…`：keyset 分页，`next` 链接都是这种形态
/// - `?page=…`：第一页（`first` 生成 `?page=1`）；之后 `next` 用游标
///
/// keyset 用 `(published_at, id)` 作游标。
/// `COUNT(*)` 只在无分页参数的摘要文档跑一次。
pub async fn get_outbox(
    State(db): State<DatabaseConnection>,
    Path(username): Path<String>,
    Query(query): Query<OutboxQuery>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let (user_id, _) = get_local_user(&db, &username).await?;
    let outbox_id = outbox_url(&base_url, &username);

    // 无任何分页参数 → 摘要文档。COUNT 只在这里跑一次。
    if query.page.is_none() && query.cursor.is_none() {
        let total: i64 = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                format!(
                    "SELECT COUNT(*) as count {} AND a.user_id = $1",
                    PUBLIC_CONTENT_PROJECTION
                ),
                [user_id.into()],
            ))
            .await
            .map_err(db_err)?
            .map(|r| r.try_get::<i64>("", "count").unwrap_or(0))
            .unwrap_or(0);

        let collection = OrderedCollection {
            context: build_ap_context(),
            collection_type: "OrderedCollection".to_string(),
            id: outbox_id.clone(),
            total_items: total.max(0) as u64,
            first: (total > 0).then(|| format!("{}?page=1", outbox_id)),
            // keyset 下没有可直接跳转的"最后一页"。AS2 不要求 `last`，
            // 爬虫按 `first` → `next` 遍历即可。
            last: None,
        };
        return Ok(crate::federation::http_cache::public_ap_document(
            &headers,
            AP_CONTENT_TYPE,
            serde_json::to_value(collection).unwrap(),
        ));
    }

    // 定位这一页的起点
    let position = match query.cursor.as_deref() {
        Some(raw) => parse_cursor(raw),
        None => PagePosition::Start,
    };

    // 多取一条用来判断"还有没有下一页"，省掉一次 COUNT
    let fetch = OUTBOX_PAGE_SIZE + 1;
    let rows = match position {
        PagePosition::Start => {
            db.query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                format!(
                    "SELECT a.object_json, a.id, a.published_at {} \
                     AND a.user_id = $1 \
                     ORDER BY a.published_at DESC, a.id DESC LIMIT $2",
                    PUBLIC_CONTENT_PROJECTION
                ),
                [user_id.into(), fetch.into()],
            ))
            .await
        }
        PagePosition::After { published_us, id } => {
            let ts = chrono::DateTime::from_timestamp_micros(published_us)
                .unwrap_or_else(chrono::Utc::now);
            db.query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                format!(
                    "SELECT a.object_json, a.id, a.published_at {} \
                       AND a.user_id = $1 \
                       AND (a.published_at, a.id) < ($2, $3) \
                     ORDER BY a.published_at DESC, a.id DESC LIMIT $4",
                    PUBLIC_CONTENT_PROJECTION
                ),
                [user_id.into(), ts.into(), id.into(), fetch.into()],
            ))
            .await
        }
    }
    .map_err(db_err)?;

    let has_more = rows.len() as i64 > OUTBOX_PAGE_SIZE;
    let page_rows = &rows[..rows.len().min(OUTBOX_PAGE_SIZE as usize)];

    let items: Vec<serde_json::Value> = page_rows
        .iter()
        .map(|r| {
            r.try_get::<serde_json::Value>("", "object_json")
                .unwrap_or(json!({}))
        })
        .collect();

    // 下一页游标 = 本页最后一条的 (published_at, id)
    let next = has_more
        .then(|| page_rows.last())
        .flatten()
        .and_then(|last| {
            let id: i32 = last.try_get("", "id").ok()?;
            let ts: chrono::DateTime<chrono::FixedOffset> =
                last.try_get("", "published_at").ok()?;
            Some(format!(
                "{}?cursor={}",
                outbox_id,
                encode_cursor(ts.timestamp_micros(), id)
            ))
        });

    let page_id = match query.cursor.as_deref() {
        Some(c) => format!("{}?cursor={}", outbox_id, c),
        None => format!("{}?page=1", outbox_id),
    };

    let page_doc = OrderedCollectionPage {
        context: build_ap_context(),
        collection_type: "OrderedCollectionPage".to_string(),
        id: page_id,
        part_of: outbox_id,
        // 省略：取一页不该再跑一次全表 COUNT（AS2 允许，摘要里已有总数）
        total_items: None,
        ordered_items: items,
        next,
        // keyset 是单向游标，没有可靠的 `prev`。AS2 不要求它。
        prev: None,
    };

    Ok(crate::federation::http_cache::public_ap_document(
        &headers,
        AP_CONTENT_TYPE,
        serde_json::to_value(page_doc).unwrap(),
    ))
}

/// GET /activities/{id}
///
/// 解引用单条公开活动。
///
/// `generate_activity_id` 生成 `{base_url}/activities/{uuid}`；本 handler 按同一投影解引用。
///
/// 可见性规则与 Outbox 完全一致（同一个投影），所以这里不会成为绕过
/// Outbox 过滤的旁路。
pub async fn get_activity(
    State(db): State<DatabaseConnection>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let activity_id = format!("{}/activities/{}", base_url.trim_end_matches('/'), id);

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT a.object_json {} AND a.activity_id = $1",
                PUBLIC_CONTENT_PROJECTION
            ),
            [activity_id.clone().into()],
        ))
        .await
        .map_err(db_err)?;

    let Some(row) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Activity not found")),
        ));
    };

    let object_json: serde_json::Value = row
        .try_get::<serde_json::Value>("", "object_json")
        .unwrap_or(json!({}));

    Ok(crate::federation::http_cache::public_ap_document(
        &headers,
        AP_CONTENT_TYPE,
        object_json,
    ))
}

// The object URLs embedded in public Create activities are dereferenceable
// documents in their own right.  Keep their projection deliberately narrow:
// the only source is the same local Create/Announce + public
// `federation_published_content` join used by the Outbox and Activity
// handlers.  Do not rebuild an object from its backing MFP table here; doing
// so would make an unpublished/private row reachable by guessing its id.

/// GET /notes/{id}
pub async fn get_note(
    State(db): State<DatabaseConnection>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    get_public_object(&db, ContentKind::Note, id, headers).await
}

/// GET /reports/{id}
pub async fn get_report(
    State(db): State<DatabaseConnection>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    get_public_object(&db, ContentKind::Report, id, headers).await
}

/// GET /phantasi/articles/{id}
pub async fn get_phantasi_article(
    State(db): State<DatabaseConnection>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    get_public_object(&db, ContentKind::PhantasiArticle, id, headers).await
}

/// GET /tapps/{id}
pub async fn get_tapp(
    State(db): State<DatabaseConnection>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    get_public_object(&db, ContentKind::Tapp, id, headers).await
}

/// GET /library/{id}
pub async fn get_library(
    State(db): State<DatabaseConnection>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    get_public_object(&db, ContentKind::Library, id, headers).await
}

async fn get_public_object(
    db: &DatabaseConnection,
    kind: ContentKind,
    raw_id: String,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let id = raw_id.trim();
    // Non-tapp object ids are emitted as literal path segments.  Reject
    // decoded separators instead of allowing a caller to make one kind's
    // handler reinterpret a path intended for another route.  Tapp ids are
    // percent-encoded by the publisher, so a decoded separator is re-encoded
    // by `object_url` below.
    if id.is_empty()
        || (kind != ContentKind::Tapp && id.chars().any(|c| matches!(c, '/' | '?' | '#')))
    {
        return Err(object_not_found());
    }

    let base_url = get_base_url().await;
    let object_id = kind.object_url(&base_url, id);
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT a.object_json {} \\
                 AND p.content_type = $1 \\
                 AND (a.object_json -> 'object' ->> 'id') = $2 \\
                 LIMIT 1",
                PUBLIC_CONTENT_PROJECTION
            ),
            [kind.as_str().into(), object_id.clone().into()],
        ))
        .await
        .map_err(db_err)?;

    let Some(row) = row else {
        return Err(object_not_found());
    };

    // Content objects are embedded under a Create envelope.  Returning only
    // that object (never the envelope) keeps the `/notes/...` family aligned
    // with the ids advertised to remote instances and prevents unrelated MFP
    // activity fields from leaking through an object URL.
    let root: serde_json::Value = row
        .try_get::<serde_json::Value>("", "object_json")
        .unwrap_or_else(|_| json!({}));
    let Some(object) = root.get("object").filter(|value| value.is_object()) else {
        return Err(object_not_found());
    };
    if !public_object_matches(kind, object, object_id.as_str()) {
        return Err(object_not_found());
    }

    Ok(crate::federation::http_cache::public_ap_document(
        &headers,
        AP_CONTENT_TYPE,
        object.clone(),
    ))
}

fn object_not_found() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(AppError::public_json("Object not found")),
    )
}

fn public_object_matches(kind: ContentKind, object: &serde_json::Value, object_id: &str) -> bool {
    object.get("id").and_then(|value| value.as_str()) == Some(object_id)
        && object.get("type").and_then(|value| value.as_str())
            == Some(kind.ap_type())
        // `mfp:contentType` 缺省视为匹配；有值则必须等于 published-content type。
        && object
            .get("mfp:contentType")
            .and_then(|value| value.as_str())
            .map(|value| value == kind.as_str())
            .unwrap_or(true)
}

// 辅助函数

async fn get_base_url() -> String {
    crate::federation::types::get_base_url().await
}

async fn get_local_user(
    db: &sea_orm::DatabaseConnection,
    username: &str,
) -> Result<(i32, String), (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, username FROM users WHERE username = $1 LIMIT 1",
            [username.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("User not found")),
            )
        })?;

    let id = row_positive_id(&row, "id").map_err(|error| {
        db_err(sea_orm::DbErr::Custom(error))
    })?;
    let username: String = row.try_get("", "username").map_err(db_err)?;
    if username.is_empty() {
        return Err(db_err(sea_orm::DbErr::Custom(
            "user username is empty".into(),
        )));
    }
    Ok((id, username))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cursor_roundtrips() {
        let c = encode_cursor(1_738_000_000_000_000, 42);
        assert_eq!(c, "1738000000000000.42");
        assert_eq!(
            parse_cursor(&c),
            PagePosition::After {
                published_us: 1_738_000_000_000_000,
                id: 42
            }
        );
    }

    /// 游标是我们生成的不透明串。远端把它截断/改坏时给第一页，
    /// 而不是 400 —— 爬虫遇到 400 会整个放弃这个 Outbox。
    #[test]
    fn malformed_cursor_falls_back_to_start() {
        for bad in [
            "", "garbage", "123", ".", "abc.def", "123.", ".456", "1.2.3",
        ] {
            assert_eq!(
                parse_cursor(bad),
                PagePosition::Start,
                "{bad:?} should fall back to the first page"
            );
        }
    }

    #[test]
    fn negative_timestamps_are_accepted() {
        // 1970 之前的 published_at 不该被当成畸形游标
        assert_eq!(
            parse_cursor("-1000.7"),
            PagePosition::After {
                published_us: -1000,
                id: 7
            }
        );
    }

    /// 分页文档不带 totalItems —— 这正是省掉每页 COUNT(*) 的体现。
    #[test]
    fn page_document_omits_total_items() {
        let page = OrderedCollectionPage {
            context: build_ap_context(),
            collection_type: "OrderedCollectionPage".to_string(),
            id: "https://x/outbox?cursor=1.2".into(),
            part_of: "https://x/outbox".into(),
            total_items: None,
            ordered_items: vec![],
            next: None,
            prev: None,
        };
        let v = serde_json::to_value(&page).unwrap();
        assert!(
            v.get("totalItems").is_none(),
            "pages must not carry totalItems; that would force a COUNT(*) per page"
        );
        assert_eq!(v["type"], "OrderedCollectionPage");
    }

    #[test]
    fn private_or_unpublished_rows_are_excluded_by_public_projection() {
        assert!(PUBLIC_CONTENT_PROJECTION.contains("JOIN federation_published_content"));
        assert!(PUBLIC_CONTENT_PROJECTION.contains("a.is_local = true"));
        assert!(PUBLIC_CONTENT_PROJECTION.contains("a.activity_type IN ('Create', 'Announce')"));
        assert!(PUBLIC_CONTENT_PROJECTION.contains("p.visibility = 'public'"));
    }

    #[test]
    fn public_object_projection_rejects_cross_type_and_not_found_objects() {
        let id = "https://example.test/notes/n-1";
        let note = json!({
            "type": "Note",
            "id": id,
            "mfp:contentType": "note"
        });
        assert!(public_object_matches(ContentKind::Note, &note, id));
        assert!(!public_object_matches(ContentKind::Report, &note, id));
        assert!(!public_object_matches(
            ContentKind::Note,
            &note,
            "https://example.test/notes/missing"
        ));

        let stale_cross_type = json!({
            "type": "Note",
            "id": id,
            "mfp:contentType": "report"
        });
        assert!(!public_object_matches(
            ContentKind::Note,
            &stale_cross_type,
            id
        ));
    }
}
