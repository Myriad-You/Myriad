//! Phantasi 阅读 - 订阅调度引擎
//!
//! 后端独立运行的调度器，负责：
//! 1. 定时检查需要更新的订阅源
//! 2. 抓取并解析订阅源内容
//! 3. 存储新文章到数据库
//! 4. 推送更新通知到前端

use chrono::Utc;
use futures::stream::{self, StreamExt};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseBackend, DatabaseConnection,
    EntityTrait, FromQueryResult, QueryFilter, QueryOrder, QuerySelect, Statement,
    TransactionTrait, Value as SeaValue, sea_query::OnConflict,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use crate::models::entities::{phantasi_items, phantasi_sources};
use crate::services::notion_service::{NotionConfig, NotionService};
use crate::services::phantasi_parser::{FeedParser, ParsedFeed, calculate_reading_stats};
use crate::services::rsshub_service::RsshubService;

fn phantasi_store_failed(context: &'static str, error: impl std::fmt::Display) -> String {
    tracing::error!(%error, context, "phantasi store failed");
    format!("Failed to {context}")
}

/// Both scheduled fetches and agent subscriptions write RSS media references
/// in the same transaction as their newly inserted items.
pub(crate) async fn insert_feed_items(
    db: &DatabaseConnection,
    items: Vec<phantasi_items::ActiveModel>,
) -> Result<Vec<phantasi_items::Model>, sea_orm::DbErr> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let origins = crate::services::media::upgrade::configured_origins().await;
    let txn = db.begin().await?;
    let inserted = match phantasi_items::Entity::insert_many(items)
        .on_conflict(
            OnConflict::columns([
                phantasi_items::Column::SourceId,
                phantasi_items::Column::Guid,
            ])
            .do_nothing()
            .to_owned(),
        )
        .exec_with_returning(&txn)
        .await
    {
        Ok(rows) => rows,
        Err(sea_orm::DbErr::RecordNotInserted) => Vec::new(),
        Err(error) => return Err(error),
    };
    for row in &inserted {
        let payload = serde_json::to_value(row)
            .map_err(|_| sea_orm::DbErr::Custom("Failed to serialize feed item".into()))?;
        crate::services::media::bind_rss_item(&txn, row.id, &payload, &origins)
            .await
            .map_err(|error| sea_orm::DbErr::Custom(error.code().into()))?;
    }
    txn.commit().await?;
    Ok(inserted)
}

// 调度器常量

/// 每轮 tick 最多处理的订阅源数量
/// 防止宕机恢复后一次性堆积大量请求
const MAX_SOURCES_PER_TICK: u64 = 50;

/// 并发抓取的最大并行数
/// 限制同时发出的 HTTP 请求数，避免网络/内存压力
const MAX_CONCURRENT_FETCHES: usize = 5;

/// 连续错误次数上限：到这里发一次通知；之后不再每轮 WARN，靠退避少抓。
const MAX_ERROR_COUNT: i32 = 10;

/// 连续失败开始退避的门槛（含）。前两次失败可能只是网络抖动，照常抓。
const BACKOFF_START_ERRORS: i32 = 3;
/// 退避上限：一天一次。死源不会被悄悄禁掉，但也不再每分钟去撞。
const BACKOFF_MAX_MINUTES: i64 = 24 * 60;

/// 连续失败越多，下一次抓取隔得越久：从第 3 次起每失败一次翻倍，最长一天。
pub(crate) fn retry_interval_minutes(update_interval: i32, error_count: i32) -> i64 {
    let base = i64::from(update_interval.max(1));
    if error_count < BACKOFF_START_ERRORS {
        return base;
    }
    let doublings = u32::try_from(error_count - BACKOFF_START_ERRORS + 1).unwrap_or(u32::MAX);
    base.saturating_mul(2_i64.saturating_pow(doublings.min(20)))
        .min(BACKOFF_MAX_MINUTES)
}

/// SQL twin of [`retry_interval_minutes`]. Must stay in the due WHERE, before LIMIT.
pub(crate) fn retry_interval_sql() -> String {
    format!(
        "CASE \
            WHEN error_count < {BACKOFF_START_ERRORS} THEN GREATEST(update_interval, 1)::int \
            ELSE LEAST( \
                {BACKOFF_MAX_MINUTES}::numeric, \
                GREATEST(update_interval, 1)::numeric \
                    * (2::numeric ^ LEAST(GREATEST(error_count - {BACKOFF_START_ERRORS} + 1, 0), 20)) \
            )::int \
         END"
    )
}

