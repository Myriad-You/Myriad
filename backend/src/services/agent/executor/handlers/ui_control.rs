//! UI 控制能力处理器
//!
//! 处理 tapp.ui, tapp.interact, router.navigate 等 UI 控制类能力。
//! HTML/JS 解析、路由/窗口/音乐纯规则见 [`crate::services::agent::ui_analysis`]。

use super::HandlerContext;
use crate::services::agent::ai_process_pure::USER_TEXT_MAX_CHARS;
use crate::models::entities::{tapp_scheduled_tasks, tapp_task_executions, tapp_widgets, tapps};
use crate::services::agent::ui_analysis::{
    build_breadcrumb, build_navigate_full_path, detect_page_type, extract_json_from_response,
    extract_route_context, generate_suggested_actions, get_page_name, is_safe_agent_tapp_id,
    is_valid_page_interact_action, is_valid_router_path, join_layer_analysis_sources,
    normalize_music_control, parse_html_elements, parse_html_structure, parse_i18n, parse_js_events,
    parse_js_functions, parse_playlist_id_param, resolve_window_close_target,
    resolve_window_focus_target, router_can_go_back,
};
use crate::services::data_paths::paths;
use crate::services::tapp_package_read::{
    installed_core_entry, installed_page_entry, installed_text_resource_plan,
};
use crate::services::tapp_validation::validate_resource_path;
use crate::services::tapp_storage::{sandbox_storage_count, sandbox_storage_entries};
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

/// Agent 详情里的授予权限投影。
///
/// 标记需重新授权时不得把批准列当授予层漏出去——只有授予权限决定行为。
fn agent_detail_granted_permissions(
    approved_permissions: &Value,
    needs_reauthorization: bool,
) -> Value {
    if needs_reauthorization {
        json!([])
    } else {
        approved_permissions.clone()
    }
}

/// 执行 UI 控制能力
pub async fn execute(
    capability_id: &str,
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    match capability_id {
        "tapp.ui" => execute_tapp_ui_analysis(params, ctx).await,
        "tapp.understand" => execute_tapp_understand(params, ctx).await,
        "tapp.interact" => execute_tapp_interact(params, ctx).await,
        "tapp.pageContent" => execute_tapp_page_content(params, ctx).await,
        "tapp.windows" => execute_tapp_windows_query(params, ctx).await,
        "tapp.window.open" => execute_tapp_window_open(params, ctx).await,
        "tapp.window.close" => execute_tapp_window_close(params, ctx).await,
        "tapp.window.focus" => execute_tapp_window_focus(params).await,
        "router.navigate" => execute_router_navigate(params).await,
        "router.state" => execute_router_state(params).await,
        "page.interact" => execute_page_interact(params).await,
        "page.understand" => execute_page_understand(params, ctx).await,
        "page.content" => execute_page_content(params, ctx).await,
        "music.control" => execute_music_control(params).await,
        "music.status" => execute_music_status().await,
        "music.playlist" => execute_music_playlist(params).await,
        _ => Err(format!("Unknown ui_control capability: {}", capability_id)),
    }
}

// Tapp UI 相关

async fn read_declared_text(tapp_dir: &Path, relative: Option<String>) -> String {
    let Some(relative) = relative else {
        return String::new();
    };
    if validate_resource_path(&relative).is_err() {
        return String::new();
    }
    tokio::fs::read_to_string(tapp_dir.join(relative))
        .await
        .unwrap_or_default()
}

/// 执行 Tapp UI 结构解析
async fn execute_tapp_ui_analysis(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let tapp_id = params
        .get("tappId")
        .and_then(|v| v.as_str())
        .ok_or("Missing tappId")?;
    // 始终使用已认证的 user_id，防止 IDOR 越权
    let user_id = ctx.user_id;
    let include_code = params
        .get("includeCode")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let element_filter = params
        .get("elementFilter")
        .and_then(|v| v.as_str())
        .unwrap_or("interactive");

    // 验证 tapp_id 安全性，防止路径穿越（domain）
    if !is_safe_agent_tapp_id(tapp_id) {
        return Err("无效的 tappId".to_string());
    }

    // 获取 Tapp 信息
    let tapp = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(tapp_id))
        .filter(tapps::Column::UserId.eq(user_id))
        .one(ctx.db)
        .await
        .map_err(|e| format!("Failed to fetch tapp: {}", e))?
        .ok_or("Tapp not found")?;

    let tapp_dir = paths().tapp_user_dir(user_id).join(tapp_id);
    let html_relative = installed_text_resource_plan(&tapp.manifest)
        .page_template
        .filter(|path| validate_resource_path(path).is_ok())
        .unwrap_or_else(|| "page.html".to_string());
    let html_content = read_declared_text(&tapp_dir, Some(html_relative)).await;

    let js_content = if include_code {
        join_layer_analysis_sources([
            read_declared_text(&tapp_dir, installed_core_entry(&tapp.manifest)).await,
            read_declared_text(&tapp_dir, installed_page_entry(&tapp.manifest)).await,
        ])
    } else {
        String::new()
    };

    // 解析 HTML 结构
    let structure = parse_html_structure(&html_content);
    let elements = parse_html_elements(&html_content, element_filter);
    let functions = if include_code {
        parse_js_functions(&js_content)
    } else {
        vec![]
    };
    let events = parse_js_events(&js_content);
    let i18n = parse_i18n(&js_content);
    let suggested_actions = generate_suggested_actions(&elements, &functions);

    Ok(json!({
        "tappId": tapp_id,
        "tappName": tapp.name,
        "version": tapp.version,
        "status": format!("{:?}", tapp.status),
        "structure": structure,
        "elements": elements,
        "functions": functions,
        "events": events,
        "i18n": i18n,
        "suggestedActions": suggested_actions,
        "files": {
            "hasHtml": !html_content.is_empty(),
            "hasJs": !js_content.is_empty(),
            "htmlSize": html_content.len(),
            "jsSize": js_content.len()
        }
    }))
}

