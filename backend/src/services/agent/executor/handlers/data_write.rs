//! 数据写入能力处理器
//!
//! 处理 platform.write, brew.subscribe, brew.mark 等写入类能力

use super::HandlerContext;
use crate::models::entities::{brew_items, brew_sources, brew_user_states, tapp_storage};
use crate::services::agent::executor::utils::VALID_PLATFORMS;
use crate::services::brew_parser::FeedParser;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::net::ToSocketAddrs;

/// 订阅源名称最大长度
const MAX_FEED_NAME_LEN: usize = 255;

/// 清洗并验证用户提供的订阅源名称
///
/// - 限制最大长度
/// - 去除首尾空白
/// - 拒绝纯空白字符串
fn sanitize_feed_name(name: &str) -> Result<String, String> {
    let trimmed: String = name.chars().take(MAX_FEED_NAME_LEN).collect();
    let trimmed = trimmed.trim().to_string();
    if trimmed.is_empty() {
        return Err("订阅源名称不能为空".to_string());
    }
    Ok(trimmed)
}

/// 验证平台名称白名单，防止路径穿越
use crate::services::agent::executor::utils::validate_platform_name;

/// 订阅 URL 最大尝试数
const MAX_FEED_URLS: usize = 10;
/// platform.write 单次最大写入条目数
const MAX_PLATFORM_WRITE_ITEMS: usize = 500;
/// update_interval 最小值（分钟）
const MIN_UPDATE_INTERVAL: i32 = 5;
/// update_interval 最大值（分钟）
const MAX_UPDATE_INTERVAL: i32 = 1440;

/// 验证订阅 URL 安全性，防止 SSRF
/// - 仅允许 http/https scheme
/// - 阻止内网 IP 地址
fn validate_subscribe_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| format!("无效的 URL: {}", url))?;

    // 只允许 http/https
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("不允许的 URL scheme: {}", scheme)),
    }

    let host = parsed.host_str().ok_or("URL 缺少 host")?;

    // 阻止明显的内网主机名
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".internal") {
        return Err("不允许访问内网地址".to_string());
    }

    // 解析 IP 并阻止内网地址
    let port = parsed
        .port()
        .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });
    let addr_str = format!("{}:{}", host, port);
    if let Ok(addrs) = addr_str.to_socket_addrs() {
        for addr in addrs {
            let ip = addr.ip();
            if ip.is_loopback() || ip.is_unspecified() {
                return Err("不允许访问回环/未指定地址".to_string());
            }
            match ip {
                std::net::IpAddr::V4(v4) => {
                    if v4.is_private() || v4.is_link_local() || v4.octets()[0] == 169 {
                        return Err("不允许访问内网地址".to_string());
                    }
                }
                std::net::IpAddr::V6(v6) => {
                    // 阻止 IPv6 回环和链路本地
                    if v6.is_loopback() || (v6.segments()[0] & 0xffc0) == 0xfe80 {
                        return Err("不允许访问内网 IPv6 地址".to_string());
                    }
                }
            }
        }
    }

    Ok(())
}

/// 执行数据写入能力
pub async fn execute(
    capability_id: &str,
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    match capability_id {
        "platform.write" => execute_platform_write(params).await,
        "platform.refresh" => execute_platform_refresh(params).await,
        "storage.set" => execute_storage_set(params, ctx).await,
        "tapp.storage" => execute_tapp_storage(params, ctx).await,
        "brew.subscribe" => execute_brew_subscribe(params, ctx).await,
        "brew.mark" => execute_brew_mark(params, ctx).await,
        "content.write" => execute_content_write(params, ctx).await,
        _ => Err(format!("Unknown data_write capability: {}", capability_id)),
    }
}

// ============================================================================
// Platform 相关
// ============================================================================

