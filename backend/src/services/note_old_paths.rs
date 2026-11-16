//! 搬家来的文章的旧地址：存、查、跳。
//!
//! 旧地址存在 `phantasi_items.old_paths`（规范化后的路径数组）。访客打开一条旧地址时，
//! 代理和 `/blog/` 下的 404 都来问 [`redirect_target`]：命中就 301 到这篇现在的博客地址。
//! 目标按当前 slug 现算，改了 slug 旧地址照样跳到新地址。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use myriad_phantasi_notes::{normalize_old_paths, old_path_lookup_key};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DbErr, EntityTrait, QuerySelect, Statement, prelude::Json,
};
use serde_json::json;

use super::phantasi_error::{phantasi_http_err, phantasi_store_http};
use crate::error::HttpError;
use crate::models::entities::phantasi_items;

/// 写旧地址时串行：查重和写入之间不能插进另一篇，否则两篇能同时占住同一条。
const OLD_PATHS_LOCK_ID: i64 = 0x4F4C_4450_4154_4853;
/// 多实例时别的实例的改动传不过来，表最多旧这么久。本实例的改动当场失效。
const TABLE_TTL: Duration = Duration::from_secs(300);

/// 旧地址 → (文章 id, 当前 slug)。
type RedirectMap = HashMap<String, (i32, Option<String>)>;

struct Table {
    generation: u64,
    loaded_at: Instant,
    map: Arc<RedirectMap>,
}

static TABLE: RwLock<Option<Table>> = RwLock::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// 发布、保存旧地址、删除笔记、改 slug 之后调用（博客重建的入口也会调用）。
pub fn invalidate_old_path_cache() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
    if let Ok(mut slot) = TABLE.write() {
        *slot = None;
    }
}

pub(crate) fn paths_from_json(value: Option<&Json>) -> Vec<String> {
    value
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) async fn old_paths_of_item<C: ConnectionTrait>(
    db: &C,
    item_id: Option<i32>,
) -> Result<Vec<String>, HttpError> {
    let Some(item_id) = item_id else {
        return Ok(Vec::new());
    };
    let value = phantasi_items::Entity::find_by_id(item_id)
        .select_only()
        .column(phantasi_items::Column::OldPaths)
        .into_tuple::<Option<Json>>()
        .one(db)
        .await
        .map_err(|error| phantasi_store_http("read note old addresses", error))?;
    Ok(paths_from_json(value.flatten().as_ref()))
}

/// 换掉一篇已发布笔记的全部旧地址。规范化不过 400，被别的文章占着 409。
/// 返回 `(存下的列表, 是否有变化)`；必须在事务里调用（锁随事务释放）。
pub(crate) async fn set_note_old_paths<C: ConnectionTrait>(
    db: &C,
    item_id: i32,
    requested: &[String],
) -> Result<(Vec<String>, bool), HttpError> {
    let paths = normalize_old_paths(requested)
        .map_err(|rejection| phantasi_http_err(StatusCode::BAD_REQUEST, rejection.message()))?;
    let current = old_paths_of_item(db, Some(item_id)).await?;
    if current == paths {
        return Ok((paths, false));
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock($1)",
        [OLD_PATHS_LOCK_ID.into()],
    ))
    .await
    .map_err(|error| phantasi_store_http("lock note old addresses", error))?;
    if !paths.is_empty() {
        let taken = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"
                SELECT p.path
                FROM phantasi_items i
                CROSS JOIN LATERAL jsonb_array_elements_text(i.old_paths) AS p(path)
                WHERE i.id <> $1
                  AND jsonb_typeof(i.old_paths) = 'array'
                  AND p.path IN (SELECT jsonb_array_elements_text($2::jsonb))
                LIMIT 1
                "#,
                [item_id.into(), json!(paths).into()],
            ))
            .await
            .map_err(|error| phantasi_store_http("check note old addresses", error))?;
        if let Some(row) = taken {
            let path = row.try_get::<String>("", "path").unwrap_or_default();
            return Err(phantasi_http_err(
                StatusCode::CONFLICT,
                format!("The old address \"{path}\" already points to another note"),
            ));
        }
    }
    let stored = if paths.is_empty() {
        None
    } else {
        Some(json!(paths))
    };
    let updated = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE phantasi_items SET old_paths = $2 WHERE id = $1",
            [item_id.into(), stored.into()],
        ))
        .await
        .map_err(|error| phantasi_store_http("save note old addresses", error))?;
    if updated.rows_affected() == 0 {
        return Err(phantasi_http_err(StatusCode::NOT_FOUND, "Note not found"));
    }
    Ok((paths, true))
}

