//! Outbox 端点（Layer 2）
//!
//! 用户的 Outbox — AP 兼容的活动历史
//! `GET /users/{username}/outbox` 返回 OrderedCollection 摘要
//! `GET /users/{username}/outbox?page=N` 返回 OrderedCollectionPage

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::Deserialize;
use serde_json::json;

use crate::federation::types::*;

const OUTBOX_PAGE_SIZE: i64 = 20;

/// Outbox 的可见性投影。
///
/// `federation_activities` 是**通用**联邦活动表：Follow/Accept、房间邀请、
/// 频道消息、密钥交换、Ring 同步、文件分块都写在这里，且 `is_local = true`。
/// 过去 Outbox 直接按 `user_id + is_local` 全表返回 `object_json`，等于把整个
/// 内部控制面匿名公开。
///
/// 现在改成 fail-closed 投影：只有**同时**满足以下两条的活动才会出现 ——
/// 1. activity 类型在下面的白名单里；
/// 2. 在 `federation_published_content` 里有一条 `visibility = 'public'` 记录。
///
/// 任何新增的活动类型默认不可见，必须显式登记成公开内容。
const PUBLIC_OUTBOX_FILTER: &str = r#"
    FROM federation_activities a
    JOIN federation_published_content p ON p.activity_id = a.activity_id
    WHERE a.user_id = $1
      AND a.is_local = true
      AND a.activity_type IN ('Create', 'Announce')
      AND p.visibility = 'public'
"#;

#[derive(Debug, Deserialize)]
pub struct OutboxQuery {
    /// 传统页码。仍然接受（远端可能缓存过这类 URL），但我们只在 `first`
    /// 里生成 `?page=1`；之后的 `next` 一律用游标。
    pub page: Option<u32>,
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
/// - `?page=N`：传统页码，仍然接受（远端可能缓存过），但我们不再生成
///
/// # 为什么不再用 `COUNT(*) + OFFSET`
///
/// 旧实现每次取页都跑一遍全表 `COUNT(*)`，并用 `OFFSET` 跳过前面的行：
///
/// - 代价随历史增长线性上升，翻到第 N 页要扫过前 N×20 行；
/// - **翻页不稳定** —— 爬取过程中有新内容发布，后续页的 OFFSET 会整体位移，
/// 远端要么漏掉条目、要么重复收到。
///
/// keyset 用 `(published_at, id)` 作游标：每页代价恒定，且新内容只会出现在
/// 游标之前，不会挪动已经翻过的窗口。
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
                format!("SELECT COUNT(*) as count {}", PUBLIC_OUTBOX_FILTER),
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
    // 传统 ?page=N 仍走 OFFSET —— 只为兼容已缓存的 URL，我们自己不生成。
    let legacy_offset = match (&position, query.page) {
        (PagePosition::Start, Some(p)) => (p.max(1) as i64 - 1) * OUTBOX_PAGE_SIZE,
        _ => 0,
    };

    // 多取一条用来判断"还有没有下一页"，省掉一次 COUNT
    let fetch = OUTBOX_PAGE_SIZE + 1;
    let rows = match position {
        PagePosition::Start => {
            db.query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                format!(
                    "SELECT a.object_json, a.id, a.published_at {} \
                     ORDER BY a.published_at DESC, a.id DESC LIMIT $2 OFFSET $3",
                    PUBLIC_OUTBOX_FILTER
                ),
                [user_id.into(), fetch.into(), legacy_offset.into()],
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
                       AND (a.published_at, a.id) < ($2, $3) \
                     ORDER BY a.published_at DESC, a.id DESC LIMIT $4",
                    PUBLIC_OUTBOX_FILTER
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
        None => format!("{}?page={}", outbox_id, query.page.unwrap_or(1).max(1)),
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
/// `generate_activity_id` 一直在生成 `{base_url}/activities/{uuid}` 形态的 id，
/// 但从来没有对应的 GET 路由 —— 远端拿到一条 Create 之后无法回查验证，
/// 转发/引用这条活动的实例也解析不出内容。
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
            r#"SELECT a.object_json
               FROM federation_activities a
               JOIN federation_published_content p ON p.activity_id = a.activity_id
               WHERE a.activity_id = $1
                 AND a.is_local = true
                 AND a.activity_type IN ('Create', 'Announce')
                 AND p.visibility = 'public'"#,
            [activity_id.clone().into()],
        ))
        .await
        .map_err(db_err)?;

    let Some(row) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Activity not found"})),
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
                Json(json!({"error": "User not found"})),
            )
        })?;

    Ok((
        row.try_get("", "id").unwrap_or(0),
        row.try_get("", "username").unwrap_or_default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