async fn execute_platform_write(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform_raw = params
        .get("platform")
        .and_then(|v| v.as_str())
        .ok_or("Missing platform parameter")?;

    // 白名单校验，防止路径穿越
    let platform = validate_platform_name(platform_raw)?;

    let items = params.get("items").ok_or("Missing items parameter")?;

    // 限制单次写入条目数量
    if let Some(arr) = items.as_array() {
        if arr.len() > MAX_PLATFORM_WRITE_ITEMS {
            return Err(format!("单次最多写入 {} 条数据", MAX_PLATFORM_WRITE_ITEMS));
        }
    }

    let cache_file = format!("cache/platforms/{}_filtered.json", platform);

    // 读取现有数据
    let mut data: Value = tokio::fs::read_to_string(&cache_file)
        .await
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(json!({"items": []}));

    // 追加新数据
    if let Some(existing_items) = data.get_mut("items").and_then(|v| v.as_array_mut()) {
        if let Some(new_items) = items.as_array() {
            existing_items.extend(new_items.clone());
        }
    }

    // 写入文件
    tokio::fs::write(
        &cache_file,
        serde_json::to_string_pretty(&data).unwrap_or_else(|_| data.to_string()),
    )
    .await
    .map_err(|e| format!("Failed to write data: {}", e))?;

    Ok(json!({
        "success": true,
        "platform": platform_raw
    }))
}

async fn execute_platform_refresh(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .ok_or("Missing platform parameter")?;

    let platforms_to_refresh: Vec<&str> = if platform == "all" {
        VALID_PLATFORMS.to_vec()
    } else {
        vec![platform]
    };

    // 尝试通过后台处理器提交刷新任务
    let mut results = Vec::new();
    for p in &platforms_to_refresh {
        match crate::services::background_processor::BACKGROUND_PROCESSOR
            .submit_task(p.to_string())
            .await
        {
            Ok(task_id) => {
                results.push(json!({
                    "platform": p,
                    "status": "submitted",
                    "taskId": task_id,
                    "message": crate::services::agent::response_agent::refresh_submitted(p)
                }));
            }
            Err(e) => {
                tracing::warn!(platform = %p, error = %e, "[platform.refresh] 提交失败");
                results.push(json!({
                    "platform": p,
                    "status": "failed",
                    "message": crate::services::agent::response_agent::refresh_submit_failed(&e.to_string())
                }));
            }
        }
    }

    let submitted = results
        .iter()
        .filter(|r| r["status"] == "submitted")
        .count();
    Ok(json!({
        "success": submitted > 0,
        "message": crate::services::agent::response_agent::refresh_submitted_summary(submitted, platforms_to_refresh.len()),
        "results": results
    }))
}

// ============================================================================
// Storage 相关
// ============================================================================