pub(crate) fn due_sources_select_sql() -> String {
    format!(
        "SELECT * FROM phantasi_sources \
         WHERE enabled = TRUE \
           AND source_type NOT IN ('link', 'note') \
           AND ( \
             last_fetched_at IS NULL \
             OR last_fetched_at <= $1::timestamptz - make_interval(mins => ({interval})) \
           ) \
         ORDER BY last_fetched_at ASC NULLS FIRST \
         LIMIT $2",
        interval = retry_interval_sql()
    )
}

#[cfg(test)]
pub(crate) fn source_is_due(
    now: chrono::DateTime<Utc>,
    last_fetched_at: Option<chrono::DateTime<Utc>>,
    update_interval: i32,
    error_count: i32,
) -> bool {
    match last_fetched_at {
        None => true,
        Some(last) => {
            (now - last).num_minutes() >= retry_interval_minutes(update_interval, error_count)
        }
    }
}

pub(crate) async fn load_due_sources(
    db: &DatabaseConnection,
    now: chrono::DateTime<Utc>,
) -> Result<Vec<phantasi_sources::Model>, String> {
    phantasi_sources::Model::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        due_sources_select_sql(),
        [
            SeaValue::String(Some(now.to_rfc3339())),
            SeaValue::Int(Some(
                i32::try_from(MAX_SOURCES_PER_TICK).unwrap_or(i32::MAX),
            )),
        ],
    ))
    .all(db)
    .await
    .map_err(|error| {
        tracing::error!(%error, "failed to query phantasi sources");
        "Failed to query sources".to_string()
    })
}

/// 调度器检查间隔（秒）
const SCHEDULER_INTERVAL_SECS: u64 = 60;
const NOTE_SCHEDULE_INTERVAL_SECS: u64 = 15;

/// 通知广播通道容量
const NOTIFICATION_CHANNEL_SIZE: usize = 100;

/// 新文章通知消息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewItemsNotification {
    #[serde(rename = "type")]
    pub msg_type: String,
    /// 所属用户；WebSocket 和持久通知必须据此隔离。
    pub user_id: i32,
    /// 订阅源 ID
    pub source_id: i32,
    /// 订阅源名称
    pub source_name: String,
    /// 新文章数量
    pub new_count: i32,
    /// 新文章标题列表（最多 5 个）
    pub titles: Vec<String>,
    pub timestamp: i64,
}

/// Phantasi 调度引擎
pub struct PhantasiSchedulerEngine {
    db: DatabaseConnection,
    parser: FeedParser,
    notion_service: NotionService,
    rsshub_service: RsshubService,
    /// 前端通知通道
    notification_tx: broadcast::Sender<NewItemsNotification>,
    /// 是否正在运行
    running: Arc<RwLock<bool>>,
}

impl PhantasiSchedulerEngine {
    /// 创建调度引擎
    pub fn new(db: DatabaseConnection) -> Self {
        let (notification_tx, _) = broadcast::channel(NOTIFICATION_CHANNEL_SIZE);
        Self {
            rsshub_service: RsshubService::new(db.clone()),
            db,
            parser: FeedParser::new(),
            notion_service: NotionService::new(),
            notification_tx,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// 获取通知订阅
    pub fn subscribe_notifications(&self) -> broadcast::Receiver<NewItemsNotification> {
        self.notification_tx.subscribe()
    }

    /// 启动调度引擎
    pub async fn start(&self) {
        let mut running = self.running.write().await;
        if *running {
            tracing::warn!("[PhantasiScheduler] Already running");
            return;
        }
        *running = true;
        drop(running);

        tracing::info!("[PhantasiScheduler] 🍵 Starting Phantasi scheduler engine");

        let db = self.db.clone();
        let running = self.running.clone();
        let notification_tx = self.notification_tx.clone();

        tokio::spawn(async move {
            let mut feeds =
                tokio::time::interval(tokio::time::Duration::from_secs(SCHEDULER_INTERVAL_SECS));
            let mut notes = tokio::time::interval(tokio::time::Duration::from_secs(
                NOTE_SCHEDULE_INTERVAL_SECS,
            ));
            feeds.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            notes.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = notes.tick() => {
                        if !*running.read().await {
                            tracing::info!("[PhantasiScheduler] Scheduler stopped");
                            break;
                        }
                        if let Err(e) = crate::services::note_publish::publish_due_note_docs(&db).await {
                            tracing::error!("[PhantasiScheduler] Note schedule error: {}", e);
                        }
                    }
                    _ = feeds.tick() => {
                        if !*running.read().await {
                            tracing::info!("[PhantasiScheduler] Scheduler stopped");
                            break;
                        }
                        if let Err(e) = Self::tick(&db, &notification_tx).await {
                            tracing::error!("[PhantasiScheduler] Tick error: {}", e);
                        }
                    }
                }
            }
        });
    }

