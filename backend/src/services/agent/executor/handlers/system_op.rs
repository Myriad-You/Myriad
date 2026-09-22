//! 系统操作能力处理器
//!
//! 处理 data.transform, scheduler.create, cache.status 等系统操作类能力。
//! 纯参数投影见 [`crate::services::agent::system_op_pure`]；
//! data.transform 管道复用 [`crate::services::tapp_data_transform`]。

use super::HandlerContext;
use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::models::entities::tapp_scheduled_tasks::{
    ExecutionTarget, MissedPolicy, ScheduleType, TaskScope,
};
use crate::services::agent::executor::utils::{
    VALID_PLATFORMS, is_valid_platform, is_valid_platform as validate_platform_name,
};
use crate::services::agent::external_pure::first_string_param;
use crate::services::agent::system_op_pure::{
    AgentExecutionTarget, AgentScheduleType, PhantasiScheduleAction, extract_raw_backend_actions,
    heartbeat_task_id, heartbeat_update_has_fields, parse_execution_target,
    parse_phantasi_schedule_action, parse_schedule_type,
};
use crate::services::background_processor::BACKGROUND_PROCESSOR;
use crate::services::data_paths::platform_filtered_file;
use crate::services::image_cache::ImageCacheService;
use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};
use crate::services::phantasi_scheduler::get_phantasi_scheduler;
use crate::services::tapp_data_transform::{
    DataTransformError, apply_pipeline, items_from_agent_input, parse_pipeline_value,
};
use crate::services::tapp_ownership::verify_tapp_ownership;
use crate::services::tapp_scheduler::{
    backend_action_permissions_of, normalize_backend_actions_parsed, scheduler_engine,
};
use serde_json::{Value, json};
use std::collections::HashMap;

/// 执行系统操作能力
pub async fn execute(
    capability_id: &str,
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    match capability_id {
        "data.transform" => execute_data_transform(params).await,
        "scheduler.create" => execute_scheduler_create(params, ctx).await,
        "scheduler.trigger" => execute_scheduler_trigger(params, ctx).await,
        "heartbeat.create" => execute_heartbeat_create(params, ctx).await,
        "heartbeat.update" => execute_heartbeat_update(params, ctx).await,
        "heartbeat.delete" => execute_heartbeat_delete(params, ctx).await,
        "heartbeat.toggle" => execute_heartbeat_toggle(params, ctx).await,
        "system.metrics" => execute_system_metrics().await,
        "cache.status" => execute_cache_status(params).await,
        "cache.clear" => execute_cache_clear(params).await,
        "rsshub.healthcheck" => execute_rsshub_healthcheck(params, ctx).await,
        "image.cache" => execute_image_cache(params).await,
        "export.data" => execute_export_data(params).await,
        "task.submit" => execute_task_submit(params).await,
        "phantasi.schedule" => execute_phantasi_schedule(params).await,
        "setup.status" => execute_setup_status(ctx).await,
        _ => Err(format!("Unknown system_op capability: {}", capability_id)),
    }
}

// 数据转换

async fn execute_data_transform(params: &HashMap<String, Value>) -> Result<Value, String> {
    let input = params.get("input").cloned().unwrap_or(json!([]));
    let steps = parse_pipeline_value(params.get("pipeline")).map_err(|err| match err {
        DataTransformError::TooManySteps => "Too many pipeline steps".to_string(),
        DataTransformError::InvalidStep => "Invalid pipeline step".to_string(),
        DataTransformError::InvalidPipeline => "pipeline must be an array".to_string(),
        other => other.message().to_string(),
    })?;
    let items = apply_pipeline(items_from_agent_input(input), steps).map_err(|err| match err {
        DataTransformError::TooManySteps => "Too many pipeline steps".to_string(),
        other => other.message().to_string(),
    })?;

    let count = items.len();
    Ok(json!({
        "data": items,
        "count": count,
        "frontendAction": {
            "type": "show_data",
            "params": {
                "count": count,
                "preview": items.iter().take(3).cloned().collect::<Vec<_>>()
            },
            "timestamp": chrono::Utc::now().timestamp_millis()
        }
    }))
}