async fn execute_storage_set(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let key = params
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or("Missing key parameter")?;
    let value = params
        .get("value")
        .cloned()
        .ok_or("Missing value parameter")?;
    let namespace = params
        .get("namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("agent_storage");

    let now = Utc::now();
    let user_id = ctx.user_id;

    // Upsert: check if key exists, update or insert
    let existing = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::TappId.eq(namespace))
        .filter(tapp_storage::Column::Key.eq(key))
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .one(ctx.db)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

    if let Some(record) = existing {
        let mut active: tapp_storage::ActiveModel = record.into();
        active.value = Set(value.clone());
        active.updated_at = Set(now.into());
        active
            .update(ctx.db)
            .await
            .map_err(|e| format!("Failed to update storage: {}", e))?;
    } else {
        let new_record = tapp_storage::ActiveModel {
            tapp_id: Set(namespace.to_string()),
            user_id: Set(user_id),
            key: Set(key.to_string()),
            value: Set(value.clone()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        };
        new_record
            .insert(ctx.db)
            .await
            .map_err(|e| format!("Failed to insert storage: {}", e))?;
    }

    Ok(json!({
        "success": true,
        "namespace": namespace,
        "key": key,
        "value": value
    }))
}

async fn execute_tapp_storage(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let tapp_id = params
        .get("tappId")
        .and_then(|v| v.as_str())
        .ok_or("Missing tappId")?;
    let action = params
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or("Missing action")?;
    let key = params.get("key").and_then(|v| v.as_str());
    let value = params.get("value");
    let user_id = ctx.user_id;

    match action {
        "get" => {
            if let Some(key) = key {
                let result = tapp_storage::Entity::find()
                    .filter(tapp_storage::Column::TappId.eq(tapp_id))
                    .filter(tapp_storage::Column::Key.eq(key))
                    .filter(tapp_storage::Column::UserId.eq(user_id))
                    .one(ctx.db)
                    .await
                    .map_err(|e| format!("Database error: {}", e))?;

                Ok(json!({
                    "success": true,
                    "key": key,
                    "value": result.map(|r| r.value)
                }))
            } else {
                let results = tapp_storage::Entity::find()
                    .filter(tapp_storage::Column::TappId.eq(tapp_id))
                    .filter(tapp_storage::Column::UserId.eq(user_id))
                    .all(ctx.db)
                    .await
                    .map_err(|e| format!("Database error: {}", e))?;

                let data: HashMap<String, Value> =
                    results.into_iter().map(|r| (r.key, r.value)).collect();

                Ok(json!({
                    "success": true,
                    "data": data
                }))
            }
        }
        "set" => {
            let key = key.ok_or("Missing key for set action")?;
            let value = value.ok_or("Missing value for set action")?.clone();
            let now = Utc::now();

            let existing = tapp_storage::Entity::find()
                .filter(tapp_storage::Column::TappId.eq(tapp_id))
                .filter(tapp_storage::Column::Key.eq(key))
                .filter(tapp_storage::Column::UserId.eq(user_id))
                .one(ctx.db)
                .await
                .map_err(|e| format!("Database error: {}", e))?;

            if let Some(record) = existing {
                let mut active: tapp_storage::ActiveModel = record.into();
                active.value = Set(value.clone());
                active.updated_at = Set(now.into());
                active
                    .update(ctx.db)
                    .await
                    .map_err(|e| format!("Failed to update storage: {}", e))?;
            } else {
                let new_record = tapp_storage::ActiveModel {
                    tapp_id: Set(tapp_id.to_string()),
                    user_id: Set(user_id),
                    key: Set(key.to_string()),
                    value: Set(value.clone()),
                    created_at: Set(now.into()),
                    updated_at: Set(now.into()),
                    ..Default::default()
                };
                new_record
                    .insert(ctx.db)
                    .await
                    .map_err(|e| format!("Failed to insert storage: {}", e))?;
            }

            Ok(json!({
                "success": true,
                "key": key,
                "value": value
            }))
        }
        "delete" => {
            let key = key.ok_or("Missing key for delete action")?;

            let result = tapp_storage::Entity::delete_many()
                .filter(tapp_storage::Column::TappId.eq(tapp_id))
                .filter(tapp_storage::Column::Key.eq(key))
                .filter(tapp_storage::Column::UserId.eq(user_id))
                .exec(ctx.db)
                .await
                .map_err(|e| format!("Database error: {}", e))?;

            Ok(json!({
                "success": true,
                "key": key,
                "deleted": result.rows_affected > 0
            }))
        }
        _ => Err(format!("Unknown storage action: {}", action)),
    }
}

// ============================================================================
// Brew 相关
// ============================================================================

/// 执行订阅源添加 - 支持智能尝试多个源
async fn execute_brew_subscribe(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let user_id = ctx.user_id;

    // 调试：打印收到的参数
    tracing::debug!(
        params_keys = ?params.keys().collect::<Vec<_>>(),
        "[Brew Subscribe] 收到的参数"
    );

    // 校验并清洗用户提供的名称，防止过长或空白字符串入库
    let custom_name: Option<String> = params
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(sanitize_feed_name)
        .transpose()?;
    let category = params.get("category").and_then(|v| v.as_str());
    let update_interval = params
        .get("updateInterval")
        .and_then(|v| v.as_i64())
        .unwrap_or(30) as i32;
    let update_interval = update_interval.clamp(MIN_UPDATE_INTERVAL, MAX_UPDATE_INTERVAL);

    // 收集要尝试的 URL 列表
    let urls_to_try: Vec<(String, Option<String>)> = if let Some(feeds) = params.get("feeds") {
        tracing::debug!(feeds = ?feeds, "[Brew Subscribe] 从 feeds 参数提取 URL");
        // 从 feeds 数组中提取 URL，按优先级排序
        extract_and_prioritize_feeds(feeds)
    } else if let Some(url) = params.get("url").and_then(|v| v.as_str()) {
        tracing::debug!(url = %url, "[Brew Subscribe] 使用单个 URL");
        // 单个 URL
        vec![(url.to_string(), None)]
    } else {
        tracing::warn!("[Brew Subscribe] 缺少 url 和 feeds 参数");
        return Err("缺少 url 或 feeds 参数".to_string());
    };

    if urls_to_try.is_empty() {
        return Err("没有可用的订阅源 URL".to_string());
    }

    tracing::info!(
        count = urls_to_try.len(),
        "[Brew] 开始尝试订阅，共 {} 个候选源",
        urls_to_try.len()
    );

    let parser = FeedParser::new();
    let mut last_error = String::new();
    let mut tried_urls = Vec::new();

    // 遍历尝试每个 URL（限制最多尝试数量）
    for (url, feed_name) in urls_to_try.into_iter().take(MAX_FEED_URLS) {
        // SSRF 防护：校验 URL 安全性
        if let Err(e) = validate_subscribe_url(&url) {
            tracing::warn!(url = %url, error = %e, "[Brew] URL 安全校验失败，跳过");
            last_error = format!("{}: {}", url, e);
            continue;
        }

        tried_urls.push(url.clone());

        // 检查是否已订阅
        let existing = brew_sources::Entity::find()
            .filter(brew_sources::Column::UserId.eq(user_id))
            .filter(brew_sources::Column::Url.eq(&url))
            .one(ctx.db)
            .await
            .map_err(|e| format!("数据库错误: {}", e))?;

        if existing.is_some() {
            tracing::debug!(url = %url, "[Brew] 跳过已订阅的源");
            continue;
        }

        // 尝试解析这个 URL
        tracing::debug!(url = %url, "[Brew] 尝试解析订阅源");

        match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            parser.fetch_and_parse(&url),
        )
        .await
        {
            Ok(Ok(feed)) => {
                // 成功解析！创建订阅
                let now = chrono::Utc::now();
                let name = custom_name
                    .clone()
                    .or(feed_name)
                    .unwrap_or(feed.title.clone());

                let new_source = brew_sources::ActiveModel {
                    user_id: Set(user_id),
                    name: Set(name.clone()),
                    url: Set(url.clone()),
                    feed_type: Set(feed.feed_type.clone()),
                    description: Set(feed.description.clone()),
                    site_url: Set(feed.site_url.clone()),
                    icon: Set(feed.icon.clone()),
                    category: Set(category.map(|s| s.to_string())),
                    enabled: Set(true),
                    error_count: Set(0),
                    item_count: Set(feed.items.len() as i32),
                    unread_count: Set(feed.items.len() as i32),
                    update_interval: Set(update_interval),
                    created_at: Set(now.into()),
                    updated_at: Set(now.into()),
                    ..Default::default()
                };

                let source = new_source
                    .insert(ctx.db)
                    .await
                    .map_err(|e| format!("创建订阅源失败: {}", e))?;

                // 批量构建文章 ActiveModel，一次性 insert 代替 N+1 个单条 insert
                let item_models: Vec<brew_items::ActiveModel> = feed
                    .items
                    .iter()
                    .take(50)
                    .map(|item| {
                        let empty_string = String::new();
                        let content_text = item
                            .content
                            .as_ref()
                            .or(item.summary.as_ref())
                            .unwrap_or(&empty_string);
                        let word_count = content_text.chars().count() as i32;
                        let reading_time = (word_count / 400).max(1);

                        let enclosures_json: Option<serde_json::Value> =
                            if item.enclosures.is_empty() {
                                None
                            } else {
                                Some(
                                    serde_json::to_value(&item.enclosures)
                                        .unwrap_or(serde_json::json!([])),
                                )
                            };
                        let categories_json: Option<serde_json::Value> =
                            if item.categories.is_empty() {
                                None
                            } else {
                                Some(
                                    serde_json::to_value(&item.categories)
                                        .unwrap_or(serde_json::json!([])),
                                )
                            };
                        let published_at = item.published_at.unwrap_or(now);

                        brew_items::ActiveModel {
                            source_id: Set(source.id),
                            guid: Set(item.guid.clone()),
                            title: Set(item.title.clone()),
                            link: Set(item.link.clone()),
                            summary: Set(item.summary.clone()),
                            content: Set(item.content.clone()),
                            author: Set(item.author.clone()),
                            image: Set(item.image.clone()),
                            audio_url: Set(item.audio_url.clone()),
                            video_url: Set(item.video_url.clone()),
                            enclosures: Set(enclosures_json),
                            categories: Set(categories_json),
                            published_at: Set(published_at.into()),
                            fetched_at: Set(now.into()),
                            word_count: Set(Some(word_count)),
                            reading_time: Set(Some(reading_time)),
                            fulltext_fetched: Set(false),
                            ..Default::default()
                        }
                    })
                    .collect();

                let inserted_count = if item_models.is_empty() {
                    0usize
                } else {
                    let total = item_models.len();
                    // 使用批量插入；guid 冲突（已存在）时跳过，避免 N+1 单条 insert
                    match brew_items::Entity::insert_many(item_models)
                        .on_conflict(
                            sea_orm::sea_query::OnConflict::column(brew_items::Column::Guid)
                                .do_nothing()
                                .to_owned(),
                        )
                        .do_nothing()
                        .exec(ctx.db)
                        .await
                    {
                        Ok(_) => total, // InsertResult 不暴露 rows_affected，保守使用 total
                        Err(e) => {
                            tracing::warn!("[Brew] 批量插入文章失败: {}", e);
                            0
                        }
                    }
                };

                tracing::info!(
                    url = %url,
                    name = %name,
                    items = inserted_count,
                    "[Brew] 订阅成功"
                );

                return Ok(json!({
                    "success": true,
                    "sourceId": source.id,
                    "name": name,
                    "url": url,
                    "itemCount": inserted_count,
                    "feedType": match feed.feed_type {
                        brew_sources::FeedType::Rss => "rss",
                        brew_sources::FeedType::Atom => "atom",
                        brew_sources::FeedType::JsonFeed => "json_feed",
                        brew_sources::FeedType::Notion => "notion",
                        brew_sources::FeedType::RssHub => "rsshub",
                    },
                    "triedUrls": tried_urls.len(),
                    "message": crate::services::agent::response_agent::subscribe_success(&name, inserted_count)
                }));
            }
            Ok(Err(e)) => {
                tracing::debug!(url = %url, error = %e, "[Brew] 解析失败，尝试下一个");
                last_error = format!("{}: {}", url, e);
            }
            Err(_) => {
                tracing::debug!(url = %url, "[Brew] 请求超时，尝试下一个");
                last_error = format!("{}: 请求超时", url);
            }
        }
    }

    // 所有 URL 都失败了
    Err(crate::services::agent::response_agent::subscribe_all_failed(tried_urls.len(), &last_error))
}