/// 执行 Tapp UI 智能理解 - 使用 AI 分析 UI 并生成操作指令
async fn execute_tapp_understand(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let analyzer = ctx
        .ai_analyzer
        .ok_or("AI analyzer not configured for UI understanding")?;

    let tapp_id = params
        .get("tappId")
        .and_then(|v| v.as_str())
        .ok_or("Missing tappId")?;
    // 始终使用已认证的 user_id，防止 IDOR
    let user_id = ctx.user_id;
    let user_intent = params
        .get("userIntent")
        .and_then(|v| v.as_str())
        .ok_or("Missing userIntent - 请描述你想要执行的操作")?;
    // 获取或复用 UI 分析结果
    let ui_analysis = if let Some(existing) = params.get("uiAnalysis") {
        existing.clone()
    } else {
        let mut ui_params = HashMap::new();
        ui_params.insert("tappId".to_string(), json!(tapp_id));
        ui_params.insert("userId".to_string(), json!(user_id));
        ui_params.insert("includeCode".to_string(), json!(true));
        ui_params.insert("elementFilter".to_string(), json!("interactive"));

        execute_tapp_ui_analysis(&ui_params, ctx).await?
    };

    let tapp_name = ui_analysis
        .get("tappName")
        .and_then(|v| v.as_str())
        .unwrap_or("未知应用");

    // 构建 AI 提示词
    let prompt = format!(
        r#"你是一个 UI 交互分析专家。分析以下 Tapp 应用的 UI 结构，根据用户意图生成操作指令。

## 应用信息
- 应用名称：{tapp_name}
- 应用 ID：{tapp_id}

## UI 结构
{ui_structure}

## 用户意图
{user_intent}

请返回 JSON 格式：
```json
{{
  "understanding": {{
    "appPurpose": "应用主要用途",
    "currentState": "当前 UI 状态",
    "availableActions": []
  }},
  "plan": {{
    "canFulfill": true/false,
    "explanation": "是否能完成用户意图",
    "steps": [
      {{ "step": 1, "action": "click|input|submit", "target": "元素ID", "value": "输入值", "reason": "原因" }}
    ],
    "requiredInputs": []
  }}
}}
```"#,
        tapp_name = tapp_name,
        tapp_id = tapp_id,
        ui_structure = serde_json::to_string_pretty(&ui_analysis).unwrap_or_default(),
        user_intent = user_intent,
    );

    let ai_result = analyzer
        .analyze(&prompt)
        .await
        .map_err(|e| format!("AI analysis failed: {}", e))?;

    let parsed: Value = extract_json_from_response(&ai_result)
        .and_then(|json_str| serde_json::from_str(&json_str).ok())
        .unwrap_or_else(|| {
            json!({
                "understanding": { "appPurpose": "无法解析", "currentState": "未知", "availableActions": [] },
                "plan": { "canFulfill": false, "explanation": ai_result, "steps": [], "requiredInputs": [] }
            })
        });

    // Analysis no longer emits executable DOM commands. Callers must create a
    // declared Agent Interaction via `tapp.interact` after reviewing this plan.
    let frontend_action: Option<Value> = None;

    Ok(json!({
        "tappId": tapp_id,
        "tappName": tapp_name,
        "userIntent": user_intent,
        "understanding": parsed.get("understanding").cloned().unwrap_or(json!({})),
        "plan": parsed.get("plan").cloned().unwrap_or(json!({})),
        "frontendAction": frontend_action,
        "uiAnalysis": { "included": true }
    }))
}

/// 执行 Tapp UI 交互操作
async fn execute_tapp_interact(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let tapp_id = params
        .get("tappId")
        .and_then(|v| v.as_str())
        .ok_or("Missing tappId")?;
    let interaction_type = params
        .get("interactionType")
        .and_then(Value::as_str)
        .ok_or("Missing interactionType")?;
    let input = params
        .get("input")
        .cloned()
        .ok_or("Missing input parameter")?;
    let task_id = ctx.task_id.clone();
    let interaction = crate::services::agent_interaction::create_agent_interaction_internal(
        ctx.db,
        ctx.user_id,
        tapp_id,
        interaction_type,
        input,
        task_id,
    )
    .await?;
    let interaction_id = interaction.interaction_id().to_string();

    Ok(json!({
        "success": true,
        "tappId": tapp_id,
        "interaction": interaction,
        "frontendAction": {
            "type": "agent_interaction",
            "tappId": tapp_id,
            "interactionId": interaction_id,
            "timestamp": chrono::Utc::now().timestamp_millis()
        }
    }))
}