// 调度器

async fn execute_scheduler_create(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let tapp_id = params
        .get("tappId")
        .or_else(|| params.get("tapp_id"))
        .and_then(Value::as_str)
        .ok_or("Missing tappId parameter")?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("Missing name parameter")?;

    verify_tapp_ownership(ctx.db, ctx.user_id, tapp_id)
        .await
        .map_err(|err| err.to_string())?;

    let schedule_type_name = params
        .get("scheduleType")
        .or_else(|| params.get("schedule_type"))
        .and_then(Value::as_str);
    let agent_schedule = parse_schedule_type(schedule_type_name)?;
    let schedule_type = match agent_schedule {
        AgentScheduleType::Cron => ScheduleType::Cron,
        AgentScheduleType::Interval => ScheduleType::Interval,
        AgentScheduleType::Once => ScheduleType::Once,
        AgentScheduleType::Daily => ScheduleType::Daily,
    };

    let schedule_config = params
        .get("schedule")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| "Missing schedule object".to_string())?;

    let raw_backend_actions = extract_raw_backend_actions(params);
    let (backend_actions, wrappers) = normalize_backend_actions_parsed(raw_backend_actions)?;
    let execution_target_name = params
        .get("executionTarget")
        .or_else(|| params.get("execution_target"))
        .and_then(Value::as_str);
    let agent_target = parse_execution_target(execution_target_name, backend_actions.is_some())?;
    let execution_target = match agent_target {
        AgentExecutionTarget::Backend => ExecutionTarget::Backend,
        AgentExecutionTarget::Frontend => ExecutionTarget::Frontend,
        AgentExecutionTarget::Both => ExecutionTarget::Both,
    };
    if agent_target.requires_backend_actions()
        && backend_actions
            .as_ref()
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        return Err(
            "backendActions are required when executionTarget is backend or both".to_string(),
        );
    }

    let role = if crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await {
        UserRole::Admin
    } else {
        UserRole::User
    };
    let mut required_permissions = vec![TappPermission::SchedulerRegister];
    required_permissions.extend(backend_action_permissions_of(&wrappers));
    {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        for permission in required_permissions {
            if !TappPermissionService::check(&config, role, permission) {
                return Err(format!(
                    "Permission denied for scheduled action: {}",
                    permission.as_str()
                ));
            }
        }
    }

    let missed_policy_name = params
        .get("missedPolicy")
        .or_else(|| params.get("missed_policy"))
        .and_then(Value::as_str)
        .unwrap_or("skip");
    let missed_policy = match missed_policy_name.to_ascii_lowercase().as_str() {
        "skip" => MissedPolicy::Skip,
        "run-once" | "runonce" => MissedPolicy::RunOnce,
        "run-all" | "runall" => MissedPolicy::RunAll,
        _ => return Err(format!("Invalid missedPolicy: {missed_policy_name}")),
    };

    let retry_config = params.get("retry").map(|retry| {
        json!({
            "max_retries": retry
                .get("maxRetries")
                .or_else(|| retry.get("max_retries"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            "retry_delay": retry
                .get("retryDelay")
                .or_else(|| retry.get("retry_delay"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
        })
    });
    let task_id = params
        .get("taskId")
        .or_else(|| params.get("task_id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("agent_{}", uuid::Uuid::new_v4().simple()));
    let payload = params.get("payload").cloned();

    let scheduler = scheduler_engine()?;
    let task = scheduler
        .register_task(
            ctx.user_id,
            tapp_id,
            &task_id,
            name,
            schedule_type,
            schedule_config.clone(),
            payload,
            execution_target,
            backend_actions,
            missed_policy,
            TaskScope::User,
            retry_config,
        )
        .await?;

    let now = chrono::Utc::now();
    tracing::info!(
        task_id = %task.task_id,
        tapp_id = %task.tapp_id,
        user_id = ctx.user_id,
        "[SchedulerCreate] Registered real Tapp scheduler task"
    );

    Ok(json!({
        "success": true,
        "taskId": task.task_id,
        "tappId": task.tapp_id,
        "name": task.name,
        "scheduleType": agent_schedule.as_str(),
        "schedule": schedule_config,
        "nextRun": task.next_run_at.map(|value| value.to_rfc3339()),
        "frontendAction": {
            "type": "show_notification",
            "params": {
                "title": crate::services::agent::response_agent::scheduled_task_created(name),
                "message": format!("{} / {}", tapp_id, agent_schedule.as_str()),
                "taskId": task_id
            },
            "timestamp": now.timestamp_millis()
        }
    }))
}

async fn execute_scheduler_trigger(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let task_id = params
        .get("taskId")
        .or_else(|| params.get("task_id"))
        .and_then(Value::as_str)
        .ok_or("Missing taskId parameter")?;
    let requested_tapp_id = params
        .get("tappId")
        .or_else(|| params.get("tapp_id"))
        .and_then(Value::as_str);

    let scheduler = scheduler_engine()?;
    let tapp_id = if let Some(tapp_id) = requested_tapp_id {
        tapp_id.to_string()
    } else {
        let matches: Vec<_> = scheduler
            .list_tasks(ctx.user_id, None)
            .await?
            .into_iter()
            .filter(|task| task.task_id == task_id)
            .collect();
        match matches.as_slice() {
            [task] => task.tapp_id.clone(),
            [] => return Err(format!("Task {task_id} not found")),
            _ => {
                return Err(format!(
                    "Task ID {task_id} exists in multiple Tapps; provide tappId"
                ));
            }
        }
    };

    scheduler
        .trigger_task(ctx.user_id, &tapp_id, task_id)
        .await?;

    Ok(json!({
        "success": true,
        "triggered": true,
        "taskId": task_id,
        "tappId": tapp_id,
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

// Agent Heartbeat（HEARTBEAT.md，非 Tapp scheduler）

async fn require_heartbeat_admin(ctx: &HandlerContext<'_>) -> Result<(), String> {
    if crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await {
        Ok(())
    } else {
        Err("Heartbeat admin required".to_string())
    }
}

fn heartbeat_manager()
-> Result<&'static std::sync::Arc<crate::services::agent::heartbeat::HeartbeatManager>, String> {
    crate::services::agent::heartbeat::get_heartbeat()
        .ok_or_else(|| "Heartbeat not initialized".to_string())
}

async fn execute_heartbeat_create(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    require_heartbeat_admin(ctx).await?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("Missing name parameter")?
        .to_string();
    let schedule = params
        .get("schedule")
        .or_else(|| params.get("cron"))
        .and_then(Value::as_str)
        .ok_or("Missing schedule parameter (5-field cron)")?
        .to_string();
    let action = params
        .get("action")
        .and_then(Value::as_str)
        .ok_or("Missing action parameter")?
        .to_string();
    let enabled = params
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let id = heartbeat_task_id(params).map(ToOwned::to_owned);

    let manager = heartbeat_manager()?;
    let task = manager
        .add_task(id, name, schedule, action, enabled)
        .await?;

    Ok(json!({
        "success": true,
        "task": task,
        "taskId": task.id,
        "frontendAction": {
            "type": "show_notification",
            "params": {
                "title": "Heartbeat created",
                "message": format!("{} · {}", task.name, task.schedule),
                "taskId": task.id
            },
            "timestamp": chrono::Utc::now().timestamp_millis()
        }
    }))
}

async fn execute_heartbeat_update(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    require_heartbeat_admin(ctx).await?;
    let task_id = heartbeat_task_id(params).ok_or("Missing id parameter")?;

    let name = params
        .get("name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let schedule = params
        .get("schedule")
        .or_else(|| params.get("cron"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let action = params
        .get("action")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let enabled = params.get("enabled").and_then(Value::as_bool);

    if !heartbeat_update_has_fields(
        name.as_deref(),
        schedule.as_deref(),
        action.as_deref(),
        enabled,
    ) {
        return Err("Provide at least one of name/schedule/action/enabled".to_string());
    }

    let manager = heartbeat_manager()?;
    let task = manager
        .update_task(task_id, name, schedule, action, enabled)
        .await?;

    Ok(json!({
        "success": true,
        "task": task,
        "taskId": task.id
    }))
}

async fn execute_heartbeat_delete(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    require_heartbeat_admin(ctx).await?;
    let task_id = heartbeat_task_id(params).ok_or("Missing id parameter")?;

    let manager = heartbeat_manager()?;
    manager.delete_task(task_id).await?;

    Ok(json!({
        "success": true,
        "deleted": true,
        "taskId": task_id
    }))
}

async fn execute_heartbeat_toggle(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    require_heartbeat_admin(ctx).await?;
    let task_id = heartbeat_task_id(params).ok_or("Missing id parameter")?;

    let manager = heartbeat_manager()?;
    let enabled = manager.toggle_task(task_id).await?;
    Ok(json!({
        "success": true,
        "taskId": task_id,
        "enabled": enabled
    }))
}

// 系统状态

async fn execute_system_metrics() -> Result<Value, String> {
    // Process-level metrics only — honest limited payload, not full host monitoring.
    let memory = myriad_process_info::process_memory_info();
    let uptime_seconds = myriad_process_info::process_uptime_seconds();
    let version = myriad_process_info::build_version();
    let config_mode = crate::CONFIG_MODE.load(std::sync::atomic::Ordering::Relaxed);
    let db_connected = !crate::CONFIG_MODE.load(std::sync::atomic::Ordering::Relaxed);

    let (bg_total, bg_pending, bg_processing, bg_completed, bg_failed) =
        BACKGROUND_PROCESSOR.get_task_stats().await;

    let agent_task_counts = {
        use crate::services::agent::executor::TASK_STORE;
        let store = TASK_STORE.read().await;
        let (total, pending, running, waiting, completed, failed, cancelled) =
            store.status_counts();
        json!({
            "total": total,
            "pending": pending,
            "running": running,
            "waitingOrPaused": waiting,
            "completed": completed,
            "failed": failed,
            "cancelled": cancelled,
            "scope": "in_memory_task_store",
        })
    };

    Ok(json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "scope": "process",
        "note": "Limited process-level metrics (memory/uptime/task counts). Not full host or cluster monitoring.",
        "memory": memory,
        "system": {
            "uptime_seconds": uptime_seconds,
            "version": version,
            "config_mode": config_mode,
            "db_connected": db_connected,
        },
        "tasks": {
            "background_processor": {
                "total": bg_total,
                "pending": bg_pending,
                "processing": bg_processing,
                "completed": bg_completed,
                "failed": bg_failed,
            },
            "agent": agent_task_counts,
        },
    }))
}

/// 平台过滤缓存状态；可选 `platform` 须在 `VALID_PLATFORMS`。
async fn execute_cache_status(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform = params.get("platform").and_then(|v| v.as_str());
    let platforms = if let Some(p) = platform {
        if !validate_platform_name(p) {
            return Err(crate::services::agent::response_agent::unsupported_platform(p));
        }
        vec![p.to_string()]
    } else {
        VALID_PLATFORMS.iter().map(|s| (*s).to_string()).collect()
    };

    let mut cache_info = Vec::new();
    let mut total_size: u64 = 0;

    for p in platforms {
        let cache_path = platform_filtered_file(&p);
        let metadata = tokio::fs::metadata(&cache_path).await;

        let (exists, size, modified) = match metadata {
            Ok(m) => {
                let modified = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| {
                        chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_default()
                    });
                (true, m.len(), modified)
            }
            Err(_) => (false, 0, None),
        };

        total_size += size;
        cache_info.push(json!({
            "platform": p,
            "exists": exists,
            "size_bytes": size,
            "size_mb": format!("{:.2}", size as f64 / 1024.0 / 1024.0),
            "modified_at": modified,
            "path": cache_path
        }));
    }

    Ok(json!({
        "caches": cache_info,
        "total_size_bytes": total_size,
        "total_size_mb": format!("{:.2}", total_size as f64 / 1024.0 / 1024.0)
    }))
}

async fn execute_cache_clear(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .ok_or("Missing platform parameter")?;

    // `VALID_PLATFORMS` 成员校验（不是路径穿越检查）
    if !validate_platform_name(platform) {
        return Err(crate::services::agent::response_agent::unsupported_platform(platform));
    }

    let cache_path = platform_filtered_file(platform);

    let size = tokio::fs::metadata(&cache_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    match tokio::fs::remove_file(&cache_path).await {
        Ok(_) => Ok(json!({
            "success": true,
            "platform": platform,
            "clearedSize": format!("{:.2} MB", size as f64 / 1024.0 / 1024.0)
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({
            "success": true,
            "platform": platform,
            "clearedSize": "0 MB",
            "message": "Cache file did not exist"
        })),
        Err(e) => Err(format!("Failed to clear cache: {}", e)),
    }
}

// 健康检查

async fn execute_rsshub_healthcheck(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    use crate::services::rsshub_service::RsshubService;

    let service = RsshubService::new(ctx.db.clone());
    if let Err(e) = service.ensure_default_instances().await {
        tracing::warn!("[rsshub.healthcheck] Failed to ensure defaults: {}", e);
    }

    let instance_id = params
        .get("instanceId")
        .or_else(|| params.get("instance_id"))
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().map(|u| u as i64))
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .map(|id| id as i32);

    let mut instances = service
        .get_instances(Some(ctx.user_id))
        .await
        .map_err(|e| {
            tracing::error!("Failed to load RSSHub instances: {e}");
            "Failed to load RSSHub instances".to_string()
        })?;

    if let Some(id) = instance_id {
        instances.retain(|i| i.id == id);
        if instances.is_empty() {
            return Err("RSSHub instance not found".to_string());
        }
    }

    if instances.is_empty() {
        return Err("No RSSHub instance is configured".to_string());
    }

    let mut results = Vec::new();
    let mut healthy_count = 0usize;

    for instance in instances {
        if !instance.enabled && instance_id.is_none() {
            results.push(json!({
                "id": instance.id,
                "name": instance.name,
                "url": instance.url,
                "status": "skipped",
                "enabled": false,
                "healthy": false,
            }));
            continue;
        }

        match service.health_check_and_record(&instance).await {
            Ok(response_time_ms) => {
                healthy_count += 1;
                results.push(json!({
                    "id": instance.id,
                    "name": instance.name,
                    "url": instance.url,
                    "status": "healthy",
                    "healthy": true,
                    "responseTimeMs": response_time_ms,
                    "enabled": instance.enabled,
                }));
            }
            Err(e) => {
                results.push(json!({
                    "id": instance.id,
                    "name": instance.name,
                    "url": instance.url,
                    "status": "unhealthy",
                    "healthy": false,
                    "error": e,
                    "enabled": instance.enabled,
                }));
            }
        }
    }

    Ok(json!({
        "instances": results,
        "total": results.len(),
        "healthyCount": healthy_count,
        "checkedAt": chrono::Utc::now().to_rfc3339(),
        "source": "configured_instances",
    }))
}

// 图片缓存

async fn execute_image_cache(params: &HashMap<String, Value>) -> Result<Value, String> {
    if let Some(url) = first_string_param(params, &["url"]) {
        let local_path = ImageCacheService::new().cache_image(&url).await?;
        return Ok(json!({
            "localPath": local_path,
            "cached": true,
            "url": url
        }));
    }

    let action = params
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("status");
    let cache_dir = crate::services::data_paths::paths().cache_images.clone();

    match action {
        "status" => {
            let mut total_files = 0u64;
            let mut total_size = 0u64;

            if cache_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&cache_dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            if let Ok(sub_entries) = std::fs::read_dir(entry.path()) {
                                for sub_entry in sub_entries.flatten() {
                                    if let Ok(meta) = sub_entry.metadata() {
                                        total_files += 1;
                                        total_size += meta.len();
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Ok(json!({
                "total_files": total_files,
                "total_size_bytes": total_size,
                "total_size_mb": format!("{:.2}", total_size as f64 / 1024.0 / 1024.0),
                "cache_dir": cache_dir.display().to_string()
            }))
        }
        "clear" => {
            let mut cleared = 0u64;
            if cache_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&cache_dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() && std::fs::remove_dir_all(entry.path()).is_ok() {
                            cleared += 1;
                        }
                    }
                }
            }
            Ok(json!({
                "cleared_directories": cleared,
                "message": "Image cache cleared"
            }))
        }
        _ => Err(format!("Unknown image cache action: {}", action)),
    }
}

// 数据导出

async fn execute_export_data(params: &HashMap<String, Value>) -> Result<Value, String> {
    let format = params
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("json");
    // Schema required `platform`; read `platform` then `type`.
    let data_type =
        first_string_param(params, &["platform", "type"]).unwrap_or_else(|| "all".into());

    let mut export_data = json!({});

    let platforms: Vec<&str> = if data_type == "all" || data_type == "platforms" {
        VALID_PLATFORMS.to_vec()
    } else if is_valid_platform(&data_type) {
        vec![data_type.as_str()]
    } else if data_type == "databases" {
        Vec::new()
    } else {
        return Err(format!("Unknown export platform: {data_type}"));
    };

    if !platforms.is_empty() {
        let mut platform_data = json!({});

        for platform in &platforms {
            let path = platform_filtered_file(platform);
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                if let Ok(data) = serde_json::from_str::<Value>(&content) {
                    platform_data[*platform] = data;
                }
            }
        }
        export_data["platforms"] = platform_data;
    }

    if data_type == "all" || data_type == "databases" {
        let dbs = ["anime_database", "game_database", "artist_database"];
        let mut db_data = json!({});

        for db in dbs {
            let path = format!("data/{}.json", db);
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                if let Ok(data) = serde_json::from_str::<Value>(&content) {
                    db_data[db] = data;
                }
            }
        }
        export_data["databases"] = db_data;
    }

    let now = chrono::Utc::now();
    // Write `cache/exports/{id}.{format}`; download uses Blob `params.content`
    let export_id = format!("export_{}", now.timestamp_millis());
    let export_path = format!("cache/exports/{}.{}", export_id, format);
    let export_content = match format {
        "json" => serde_json::to_string_pretty(&export_data).unwrap_or_default(),
        "csv" => {
            // Simple CSV: flatten top-level keys
            let mut csv = String::new();
            if let Value::Object(map) = &export_data {
                for (key, val) in map {
                    csv.push_str(&format!("{},{}\n", key, val));
                }
            }
            csv
        }
        _ => serde_json::to_string(&export_data).unwrap_or_default(),
    };

    // Ensure dir exists and write
    tokio::fs::create_dir_all("cache/exports")
        .await
        .map_err(|e| {
            tracing::error!("Failed to create export directory: {e}");
            "Failed to export data".to_string()
        })?;
    tokio::fs::write(&export_path, &export_content)
        .await
        .map_err(|e| {
            tracing::error!("Failed to write export file: {e}");
            "Failed to export data".to_string()
        })?;

    let filename = format!("myriad_export_{}.{}", data_type, format);
    Ok(json!({
        "format": format,
        "data_type": data_type,
        "data": export_data,
        "content": export_content,
        "filename": filename,
        "exportId": export_id,
        "exported_at": now.to_rfc3339(),
        "frontendAction": {
            "type": "download_file",
            "params": {
                "exportId": export_id,
                "filename": filename,
                "format": format,
                "content": export_content
            },
            "timestamp": now.timestamp_millis()
        }
    }))
}

// 后台任务

async fn execute_task_submit(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    match crate::services::background_processor::submit_and_start_platform_task(platform.clone())
        .await
    {
        Ok(task_id) => Ok(json!({
            "success": true,
            "taskId": task_id,
            "platform": platform,
            "status": "submitted",
            "message": "Task submitted to background processor",
            "timestamp": chrono::Utc::now().to_rfc3339()
        })),
        Err(e) => Err(format!("Failed to submit task: {}", e)),
    }
}

async fn execute_phantasi_schedule(params: &HashMap<String, Value>) -> Result<Value, String> {
    let action = parse_phantasi_schedule_action(params.get("action").and_then(|v| v.as_str()))?;
    let source_id = params.get("sourceId").and_then(|v| v.as_i64());

    match action {
        PhantasiScheduleAction::Start => match get_phantasi_scheduler() {
            Some(scheduler) => {
                scheduler.start().await;
                Ok(json!({
                    "success": true,
                    "action": "start",
                    "status": "started",
                    "message": "Phantasi scheduler started"
                }))
            }
            _ => Err("Phantasi scheduler not initialized".to_string()),
        },
        PhantasiScheduleAction::Stop => match get_phantasi_scheduler() {
            Some(scheduler) => {
                scheduler.stop().await;
                Ok(json!({
                    "success": true,
                    "action": "stop",
                    "status": "stopped",
                    "message": "Phantasi scheduler stopped"
                }))
            }
            _ => Err("Phantasi scheduler not initialized".to_string()),
        },
        PhantasiScheduleAction::Refresh => match get_phantasi_scheduler() {
            Some(scheduler) => {
                if let Some(sid) = source_id {
                    match scheduler.refresh_source(sid as i32).await {
                        Ok(new_count) => Ok(json!({
                            "success": true,
                            "action": "refresh",
                            "sourceId": sid,
                            "status": "refreshed",
                            "newItems": new_count,
                            "message": format!("Refreshed source, {} new items", new_count)
                        })),
                        Err(e) => Err(format!("Failed to refresh source: {}", e)),
                    }
                } else {
                    match scheduler.refresh_all_enabled().await {
                        Ok((attempted, refreshed, failed, new_items)) => Ok(json!({
                            "success": failed == 0,
                            "action": "refresh",
                            "status": "refreshed",
                            "attempted": attempted,
                            "refreshed": refreshed,
                            "failed": failed,
                            "newItems": new_items,
                            "message": format!(
                                "Refreshed {refreshed} sources ({failed} failed), {new_items} new items"
                            )
                        })),
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to refresh sources");
                            Err("Failed to refresh sources".to_string())
                        }
                    }
                }
            }
            _ => Err("Phantasi scheduler not initialized".to_string()),
        },
        PhantasiScheduleAction::Status => {
            let scheduler_active = get_phantasi_scheduler().is_some();
            Ok(json!({
                "action": "status",
                "running": scheduler_active,
                "available": scheduler_active,
                "checkedAt": chrono::Utc::now().to_rfc3339()
            }))
        }
    }
}

async fn execute_setup_status(ctx: &HandlerContext<'_>) -> Result<Value, String> {
    let progress = crate::api::setup::inspect_setup_progress(ctx.db).await;
    Ok(json!({
        "isSetupRequired": progress.is_setup_required,
        "hasDatabase": progress.has_database,
        "hasAdminUser": progress.has_admin_user,
        "missingConfigs": progress.missing_configs,
        "checkedAt": chrono::Utc::now().to_rfc3339()
    }))
}