/// 从 feeds 数组中提取并排序 URL
/// 优先级：已验证 > 官方源 > HTTPS > HTTP
fn extract_and_prioritize_feeds(feeds: &Value) -> Vec<(String, Option<String>)> {
    let Some(feeds_arr) = feeds.as_array() else {
        return vec![];
    };

    let mut result: Vec<(String, Option<String>, i32)> = feeds_arr
        .iter()
        .filter_map(|f| {
            let url = f.get("url")?.as_str()?.to_string();
            let name = f
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let verified = f.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
            let source = f.get("source").and_then(|v| v.as_str()).unwrap_or("");

            // 计算优先级分数（越高越优先）
            let mut score = 0;
            if verified {
                score += 100;
            }
            // 官方源优先
            if source.contains("official") || url.contains("zhihu.com") {
                score += 50;
            }
            // HTTPS 优先
            if url.starts_with("https://") {
                score += 20;
            }
            // 知名服务优先
            if url.contains("feedx.net") || url.contains("feedburner") {
                score += 30;
            }
            // RSSHub 可能不稳定，降低优先级
            if url.contains("rsshub") {
                score -= 10;
            }

            Some((url, name, score))
        })
        .collect();

    // 按分数降序排序
    result.sort_by_key(|b| Reverse(b.2));

    // 返回 URL 和名称
    result
        .into_iter()
        .map(|(url, name, _)| (url, name))
        .collect()
}