// Tapp 页面内容层级展示

async fn find_accessible_tapp(
    ctx: &HandlerContext<'_>,
    tapp_id: &str,
) -> Result<tapps::Model, String> {
    crate::services::tapp_ownership::verify_tapp_ownership(ctx.db, ctx.user_id, tapp_id)
        .await
        .map_err(|err| err.to_string())?;

    if let Some(tapp) = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(tapp_id))
        .filter(tapps::Column::UserId.eq(ctx.user_id))
        .one(ctx.db)
        .await
        .map_err(|e| format!("Failed to fetch Tapp: {e}"))?
    {
        return Ok(tapp);
    }

    let mut query = tapps::Entity::find().filter(tapps::Column::TappId.eq(tapp_id));
    if !crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await {
        let admin_id = crate::services::tapp_ownership::get_admin_user_id(ctx.db)
            .await
            .map_err(|err| err.to_string())?;
        query = query.filter(tapps::Column::UserId.eq(admin_id));
    }

    query
        .one(ctx.db)
        .await
        .map_err(|e| format!("Failed to fetch Tapp: {e}"))?
        .ok_or_else(|| "Tapp not found".to_string())
}

/// 执行 Tapp 页面内容 - 按层级展示 Tapp 数据
pub(super) async fn execute_tapp_page_content(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let level = params
        .get("level")
        .and_then(|v| v.as_str())
        .unwrap_or("apps");
    let tapp_id = params.get("tappId").and_then(|v| v.as_str());
    let task_id = params.get("taskId").and_then(|v| v.as_str());
    // 始终使用已认证的 user_id，防止 IDOR
    let user_id = ctx.user_id;
    let filter = params
        .get("filter")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

    match level {
        "apps" => {
            // 应用列表层级（与 catalog / tapp.list 同一可见性边界）
            let mut query = tapps::Entity::find();
            let admin_id = crate::services::tapp_ownership::get_admin_user_id(ctx.db)
                .await
                .map_err(|err| err.to_string())?;
            let is_admin = crate::services::agent::user_is_current_admin(ctx.db, user_id).await;
            if !is_admin {
                query = query.filter(
                    tapps::Column::UserId
                        .eq(user_id)
                        .or(tapps::Column::UserId.eq(admin_id)),
                );
            }

            // 状态筛选
            match filter {
                "running" => {
                    query = query.filter(tapps::Column::Status.eq(tapps::TappStatus::Running));
                }
                "installed" => {
                    query = query.filter(tapps::Column::Status.eq(tapps::TappStatus::Installed));
                }
                "error" => {
                    query = query.filter(tapps::Column::Status.eq(tapps::TappStatus::Error));
                }
                _ => {}
            }

            let apps = query
                .order_by_desc(tapps::Column::UpdatedAt)
                .all(ctx.db)
                .await
                .map_err(|e| format!("Failed to fetch tapps: {}", e))?;
            let apps: Vec<_> = apps
                .into_iter()
                .filter(|app| {
                    crate::services::tapp_ownership::install_visible_to_viewer(
                        app, user_id, admin_id, is_admin,
                    )
                })
                .collect();

            let running_count = apps
                .iter()
                .filter(|a| a.status == tapps::TappStatus::Running)
                .count();

            let app_list: Vec<Value> = apps
                .iter()
                .take(limit)
                .map(|app| {
                    json!({
                        "id": app.id,
                        "tappId": app.tapp_id.clone(),
                        "name": app.name.clone(),
                        "version": app.version.clone(),
                        "description": app.description.clone(),
                        "icon": app.icon.clone(),
                        "themeColor": app.theme_color.clone(),
                        "status": format!("{:?}", app.status),
                        "lastRunAt": app.last_run_at.map(|t| t.to_string()),
                        "errorMessage": app.error_message.clone()
                    })
                })
                .collect();

            Ok(json!({
                "level": "apps",
                "hierarchy": {
                    "level": "list",
                    "current": { "view": "all_apps" }
                },
                "content": {
                    "title": "Tapp 应用",
                    "apps": app_list,
                    "metadata": { "totalApps": apps.len() }
                },
                "stats": {
                    "totalApps": apps.len(),
                    "runningApps": running_count,
                    "currentFilter": filter
                },
                "navigation": {
                    "currentFilter": filter,
                    "availableFilters": ["all", "running", "installed", "error"],
                    "canGoBack": false,
                    "parentPath": "/"
                },
                "actions": {
                    "available": ["installTapp", "uninstallTapp", "runTapp", "stopTapp"]
                }
            }))
        }
        "detail" => {
            // 应用详情层级
            let tapp_id_str = tapp_id.ok_or("Missing tappId for detail level")?;
            let app = find_accessible_tapp(ctx, tapp_id_str).await?;

            // 获取组件数量
            let widget_count = tapp_widgets::Entity::find()
                .filter(tapp_widgets::Column::TappId.eq(tapp_id_str))
                .filter(tapp_widgets::Column::UserId.eq(app.user_id))
                .count(ctx.db)
                .await
                .unwrap_or(0);

            // 获取存储数量
            let storage_count = sandbox_storage_count(ctx.db, user_id, tapp_id_str)
                .await
                .unwrap_or(0);

            // 获取任务数量
            let task_count = tapp_scheduled_tasks::Entity::find()
                .filter(tapp_scheduled_tasks::Column::TappId.eq(tapp_id_str))
                .filter(tapp_scheduled_tasks::Column::UserId.eq(user_id))
                .count(ctx.db)
                .await
                .unwrap_or(0);

            Ok(json!({
                "level": "detail",
                "hierarchy": {
                    "level": "detail",
                    "current": {
                        "type": "tapp",
                        "id": app.id,
                        "tappId": app.tapp_id.clone()
                    }
                },
                "content": {
                    "title": app.name.clone(),
                    "detail": {
                        "id": app.id,
                        "tappId": app.tapp_id.clone(),
                        "name": app.name.clone(),
                        "version": app.version.clone(),
                        "description": app.description.clone(),
                        "author": app.author.clone(),
                        "icon": app.icon.clone(),
                        "themeColor": app.theme_color.clone(),
                        "status": format!("{:?}", app.status),
                        "grantedPermissions": agent_detail_granted_permissions(
                            &app.approved_permissions,
                            app.needs_reauthorization,
                        ),
                        "needsReauthorization": app.needs_reauthorization,
                        "manifest": app.manifest.clone(),
                        "installedAt": app.installed_at.to_string(),
                        "lastRunAt": app.last_run_at.map(|t| t.to_string()),
                        "errorMessage": app.error_message.clone()
                    }
                },
                "stats": {
                    "widgetCount": widget_count,
                    "storageCount": storage_count,
                    "taskCount": task_count
                },
                "navigation": {
                    "canGoBack": true,
                    "parentPath": "/tapps",
                    "childPaths": {
                        "widgets": format!("/tapps/{}/widgets", tapp_id_str),
                        "storage": format!("/tapps/{}/storage", tapp_id_str),
                        "tasks": format!("/tapps/{}/tasks", tapp_id_str)
                    }
                },
                "actions": {
                    "available": [
                        "runTapp", "stopTapp", "restartTapp",
                        "updatePermissions", "viewLogs", "uninstallTapp"
                    ]
                }
            }))
        }
        "widgets" => {
            // 组件列表层级
            let tapp_id_str = tapp_id.ok_or("Missing tappId for widgets level")?;
            let app = find_accessible_tapp(ctx, tapp_id_str).await?;

            let widgets = tapp_widgets::Entity::find()
                .filter(tapp_widgets::Column::TappId.eq(tapp_id_str))
                .filter(tapp_widgets::Column::UserId.eq(app.user_id))
                .all(ctx.db)
                .await
                .map_err(|e| format!("Failed to fetch widgets: {}", e))?;

            let widget_list: Vec<Value> = widgets
                .iter()
                .map(|w| {
                    json!({
                        "id": w.id,
                        "widgetId": w.widget_id.clone(),
                        "name": w.name.clone(),
                        "description": w.description.clone(),
                        "icon": w.icon.clone(),
                        "defaultSize": w.default_size.clone(),
                        "sizes": w.sizes.clone(),
                        "category": w.category.clone(),
                        "config": w.config.clone()
                    })
                })
                .collect();

            Ok(json!({
                "level": "widgets",
                "hierarchy": {
                    "level": "nested",
                    "parent": { "type": "tapp", "tappId": tapp_id_str },
                    "current": { "view": "widget_list" }
                },
                "content": {
                    "title": "组件列表",
                    "widgets": widget_list,
                    "metadata": { "tappId": tapp_id_str, "totalWidgets": widgets.len() }
                },
                "stats": { "totalWidgets": widgets.len() },
                "navigation": {
                    "canGoBack": true,
                    "parentPath": format!("/tapps/{}", tapp_id_str)
                },
                "actions": {
                    "available": ["addWidget", "removeWidget", "configureWidget"]
                }
            }))
        }
        "storage" => {
            // 存储数据层级
            let tapp_id_str = tapp_id.ok_or("Missing tappId for storage level")?;
            find_accessible_tapp(ctx, tapp_id_str).await?;

            let mut storage_items = sandbox_storage_entries(ctx.db, user_id, tapp_id_str)
                .await
                .map_err(|e| format!("Failed to fetch storage: {}", e))?;
            storage_items.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

            let storage_list: Vec<Value> = storage_items
                .iter()
                .take(limit)
                .map(|s| {
                    json!({
                        "id": s.id,
                        "key": s.key.clone(),
                        "value": s.value.clone(),
                        "createdAt": s.created_at.to_string(),
                        "updatedAt": s.updated_at.to_string()
                    })
                })
                .collect();

            Ok(json!({
                "level": "storage",
                "hierarchy": {
                    "level": "nested",
                    "parent": { "type": "tapp", "tappId": tapp_id_str },
                    "current": { "view": "storage_list" }
                },
                "content": {
                    "title": "存储数据",
                    "storage": storage_list,
                    "metadata": {
                        "tappId": tapp_id_str,
                        "totalItems": storage_items.len()
                    }
                },
                "stats": { "totalItems": storage_items.len() },
                "navigation": {
                    "canGoBack": true,
                    "parentPath": format!("/tapps/{}", tapp_id_str)
                },
                "actions": {
                    "available": ["getStorage", "setStorage", "deleteStorage", "clearStorage"]
                }
            }))
        }
        "tasks" => {
            // 定时任务列表层级
            let tapp_id_str = tapp_id.ok_or("Missing tappId for tasks level")?;
            find_accessible_tapp(ctx, tapp_id_str).await?;

            let tasks = tapp_scheduled_tasks::Entity::find()
                .filter(tapp_scheduled_tasks::Column::TappId.eq(tapp_id_str))
                .filter(tapp_scheduled_tasks::Column::UserId.eq(user_id))
                .order_by_desc(tapp_scheduled_tasks::Column::UpdatedAt)
                .all(ctx.db)
                .await
                .map_err(|e| format!("Failed to fetch tasks: {}", e))?;

            let enabled_count = tasks.iter().filter(|t| t.enabled).count();

            let task_list: Vec<Value> = tasks
                .iter()
                .take(limit)
                .map(|t| {
                    let schedule_type = match t.schedule_type {
                        tapp_scheduled_tasks::ScheduleType::Cron => "cron",
                        tapp_scheduled_tasks::ScheduleType::Interval => "interval",
                        tapp_scheduled_tasks::ScheduleType::Once => "once",
                        tapp_scheduled_tasks::ScheduleType::Daily => "daily",
                    };
                    let execution_target = match t.execution_target {
                        tapp_scheduled_tasks::ExecutionTarget::Backend => "backend",
                        tapp_scheduled_tasks::ExecutionTarget::Frontend => "frontend",
                        tapp_scheduled_tasks::ExecutionTarget::Both => "both",
                    };
                    let scope = match t.scope {
                        tapp_scheduled_tasks::TaskScope::User => "user",
                        tapp_scheduled_tasks::TaskScope::Tapp => "tapp",
                        tapp_scheduled_tasks::TaskScope::TappPerUser => "tapp-per-user",
                        tapp_scheduled_tasks::TaskScope::Global => "global",
                    };
                    json!({
                        "id": t.id,
                        "taskId": t.task_id.clone(),
                        "name": t.name.clone(),
                        "scheduleType": schedule_type,
                        "scheduleConfig": t.schedule_config.clone(),
                        "executionTarget": execution_target,
                        "scope": scope,
                        "enabled": t.enabled,
                        "nextRunAt": t.next_run_at.map(|t| t.to_string()),
                        "lastRunAt": t.last_run_at.map(|t| t.to_string()),
                        "lastRunResult": t.last_run_result.clone(),
                        "stats": t.stats.clone()
                    })
                })
                .collect();

            Ok(json!({
                "level": "tasks",
                "hierarchy": {
                    "level": "nested",
                    "parent": { "type": "tapp", "tappId": tapp_id_str },
                    "current": { "view": "task_list" }
                },
                "content": {
                    "title": "定时任务",
                    "tasks": task_list,
                    "metadata": {
                        "tappId": tapp_id_str,
                        "totalTasks": tasks.len()
                    }
                },
                "stats": {
                    "totalTasks": tasks.len(),
                    "enabledTasks": enabled_count
                },
                "navigation": {
                    "canGoBack": true,
                    "parentPath": format!("/tapps/{}", tapp_id_str)
                },
                "actions": {
                    "available": ["enableTask", "disableTask", "runTaskNow", "viewExecutions"]
                }
            }))
        }
        "executions" => {
            // 任务执行记录层级
            let task_id_str = task_id.ok_or("Missing taskId for executions level")?;

            // 任务 ID 只在 Tapp 内唯一；按当前用户收窄，并在有歧义时要求 tappId。
            let mut task_query = tapp_scheduled_tasks::Entity::find()
                .filter(tapp_scheduled_tasks::Column::TaskId.eq(task_id_str))
                .filter(tapp_scheduled_tasks::Column::UserId.eq(user_id));
            if let Some(tapp_id) = tapp_id {
                task_query = task_query.filter(tapp_scheduled_tasks::Column::TappId.eq(tapp_id));
            }
            let matching_tasks = task_query
                .all(ctx.db)
                .await
                .map_err(|e| format!("Failed to fetch task: {}", e))?;
            let task = match matching_tasks.as_slice() {
                [task] => Some(task.clone()),
                [] => None,
                _ => return Err("Task ID is ambiguous; provide tappId".to_string()),
            };

            let mut execution_query = tapp_task_executions::Entity::find()
                .filter(tapp_task_executions::Column::TaskId.eq(task_id_str))
                .filter(tapp_task_executions::Column::UserId.eq(user_id));
            if let Some(tapp_id) = tapp_id {
                execution_query =
                    execution_query.filter(tapp_task_executions::Column::TappId.eq(tapp_id));
            }
            let executions = execution_query
                .order_by_desc(tapp_task_executions::Column::ExecutedAt)
                .all(ctx.db)
                .await
                .map_err(|e| format!("Failed to fetch executions: {}", e))?;

            let success_count = executions
                .iter()
                .filter(|e| e.status == tapp_task_executions::ExecutionStatus::Success)
                .count();

            let execution_list: Vec<Value> = executions
                .iter()
                .take(limit)
                .map(|e| {
                    json!({
                        "id": e.id,
                        "status": format!("{:?}", e.status).to_lowercase(),
                        "scheduledAt": e.scheduled_at.to_string(),
                        "executedAt": e.executed_at.to_string(),
                        "completedAt": e.completed_at.map(|t| t.to_string()),
                        "durationMs": e.duration_ms,
                        "result": e.result.clone(),
                        "error": e.error.clone(),
                        "retryCount": e.retry_count,
                        "isCompensation": e.is_compensation
                    })
                })
                .collect();

            Ok(json!({
                "level": "executions",
                "hierarchy": {
                    "level": "detail",
                    "parent": task.as_ref().map(|t| json!({
                        "type": "task",
                        "taskId": t.task_id.clone(),
                        "name": t.name.clone()
                    })),
                    "current": { "view": "execution_list" }
                },
                "content": {
                    "title": task.as_ref().map(|t| format!("{} 执行记录", t.name)).unwrap_or_else(|| "执行记录".to_string()),
                    "executions": execution_list,
                    "task": task.as_ref().map(|t| json!({
                        "id": t.id,
                        "taskId": t.task_id.clone(),
                        "name": t.name.clone(),
                        "enabled": t.enabled
                    })),
                    "metadata": {
                        "taskId": task_id_str,
                        "totalExecutions": executions.len()
                    }
                },
                "stats": {
                    "totalExecutions": executions.len(),
                    "successCount": success_count,
                    "failureCount": executions.len() - success_count
                },
                "navigation": {
                    "canGoBack": true,
                    "parentPath": task.as_ref().map(|t| format!("/tapps/{}/tasks", t.tapp_id))
                },
                "actions": {
                    "available": ["retryExecution", "clearExecutions"]
                }
            }))
        }
        _ => Err(format!("Unknown tapp page level: {}", level)),
    }
}

