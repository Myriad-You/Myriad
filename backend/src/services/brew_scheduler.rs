//! Brew 阅读 - 订阅调度引擎
//!
//! 后端独立运行的调度器，负责：
//! 1. 定时检查需要更新的订阅源
//! 2. 抓取并解析订阅源内容
//! 3. 存储新文章到数据库
//! 4. 推送更新通知到前端

use chrono::{Duration, Utc};
use futures::stream::{self, StreamExt};
use sea_orm::{
    sea_query::OnConflict, ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::models::entities::{brew_items, brew_sources};
use crate::services::brew_parser::{calculate_reading_stats, FeedParser, ParsedFeed};
use crate::services::notion_service::{NotionConfig, NotionService};
use crate::services::rsshub_service::RsshubService;

// ==================== 调度器常量 ====================

/// 每轮 tick 最多处理的订阅源数量
/// 防止宕机恢复后一次性堆积大量请求
const MAX_SOURCES_PER_TICK: u64 = 50;

/// 并发抓取的最大并行数
/// 限制同时发出的 HTTP 请求数，避免网络/内存压力
const MAX_CONCURRENT_FETCHES: usize = 5;

/// 连续错误次数上限，超过后记录警告（用户自行决定是否禁用）
const MAX_ERROR_COUNT: i32 = 10;

/// 调度器检查间隔（秒）
const SCHEDULER_INTERVAL_SECS: u64 = 60;

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
    /// 时间戳
    pub timestamp: i64,
}

/// Brew 调度引擎
pub struct BrewSchedulerEngine {
    db: DatabaseConnection,
    parser: FeedParser,
    notion_service: NotionService,
    rsshub_service: RsshubService,
    /// 前端通知通道
    notification_tx: broadcast::Sender<NewItemsNotification>,
    /// 是否正在运行
    running: Arc<RwLock<bool>>,
}

