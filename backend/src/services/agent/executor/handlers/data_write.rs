//! 数据写入能力处理器
//!
//! 处理 platform.write, phantasi.subscribe, phantasi.mark 等写入类能力。
//! 纯 URL/名称/feed 优先级规则见 [`crate::services::agent::data_write_pure`]。

use super::HandlerContext;
use crate::models::entities::{
    phantasi_items, phantasi_sources, phantasi_user_states, tapp_storage,
};
use crate::services::agent::data_write_pure::{
    clamp_update_interval_minutes, collect_subscribe_url_candidates, is_disallowed_subscribe_ip,
    platform_write_cap_error, platform_write_items_over_cap, sanitize_feed_name,
    take_feed_urls_to_try, validate_subscribe_url_policy,
};
use crate::services::agent::executor::utils::VALID_PLATFORMS;
use crate::services::agent::executor::utils::validate_platform_name;
use crate::services::agent::external_pure::first_i64_param;
use crate::services::data_paths::platform_filtered_file;
use crate::services::phantasi_parser::FeedParser;
use crate::services::tapp_storage::{
    read_storage_value, sandbox_storage_entries, validate_sandbox_storage_key,
    validate_storage_value_size, write_storage_value,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QuerySelect,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::ToSocketAddrs;

fn write_store_failed(context: &'static str, error: impl std::fmt::Display) -> String {
    tracing::error!(%error, context, "agent data_write store failed");
    format!("Failed to {context}")
}

/// 验证订阅 URL 安全性，防止 SSRF（纯策略 + DNS 解析检查）。
fn validate_subscribe_url(url: &str) -> Result<(), String> {
    validate_subscribe_url_policy(url)?;
    let parsed = url::Url::parse(url).map_err(|_| "Invalid URL".to_string())?;
    let host = parsed.host_str().ok_or("This URL is missing a host")?;
    // Hostname path: resolve and reject private IPs (IO).
    if host.parse::<std::net::IpAddr>().is_err() {
        let port = parsed
            .port()
            .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });
        let addr_str = format!("{host}:{port}");
        if let Ok(addrs) = addr_str.to_socket_addrs() {
            for addr in addrs {
                if is_disallowed_subscribe_ip(addr.ip()) {
                    return Err("This address is not allowed".to_string());
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
        "phantasi.subscribe" => execute_phantasi_subscribe(params, ctx).await,
        "phantasi.mark" => execute_phantasi_mark(params, ctx).await,
        "content.write" => execute_content_write(params, ctx).await,
        "seo.apply" => super::seo::execute_seo_apply(params, ctx).await,
        _ => Err(format!("Unknown data_write capability: {}", capability_id)),
    }
}

// Platform 相关

async fn execute_platform_write(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform_raw = params
        .get("platform")
        .and_then(|v| v.as_str())
        .ok_or("Missing platform parameter")?;

    // Whitelist: `all` or VALID_PLATFORMS (no `..` / separator check).
    let platform = validate_platform_name(platform_raw)?;

    let items = params.get("items").ok_or("Missing items parameter")?;

    // 限制单次写入条目数量
    if let Some(arr) = items.as_array() {
        if platform_write_items_over_cap(arr.len()) {
            return Err(platform_write_cap_error());
        }
    }

    let cache_file = platform_filtered_file(platform);

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
    .map_err(|e| {
        tracing::error!("Failed to write data: {e}");
        "Failed to write data".to_string()
    })?;

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
        match crate::services::background_processor::submit_and_start_platform_task(p.to_string())
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
                tracing::warn!(platform = %p, error = %e, "[platform.refresh] submit failed");
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

// Storage 相关

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
    let user_id = ctx.user_id;
    validate_sandbox_storage_key(key).map_err(str::to_string)?;
    validate_storage_value_size(&value).map_err(|error| error.to_string())?;
    write_storage_value(ctx.db, user_id, namespace, key, value.clone())
        .await
        .map_err(|error| error.to_string())?;

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
    if let Some(key) = key {
        validate_sandbox_storage_key(key).map_err(str::to_string)?;
    }

    match action {
        "get" => {
            if let Some(key) = key {
                let value = read_storage_value(ctx.db, user_id, tapp_id, key)
                    .await
                    .map_err(|error| error.to_string())?;

                Ok(json!({
                    "success": true,
                    "key": key,
                    "value": value
                }))
            } else {
                let results = sandbox_storage_entries(ctx.db, user_id, tapp_id)
                    .await
                    .map_err(|error| error.to_string())?;

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
            validate_storage_value_size(&value).map_err(|error| error.to_string())?;
            write_storage_value(ctx.db, user_id, tapp_id, key, value.clone())
                .await
                .map_err(|error| error.to_string())?;

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
                .map_err(|error| write_store_failed("delete Tapp storage", error))?;

            Ok(json!({
                "success": true,
                "key": key,
                "deleted": result.rows_affected > 0
            }))
        }
        _ => Err(format!("Unknown storage action: {}", action)),
    }
}

// Phantasi 相关

/// 执行订阅源添加 - 支持智能尝试多个源
async fn execute_phantasi_subscribe(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let user_id = ctx.user_id;

    tracing::debug!(
        params_keys = ?params.keys().collect::<Vec<_>>(),
        "[Phantasi Subscribe] received params"
    );

    // 校验并清洗用户提供的名称，防止过长或空白字符串入库
    let custom_name: Option<String> = params
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(sanitize_feed_name)
        .transpose()?;
    let category = params.get("category").and_then(|v| v.as_str());
    let update_interval = clamp_update_interval_minutes(
        params
            .get("updateInterval")
            .and_then(|v| v.as_i64())
            .unwrap_or(30) as i32,
    );

    // 收集要尝试的 URL 列表
    if params.get("feeds").is_some() {
        tracing::debug!(feeds = ?params.get("feeds"), "[Phantasi Subscribe] extracting URLs from feeds");
    } else if let Some(url) = params.get("url").and_then(|v| v.as_str()) {
        tracing::debug!(url = %url, "[Phantasi Subscribe] using a single URL");
    } else {
        tracing::warn!("[Phantasi Subscribe] missing url and feeds");
    }
    let urls_to_try = collect_subscribe_url_candidates(
        params.get("feeds"),
        params.get("url").and_then(|v| v.as_str()),
    )?;

    tracing::info!(
        count = urls_to_try.len(),
        "[Phantasi] trying {} candidate feeds",
        urls_to_try.len()
    );

    let parser = FeedParser::new();
    let mut last_error = String::new();
    let mut tried_urls = Vec::new();

    // 遍历尝试每个 URL（限制最多尝试数量）
    for (url, feed_name) in take_feed_urls_to_try(urls_to_try) {
        // SSRF 防护：校验 URL 安全性
        if let Err(e) = validate_subscribe_url(&url) {
            tracing::warn!(url = %url, error = %e, "[Phantasi] URL failed security check, skip");
            last_error = format!("{}: {}", url, e);
            continue;
        }

        tried_urls.push(url.clone());

        // 检查是否已订阅
        let existing = phantasi_sources::Entity::find()
            .filter(phantasi_sources::Column::Url.eq(&url))
            .one(ctx.db)
            .await
            .map_err(|error| write_store_failed("check existing phantasi source", error))?;

        if existing.is_some() {
            tracing::debug!(url = %url, "[Phantasi] skip already subscribed feed");
            continue;
        }

        // 尝试解析这个 URL
        tracing::debug!(url = %url, "[Phantasi] trying to parse feed");

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

                // item_count filled after insert from rows_affected
                // (take(50) + ON CONFLICT skips must not over-count).
                let new_source = phantasi_sources::ActiveModel {
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
                    item_count: Set(0),
                    unread_count: Set(0),
                    update_interval: Set(update_interval),
                    created_at: Set(now.into()),
                    updated_at: Set(now.into()),
                    ..Default::default()
                };

                let source = new_source.insert(ctx.db).await.map_err(|e| {
                    tracing::error!("Failed to create phantasi source: {e}");
                    "Failed to create feed".to_string()
                })?;

                // 批量构建文章 ActiveModel，一次性 insert 代替 N+1 个单条 insert
                let item_models: Vec<phantasi_items::ActiveModel> = feed
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

                        phantasi_items::ActiveModel {
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
                            // 与调度器一样：先 NULL，插入后再异步让 AI 建议。
                            topic: Set(None),
                            ..Default::default()
                        }
                    })
                    .collect();

                // Shared insertion keeps RSS references atomic and counts only inserted rows.
                let inserted_count = if item_models.is_empty() {
                    0usize
                } else {
                    match crate::services::phantasi_scheduler::insert_feed_items(
                        ctx.db,
                        item_models,
                    )
                    .await
                    {
                        Ok(rows) => rows.len(),
                        Err(e) => {
                            tracing::warn!("[Phantasi] bulk insert items failed: {}", e);
                            0
                        }
                    }
                };

                if inserted_count > 0 {
                    let mut source_active: phantasi_sources::ActiveModel = source.clone().into();
                    source_active.item_count = Set(inserted_count as i32);
                    if let Err(e) = source_active.update(ctx.db).await {
                        tracing::warn!("[Phantasi] failed to update source counts: {}", e);
                    }
                    let topic_db = ctx.db.clone();
                    let source_id = source.id;
                    tokio::spawn(async move {
                        crate::services::phantasi_topics::recommend_unlabeled_for_source(
                            &topic_db,
                            source_id,
                            inserted_count,
                        )
                        .await;
                    });
                }

                tracing::info!(
                    url = %url,
                    name = %name,
                    items = inserted_count,
                    "[Phantasi] subscribed"
                );

                return Ok(json!({
                    "success": true,
                    "sourceId": source.id,
                    "name": name,
                    "url": url,
                    "itemCount": inserted_count,
                    "feedType": match feed.feed_type {
                        phantasi_sources::FeedType::Rss => "rss",
                        phantasi_sources::FeedType::Atom => "atom",
                        phantasi_sources::FeedType::JsonFeed => "json_feed",
                        phantasi_sources::FeedType::Notion => "notion",
                        phantasi_sources::FeedType::RssHub => "rsshub",
                    },
                    "triedUrls": tried_urls.len(),
                    "message": crate::services::agent::response_agent::subscribe_success(&name, inserted_count)
                }));
            }
            Ok(Err(e)) => {
                tracing::debug!(url = %url, error = %e, "[Phantasi] parse failed, try next");
                last_error = format!("{}: {}", url, e);
            }
            Err(_) => {
                tracing::debug!(url = %url, "[Phantasi] request timed out, try next");
                last_error = format!("{url}: timed out");
            }
        }
    }

    // No new source created (SSRF skip / already-subscribed / parse fail).
    Err(crate::services::agent::response_agent::subscribe_all_failed(tried_urls.len(), &last_error))
}