// Tapp 多窗口管理

/// 查询当前打开的窗口状态
async fn execute_tapp_windows_query(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let include_ui = params
        .get("includeUiAnalysis")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 获取用户所有有页面的 Tapp
    let all_tapps = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(ctx.user_id))
        .all(ctx.db)
        .await
        .map_err(|e| format!("Failed to fetch tapps: {}", e))?;

    let available_tapps: Vec<Value> = all_tapps
        .iter()
        .filter(|t| {
            crate::services::tapp_package_read::manifest_declares_page(&t.manifest)
        })
        .map(|t| {
            json!({
                "tappId": t.tapp_id,
                "name": t.name,
                "description": t.description,
                "icon": t.icon,
                "version": t.version
            })
        })
        .collect();

    Ok(json!({
        "success": true,
        "message": "窗口状态需要从前端获取",
        "availableTapps": available_tapps,
        "maxWindows": 3,
        "frontendAction": {
            "type": "query_windows",
            "action": "getWindowState",
            "includeUiAnalysis": include_ui,
            "timestamp": chrono::Utc::now().timestamp_millis()
        },
        "instructions": {
            "description": "前端应返回当前窗口状态",
            "expectedResponse": {
                "windows": "当前打开的窗口列表",
                "activeWindowId": "活跃窗口 ID",
                "windowCount": "窗口数量"
            }
        }
    }))
}

