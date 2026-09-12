use super::super::HandlerContext;
use super::platform::extract_platform_items;
use crate::models::entities::{tapp_scheduled_tasks, tapps};
use crate::services::agent::executor::utils::{validate_platform_name, VALID_PLATFORMS};
use crate::services::data_paths::platform_filtered_file;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::{json, Value};
use std::collections::HashMap;

pub(super) async fn execute_platform_connection(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    validate_platform_name(platform)?;

    let configured = {
        let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        crate::api::config::platform_configured_flags(&dynamic)
    };
    let configured_map: HashMap<&str, bool> = configured.iter().copied().collect();

    let platforms: Vec<&str> = if platform == "all" {
        configured.iter().map(|(name, _)| *name).collect()
    } else {
        vec![platform]
    };

    let mut connections = Vec::new();
    for p in platforms {
        let configured = configured_map.get(p).copied().unwrap_or(false);
        let cache_file = crate::services::platform_cache::platform_filtered_cache_path(p)?;
        let meta = tokio::fs::metadata(&cache_file).await.ok();
        let has_data = meta.is_some();
        let last_sync = meta
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());

        connections.push(json!({
            "platform": p,
            "configured": configured,
            "hasData": has_data,
            "connected": configured && has_data,
            "lastSync": last_sync
        }));
    }

    Ok(json!({
        "connections": connections
    }))
}

/// 数据统计概览
pub(super) async fn execute_stats_overview(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let _ = params; // 未使用参数
    let mut stats = json!({
        "totalGames": 0,
        "totalPlaytime": 0,
        "totalAnime": 0,
        "totalBangumiCollections": 0,
        "totalSongs": 0,
        "totalRepos": 0
    });

    // Steam 统计
    if let Ok(content) = tokio::fs::read_to_string(platform_filtered_file("steam")).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            if let Some(games) = data
                .get("content_analysis")
                .and_then(|v| v.get("recent_games"))
                .and_then(|v| v.as_array())
            {
                stats["totalGames"] = json!(games.len());
                let total_time: i64 = games
                    .iter()
                    .filter_map(|g| g.get("playtime").and_then(|t| t.as_i64()))
                    .sum();
                stats["totalPlaytime"] = json!(total_time);
            }
        }
    }

    // Bilibili 统计
    if let Ok(content) = tokio::fs::read_to_string(platform_filtered_file("bilibili")).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            if let Some(anime) = data
                .get("content_analysis")
                .and_then(|v| v.get("anime_analysis"))
                .and_then(|v| v.as_array())
            {
                stats["totalAnime"] = json!(anime.len());
            }
        }
    }

    // Bangumi 统计
    if let Ok(content) = tokio::fs::read_to_string(platform_filtered_file("bangumi")).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let items = extract_platform_items("bangumi", &data);
            stats["totalBangumiCollections"] = json!(items.len());
        }
    }

    // GitHub 统计
    if let Ok(content) = tokio::fs::read_to_string(platform_filtered_file("github")).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            if let Some(repos) = data
                .get("content_analysis")
                .and_then(|v| v.get("recent_repos"))
                .and_then(|v| v.as_array())
            {
                stats["totalRepos"] = json!(repos.len());
            }
        }
    }

    // Netease 统计
    if let Ok(content) = tokio::fs::read_to_string(platform_filtered_file("netease")).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            if let Some(songs) = data
                .get("content_analysis")
                .and_then(|v| v.get("favorite_songs"))
                .and_then(|v| v.as_array())
            {
                stats["totalSongs"] = json!(songs.len());
            }
        }
    }

    Ok(stats)
}