    /// 停止调度引擎
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
        tracing::info!("[PhantasiScheduler] 🍵 Stopping Phantasi scheduler engine");
    }

    /// 主调度循环 tick
    async fn tick(
        db: &DatabaseConnection,
        notification_tx: &broadcast::Sender<NewItemsNotification>,
    ) -> Result<(), String> {
        let now = Utc::now();
        tracing::debug!("[PhantasiScheduler] Tick at {}", now);

        // Due predicate is in SQL so LIMIT cannot starve later-due sources.
        let due_sources = load_due_sources(db, now).await?;

        if due_sources.is_empty() {
            tracing::debug!("[PhantasiScheduler] No sources due for update");
            return Ok(());
        }

        tracing::info!(
            "[PhantasiScheduler] Processing {} sources (max {} concurrent)",
            due_sources.len(),
            MAX_CONCURRENT_FETCHES
        );

        // 并发处理，最多 MAX_CONCURRENT_FETCHES 个同时运行。
        // 收集本 tick 内新增了分类内容的 user_id，批末触发 phantasi-recommend 环网同步。
        let db_ref = db.clone();
        let tx_ref = notification_tx.clone();
        let ring_users = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::<
            i32,
        >::new()));

        stream::iter(due_sources)
            .map(|source| {
                let db = db_ref.clone();
                let tx = tx_ref.clone();
                let ring_users = ring_users.clone();
                async move {
                    let user_id = source.user_id;
                    let source_category = source.category.clone();
                    let result = Self::process_source(&db, &tx, source, now).await;
                    if let Ok(new_count) = &result {
                        if *new_count > 0
                            && source_category
                                .as_deref()
                                .map(|c| !c.trim().is_empty())
                                .unwrap_or(false)
                        {
                            ring_users.lock().await.insert(user_id);
                        }
                    }
                    result.map(|_| ())
                }
            })
            .buffer_unordered(MAX_CONCURRENT_FETCHES)
            .for_each(|result| {
                if let Err(e) = result {
                    tracing::error!("[PhantasiScheduler] Source processing error: {}", e);
                }
                futures::future::ready(())
            })
            .await;

        // Best-effort: batch ring sync once per user per tick (not per item)
        let users: Vec<i32> = ring_users.lock().await.iter().copied().collect();
        for uid in users {
            crate::federation::ring::maybe_trigger_phantasi_recommend_sync_for_user(db, uid).await;
        }

        Ok(())
    }

    /// 处理单个订阅源（抓取 + 存储）。
    /// Returns the number of newly inserted items on success (0 if none).
    async fn process_source(
        db: &DatabaseConnection,
        notification_tx: &broadcast::Sender<NewItemsNotification>,
        source: phantasi_sources::Model,
        now: chrono::DateTime<Utc>,
    ) -> Result<i32, String> {
        // 入口型只是快捷入口，笔记的内容本来就在库里 —— 两者都没有上游可抓。
        if !source.source_type.is_fetchable() {
            tracing::debug!(
                "[PhantasiScheduler] Skipping non-fetchable source: {} ({})",
                source.name,
                source.url
            );
            return Ok(0);
        }

        tracing::info!(
            "[PhantasiScheduler] Updating source: {} ({}) [type: {:?}]",
            source.name,
            source.url,
            source.feed_type
        );

        let mut active: phantasi_sources::ActiveModel = source.clone().into();
        // 提前更新 last_fetched_at，即使失败也记录，避免频繁重试失败源
        active.last_fetched_at = Set(Some(now.into()));

        let parser = FeedParser::new();
        let notion_service = NotionService::new();
        let rsshub_service = RsshubService::new(db.clone());

        let fetch_result: Result<(ParsedFeed, Option<String>), String> = match source.feed_type {
            phantasi_sources::FeedType::Notion => {
                Self::fetch_notion_source(&notion_service, &source)
                    .await
                    .map(|feed| (feed, None))
            }
            phantasi_sources::FeedType::RssHub => {
                Self::fetch_rsshub_source(&rsshub_service, &source)
                    .await
                    .map(|feed| (feed, None))
            }
            _ => parser
                .fetch_feed(&source.url)
                .await
                .map(|fetched| (fetched.feed, fetched.permanent_url))
                .map_err(|error| {
                    tracing::warn!(%error, "phantasi fetch failed");
                    error.user_message()
                }),
        };

        match fetch_result {
            Ok((feed, permanent_url)) => {
                active.last_success_at = Set(Some(now.into()));
                active.last_error = Set(None);
                active.error_count = Set(0);

                if let Some(new_url) = permanent_url {
                    if new_url != source.url {
                        tracing::info!(
                            old = %source.url,
                            new = %new_url,
                            source = %source.name,
                            "[PhantasiScheduler] Feed permanently moved; updating URL"
                        );
                        active.url = Set(new_url);
                    }
                }

                if source.name.is_empty() || source.name == source.url {
                    active.name = Set(feed.title.clone());
                }
                if source.description.is_none() {
                    active.description = Set(feed.description.clone());
                }
                if source.site_url.is_none() {
                    active.site_url = Set(feed.site_url.clone());
                }
                if source.icon.is_none() {
                    if let Some(icon_url) = &feed.icon {
                        Self::try_download_icon(&mut active, source.id, &source.name, icon_url)
                            .await;
                    }
                }

                let updated_source = active
                    .update(db)
                    .await
                    .map_err(|error| phantasi_store_failed("update source", error))?;

                let new_count =
                    Self::save_items(db, &updated_source, &feed, notification_tx).await?;

                if new_count > 0 {
                    tracing::info!(
                        "[PhantasiScheduler] Added {} new items for source: {}",
                        new_count,
                        updated_source.name
                    );
                }
                Ok(new_count)
            }
            Err(e) => {
                tracing::warn!(
                    "[PhantasiScheduler] Failed to fetch source {}: {}",
                    source.name,
                    e
                );

                active.last_error = Set(Some(e.clone()));
                let failures = source.error_count + 1;
                active.error_count = Set(failures);

                if failures == MAX_ERROR_COUNT {
                    tracing::warn!(
                        "[PhantasiScheduler] Source '{}' has {} consecutive errors; backing off to every {} min",
                        source.name,
                        failures,
                        retry_interval_minutes(source.update_interval, failures)
                    );
                    if let Some(manager) =
                        crate::services::agent::notifications::get_notification_manager()
                    {
                        manager
                            .notify_phantasi_source_error(
                                source.user_id,
                                source.id,
                                &source.name,
                                &e,
                            )
                            .await;
                    }
                } else if failures > MAX_ERROR_COUNT {
                    tracing::debug!(
                        "[PhantasiScheduler] Source '{}' still failing ({} in a row), next try in {} min",
                        source.name,
                        failures,
                        retry_interval_minutes(source.update_interval, failures)
                    );
                }

                active
                    .update(db)
                    .await
                    .map_err(|error| phantasi_store_failed("update source", error))?;
                Ok(0)
            }
        }
    }

    /// 尝试下载并保存订阅源图标（复用于 tick 和 refresh_source）
    async fn try_download_icon(
        active: &mut phantasi_sources::ActiveModel,
        source_id: i32,
        source_name: &str,
        icon_url: &str,
    ) {
        let icon_service = crate::services::icon_service::IconService::new();
        match icon_service.download_icon(source_id, icon_url).await {
            Ok(Some(icon_info)) => {
                active.icon = Set(Some(icon_info.local_path));
                tracing::info!(
                    "[PhantasiScheduler] Downloaded icon for source {}: {}",
                    source_name,
                    icon_url
                );
            }
            Ok(None) => {
                tracing::debug!(
                    "[PhantasiScheduler] Icon download returned empty for source {}",
                    source_name
                );
            }
            Err(e) => {
                tracing::warn!(
                    "[PhantasiScheduler] Failed to download icon for source {}: {}",
                    source_name,
                    e
                );
            }
        }
    }

    /// 存储新文章
    /// 性能优化：批量检查文章是否存在，避免 N+1 查询
    async fn save_items(
        db: &DatabaseConnection,
        source: &phantasi_sources::Model,
        feed: &ParsedFeed,
        notification_tx: &broadcast::Sender<NewItemsNotification>,
    ) -> Result<i32, String> {
        let now = Utc::now();

        // 批量获取所有已存在的 guid，避免 N+1 查询
        let all_guids: Vec<&str> = feed.items.iter().map(|item| item.guid.as_str()).collect();
        let existing_guids: std::collections::HashSet<String> = phantasi_items::Entity::find()
            .filter(phantasi_items::Column::SourceId.eq(source.id))
            .filter(phantasi_items::Column::Guid.is_in(all_guids))
            .select_only()
            .column(phantasi_items::Column::Guid)
            .into_tuple::<String>()
            .all(db)
            .await
            .map_err(|error| phantasi_store_failed("check existing items", error))?
            .into_iter()
            .collect();

        let mut new_items: Vec<phantasi_items::ActiveModel> = Vec::new();

        let image_cache = crate::services::image_cache::ImageCacheService::new();
        let newcomers: Vec<_> = feed
            .items
            .iter()
            .filter(|item| !existing_guids.contains(&item.guid))
            .cloned()
            .collect();
        let mut processed_images: Vec<_> = stream::iter(newcomers.iter().cloned().enumerate())
            .map(|(index, item)| {
                let image_cache = image_cache.clone();
                async move {
                    let processed = image_cache.process_image_url(item.image.as_deref()).await;
                    (index, processed)
                }
            })
            .buffer_unordered(4)
            .collect()
            .await;
        processed_images.sort_by_key(|(index, _)| *index);

        for (item, (_, processed_image)) in newcomers.into_iter().zip(processed_images) {
            let content_for_stats = item
                .content
                .as_deref()
                .or(item.summary.as_deref())
                .unwrap_or("");
            let (word_count, reading_time) = calculate_reading_stats(content_for_stats);

            let new_item = phantasi_items::ActiveModel {
                source_id: Set(source.id),
                guid: Set(item.guid.clone()),
                title: Set(item.title.clone()),
                link: Set(item.link.clone()),
                summary: Set(item.summary.clone()),
                content: Set(item.content.clone()),
                author: Set(item.author.clone()),
                image: Set(processed_image),
                audio_url: Set(item.audio_url.clone()),
                video_url: Set(item.video_url.clone()),
                enclosures: Set(if item.enclosures.is_empty() {
                    None
                } else {
                    serde_json::to_value(&item.enclosures).ok()
                }),
                categories: Set(if item.categories.is_empty() {
                    None
                } else {
                    serde_json::to_value(&item.categories).ok()
                }),
                published_at: Set(item.published_at.unwrap_or(now).into()),
                fetched_at: Set(now.into()),
                word_count: Set(Some(word_count)),
                reading_time: Set(Some(reading_time)),
                fulltext_fetched: Set(item.content.is_some()),
                // 主题由入库后的 AI 建议或站长手填；这里保持 NULL。
                topic: Set(None),
                ..Default::default()
            };

            new_items.push(new_item);
        }

        // Concurrent-safe batch insert (unique: source_id + guid).
        //
        // Strategy: ON CONFLICT DO NOTHING + RETURNING so:
        // - only rows PostgreSQL actually inserted are counted (no over-count on race);
        // - all-conflict / empty RETURNING is intentional success with 0 inserts, not a
        //   hard failure (another worker may have inserted the same guids first);
        // - real DB errors still fail the fetch.
        // new_count and notification titles come only from returned models.
        let (new_count, new_titles) = if new_items.is_empty() {
            (0_i32, Vec::new())
        } else {
            let candidate_len = new_items.len();
            let inserted = match insert_feed_items(db, new_items).await {
                Ok(models) => models,
                // SeaORM may surface zero RETURNING rows as RecordNotInserted; for our
                // DO NOTHING path that means concurrent/idempotent skips — count 0.
                Err(sea_orm::DbErr::RecordNotInserted) => {
                    tracing::debug!(
                        source_id = source.id,
                        candidates = candidate_len,
                        "[PhantasiScheduler] insert skipped all candidates (concurrent ON CONFLICT DO NOTHING)"
                    );
                    Vec::new()
                }
                Err(e) => {
                    tracing::error!(%e, source_id = source.id, "failed to batch insert phantasi items");
                    return Err("Failed to batch insert items".to_string());
                }
            };
            if !inserted.is_empty() {
                let topic_db = db.clone();
                let topic_ids: Vec<i32> = inserted.iter().map(|m| m.id).collect();
                tokio::spawn(async move {
                    crate::services::phantasi_topics::recommend_topics_for_item_ids(
                        &topic_db, &topic_ids,
                    )
                    .await;
                });
            }
            let titles: Vec<String> = inserted.iter().map(|m| m.title.clone()).take(5).collect();
            (inserted.len() as i32, titles)
        };

        if new_count > 0 {
            let mut source_active: phantasi_sources::ActiveModel = source.clone().into();
            source_active.item_count = Set(source.item_count + new_count);
            source_active.updated_at = Set(now.into());
            source_active
                .update(db)
                .await
                .map_err(|error| phantasi_store_failed("update source counts", error))?;

            let notification = NewItemsNotification {
                msg_type: "phantasi:new_items".to_string(),
                user_id: source.user_id,
                source_id: source.id,
                source_name: source.name.clone(),
                new_count,
                titles: new_titles,
                timestamp: now.timestamp_millis(),
            };

            if let Some(manager) = crate::services::agent::notifications::get_notification_manager()
            {
                manager
                    .notify_phantasi_new_items(
                        source.user_id,
                        source.id,
                        &source.name,
                        new_count,
                        &notification.titles,
                    )
                    .await;
            }

            if let Err(e) = notification_tx.send(notification) {
                tracing::debug!("[PhantasiScheduler] No notification subscribers: {}", e);
            }
        }

        Ok(new_count)
    }

    /// 抓取 Notion 订阅源
    async fn fetch_notion_source(
        notion_service: &NotionService,
        source: &phantasi_sources::Model,
    ) -> Result<ParsedFeed, String> {
        let extra_config = source
            .extra_config
            .as_ref()
            .ok_or_else(|| "Notion source requires extra_config with token".to_string())?;

        let token = extra_config["token"]
            .as_str()
            .ok_or_else(|| "Notion token not found in extra_config".to_string())?;

        let (resource_type, resource_id) =
            NotionService::parse_notion_url(&source.url).map_err(|e| e.to_string())?;

        let filter = extra_config.get("filter").cloned();
        let sort = extra_config.get("sort").cloned();

        let config = NotionConfig {
            token: token.to_string(),
            resource_id,
            resource_type,
            filter,
            sort,
        };

        notion_service.fetch(&config).await.map_err(|error| {
            tracing::warn!(%error, "notion fetch failed");
            error.to_string()
        })
    }

    /// 抓取 RSSHub 订阅源（带故障转移）
    async fn fetch_rsshub_source(
        rsshub_service: &RsshubService,
        source: &phantasi_sources::Model,
    ) -> Result<ParsedFeed, String> {
        let route = if let Some(ref route) = source.rsshub_route {
            route.clone()
        } else {
            rsshub_service
                .extract_route(&source.url)
                .ok_or_else(|| format!("Cannot extract RSSHub route from URL: {}", source.url))?
        };

        rsshub_service
            .fetch_with_failover(&route, Some(source.user_id))
            .await
    }

    /// 手动刷新单个订阅源
    pub async fn refresh_source(&self, source_id: i32) -> Result<i32, String> {
        let source = phantasi_sources::Entity::find_by_id(source_id)
            .one(&self.db)
            .await
            .map_err(|error| phantasi_store_failed("find source", error))?
            .ok_or_else(|| "Source not found".to_string())?;

        // 手动刷新也保持为无操作，且不写入 last_fetched_at/last_error。
        if !source.source_type.is_fetchable() {
            return Ok(0);
        }

        let now = Utc::now();

        let mut active: phantasi_sources::ActiveModel = source.clone().into();
        active.last_fetched_at = Set(Some(now.into()));

        let fetch_result: Result<(ParsedFeed, Option<String>), String> = match source.feed_type {
            phantasi_sources::FeedType::Notion => {
                Self::fetch_notion_source(&self.notion_service, &source)
                    .await
                    .map(|feed| (feed, None))
            }
            phantasi_sources::FeedType::RssHub => {
                Self::fetch_rsshub_source(&self.rsshub_service, &source)
                    .await
                    .map(|feed| (feed, None))
            }
            _ => self
                .parser
                .fetch_feed(&source.url)
                .await
                .map(|fetched| (fetched.feed, fetched.permanent_url))
                .map_err(|error| {
                    tracing::warn!(%error, "phantasi fetch failed");
                    error.user_message()
                }),
        };

        match fetch_result {
            Ok((feed, permanent_url)) => {
                active.last_success_at = Set(Some(now.into()));
                active.last_error = Set(None);
                active.error_count = Set(0);

                if let Some(new_url) = permanent_url {
                    if new_url != source.url {
                        tracing::info!(
                            old = %source.url,
                            new = %new_url,
                            source = %source.name,
                            "[PhantasiScheduler] Feed permanently moved; updating URL"
                        );
                        active.url = Set(new_url);
                    }
                }

                if source.description.is_none() {
                    active.description = Set(feed.description.clone());
                }
                if source.site_url.is_none() {
                    active.site_url = Set(feed.site_url.clone());
                }
                if source.icon.is_none() {
                    if let Some(icon_url) = &feed.icon {
                        Self::try_download_icon(&mut active, source.id, &source.name, icon_url)
                            .await;
                    }
                }

                let updated_source = active
                    .update(&self.db)
                    .await
                    .map_err(|error| phantasi_store_failed("update source", error))?;

                let new_count =
                    Self::save_items(&self.db, &updated_source, &feed, &self.notification_tx)
                        .await?;

                // Best-effort: push categorized phantasi into phantasi-recommend rings
                if new_count > 0
                    && updated_source
                        .category
                        .as_deref()
                        .map(|c| !c.trim().is_empty())
                        .unwrap_or(false)
                {
                    crate::federation::ring::maybe_trigger_phantasi_recommend_sync_for_user(
                        &self.db,
                        updated_source.user_id,
                    )
                    .await;
                }

                Ok(new_count)
            }
            Err(e) => {
                active.last_error = Set(Some(e.clone()));
                active.error_count = Set(source.error_count + 1);
                active
                    .update(&self.db)
                    .await
                    .map_err(|error| phantasi_store_failed("update source", error))?;

                Err(e)
            }
        }
    }

    /// Refresh enabled fetchable sources now. Caps at `MAX_SOURCES_PER_TICK`, stale first.
    pub async fn refresh_all_enabled(&self) -> Result<(usize, usize, usize, i32), String> {
        let sources = phantasi_sources::Entity::find()
            .filter(phantasi_sources::Column::Enabled.eq(true))
            .filter(
                phantasi_sources::Column::SourceType
                    .is_not_in(phantasi_sources::NON_FETCHABLE_SOURCE_TYPES),
            )
            .order_by_asc(phantasi_sources::Column::LastFetchedAt)
            .limit(MAX_SOURCES_PER_TICK)
            .all(&self.db)
            .await
            .map_err(|error| {
                tracing::error!(%error, "failed to query phantasi sources");
                "Failed to query sources".to_string()
            })?;

        let attempted = sources.len();
        let mut refreshed = 0usize;
        let mut failed = 0usize;
        let mut new_items = 0i32;
        for source in sources {
            match self.refresh_source(source.id).await {
                Ok(n) => {
                    refreshed += 1;
                    new_items += n;
                }
                Err(error) => {
                    tracing::warn!(
                        source_id = source.id,
                        %error,
                        "[PhantasiScheduler] refresh_all source failed"
                    );
                    failed += 1;
                }
            }
        }
        Ok((attempted, refreshed, failed, new_items))
    }
}