/// 打开新的 Tapp 窗口
async fn execute_tapp_window_open(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let tapp_id = params.get("tappId").and_then(|v| v.as_str());
    let tapp_name = params.get("tappName").and_then(|v| v.as_str());
    let position = params.get("position");
    let size = params.get("size");

    let tapp = if let Some(id) = tapp_id {
        tapps::Entity::find()
            .filter(tapps::Column::TappId.eq(id))
            .filter(tapps::Column::UserId.eq(ctx.user_id))
            .one(ctx.db)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Agent ui_control database error");
                "Database error".to_string()
            })?
    } else if let Some(name) = tapp_name {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(ctx.user_id))
            .filter(tapps::Column::Name.contains(name))
            .one(ctx.db)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Agent ui_control database error");
                "Database error".to_string()
            })?
    } else {
        return Err("Missing tappId or tappName parameter".to_string());
    };

    let tapp = tapp.ok_or("Tapp not found")?;

    let has_page = crate::services::tapp_package_read::manifest_declares_page(&tapp.manifest);
    if !has_page {
        return Err(format!(
            "Tapp '{}' does not have a page component",
            tapp.name
        ));
    }

    Ok(json!({
        "success": true,
        "tappId": tapp.tapp_id,
        "tappName": tapp.name,
        "frontendAction": {
            "type": "open_window",
            "tappId": tapp.tapp_id,
            "position": position,
            "size": size,
            "timestamp": chrono::Utc::now().timestamp_millis()
        }
    }))
}