/// 用户画像摘要
pub(super) async fn execute_profile_summary(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let platforms = params
        .get("platforms")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_else(|| VALID_PLATFORMS.to_vec());

    let mut activities = Vec::new();
    let mut platform_stats = json!({});

    for platform in &platforms {
        let cache_file = platform_filtered_file(platform);
        if let Ok(content) = tokio::fs::read_to_string(&cache_file).await {
            if let Ok(data) = serde_json::from_str::<Value>(&content) {
                // 提取用户名
                let username = data
                    .get("user_summary")
                    .and_then(|v| v.get("username"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");

                // 提取统计
                let items = extract_platform_items(platform, &data);

                platform_stats[*platform] = json!({
                    "username": username,
                    "itemCount": items.len()
                });

                // 按平台 slug 推固定 activity 文案
                match *platform {
                    "steam" => activities.push("Gaming".to_string()),
                    "bilibili" => activities.push("Watching anime".to_string()),
                    "bangumi" => activities.push("Collecting anime/books/games".to_string()),
                    "mal" => activities.push("Anime/manga list".to_string()),
                    "github" => activities.push("Coding".to_string()),
                    "netease" => activities.push("Listening to music".to_string()),
                    "x" => activities.push("Posting and interacting".to_string()),
                    "discord" => activities.push("Community chat".to_string()),
                    _ => {}
                }
            }
        }
    }

    Ok(json!({
        "summary": crate::services::agent::response_agent::active_platforms(platforms.len()),
        "activities": activities,
        "platformStats": platform_stats
    }))
}

/// 全局搜索
pub(super) async fn execute_search_global(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let platforms = params
        .get("platforms")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_else(|| VALID_PLATFORMS.to_vec());
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

    // query.is_empty() 则空结果
    if query.is_empty() {
        return Ok(json!({
            "query": query,
            "results": [],
            "total": 0,
            "message": crate::services::agent::response_agent::search_empty_hint(),
            "supportedPlatforms": VALID_PLATFORMS,
            "hint": "Try searching your existing data, for example: 'search my Steam games' or 'look at GitHub repos'"
        }));
    }

    let mut results = Vec::new();
    let query_lower = query.to_lowercase();

    for platform in &platforms {
        let cache_file = platform_filtered_file(platform);
        if let Ok(content) = tokio::fs::read_to_string(&cache_file).await {
            if let Ok(data) = serde_json::from_str::<Value>(&content) {
                let items = extract_platform_items(platform, &data);

                for item in items {
                    // Match `name` then `title` only.
                    let matches = item
                        .get("name")
                        .or(item.get("title"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_lowercase().contains(&query_lower))
                        .unwrap_or(false);

                    if matches {
                        results.push(json!({
                            "platform": platform,
                            "item": item
                        }));

                        if results.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }

        if results.len() >= limit {
            break;
        }
    }

    let message = if results.is_empty() {
        crate::services::agent::response_agent::search_no_results(query)
    } else {
        crate::services::agent::response_agent::search_results_found(results.len(), query)
    };

    Ok(json!({
        "query": query,
        "results": results,
        "total": results.len(),
        "message": message,
        "searchedPlatforms": platforms
    }))
}

/// 查询后台任务状态（agent_tasks / in-memory TASK_STORE）
pub(super) async fn execute_task_status(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    use crate::services::agent::executor::{get_task_for_user, get_user_tasks};
    use crate::services::agent::types::TaskStatus;

    fn status_str(status: &TaskStatus) -> &'static str {
        match status {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::WaitingForInput => "waiting_for_input",
            TaskStatus::Paused => "paused",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    fn task_to_json(task: &crate::services::agent::types::TaskState) -> Value {
        json!({
            "taskId": task.task_id,
            "recipeId": task.recipe_id,
            "status": status_str(&task.status),
            "progress": task.progress,
            "currentStep": task.current_step,
            "error": task.error,
            "startedAt": task.started_at.to_rfc3339(),
            "completedAt": task.completed_at.map(|t| t.to_rfc3339()),
            "pendingQuestion": task.pending_question.as_ref().map(|q| json!({
                "question": q.question,
                "options": q.options,
            })),
        })
    }

    let task_id = params
        .get("taskId")
        .or_else(|| params.get("task_id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if let Some(task_id) = task_id {
        return match get_task_for_user(task_id, ctx.user_id).await {
            Some(task) => Ok(task_to_json(&task)),
            None => Err("Task not found".to_string()),
        };
    }

    // 无 taskId：返回当前用户最近任务列表，不假装单任务已完成
    let mut tasks = get_user_tasks(ctx.user_id).await;
    tasks.sort_by_key(|t| std::cmp::Reverse(t.started_at));
    let limit = std::cmp::min(
        params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20),
        100,
    ) as usize;
    tasks.truncate(limit);

    let items: Vec<Value> = tasks.iter().map(task_to_json).collect();
    Ok(json!({
        "tasks": items,
        "total": items.len(),
        "message": if items.is_empty() {
            "No agent tasks to show"
        } else {
            "Recent agent tasks"
        }
    }))
}

/// Filtered-cache snapshot (`modified` + `len`); not a history query.
pub(super) async fn execute_metadata_history(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10);

    // `tokio::fs::metadata` on `{platform}_filtered.json` only.
    let mut history = Vec::new();
    let cache_file = platform_filtered_file(platform);

    if let Ok(metadata) = tokio::fs::metadata(&cache_file).await {
        if let Ok(modified) = metadata.modified() {
            let modified_time = chrono::DateTime::<chrono::Utc>::from(modified);
            history.push(json!({
                "platform": platform,
                "lastModified": modified_time.to_rfc3339(),
                "size": metadata.len()
            }));
        }
    }

    Ok(json!({
        "platform": platform,
        "history": history,
        "limit": limit,
        "note": "Full history requires database integration"
    }))
}

/// Tapp 列表
pub(super) async fn execute_tapp_list(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let admin_id = crate::services::tapp_ownership::get_admin_user_id(ctx.db)
        .await
        .map_err(|e| e.to_string())?;
    let is_admin = crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await;
    let mut query = tapps::Entity::find();
    if !is_admin {
        query = query.filter(
            tapps::Column::UserId
                .eq(ctx.user_id)
                .or(tapps::Column::UserId.eq(admin_id)),
        );
    }

    let enabled_filter = params.get("enabled").and_then(Value::as_bool);
    let category_filter = params.get("category").and_then(Value::as_str);
    let records = query
        .order_by_desc(tapps::Column::UpdatedAt)
        .all(ctx.db)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to fetch Tapps");
            "Failed to fetch Tapps".to_string()
        })?;
    let items: Vec<Value> = records
        .into_iter()
        .filter(|tapp| {
            crate::services::tapp_ownership::install_visible_to_viewer(
                tapp,
                ctx.user_id,
                admin_id,
                is_admin,
            )
        })
        .filter(|tapp| {
            enabled_filter.is_none_or(|enabled| {
                let active = matches!(
                    tapp.status,
                    tapps::TappStatus::Installed | tapps::TappStatus::Running
                );
                active == enabled
            })
        })
        .filter(|tapp| {
            category_filter.is_none_or(|category| {
                tapp.manifest
                    .get("category")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == category)
            })
        })
        .map(|tapp| {
            json!({
                "id": tapp.tapp_id,
                "name": tapp.name,
                "version": tapp.version,
                "description": tapp.description,
                "icon": tapp.icon,
                "status": format!("{:?}", tapp.status).to_lowercase(),
                "hasCore": crate::services::tapp_package_read::manifest_declares_core(&tapp.manifest),
                "hasPage": crate::services::tapp_package_read::manifest_declares_page(&tapp.manifest),
                "hasWidget": crate::services::tapp_package_read::manifest_declares_widgets(&tapp.manifest),
                "backgroundRequirements": tapp.manifest
                    .get("backgroundRequirements")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            })
        })
        .collect();

    Ok(json!({
        "tapps": items,
        "total": items.len()
    }))
}

/// 定时任务列表
pub(super) async fn execute_scheduler_list(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let tapp_id = params.get("tappId").and_then(Value::as_str);
    let enabled_filter = params.get("enabled").and_then(Value::as_bool);
    let scheduler = crate::services::tapp_scheduler::scheduler_engine()?;
    let scheduler = scheduler.read().await;
    let tasks = scheduler.list_tasks(ctx.user_id, tapp_id).await?;
    let tasks: Vec<Value> = tasks
        .into_iter()
        .filter(|task| enabled_filter.is_none_or(|enabled| task.enabled == enabled))
        .map(|task| {
            let schedule_type = match task.schedule_type {
                tapp_scheduled_tasks::ScheduleType::Cron => "cron",
                tapp_scheduled_tasks::ScheduleType::Interval => "interval",
                tapp_scheduled_tasks::ScheduleType::Once => "once",
                tapp_scheduled_tasks::ScheduleType::Daily => "daily",
            };
            let execution_target = match task.execution_target {
                tapp_scheduled_tasks::ExecutionTarget::Backend => "backend",
                tapp_scheduled_tasks::ExecutionTarget::Frontend => "frontend",
                tapp_scheduled_tasks::ExecutionTarget::Both => "both",
            };
            let scope = match task.scope {
                tapp_scheduled_tasks::TaskScope::User => "user",
                tapp_scheduled_tasks::TaskScope::Tapp => "tapp",
                tapp_scheduled_tasks::TaskScope::TappPerUser => "tapp-per-user",
                tapp_scheduled_tasks::TaskScope::Global => "global",
            };
            json!({
                "id": task.id,
                "taskId": task.task_id,
                "tappId": task.tapp_id,
                "name": task.name,
                "scheduleType": schedule_type,
                "schedule": task.schedule_config,
                "payload": task.payload,
                "executionTarget": execution_target,
                "backendActions": task.backend_actions,
                "enabled": task.enabled,
                "scope": scope,
                "nextRunAt": task.next_run_at.map(|value| value.to_rfc3339()),
                "lastRunAt": task.last_run_at.map(|value| value.to_rfc3339()),
                "lastRunResult": task.last_run_result,
                "stats": task.stats,
            })
        })
        .collect();

    Ok(json!({
        "tasks": tasks,
        "total": tasks.len()
    }))
}

/// Agent Heartbeat 任务列表（HEARTBEAT.md）
pub(super) async fn execute_heartbeat_list(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    if !crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await {
        return Err("Heartbeat admin required".to_string());
    }
    let manager = crate::services::agent::heartbeat::get_heartbeat()
        .ok_or_else(|| "Heartbeat not initialized".to_string())?;
    let enabled_filter = params.get("enabled").and_then(Value::as_bool);
    let tasks: Vec<Value> = manager
        .get_tasks()
        .await
        .into_iter()
        .filter(|t| enabled_filter.is_none_or(|en| t.enabled == en))
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "schedule": t.schedule,
                "action": t.action,
                "enabled": t.enabled,
                "lastRun": t.last_run.map(|dt| dt.to_rfc3339()),
                "lastResult": t.last_result,
            })
        })
        .collect();
    let total = tasks.len();
    Ok(json!({
        "tasks": tasks,
        "total": total
    }))
}

