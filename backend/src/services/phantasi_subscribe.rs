//! 新建订阅源的唯一入口：判定「是否已订阅」并在同一把锁下插入。
//!
//! 手动添加、OPML 导入、友链审核通过、Agent 订阅都走 [`create_or_find_source`]，
//! 去重规则只有一条：新源的身份 URL（`url`，友链再加申请站点）规范化成
//! `url_match_key` 后，任何 `url_key` 或 `site_url_key` 命中的已有源都算同一个。
//!
//! 并发创建靠事务级 advisory 锁串行：锁住新行将暴露的全部键（身份键 + 自身
//! `site_url` 键）再查再插，两个等价的并发创建必然有一个看见另一个。
//! `url_key` 另有全站 UNIQUE 索引兜底（存量重复由启动 heal 合并）：绕过锁的
//! 写入撞上它时，插入退回到重新查询。

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, Condition, ConnectionTrait, DatabaseBackend,
    DatabaseTransaction, DbErr, EntityTrait, QueryFilter, QueryOrder, Statement, TransactionTrait,
};

use crate::models::entities::phantasi_sources::{self, url_match_key};

/// 待创建的订阅源。
pub(crate) struct NewSource {
    model: phantasi_sources::ActiveModel,
    /// `url` 之外同样标识这份订阅的 URL。友链审核传申请站点，同站已有入口即复用。
    identity_urls: Vec<String>,
}

/// 创建结果：新插入的行，或按去重规则命中的已有行。
pub(crate) enum SourceCreation {
    Created(phantasi_sources::Model),
    Existing(phantasi_sources::Model),
}

impl NewSource {
    pub(crate) fn new(model: phantasi_sources::ActiveModel) -> Self {
        Self {
            model,
            identity_urls: Vec::new(),
        }
    }

    pub(crate) fn also_identified_by(mut self, url: impl Into<String>) -> Self {
        self.identity_urls.push(url.into());
        self
    }

    fn url(&self) -> Result<&str, DbErr> {
        match &self.model.url {
            ActiveValue::Set(url) | ActiveValue::Unchanged(url) => Ok(url),
            ActiveValue::NotSet => Err(DbErr::Custom("new phantasi source needs a url".into())),
        }
    }

    /// 去重查询用的键：`url` 与额外身份 URL。
    fn identity_keys(&self) -> Result<Vec<String>, DbErr> {
        let mut keys = vec![url_match_key(self.url()?)];
        for url in &self.identity_urls {
            push_unique(&mut keys, url_match_key(url));
        }
        Ok(keys)
    }

    /// 加锁用的键：身份键再加新行自己的 `site_url` 键。后者不参与本次去重，
    /// 但会写进 `site_url_key`，并发创建者可能正按它去重。
    fn lock_keys(&self) -> Result<Vec<String>, DbErr> {
        let mut keys = self.identity_keys()?;
        if let ActiveValue::Set(Some(site)) | ActiveValue::Unchanged(Some(site)) =
            &self.model.site_url
        {
            push_unique(&mut keys, url_match_key(site));
        }
        Ok(keys)
    }
}

fn push_unique(keys: &mut Vec<String>, key: String) {
    if !keys.contains(&key) {
        keys.push(key);
    }
}

pub(crate) fn source_url_lock_key(match_key: &str) -> String {
    format!("myriad:phantasi:source_url:{match_key}")
}

/// 按固定顺序拿事务级 advisory 锁，事务结束自动释放。必须在事务里调用。
pub(crate) async fn lock_source_url_keys<C: ConnectionTrait>(
    db: &C,
    keys: &[String],
) -> Result<(), DbErr> {
    let mut sorted = keys.to_vec();
    sorted.sort();
    sorted.dedup();
    for key in sorted {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            [source_url_lock_key(&key).into()],
        ))
        .await?;
    }
    Ok(())
}

/// 任何 `url_key` 或 `site_url_key` 落在 `keys` 里的已有源（最早那条）。
pub(crate) async fn find_existing_source<C: ConnectionTrait>(
    db: &C,
    keys: &[String],
) -> Result<Option<phantasi_sources::Model>, DbErr> {
    if keys.is_empty() {
        return Ok(None);
    }
    phantasi_sources::Entity::find()
        .filter(
            Condition::any()
                .add(phantasi_sources::Column::UrlKey.is_in(keys.iter().map(String::as_str)))
                .add(phantasi_sources::Column::SiteUrlKey.is_in(keys.iter().map(String::as_str))),
        )
        .order_by_asc(phantasi_sources::Column::Id)
        .one(db)
        .await
}

/// 不加锁的预检，给要先联网探测的路径省掉一次无谓抓取。结论以
/// [`create_or_find_source`] 为准。
pub(crate) async fn find_subscribed<C: ConnectionTrait>(
    db: &C,
    url: &str,
) -> Result<Option<phantasi_sources::Model>, DbErr> {
    find_existing_source(db, &[url_match_key(url)]).await
}