/// 关闭 Tapp 窗口
async fn execute_tapp_window_close(
    params: &HashMap<String, Value>,
    _ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let window_id = params.get("windowId").and_then(|v| v.as_str());
    let tapp_id = params.get("tappId").and_then(|v| v.as_str());
    let position = params.get("position").and_then(|v| v.as_str());

    let close_target = resolve_window_close_target(window_id, tapp_id, position)?;

    Ok(json!({
        "success": true,
        "frontendAction": {
            "type": "close_window",
            "target": close_target,
            "timestamp": chrono::Utc::now().timestamp_millis()
        }
    }))
}

/// 聚焦窗口
async fn execute_tapp_window_focus(params: &HashMap<String, Value>) -> Result<Value, String> {
    let window_id = params.get("windowId").and_then(|v| v.as_str());
    let tapp_id = params.get("tappId").and_then(|v| v.as_str());
    let tapp_name = params.get("tappName").and_then(|v| v.as_str());
    let position = params.get("position").and_then(|v| v.as_str());

    let focus_target = resolve_window_focus_target(window_id, tapp_id, tapp_name, position)?;

    Ok(json!({
        "success": true,
        "frontendAction": {
            "type": "focus_window",
            "target": focus_target,
            "timestamp": chrono::Utc::now().timestamp_millis()
        }
    }))
}