/// RSSHub 实例列表（来自 brew 的 rsshub_instances 表）
pub(super) async fn execute_rsshub_instances(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    use crate::models::entities::rsshub_instances::{self, HealthStatus};
    use crate::services::rsshub_service::RsshubService;

    let _ = params;
    let service = RsshubService::new(ctx.db.clone());

    service.ensure_default_instances().await.map_err(|e| {
        tracing::error!(error = %e, "Agent RSSHub default instances failed");
        "Could not set up RSSHub".to_string()
    })?;

    let instances = service
        .get_instances(Some(ctx.user_id))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Agent RSSHub instances read failed");
            "Could not read RSSHub instances".to_string()
        })?;

    if instances.is_empty() {
        return Err("No RSSHub instances are configured".to_string());
    }

    let healthy_count = instances
        .iter()
        .filter(|i| i.enabled && matches!(i.health_status, HealthStatus::Healthy))
        .count();

    let items: Vec<Value> = instances
        .into_iter()
        .map(|m| {
            let response: rsshub_instances::InstanceResponse = m.into();
            json!({
                "id": response.id,
                "name": response.name,
                "url": response.url,
                "enabled": response.enabled,
                "priority": response.priority,
                "healthStatus": response.health_status,
                "lastHealthCheck": response.last_health_check,
                "lastResponseTimeMs": response.last_response_time_ms,
                "consecutiveFailures": response.consecutive_failures,
                "successRate": response.success_rate,
                "isGlobal": response.user_id.is_none(),
            })
        })
        .collect();

    Ok(json!({
        "instances": items,
        "total": items.len(),
        "healthyCount": healthy_count,
    }))
}