/// 全局调度引擎实例
static PHANTASI_SCHEDULER: once_cell::sync::OnceCell<Arc<PhantasiSchedulerEngine>> =
    once_cell::sync::OnceCell::new();

/// 初始化 Phantasi 调度引擎
pub async fn init_phantasi_scheduler(db: DatabaseConnection) {
    let engine = Arc::new(PhantasiSchedulerEngine::new(db));
    engine.start().await;

    if PHANTASI_SCHEDULER.set(engine).is_err() {
        tracing::warn!("[PhantasiScheduler] Scheduler already initialized");
    }
}

/// 获取 Phantasi 调度引擎实例
pub fn get_phantasi_scheduler() -> Option<Arc<PhantasiSchedulerEngine>> {
    PHANTASI_SCHEDULER.get().cloned()
}

/// 停止 Phantasi 调度引擎
pub async fn shutdown_phantasi_scheduler() {
    if let Some(engine) = PHANTASI_SCHEDULER.get() {
        engine.stop().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_and_briefly_failing_sources_keep_their_interval() {
        assert_eq!(retry_interval_minutes(30, 0), 30);
        assert_eq!(retry_interval_minutes(30, 2), 30);
        // 0 分钟的源不能变成每轮都抓
        assert_eq!(retry_interval_minutes(0, 0), 1);
    }

    #[test]
    fn repeated_failures_back_off_and_cap_at_a_day() {
        assert_eq!(retry_interval_minutes(30, 3), 60);
        assert_eq!(retry_interval_minutes(30, 4), 120);
        assert_eq!(retry_interval_minutes(30, 8), 1440);
        assert_eq!(retry_interval_minutes(30, 1236), BACKOFF_MAX_MINUTES);
        assert_eq!(
            retry_interval_minutes(i32::MAX, i32::MAX),
            BACKOFF_MAX_MINUTES
        );
    }

    #[test]
    fn cover_fetch_is_bounded_not_serial() {
        let src = include_str!("phantasi_scheduler.rs");
        let impl_src = src.split("mod tests").next().expect("impl");
        assert!(impl_src.contains("buffer_unordered(4)"));
        assert!(impl_src.contains("image_cache.clone()"));
        assert!(
            !impl_src.contains("for item in newcomers"),
            "cover processing must not walk newcomers serially before the unordered buffer"
        );
    }

    #[test]
    fn fetch_does_not_write_site_unread() {
        let src = include_str!("phantasi_scheduler.rs");
        let impl_src = src.split("mod tests").next().expect("impl");
        assert!(
            !impl_src.contains("unread_count"),
            "scheduler must not write site-wide source unread_count"
        );
    }

    #[test]
    fn due_predicate_is_applied_before_limit() {
        let sql = due_sources_select_sql();
        let interval_at = sql.find("error_count").expect("due interval");
        let limit_at = sql.find("LIMIT").expect("limit");
        assert!(
            interval_at < limit_at,
            "due predicate must be in WHERE, not after LIMIT"
        );
        assert!(sql.contains(&BACKOFF_START_ERRORS.to_string()));
        assert!(sql.contains(&BACKOFF_MAX_MINUTES.to_string()));
        let interval = retry_interval_sql();
        assert!(
            interval.contains("::int") && !interval.contains("::bigint"),
            "make_interval(mins) is integer; the CASE must produce int"
        );
        assert!(
            sql.contains(&format!("make_interval(mins => ({interval}))")),
            "due SQL must use the integer interval as the mins argument"
        );
        let tick = include_str!("phantasi_scheduler.rs")
            .split("async fn tick(")
            .nth(1)
            .and_then(|rest| rest.split("async fn process_source").next())
            .expect("tick");
        assert!(tick.contains("load_due_sources"));
        assert!(
            !tick.contains("into_iter()"),
            "tick must not filter due sources in memory after LIMIT"
        );
    }

    #[test]
    fn source_is_due_matches_retry_interval() {
        let now = Utc::now();
        assert!(source_is_due(now, None, 60, 0));
        assert!(!source_is_due(
            now,
            Some(now - chrono::Duration::minutes(29)),
            30,
            0
        ));
        assert!(source_is_due(
            now,
            Some(now - chrono::Duration::minutes(30)),
            30,
            0
        ));
        assert!(!source_is_due(
            now,
            Some(now - chrono::Duration::minutes(59)),
            30,
            3
        ));
        assert!(source_is_due(
            now,
            Some(now - chrono::Duration::minutes(60)),
            30,
            3
        ));
    }

    #[tokio::test]
    async fn limit_does_not_starve_later_due_sources() {
        use crate::models::entities::phantasi_sources::{self, FeedType, SourceType};
        use sea_orm::{
            ActiveModelTrait, ActiveValue::Set, ConnectOptions, ConnectionTrait, Database,
            DatabaseBackend, Schema,
        };

        let Ok(url) = std::env::var("PHANTASI_TEST_DATABASE_URL") else {
            return;
        };
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let db = Database::connect(options).await.unwrap();
        let schema = Schema::new(DatabaseBackend::Postgres);
        let sql = schema
            .create_table_from_entity(phantasi_sources::Entity)
            .to_string(sea_orm::sea_query::PostgresQueryBuilder)
            .replacen("CREATE TABLE", "CREATE TEMP TABLE", 1);
        db.execute_unprepared(&sql).await.unwrap();
        let now = Utc::now();
        let fresh = now - chrono::Duration::minutes(2);
        let stale = now - chrono::Duration::minutes(120);
        for i in 0..50 {
            phantasi_sources::ActiveModel {
                user_id: Set(1),
                name: Set(format!("fresh-{i}")),
                url: Set(format!("https://fresh.example/{i}")),
                feed_type: Set(FeedType::Rss),
                source_type: Set(SourceType::Rss),
                update_interval: Set(60),
                enabled: Set(true),
                error_count: Set(0),
                item_count: Set(0),
                admin_only: Set(false),
                last_fetched_at: Set(Some(fresh.into())),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let due = phantasi_sources::ActiveModel {
            user_id: Set(1),
            name: Set("due".into()),
            url: Set("https://due.example/feed".into()),
            feed_type: Set(FeedType::Rss),
            source_type: Set(SourceType::Rss),
            update_interval: Set(30),
            enabled: Set(true),
            error_count: Set(0),
            item_count: Set(0),
            admin_only: Set(false),
            last_fetched_at: Set(Some(stale.into())),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let loaded = load_due_sources(&db, now).await.unwrap();
        assert!(
            loaded.iter().any(|source| source.id == due.id),
            "a due source after 50 not-due rows must still be selected"
        );
        assert!(
            loaded.iter().all(|source| source_is_due(
                now,
                source.last_fetched_at.map(|ts| ts.with_timezone(&Utc)),
                source.update_interval,
                source.error_count
            )),
            "SQL due rows must match the rust due predicate"
        );
    }
}