// 路由和页面交互

async fn execute_router_navigate(params: &HashMap<String, Value>) -> Result<Value, String> {
    let path = params
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("Missing path parameter")?;
    let route_params = params.get("params").cloned().unwrap_or(json!({}));
    let query_params = params.get("query").cloned().unwrap_or(json!({}));
    let replace = params
        .get("replace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !is_valid_router_path(path) {
        return Err(format!("Invalid route path: {}", path));
    }
    let full_path = build_navigate_full_path(path, &query_params);

    Ok(json!({
        "success": true,
        "currentPath": full_path,
        "frontendAction": {
            "type": "navigate",
            "path": path,
            "params": route_params,
            "query": query_params,
            "fullPath": full_path,
            "replace": replace,
            "timestamp": chrono::Utc::now().timestamp_millis()
        }
    }))
}

async fn execute_page_interact(params: &HashMap<String, Value>) -> Result<Value, String> {
    let action = params
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or("Missing action parameter")?;
    let target = params.get("target").ok_or("Missing target parameter")?;
    let value = params.get("value").and_then(|v| v.as_str());

    if !is_valid_page_interact_action(action) {
        return Err(format!("Invalid action: {}", action));
    }

    Ok(json!({
        "success": true,
        "action": action,
        "target": target,
        "frontendAction": {
            "type": "page_interact",
            "action": action,
            "target": target,
            "value": value,
            "timestamp": chrono::Utc::now().timestamp_millis()
        }
    }))
}

async fn execute_page_understand(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let page_context = params.get("context").cloned().unwrap_or(json!({}));
    let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");

    if let Some(analyzer) = ctx.ai_analyzer {
        let context_str = serde_json::to_string_pretty(&page_context).unwrap_or_default();
        let truncated_context: String = context_str.chars().take(USER_TEXT_MAX_CHARS).collect();

        let prompt = format!(
            "你是一个页面交互分析助手。请分析当前页面上下文并理解用户意图，生成操作计划。\n\n\
            页面上下文：\n{}\n\n\
            用户请求：{}\n\n\
            请返回 JSON 格式的操作计划：\n\
            {{\n\
              \"understood_intent\": \"对用户意图的理解\",\n\
              \"actions\": [\n\
                {{\"type\": \"click|input|navigate|scroll\", \"target\": \"目标元素描述\", \"value\": \"输入值（如有）\"}}\n\
              ],\n\
              \"explanation\": \"操作计划说明\"\n\
            }}\n\n\
            请直接返回 JSON。",
            truncated_context, query
        );

        let result = analyzer
            .analyze(&prompt)
            .await
            .map_err(|e| format!("AI analysis failed: {}", e))?;

        return Ok(json!({
            "query": query,
            "plan": result,
            "understood": true
        }));
    }

    Ok(json!({
        "query": query,
        "understood": false,
        "message": "AI analyzer not available"
    }))
}

// 路由状态和音乐控制