/// 上下文引用能力
pub(super) async fn execute_context_reference(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let value = params
        .get("value")
        .cloned()
        .ok_or_else(|| "Referenced step output not found".to_string())?;
    let type_name = match &value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    Ok(json!({
        "value": value,
        "type": type_name
    }))
}

// 补充能力

/// 数据库查询 (anime/game/artist)
pub(super) async fn execute_database_query(
    capability_id: &str,
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let db_type = capability_id.split('.').next_back().unwrap_or("anime");
    let title_query = params
        .get("title")
        .or(params.get("name"))
        .and_then(|v| v.as_str());
    let genre_query = params.get("genre").and_then(|v| v.as_str());

    let db_file = format!("data/{}_database.json", db_type);
    let content = tokio::fs::read_to_string(&db_file).await.map_err(|_| {
        format!(
            "{} database file is missing ({}). Import the related data first.",
            db_type, db_file
        )
    })?;

    let data: Value = serde_json::from_str(&content).unwrap_or(json!({}));
    let entries = data
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = entries
        .into_iter()
        .filter(|entry| {
            let title_match = title_query
                .map(|q| {
                    entry
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(|t| t.to_lowercase().contains(&q.to_lowercase()))
                        .unwrap_or(false)
                })
                .unwrap_or(true);

            let genre_match = genre_query
                .map(|q| {
                    entry
                        .get("genres")
                        .or(entry.get("genre"))
                        .and_then(|v| v.as_array())
                        .map(|genres| {
                            genres.iter().any(|g| {
                                g.as_str()
                                    .map(|s| s.to_lowercase().contains(&q.to_lowercase()))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(true);

            title_match && genre_match
        })
        .take(50)
        .collect();

    Ok(json!({
        "database": db_type,
        "results": results,
        "count": results.len()
    }))
}

/// 随机内容
pub(super) async fn execute_random_content(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("steam");
    let count = params.get("count").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    let cache_file = platform_filtered_file(platform);
    if let Ok(content) = tokio::fs::read_to_string(&cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let items =
                crate::services::platform_items::extract_platform_items_for_random(platform, &data);

            // 随机选取
            use rand::seq::IndexedRandom;
            let mut rng = rand::rng();
            let selected: Vec<_> = items
                .sample(&mut rng, count.min(items.len()))
                .cloned()
                .collect();

            return Ok(json!({
                "platform": platform,
                "items": selected,
                "totalAvailable": items.len()
            }));
        }
    }

    Err(format!("No data available for platform: {}", platform))
}

/// 报告列表：平台报告走 `platform_reports`；Agent `report.create` 走 `tapp_storage`。
pub(super) async fn execute_report_list(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    use crate::models::entities::tapp_storage;
    use crate::services::agent::resource_create_pure::AGENT_REPORTS_TAPP_ID;
    use crate::services::tapp_reports::{list_user_platform_reports, platform_report_list_item};

    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .clamp(1, 50) as usize;
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let preferred = crate::api::reports::public_report_owner_user_id(ctx.db).await;
    let owner_id = crate::api::reports::resolve_report_user_id_for_public_read(ctx.db, preferred)
        .await
        .unwrap_or(preferred);

    let rows = list_user_platform_reports(ctx.db, owner_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut seen = std::collections::HashSet::new();
    let mut reports = Vec::new();
    for row in rows {
        if row.platform == "all" {
            continue;
        }
        if let Some(p) = platform {
            if !row.platform.eq_ignore_ascii_case(p) {
                continue;
            }
        }
        if !seen.insert(row.platform.clone()) {
            continue;
        }
        reports.push(platform_report_list_item(&row));
        if reports.len() >= limit {
            break;
        }
    }

    if platform.is_none() && reports.len() < limit {
        let remaining = (limit - reports.len()) as u64;
        let agent_rows = tapp_storage::Entity::find()
            .filter(tapp_storage::Column::TappId.eq(AGENT_REPORTS_TAPP_ID))
            .filter(tapp_storage::Column::UserId.eq(ctx.user_id))
            .order_by_desc(tapp_storage::Column::CreatedAt)
            .limit(remaining)
            .all(ctx.db)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to list agent reports");
                "Failed to list agent reports".to_string()
            })?;
        for row in agent_rows {
            let value = &row.value;
            reports.push(json!({
                "id": value.get("id").cloned().unwrap_or(json!(row.key)),
                "title": value.get("title").and_then(Value::as_str).unwrap_or(""),
                "type": "agent",
                "format": value.get("format").and_then(Value::as_str).unwrap_or("markdown"),
                "createdAt": value
                    .get("createdAt")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| row.created_at.to_rfc3339()),
            }));
        }
    }

    Ok(json!({
        "reports": reports,
        "total": reports.len()
    }))
}