async fn execute_brew_mark(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let item_id = params
        .get("itemId")
        .and_then(|v| v.as_i64())
        .ok_or("Missing itemId")? as i32;
    let action = params
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or("Missing action")?;

    let now = Utc::now();
    let user_id = ctx.user_id;

    // 验证文章存在
    let item = brew_items::Entity::find_by_id(item_id)
        .one(ctx.db)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or("Article not found")?;

    // 验证文章所属 source 归当前用户所有，防止越权操作
    let source = brew_sources::Entity::find_by_id(item.source_id)
        .filter(brew_sources::Column::UserId.eq(user_id))
        .one(ctx.db)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or("无权操作该文章")?;

    // 查找或创建用户状态
    let existing = brew_user_states::Entity::find()
        .filter(brew_user_states::Column::UserId.eq(user_id))
        .filter(brew_user_states::Column::ItemId.eq(item_id))
        .one(ctx.db)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

    let (is_read, is_starred) = match action {
        "read" => (Some(true), None),
        "unread" => (Some(false), None),
        "star" => (None, Some(true)),
        "unstar" => (None, Some(false)),
        "later" => (Some(false), Some(true)),
        _ => return Err(format!("Unknown mark action: {}", action)),
    };

    let was_read = existing.as_ref().map(|e| e.is_read).unwrap_or(false);

    if let Some(state) = existing {
        let mut active: brew_user_states::ActiveModel = state.into();
        if let Some(read) = is_read {
            active.is_read = Set(read);
            if read {
                active.read_at = Set(Some(now.into()));
            }
        }
        if let Some(starred) = is_starred {
            active.is_starred = Set(starred);
            if starred {
                active.starred_at = Set(Some(now.into()));
            }
        }
        active.updated_at = Set(now.into());
        active
            .update(ctx.db)
            .await
            .map_err(|e| format!("Failed to update state: {}", e))?;
    } else {
        let new_state = brew_user_states::ActiveModel {
            user_id: Set(user_id),
            item_id: Set(item_id),
            is_read: Set(is_read.unwrap_or(false)),
            is_starred: Set(is_starred.unwrap_or(false)),
            read_at: Set(if is_read == Some(true) {
                Some(now.into())
            } else {
                None
            }),
            starred_at: Set(if is_starred == Some(true) {
                Some(now.into())
            } else {
                None
            }),
            updated_at: Set(now.into()),
            ..Default::default()
        };
        new_state
            .insert(ctx.db)
            .await
            .map_err(|e| format!("Failed to create state: {}", e))?;
    }

    // 更新 source 的 unread_count（附带 user_id 条件，确保仅修改自己的 source）
    if let Some(read) = is_read {
        if read != was_read {
            let delta = if read { -1 } else { 1 };
            let _ = ctx
                .db
                .execute(sea_orm::Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Postgres,
                    "UPDATE brew_sources SET unread_count = GREATEST(unread_count + $1, 0) WHERE id = $2 AND user_id = $3",
                    [delta.into(), source.id.into(), user_id.into()],
                ))
                .await;
        }
    }

    let status = match action {
        "read" => "已读",
        "unread" => "未读",
        "star" => "已收藏",
        "unstar" => "取消收藏",
        "later" => "稍后阅读",
        _ => "未知",
    };

    Ok(json!({
        "success": true,
        "itemId": item_id,
        "action": action,
        "status": status,
        "title": item.title
    }))
}