/// 获取当前路由状态
async fn execute_router_state(params: &HashMap<String, Value>) -> Result<Value, String> {
    let current_path = params
        .get("currentPath")
        .and_then(|v| v.as_str())
        .unwrap_or("/");

    let page_type = detect_page_type(current_path);
    let page_name = get_page_name(current_path, page_type);
    let breadcrumb = build_breadcrumb(current_path);

    // 从路径中提取上下文信息
    let context = extract_route_context(current_path, params);

    Ok(json!({
        "currentPath": current_path,
        "params": params.get("routeParams").cloned().unwrap_or(json!({})),
        "query": params.get("queryParams").cloned().unwrap_or(json!({})),
        "pageName": page_name,
        "pageType": page_type,
        "context": context,
        "breadcrumb": breadcrumb,
        "canGoBack": router_can_go_back(current_path),
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// 音乐播放器控制
async fn execute_music_control(params: &HashMap<String, Value>) -> Result<Value, String> {
    let action = params
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or("缺少 action 参数（play/pause/next/previous/volume/mute/unmute）")?;

    let volume = params.get("volume").and_then(|v| v.as_f64());
    let position = params.get("position").and_then(|v| v.as_f64());
    let normalized = normalize_music_control(action, volume, position)?;
    let timestamp = chrono::Utc::now().timestamp_millis();

    let mut frontend_action = json!({
        "type": "music_control",
        "action": normalized.action,
        "timestamp": timestamp
    });
    if let Some(value) = normalized.value {
        frontend_action["value"] = value;
    }

    Ok(json!({
        "success": true,
        "action": action,
        "frontendAction": frontend_action,
        "message": normalized.message
    }))
}

/// 获取音乐播放器状态（状态只在浏览器；回 frontendAction 让前端探测）
async fn execute_music_status() -> Result<Value, String> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    Ok(json!({
        "frontendAction": {
            "type": "music_get_status",
            "timestamp": timestamp
        }
    }))
}

/// 加载并播放歌单
async fn execute_music_playlist(params: &HashMap<String, Value>) -> Result<Value, String> {
    let playlist_id = params.get("playlistId").and_then(parse_playlist_id_param);
    let playlist_id = match playlist_id {
        Some(id) => id,
        None => {
            tracing::warn!(
                params = ?params,
                "[ui_control] music.playlist: Missing playlistId"
            );
            return Err("Missing playlistId parameter. Use netease.searchPlaylist first to get a playlist ID.".to_string());
        }
    };

    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("netease");
    let auto_play = params
        .get("autoPlay")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let timestamp = chrono::Utc::now().timestamp_millis();

    tracing::info!(
        playlist_id = %playlist_id,
        source = %source,
        auto_play = %auto_play,
        "[ui_control] music.playlist: Loading playlist"
    );

    Ok(json!({
        "success": true,
        "playlistId": playlist_id,
        "source": source,
        "autoPlay": auto_play,
        "frontendAction": {
            "type": "music_load_playlist",
            "playlistId": playlist_id,
            "source": source,
            "autoPlay": auto_play,
            "timestamp": timestamp
        },
        "message": format!("正在加载{}歌单...", if source == "netease" { "网易云" } else { "QQ音乐" })
    }))
}

// 页面内容

/// 读取当前页面内容
async fn execute_page_content(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let current_path = params
        .get("currentPath")
        .and_then(|v| v.as_str())
        .unwrap_or("/");
    let page_type = params
        .get("pageType")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| detect_page_type(current_path));
    let context = params.get("context").cloned().unwrap_or(json!({}));

    if let Some(snapshot) = context
        .get("content")
        .or_else(|| context.get("html"))
        .or_else(|| context.get("text"))
        .filter(|v| match v {
            Value::String(s) => !s.trim().is_empty(),
            Value::Object(o) => !o.is_empty(),
            Value::Array(a) => !a.is_empty(),
            _ => false,
        })
    {
        return Ok(json!({
            "pageType": page_type,
            "currentPath": current_path,
            "content": snapshot,
            "source": "snapshot"
        }));
    }

    let route_ctx = extract_route_context(current_path, params);
    match page_type {
        "brew" => {
            let mut brew_params = HashMap::new();
            if let Some(id) = context.get("sourceId").cloned().filter(|v| !v.is_null()) {
                brew_params.insert("sourceId".into(), id);
            }
            if let Some(id) = context.get("itemId").cloned().filter(|v| !v.is_null()) {
                brew_params.insert("itemId".into(), id);
            }
            if let Some(cat) = context.get("category").cloned().filter(|v| !v.is_null()) {
                brew_params.insert("category".into(), cat);
            }
            let level = if brew_params.contains_key("itemId") {
                "article"
            } else if brew_params.contains_key("sourceId") {
                "items"
            } else {
                "sources"
            };
            brew_params.insert("level".into(), json!(level));
            super::data_read::execute("brew.page", &brew_params, ctx).await
        }
        "tapp" => {
            let tapp_id = context
                .get("tappId")
                .or_else(|| params.get("tappId"))
                .cloned()
                .filter(|v| v.as_str().map(|s| !s.trim().is_empty()).unwrap_or(false));
            let Some(tapp_id) = tapp_id else {
                return Err(
                    "page.content 读取 Tapp 页需要 context.tappId，或由前端提供 content 快照"
                        .to_string(),
                );
            };
            let mut tapp_params = HashMap::new();
            tapp_params.insert("tappId".into(), tapp_id);
            super::data_read::execute("tapp.page", &tapp_params, ctx).await
        }
        "platform" => {
            let platform = context
                .get("platform")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .or_else(|| route_ctx.get("platform").and_then(Value::as_str));
            let Some(platform) = platform else {
                return Err(
                    "page.content 读取平台页需要 context.platform，或由前端提供 content 快照"
                        .to_string(),
                );
            };
            let mut platform_params = HashMap::new();
            platform_params.insert("platform".into(), json!(platform));
            super::data_read::execute("platform.read", &platform_params, ctx).await
        }
        "report" => super::data_read::execute("report.list", params, ctx).await,
        _ => Err(format!(
            "无法读取页面内容：pageType={page_type} 没有对应的数据能力，且未提供 context 快照"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marked_detail_does_not_leak_approved_as_granted() {
        let approved = json!(["ui:theme", "media:control"]);
        assert_eq!(
            agent_detail_granted_permissions(&approved, true),
            json!([])
        );
        assert_eq!(
            agent_detail_granted_permissions(&approved, false),
            approved
        );
    }
}