/// 这条请求路径是不是某篇笔记的旧地址；是就给出它现在的博客地址。
pub async fn redirect_target<C: ConnectionTrait>(
    db: &C,
    request_path: &str,
) -> Result<Option<String>, DbErr> {
    let Some(key) = old_path_lookup_key(request_path) else {
        return Ok(None);
    };
    let map = redirect_map(db).await?;
    Ok(map
        .get(&key)
        .map(|(id, slug)| post_location(*id, slug.as_deref())))
}

/// 和博客生成文章页时同一个规则：slug 不合法就用 `n{id}`。
fn post_location(item_id: i32, slug: Option<&str>) -> String {
    match slug.filter(|slug| myriad_blog::validate_slug(slug)) {
        Some(slug) => myriad_blog::note_blog_path(slug),
        None => myriad_blog::note_blog_path(&myriad_blog::fallback_slug(item_id)),
    }
}

async fn redirect_map<C: ConnectionTrait>(db: &C) -> Result<Arc<RedirectMap>, DbErr> {
    let generation = GENERATION.load(Ordering::Acquire);
    if let Ok(slot) = TABLE.read()
        && let Some(table) = slot.as_ref()
        && table.generation == generation
        && table.loaded_at.elapsed() < TABLE_TTL
    {
        return Ok(table.map.clone());
    }
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
            SELECT i.id, i.slug, p.path
            FROM phantasi_items i
            INNER JOIN phantasi_sources s ON s.id = i.source_id
            CROSS JOIN LATERAL jsonb_array_elements_text(i.old_paths) AS p(path)
            WHERE s.source_type = 'note'
              AND jsonb_typeof(i.old_paths) = 'array'
            ORDER BY i.id
            "#,
        ))
        .await?;
    let mut map = RedirectMap::new();
    for row in rows {
        let (Ok(id), Ok(path)) = (
            row.try_get::<i32>("", "id"),
            row.try_get::<String>("", "path"),
        ) else {
            continue;
        };
        let slug = row.try_get::<Option<String>>("", "slug").ok().flatten();
        // 写入时已经查过重；万一有重复，先发布的那篇留着。
        map.entry(path).or_insert((id, slug));
    }
    let map = Arc::new(map);
    // 读库期间有人改过就不存：存了会把刚失效的旧表又放回去。
    if GENERATION.load(Ordering::Acquire) == generation
        && let Ok(mut slot) = TABLE.write()
    {
        *slot = Some(Table {
            generation,
            loaded_at: Instant::now(),
            map: map.clone(),
        });
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_follows_the_current_slug() {
        assert_eq!(post_location(7, Some("hello")), "/blog/posts/hello/");
        assert_eq!(post_location(7, None), "/blog/posts/n7/");
        assert_eq!(post_location(7, Some("Bad Slug")), "/blog/posts/n7/");
    }

    #[test]
    fn stored_json_reads_back_as_paths() {
        assert_eq!(
            paths_from_json(Some(&json!(["/a", 3, "/b"]))),
            vec!["/a".to_string(), "/b".to_string()]
        );
        assert!(paths_from_json(Some(&json!({"a": 1}))).is_empty());
        assert!(paths_from_json(None).is_empty());
    }

    async fn insert_note_item(db: &sea_orm::DatabaseConnection, slug: &str) -> i32 {
        use crate::models::entities::phantasi_items;
        use sea_orm::{ActiveModelTrait, Set};
        let now = crate::clock::now();
        phantasi_items::ActiveModel {
            source_id: Set(1),
            guid: Set(format!("old-path-note-{}", uuid::Uuid::new_v4())),
            title: Set(slug.into()),
            link: Set(myriad_blog::note_blog_path(slug)),
            slug: Set(Some(slug.into())),
            content_md: Set(Some("body".into())),
            published_at: Set(now.into()),
            fetched_at: Set(now.into()),
            fulltext_fetched: Set(true),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn set_in_txn(
        db: &sea_orm::DatabaseConnection,
        item_id: i32,
        paths: &[&str],
    ) -> Result<(Vec<String>, bool), HttpError> {
        use sea_orm::TransactionTrait;
        let txn = db.begin().await.unwrap();
        let paths: Vec<String> = paths.iter().map(|path| path.to_string()).collect();
        let result = set_note_old_paths(&txn, item_id, &paths).await;
        if result.is_ok() {
            txn.commit().await.unwrap();
            invalidate_old_path_cache();
        }
        result
    }

    /// 存、查重、查表、改 slug 后跟着走、删文后失效，一次跑完：跳转表是进程里唯一的一份。
    #[tokio::test]
    async fn old_addresses_store_conflict_lookup_and_follow_the_slug() {
        use crate::models::entities::{phantasi_items, phantasi_note_docs, phantasi_sources};
        use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, Set, TransactionTrait};
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "note_old_paths_test").await;
        let db = isolated.db.clone();
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (1, 'old-path-owner')")
            .await
            .unwrap();
        let now = crate::clock::now();
        phantasi_sources::ActiveModel {
            user_id: Set(1),
            name: Set("Notes".into()),
            url: Set("myriad:notes".into()),
            feed_type: Set(phantasi_sources::FeedType::Rss),
            source_type: Set(phantasi_sources::SourceType::Note),
            update_interval: Set(0),
            enabled: Set(true),
            error_count: Set(0),
            item_count: Set(2),
            admin_only: Set(false),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let first = insert_note_item(&db, "hello").await;
        let second = insert_note_item(&db, "other").await;

        let (stored, changed) = set_in_txn(
            &db,
            first,
            &["https://old.example/2024/05/hello/?p=1", "/文章/你好/", ""],
        )
        .await
        .unwrap();
        assert!(changed);
        assert_eq!(stored, vec!["/2024/05/hello", "/文章/你好"]);
        assert_eq!(old_paths_of_item(&db, Some(first)).await.unwrap(), stored);
        // 同样的一组再存一次什么也不改。
        assert!(
            !set_in_txn(&db, first, &["/2024/05/hello/", "/文章/你好"])
                .await
                .unwrap()
                .1
        );

        for path in [
            "/2024/05/hello",
            "/2024/05/hello/",
            "/%E6%96%87%E7%AB%A0/%E4%BD%A0%E5%A5%BD/",
        ] {
            assert_eq!(
                redirect_target(&db, path).await.unwrap().as_deref(),
                Some("/blog/posts/hello/"),
                "{path}"
            );
        }
        assert_eq!(redirect_target(&db, "/2024/05/other/").await.unwrap(), None);

        // 另一篇占不走同一条；站内地址和格式不对的不收。
        let taken = set_in_txn(&db, second, &["/x", "/2024/05/hello"])
            .await
            .unwrap_err();
        assert_eq!(taken.0.status(), StatusCode::CONFLICT);
        assert!(
            old_paths_of_item(&db, Some(second))
                .await
                .unwrap()
                .is_empty()
        );
        let reserved = set_in_txn(&db, second, &["/api/blog/redirect"])
            .await
            .unwrap_err();
        assert_eq!(reserved.0.status(), StatusCode::BAD_REQUEST);
        set_in_txn(&db, second, &["/2024/05/other/"]).await.unwrap();
        assert_eq!(
            redirect_target(&db, "/2024/05/other")
                .await
                .unwrap()
                .as_deref(),
            Some("/blog/posts/other/")
        );

        // 改 slug 走博客重建的入口，表跟着失效，旧地址跳到新 slug。
        let txn = db.begin().await.unwrap();
        crate::services::note_publish::set_published_note_slug(&txn, first, 1, "hello-again")
            .await
            .unwrap();
        txn.commit().await.unwrap();
        crate::services::blog::request_rebuild();
        assert_eq!(
            redirect_target(&db, "/2024/05/hello")
                .await
                .unwrap()
                .as_deref(),
            Some("/blog/posts/hello-again/")
        );

        // 清空后不再跳；删掉文章后也不再跳。
        set_in_txn(&db, second, &[]).await.unwrap();
        assert_eq!(redirect_target(&db, "/2024/05/other").await.unwrap(), None);
        phantasi_items::Entity::delete_by_id(first)
            .exec(&db)
            .await
            .unwrap();
        crate::services::blog::request_rebuild();
        assert_eq!(redirect_target(&db, "/2024/05/hello").await.unwrap(), None);

        // 草稿不能设旧地址。
        let draft = phantasi_note_docs::ActiveModel {
            user_id: Set(1),
            title: Set("Draft".into()),
            content_md: Set("body".into()),
            status: Set("draft".into()),
            revision: Set(1),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let request: crate::services::note_documents::NoteDocWriteRequest =
            serde_json::from_value(json!({"revision": 1, "old_paths": ["/a"]})).unwrap();
        let refused = crate::services::note_documents::save_note_doc(&db, 1, draft.id, request)
            .await
            .unwrap_err();
        assert_eq!(refused.0.status(), StatusCode::BAD_REQUEST);

        invalidate_old_path_cache();
        isolated.drop().await;
    }
}