// ============================================================================
// Content 相关
// ============================================================================

async fn execute_content_write(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let content_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    // 支持从上游步骤通过 inputFrom 解析后注入的内容
    let content = params
        .get("content")
        .or_else(|| params.get("input"))
        .or_else(|| params.get("data"))
        .cloned()
        .unwrap_or(json!(null));
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("未命名内容");

    let content_id = format!("content_{}", Utc::now().timestamp_millis());
    let now = Utc::now();

    let content_data = json!({
        "id": content_id,
        "type": content_type,
        "title": title,
        "content": content,
        "createdAt": now.to_rfc3339()
    });

    // 持久化到 tapp_storage
    let new_record = tapp_storage::ActiveModel {
        tapp_id: Set("agent_content".to_string()),
        user_id: Set(ctx.user_id),
        key: Set(content_id.clone()),
        value: Set(content_data),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };
    new_record
        .insert(ctx.db)
        .await
        .map_err(|e| format!("Failed to save content: {}", e))?;

    Ok(json!({
        "success": true,
        "contentId": content_id,
        "type": content_type,
        "title": title,
        "content": content,
        "frontendAction": {
            "type": "show_notification",
            "params": {
                "title": crate::services::agent::response_agent::content_saved(title),
                "contentId": content_id
            },
            "timestamp": now.timestamp_millis()
        }
    }))
}