async fn execute_phantasi_mark(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let item_id = first_i64_param(params, &["itemId", "item_id"]).ok_or("Missing itemId")? as i32;
    let action = params
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or("Missing action")?;

    let now = Utc::now();
    let user_id = ctx.user_id;

    // 验证文章存在
    let (source_id, title) = phantasi_items::Entity::find_by_id(item_id)
        .select_only()
        .columns([
            phantasi_items::Column::SourceId,
            phantasi_items::Column::Title,
        ])
        .into_tuple::<(i32, String)>()
        .one(ctx.db)
        .await
        .map_err(|error| write_store_failed("find article", error))?
        .ok_or("Article not found")?;

    // 共享订阅库：按可见源标状态，不按创建者
    let is_admin = crate::services::agent::user_is_current_admin(ctx.db, user_id).await;
    let source = phantasi_sources::Entity::find_by_id(source_id)
        .one(ctx.db)
        .await
        .map_err(|error| write_store_failed("find phantasi source", error))?
        .ok_or("This article cannot be changed")?;
    if source.admin_only && !is_admin {
        return Err("This article cannot be changed".to_string());
    }
    if matches!(action, "star" | "unstar" | "later") && !is_admin {
        return Err("Forbidden".to_string());
    }

    // 查找或创建用户状态
    let existing = phantasi_user_states::Entity::find()
        .filter(phantasi_user_states::Column::UserId.eq(user_id))
        .filter(phantasi_user_states::Column::ItemId.eq(item_id))
        .one(ctx.db)
        .await
        .map_err(|error| write_store_failed("find reading state", error))?;

    let (is_read, is_starred) = match action {
        "read" => (Some(true), None),
        "unread" => (Some(false), None),
        "star" => (None, Some(true)),
        "unstar" => (None, Some(false)),
        "later" => (Some(false), Some(true)),
        _ => return Err(format!("Unknown mark action: {}", action)),
    };

    let was_starred = existing.as_ref().map(|e| e.is_starred).unwrap_or(false);

    if let Some(state) = existing {
        let mut active: phantasi_user_states::ActiveModel = state.into();
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
            .map_err(|error| write_store_failed("update reading state", error))?;
    } else {
        let new_state = phantasi_user_states::ActiveModel {
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
            .map_err(|error| write_store_failed("create reading state", error))?;
    }

    if is_starred == Some(true) && !was_starred {
        crate::services::agent::merope::spawn_ingest(
            user_id,
            "phantasi.starred",
            format!("Starred \"{}\"", title),
        );
    }

    let status = match action {
        "read" => "Read",
        "unread" => "Unread",
        "star" => "Starred",
        "unstar" => "Unstarred",
        "later" => "Read later",
        _ => "Unknown",
    };

    Ok(json!({
        "success": true,
        "itemId": item_id,
        "action": action,
        "status": status,
        "title": title
    }))
}

// Content 相关

async fn execute_content_write(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let content_type = params
        .get("contentType")
        .or_else(|| params.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    // 支持从上游步骤通过 inputFrom 解析后注入的内容
    let content = params
        .get("content")
        .or_else(|| params.get("input"))
        .or_else(|| params.get("data"))
        .cloned()
        .unwrap_or(json!(null));
    let target = params.get("target");
    let target_type = target
        .and_then(|target| target.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("storage");
    let text = match &content {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if target_type == "clipboard" {
        return Ok(json!({
            "success": true,
            "target": "clipboard",
            "frontendAction": {
                "type": "copy_clipboard",
                "value": text,
                "timestamp": Utc::now().timestamp_millis()
            }
        }));
    }
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled content");
    if target_type == "file" {
        let filename = target
            .and_then(|t| t.get("name"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("{title}.{}", content_type_extension(content_type)));
        return Ok(json!({
            "success": true,
            "target": "file",
            "filename": filename,
            "frontendAction": {
                "type": "download_file",
                "params": {
                    "filename": filename,
                    "format": content_type,
                    "content": text
                },
                "timestamp": Utc::now().timestamp_millis()
            }
        }));
    }

    let tapp_id = match target_type {
        "tapp" => target
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("Missing target.id for tapp")?,
        "storage" => target
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("agent_storage"),
        other => {
            return Err(format!("Unknown content target: {other}"));
        }
    };
    if target_type == "tapp" {
        crate::services::tapp_ownership::verify_tapp_ownership(ctx.db, ctx.user_id, tapp_id)
            .await
            .map_err(|err| err.to_string())?;
    }

    let key = target
        .and_then(|t| t.get("name"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("content_{}", Utc::now().timestamp_millis()));
    validate_sandbox_storage_key(&key).map_err(str::to_string)?;
    let mut value = content.clone();
    if params
        .get("append")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        if let Ok(existing) = read_storage_value(ctx.db, ctx.user_id, tapp_id, &key).await {
            if !existing.is_null() {
                value = append_storage_value(existing, content);
            }
        }
    }
    validate_storage_value_size(&value).map_err(|error| error.to_string())?;
    write_storage_value(ctx.db, ctx.user_id, tapp_id, &key, value.clone())
        .await
        .map_err(|error| error.to_string())?;

    let now = Utc::now();
    Ok(json!({
        "success": true,
        "target": target_type,
        "targetId": tapp_id,
        "contentId": key,
        "type": content_type,
        "title": title,
        "content": value,
        "frontendAction": {
            "type": "show_notification",
            "params": {
                "title": crate::services::agent::response_agent::content_saved(title),
                "contentId": key
            },
            "timestamp": now.timestamp_millis()
        }
    }))
}

fn content_type_extension(content_type: &str) -> &'static str {
    match content_type {
        "markdown" => "md",
        "html" => "html",
        "json" => "json",
        _ => "txt",
    }
}

fn append_storage_value(existing: Value, incoming: Value) -> Value {
    match (existing, incoming) {
        (Value::String(left), Value::String(right)) => Value::String(format!("{left}{right}")),
        (Value::Array(mut left), Value::Array(right)) => {
            left.extend(right);
            Value::Array(left)
        }
        (Value::Array(mut left), right) => {
            left.push(right);
            Value::Array(left)
        }
        (left, right) => json!([left, right]),
    }
}

#[cfg(test)]
mod phantasi_mark_visibility_tests {
    #[test]
    fn mark_uses_visibility_not_source_owner() {
        let src = include_str!("data_write.rs");
        let start = src
            .find("async fn execute_phantasi_mark")
            .expect("execute_phantasi_mark");
        let body = &src[start..];
        let end = body[1..]
            .find("\nasync fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let mark = &body[..end];
        assert!(mark.contains("user_is_current_admin"));
        assert!(mark.contains("source.admin_only"));
        assert!(
            mark.contains("\"star\" | \"unstar\" | \"later\"") && mark.contains("!is_admin"),
            "star writes are admin-only host identity"
        );
        assert!(
            !mark.contains("source.user_id") && !mark.contains("phantasi_sources::Column::UserId"),
            "shared catalog marks visible sources, not the creator"
        );
        assert!(
            !mark.contains("unread_count"),
            "HTTP list overlays per-user unread; Agent mark must not write the site-wide column"
        );
    }

    #[test]
    fn subscribe_does_not_write_site_unread() {
        let src = include_str!("data_write.rs");
        let start = src
            .find("async fn execute_phantasi_subscribe")
            .expect("subscribe");
        let body = &src[start..];
        let end = body[1..]
            .find("\nasync fn execute_phantasi_mark")
            .or_else(|| body[1..].find("\nasync fn "))
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let subscribe = &body[..end];
        let after_insert = subscribe
            .split("if inserted_count > 0")
            .nth(1)
            .expect("subscribe count update");
        assert!(
            !after_insert.contains("unread_count"),
            "Agent subscribe must not increment site-wide source unread_count after insert"
        );
    }
}