/// 规范化键 → advisory 锁 → 按 `url_key`/`site_url_key` 查 → 插入。
/// 调用方负责提交事务；锁持有到事务结束，提交前别做联网等慢操作。
pub(crate) async fn create_or_find_source(
    txn: &DatabaseTransaction,
    new: NewSource,
) -> Result<SourceCreation, DbErr> {
    let identity_keys = new.identity_keys()?;
    lock_source_url_keys(txn, &new.lock_keys()?).await?;
    if let Some(existing) = find_existing_source(txn, &identity_keys).await? {
        return Ok(SourceCreation::Existing(existing));
    }
    // 保存点包住插入：唯一冲突只回滚这一步，调用方的事务还能重新查询。
    let savepoint = txn.begin().await?;
    match new.model.insert(&savepoint).await {
        Ok(created) => {
            savepoint.commit().await?;
            Ok(SourceCreation::Created(created))
        }
        Err(error) if crate::federation::types::is_unique_violation(&error) => {
            savepoint.rollback().await?;
            find_existing_source(txn, &identity_keys)
                .await?
                .map(SourceCreation::Existing)
                .ok_or(error)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::entities::phantasi_sources::{FeedType, SourceType};
    use sea_orm::{ActiveValue::Set, DatabaseConnection, PaginatorTrait, TransactionTrait};

    fn model(url: &str, site_url: Option<&str>) -> phantasi_sources::ActiveModel {
        let now = crate::clock::now();
        phantasi_sources::ActiveModel {
            user_id: Set(1),
            name: Set(url.to_string()),
            url: Set(url.to_string()),
            feed_type: Set(FeedType::Rss),
            source_type: Set(SourceType::Rss),
            site_url: Set(site_url.map(str::to_string)),
            update_interval: Set(30),
            enabled: Set(true),
            error_count: Set(0),
            item_count: Set(0),
            admin_only: Set(false),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        }
    }

    async fn create(db: &DatabaseConnection, new: NewSource) -> SourceCreation {
        let txn = db.begin().await.unwrap();
        let created = create_or_find_source(&txn, new).await.unwrap();
        txn.commit().await.unwrap();
        created
    }

    #[test]
    fn lock_keys_cover_identity_and_own_site() {
        let new = NewSource::new(model(
            "https://Blog.example/feed/",
            Some("https://blog.example/"),
        ))
        .also_identified_by("https://friend.example");
        assert_eq!(
            new.identity_keys().unwrap(),
            vec!["https://blog.example/feed", "https://friend.example"]
        );
        assert_eq!(
            new.lock_keys().unwrap(),
            vec![
                "https://blog.example/feed",
                "https://friend.example",
                "https://blog.example"
            ]
        );
        assert_eq!(
            source_url_lock_key("https://example.com/blog"),
            "myriad:phantasi:source_url:https://example.com/blog"
        );
    }

    #[tokio::test]
    async fn concurrent_equivalent_creates_yield_one_row() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = fixture.db.clone();
        db.execute_unprepared(
            "INSERT INTO users (id, username) VALUES (1, 'phantasi-subscribe') ON CONFLICT DO NOTHING",
        )
        .await
        .unwrap();

        // Spellings that normalize to one key, racing on four connections.
        let spellings = [
            "https://Race.example/feed",
            "https://race.example/feed/",
            "https://race.example/feed#top",
            "https://RACE.example/feed//",
        ];
        let results = futures::future::join_all(
            spellings
                .iter()
                .map(|url| create(&db, NewSource::new(model(url, None)))),
        )
        .await;
        let created = results
            .iter()
            .filter(|r| matches!(r, SourceCreation::Created(_)))
            .count();
        assert_eq!(created, 1, "exactly one racer inserts");
        assert_eq!(
            phantasi_sources::Entity::find()
                .filter(phantasi_sources::Column::UrlKey.eq("https://race.example/feed"))
                .count(&db)
                .await
                .unwrap(),
            1
        );

        // A feed whose site is a new link entry, created concurrently with that entry:
        // the link's identity key is the feed's own site key, so they serialize and
        // the later one sees the earlier.
        let (feed, link) = tokio::join!(
            create(
                &db,
                NewSource::new(model(
                    "https://site.example/rss",
                    Some("https://site.example")
                ))
            ),
            create(&db, NewSource::new(model("https://site.example/", None)))
        );
        let rows = phantasi_sources::Entity::find()
            .filter(
                Condition::any()
                    .add(phantasi_sources::Column::UrlKey.eq("https://site.example"))
                    .add(phantasi_sources::Column::SiteUrlKey.eq("https://site.example")),
            )
            .count(&db)
            .await
            .unwrap();
        match (&feed, &link) {
            // Feed first: the link finds it by site_url_key.
            (SourceCreation::Created(_), SourceCreation::Existing(found)) => {
                assert_eq!(found.url, "https://site.example/rss");
                assert_eq!(rows, 1);
            }
            // Link first: a feed of that site is a different subscription.
            (SourceCreation::Created(_), SourceCreation::Created(_)) => assert_eq!(rows, 2),
            _ => panic!("the feed is never a duplicate of a bare site link"),
        }

        // Two feeds of one site are different subscriptions.
        let second = create(
            &db,
            NewSource::new(model(
                "https://site.example/comments/rss",
                Some("https://site.example"),
            )),
        )
        .await;
        assert!(matches!(second, SourceCreation::Created(_)));

        drop(db);
        fixture.close().await;
    }
}