impl BrewSchedulerEngine {
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
            tracing::warn!("[BrewScheduler] Already running");
            return;
        }
        *running = true;
        drop(running);

        tracing::info!("[BrewScheduler] 🍵 Starting Brew scheduler engine");

        let db = self.db.clone();
        let running = self.running.clone();
        let notification_tx = self.notification_tx.clone();

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(SCHEDULER_INTERVAL_SECS));

            loop {
                interval.tick().await;

                if !*running.read().await {
                    tracing::info!("[BrewScheduler] Scheduler stopped");
                    break;
                }

                if let Err(e) = Self::tick(&db, &notification_tx).await {
                    tracing::error!("[BrewScheduler] Tick error: {}", e);
                }
            }
        });
    }

    /// 停止调度引擎
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
        tracing::info!("[BrewScheduler] 🍵 Stopping Brew scheduler engine");
    }

    /// 主调度循环 tick
    async fn tick(
        db: &DatabaseConnection,
        notification_tx: &broadcast::Sender<NewItemsNotification>,
    ) -> Result<(), String> {
        let now = Utc::now();
        tracing::debug!("[BrewScheduler] Tick at {}", now);

        // 查找需要更新的订阅源，限制本轮最大数量防止堆积
        let all_due = brew_sources::Entity::find()
            .filter(brew_sources::Column::Enabled.eq(true))
            .filter(brew_sources::Column::SourceType.ne(brew_sources::SourceType::Link))
            .filter(
                Condition::any()
                    .add(brew_sources::Column::LastFetchedAt.is_null())
                    .add(brew_sources::Column::LastFetchedAt.lt(now - Duration::minutes(1))),
            )
            .order_by_asc(brew_sources::Column::LastFetchedAt)
            .limit(MAX_SOURCES_PER_TICK)
            .all(db)
            .await
            .map_err(|e| format!("Failed to query sources: {}", e))?;

        // 过滤出真正到了更新间隔的订阅源
        let due_sources: Vec<_> = all_due
            .into_iter()
            .filter(|source| match source.last_fetched_at {
                None => true,
                Some(last) => {
                    let elapsed = now - last.with_timezone(&Utc);
                    elapsed.num_minutes() >= source.update_interval as i64
                }
            })
            .collect();

        if due_sources.is_empty() {
            tracing::debug!("[BrewScheduler] No sources due for update");
            return Ok(());
        }

        tracing::info!(
            "[BrewScheduler] Processing {} sources (max {} concurrent)",
            due_sources.len(),
            MAX_CONCURRENT_FETCHES
        );

        // 并发处理，最多 MAX_CONCURRENT_FETCHES 个同时运行。
        // 收集本 tick 内新增了分类内容的 user_id，批末触发 brew-recommend 环网同步。
        let db_ref = db.clone();
        let tx_ref = notification_tx.clone();
        let ring_users = std::sync::Arc::new(tokio::sync::Mutex::new(
            std::collections::HashSet::<i32>::new(),
        ));

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
                    tracing::error!("[BrewScheduler] Source processing error: {}", e);
                }
                futures::future::ready(())
            })
            .await;

        // Best-effort: batch ring sync once per user per tick (not per item)
        let users: Vec<i32> = ring_users.lock().await.iter().copied().collect();
        for uid in users {
            crate::federation::ring::maybe_trigger_brew_recommend_sync_for_user(db, uid).await;
        }

        Ok(())
    }

    /// 处理单个订阅源（抓取 + 存储）。
    /// Returns the number of newly inserted items on success (0 if none).
    async fn process_source(
        db: &DatabaseConnection,
        notification_tx: &broadcast::Sender<NewItemsNotification>,
        source: brew_sources::Model,
        now: chrono::DateTime<Utc>,
    ) -> Result<i32, String> {
        // 纯链接只是快捷入口，不应该进入任何抓取路径。
        if source.source_type == brew_sources::SourceType::Link {
            tracing::debug!(
                "[BrewScheduler] Skipping pure link source: {} ({})",
                source.name,
                source.url
            );
            return Ok(0);
        }

        tracing::info!(
            "[BrewScheduler] Updating source: {} ({}) [type: {:?}]",
            source.name,
            source.url,
            source.feed_type
        );

        let mut active: brew_sources::ActiveModel = source.clone().into();
        // 提前更新 last_fetched_at，即使失败也记录，避免频繁重试失败源
        active.last_fetched_at = Set(Some(now.into()));

        let parser = FeedParser::new();
        let notion_service = NotionService::new();
        let rsshub_service = RsshubService::new(db.clone());

        let fetch_result: Result<ParsedFeed, String> = match source.feed_type {
            brew_sources::FeedType::Notion => {
                Self::fetch_notion_source(&notion_service, &source).await
            }
            brew_sources::FeedType::RssHub => {
                Self::fetch_rsshub_source(&rsshub_service, &source).await
            }
            _ => parser
                .fetch_and_parse(&source.url)
                .await
                .map_err(|e| e.to_string()),
        };

        match fetch_result {
            Ok(feed) => {
                active.last_success_at = Set(Some(now.into()));
                active.last_error = Set(None);
                active.error_count = Set(0);

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
                    .map_err(|e| format!("Failed to update source: {}", e))?;

                let new_count =
                    Self::save_items(db, &updated_source, &feed, notification_tx).await?;

                if new_count > 0 {
                    tracing::info!(
                        "[BrewScheduler] Added {} new items for source: {}",
                        new_count,
                        updated_source.name
                    );
                }
                Ok(new_count)
            }
            Err(e) => {
                tracing::warn!(
                    "[BrewScheduler] Failed to fetch source {}: {}",
                    source.name,
                    e
                );

                active.last_error = Set(Some(e.to_string()));
                active.error_count = Set(source.error_count + 1);

                if source.error_count + 1 >= MAX_ERROR_COUNT {
                    tracing::warn!(
                        "[BrewScheduler] Source '{}' has {} consecutive errors, consider disabling",
                        source.name,
                        source.error_count + 1
                    );
                    if source.error_count + 1 == MAX_ERROR_COUNT {
                        if let Some(manager) =
                            crate::services::agent::notifications::get_notification_manager()
                        {
                            manager
                                .notify_brew_source_error(
                                    source.user_id,
                                    source.id,
                                    &source.name,
                                    &e,
                                )
                                .await;
                        }
                    }
                }

                active
                    .update(db)
                    .await
                    .map_err(|e| format!("Failed to update source after error: {}", e))?;
                Ok(0)
            }
        }
    }

    /// 尝试下载并保存订阅源图标（复用于 tick 和 refresh_source）
    async fn try_download_icon(
        active: &mut brew_sources::ActiveModel,
        source_id: i32,
        source_name: &str,
        icon_url: &str,
    ) {
        let icon_service = crate::services::icon_service::IconService::new();
        match icon_service.download_icon(source_id, icon_url).await {
            Ok(Some(icon_info)) => {
                active.icon = Set(Some(icon_info.local_path));
                tracing::info!(
                    "[BrewScheduler] Downloaded icon for source {}: {}",
                    source_name,
                    icon_url
                );
            }
            Ok(None) => {
                tracing::debug!(
                    "[BrewScheduler] Icon download returned empty for source {}",
                    source_name
                );
            }
            Err(e) => {
                tracing::warn!(
                    "[BrewScheduler] Failed to download icon for source {}: {}",
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
        source: &brew_sources::Model,
        feed: &ParsedFeed,
        notification_tx: &broadcast::Sender<NewItemsNotification>,
    ) -> Result<i32, String> {
        let now = Utc::now();

        // 批量获取所有已存在的 guid，避免 N+1 查询
        let all_guids: Vec<&str> = feed.items.iter().map(|item| item.guid.as_str()).collect();
        let existing_guids: std::collections::HashSet<String> = brew_items::Entity::find()
            .filter(brew_items::Column::SourceId.eq(source.id))
            .filter(brew_items::Column::Guid.is_in(all_guids))
            .select_only()
            .column(brew_items::Column::Guid)
            .into_tuple::<String>()
            .all(db)
            .await
            .map_err(|e| format!("Failed to check existing items: {}", e))?
            .into_iter()
            .collect();

        let mut new_items: Vec<brew_items::ActiveModel> = Vec::new();
        let mut new_titles: Vec<String> = Vec::new();

        let image_cache = crate::services::image_cache::ImageCacheService::new();

        for item in &feed.items {
            if existing_guids.contains(&item.guid) {
                continue;
            }

            let content_for_stats = item
                .content
                .as_deref()
                .or(item.summary.as_deref())
                .unwrap_or("");
            let (word_count, reading_time) = calculate_reading_stats(content_for_stats);

            let processed_image = image_cache.process_image_url(item.image.as_deref()).await;

            let new_item = brew_items::ActiveModel {
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
                ..Default::default()
            };

            if new_titles.len() < 5 {
                new_titles.push(item.title.clone());
            }
            new_items.push(new_item);
        }

        // 批量插入，ON CONFLICT DO NOTHING 防止并发竞态下的重复键错误
        let new_count = new_items.len() as i32;
        if !new_items.is_empty() {
            let on_conflict =
                OnConflict::columns([brew_items::Column::SourceId, brew_items::Column::Guid])
                    .do_nothing()
                    .to_owned();
            brew_items::Entity::insert_many(new_items)
                .on_conflict(on_conflict)
                .do_nothing()
                .exec(db)
                .await
                .map_err(|e| format!("Failed to batch insert items: {}", e))?;
        }

        if new_count > 0 {
            let mut source_active: brew_sources::ActiveModel = source.clone().into();
            source_active.item_count = Set(source.item_count + new_count);
            source_active.unread_count = Set(source.unread_count + new_count);
            source_active.updated_at = Set(now.into());
            source_active
                .update(db)
                .await
                .map_err(|e| format!("Failed to update source counts: {}", e))?;

            let notification = NewItemsNotification {
                msg_type: "brew:new_items".to_string(),
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
                    .notify_brew_new_items(
                        source.user_id,
                        source.id,
                        &source.name,
                        new_count,
                        &notification.titles,
                    )
                    .await;
            }

            if let Err(e) = notification_tx.send(notification) {
                tracing::debug!("[BrewScheduler] No notification subscribers: {}", e);
            }
        }

        Ok(new_count)
    }

    /// 抓取 Notion 订阅源
    async fn fetch_notion_source(
        notion_service: &NotionService,
        source: &brew_sources::Model,
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

        notion_service
            .fetch(&config)
            .await
            .map_err(|e| e.to_string())
    }

    /// 抓取 RSSHub 订阅源（带故障转移）
    async fn fetch_rsshub_source(
        rsshub_service: &RsshubService,
        source: &brew_sources::Model,
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
        let source = brew_sources::Entity::find_by_id(source_id)
            .one(&self.db)
            .await
            .map_err(|e| format!("Failed to find source: {}", e))?
            .ok_or_else(|| "Source not found".to_string())?;

        // 手动刷新也保持为无操作，且不写入 last_fetched_at/last_error。
        if source.source_type == brew_sources::SourceType::Link {
            return Ok(0);
        }

        let now = Utc::now();

        let mut active: brew_sources::ActiveModel = source.clone().into();
        active.last_fetched_at = Set(Some(now.into()));

        let fetch_result: Result<ParsedFeed, String> = match source.feed_type {
            brew_sources::FeedType::Notion => {
                Self::fetch_notion_source(&self.notion_service, &source).await
            }
            brew_sources::FeedType::RssHub => {
                Self::fetch_rsshub_source(&self.rsshub_service, &source).await
            }
            _ => self
                .parser
                .fetch_and_parse(&source.url)
                .await
                .map_err(|e| e.to_string()),
        };

        match fetch_result {
            Ok(feed) => {
                active.last_success_at = Set(Some(now.into()));
                active.last_error = Set(None);
                active.error_count = Set(0);

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
                    .map_err(|e| format!("Failed to update source: {}", e))?;

                let new_count = Self::save_items(
                    &self.db,
                    &updated_source,
                    &feed,
                    &self.notification_tx,
                )
                .await?;

                // Best-effort: push categorized brew into brew-recommend rings
                if new_count > 0
                    && updated_source
                        .category
                        .as_deref()
                        .map(|c| !c.trim().is_empty())
                        .unwrap_or(false)
                {
                    crate::federation::ring::maybe_trigger_brew_recommend_sync_for_user(
                        &self.db,
                        updated_source.user_id,
                    )
                    .await;
                }

                Ok(new_count)
            }
            Err(e) => {
                active.last_error = Set(Some(e.to_string()));
                active.error_count = Set(source.error_count + 1);
                active
                    .update(&self.db)
                    .await
                    .map_err(|ee| format!("Failed to update source: {}", ee))?;

                Err(e)
            }
        }
    }
}

/// 全局调度引擎实例
static BREW_SCHEDULER: once_cell::sync::OnceCell<Arc<BrewSchedulerEngine>> =
    once_cell::sync::OnceCell::new();

/// 初始化 Brew 调度引擎
pub async fn init_brew_scheduler(db: DatabaseConnection) {
    let engine = Arc::new(BrewSchedulerEngine::new(db));
    engine.start().await;

    if BREW_SCHEDULER.set(engine).is_err() {
        tracing::warn!("[BrewScheduler] Scheduler already initialized");
    }
}

/// 获取 Brew 调度引擎实例
pub fn get_brew_scheduler() -> Option<Arc<BrewSchedulerEngine>> {
    BREW_SCHEDULER.get().cloned()
}

/// 停止 Brew 调度引擎
pub async fn shutdown_brew_scheduler() {
    if let Some(engine) = BREW_SCHEDULER.get() {
        engine.stop().await;
    }
}
