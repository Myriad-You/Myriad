//! 资源创建能力处理器
//!
//! 处理 tapp.generate, report.create, reminder.create 等资源创建类能力。
//! 纯投影见 [`crate::services::agent::resource_create_pure`]。

use super::HandlerContext;
use crate::models::entities::{tapp_storage, tapps};
use crate::services::agent::resource_create_pure::{
    agent_page_require_core_source, extract_html_title, extract_note_content, extract_string_tags,
    format_bookmark_id, format_note_id, format_reminder_id, format_report_id,
    generated_tapp_fallback, limit_html_for_title, manifest_permission_strings,
    normalize_agent_tapp_manifest, note_auto_title, parse_generated_tapp_json,
    reminder_repeat_or_default, render_report_content, require_nonempty_code,
    resolve_bookmark_title, truncate_json_for_prompt, AGENT_BOOKMARKS_TAPP_ID, AGENT_NOTES_TAPP_ID,
    AGENT_REMINDERS_TAPP_ID, AGENT_REPORTS_TAPP_ID,
};
use crate::services::data_paths::paths;
use crate::services::permission_service::{TappPermissionService, UserRole};
use crate::services::tapp_install::select_install_approved_permissions;
use crate::services::tapp_package_read::{installed_core_entry, installed_page_entry};
use crate::GLOBAL_DYNAMIC_CONFIG;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use serde_json::{json, Value};
use std::collections::HashMap;

/// 执行资源创建能力
pub async fn execute(
    capability_id: &str,
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    match capability_id {
        "tapp.generate" => execute_tapp_generate(params, ctx).await,
        "tapp.install" => execute_tapp_install(params, ctx).await,
        "report.create" => execute_report_create(params, ctx).await,
        "reminder.create" => execute_reminder_create(params, ctx).await,
        "note.create" => execute_note_create(params, ctx).await,
        "bookmark.save" => execute_bookmark_save(params, ctx).await,
        _ => Err(format!(
            "Unknown resource_create capability: {}",
            capability_id
        )),
    }
}

fn persist_resource_error(kind: &str, error: impl std::fmt::Display) -> String {
    tracing::error!(%error, kind, "Failed to save agent resource");
    format!("Failed to save {kind}")
}

// Tapp 生成

async fn persist_agent_tapp(
    ctx: &HandlerContext<'_>,
    tapp_id: &str,
    name: &str,
    description: Option<String>,
    code: &str,
    manifest: Value,
    author: Value,
) -> Result<chrono::DateTime<Utc>, String> {
    require_nonempty_code(code)?;
    let manifest =
        normalize_agent_tapp_manifest(manifest, tapp_id, name, description.as_deref(), &author)?;
    let requested_permissions = manifest_permission_strings(&manifest);
    let approved_permissions = select_install_approved_permissions(&requested_permissions, &[]);
    let role = if crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await {
        UserRole::Admin
    } else {
        UserRole::User
    };
    let granted_permissions = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        TappPermissionService::filter_permissions_for_role(&config, role, &approved_permissions)
    }
    .map_err(|error| format!("{}: {}", error.code(), error.message()))?;

    let tapp_dir = paths().tapp_user_dir(ctx.user_id).join(tapp_id);
    let core_entry = installed_core_entry(&manifest)
        .ok_or_else(|| "Tapp core.entry is required after normalize".to_string())?;
    let page_entry = installed_page_entry(&manifest)
        .ok_or_else(|| "Tapp page.entry is required after normalize".to_string())?;
    let page_source = agent_page_require_core_source(&page_entry, &core_entry)?;
    let code_path = tapp_dir.join(&core_entry);
    let page_path = tapp_dir.join(&page_entry);
    let manifest_path = tapp_dir.join("manifest.json");
    if let Some(parent) = code_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to create Tapp directory");
            "Failed to create Tapp directory".to_string()
        })?;
    }
    if let Some(parent) = page_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to create Tapp directory");
            "Failed to create Tapp directory".to_string()
        })?;
    }
    tokio::fs::write(&code_path, code).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to write Tapp core");
        "Failed to write Tapp core".to_string()
    })?;
    tokio::fs::write(&page_path, page_source)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to write Tapp page entry");
            "Failed to write Tapp page entry".to_string()
        })?;
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| {
        tracing::error!(error = %e, "Failed to serialize Tapp manifest");
        "Failed to serialize Tapp manifest".to_string()
    })?;
    tokio::fs::write(&manifest_path, manifest_json)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to write Tapp manifest");
            "Failed to write Tapp manifest".to_string()
        })?;

    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("1.0.0")
        .to_string();
    let icon = manifest
        .get("icon")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let theme_color = manifest
        .get("themeColor")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let now = Utc::now();
    let new_tapp = tapps::ActiveModel {
        tapp_id: Set(tapp_id.to_string()),
        user_id: Set(ctx.user_id),
        name: Set(name.to_string()),
        version: Set(version),
        description: Set(description),
        author: Set(Some(author)),
        icon: Set(icon),
        theme_color: Set(theme_color),
        manifest: Set(manifest),
        status: Set(tapps::TappStatus::Running),
        granted_permissions: Set(json!(granted_permissions)),
        approved_permissions: Set(json!(approved_permissions)),
        file_path: Set(manifest_path.to_string_lossy().to_string()),
        code_path: Set(code_path.to_string_lossy().to_string()),
        installed_at: Set(now.into()),
        last_run_at: Set(None),
        updated_at: Set(now.into()),
        error_message: Set(None),
        ..Default::default()
    };
    if let Err(error) = new_tapp.insert(ctx.db).await {
        let _ = tokio::fs::remove_dir_all(&tapp_dir).await;
        tracing::error!(%error, "Failed to persist Tapp");
        return Err("Failed to persist Tapp".to_string());
    }

    Ok(now)
}

async fn execute_tapp_generate(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let analyzer = ctx
        .ai_analyzer
        .ok_or("AI analyzer not configured for Tapp generation")?;

    let description = params
        .get("description")
        .and_then(Value::as_str)
        .or_else(|| params.get("requirements").and_then(Value::as_str))
        .unwrap_or("一个简单的 Tapp 应用");
    let input_data = params.get("input").or_else(|| params.get("data"));
    let input_context = if let Some(data) = input_data {
        let truncated = truncate_json_for_prompt(data, 4000);
        format!(
            "\n\n以下是需要可视化/展示的数据（来自上游步骤的输出）：\n```json\n{}\n```\n\n请基于这些数据生成可视化看板或交互界面。",
            truncated
        )
    } else {
        String::new()
    };

    let prompt = format!(
        r#"请根据以下描述生成一个 Myriad Tapp 应用。

描述：{description}{input_context}

要求：
1. 输出浏览器可直接运行的 JavaScript，不要输出需要构建的 TypeScript
2. 使用全局 Tapp SDK（例如 Tapp.storage、Tapp.pages、Tapp.widgets）
3. core 只放共享状态和后台逻辑，Page/Widget 只负责视图；需要刷新后自动常驻时在 manifest.backgroundRequirements 声明
4. manifest 必须包含 permissions 和 category，按需包含 core / page / widgets / backgroundRequirements。不要写 main、hasPage、cssMode、styles、pageTemplate、pageStyles、widgetStyles、pageModules
5. 如果有数据输入，将数据内嵌到代码中直接展示

只返回合法 JSON：
{{
  "manifest": {{
    "name": "...",
    "version": "1.0.0",
    "description": "...",
    "category": "utility",
    "permissions": [],
    "core": {{ "entry": "core.js" }},
    "page": {{ "entry": "page/index.js" }}
  }},
  "code": "完整 JavaScript 代码（将写入 core.js；page/index.js 会 require 它）"
}}"#
    );

    let result = analyzer.analyze(&prompt).await.map_err(|e| {
        tracing::error!(error = %e, "Tapp generation failed");
        "Tapp generation failed".to_string()
    })?;
    let parsed =
        parse_generated_tapp_json(&result).unwrap_or_else(|| generated_tapp_fallback(&result));

    let tapp_id = format!("agent.generated.{}", uuid::Uuid::new_v4().simple());
    let manifest = parsed.get("manifest").cloned().unwrap_or_else(|| json!({}));
    let tapp_name = manifest
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Agent Generated Tapp")
        .to_string();
    let tapp_description = manifest
        .get("description")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let code = parsed
        .get("code")
        .and_then(Value::as_str)
        .ok_or("Generated response is missing code")?;
    let author = json!({"name": "Agent", "type": "ai_generated"});
    let now = persist_agent_tapp(
        ctx,
        &tapp_id,
        &tapp_name,
        tapp_description,
        code,
        manifest,
        author,
    )
    .await?;

    tracing::info!(
        tapp_id = %tapp_id,
        name = %tapp_name,
        "[TappGenerate] Tapp created through the current runtime layout"
    );

    Ok(json!({
        "success": true,
        "tappId": tapp_id,
        "name": tapp_name,
        "tapp": parsed,
        "frontendAction": {
            "type": "open_window",
            "tappId": tapp_id,
            "timestamp": now.timestamp_millis()
        }
    }))
}

// Tapp 安装

async fn execute_tapp_install(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let code = params
        .get("code")
        .and_then(Value::as_str)
        .ok_or("Missing code parameter. Provide browser-ready Tapp JavaScript.")?;
    let mut manifest = params.get("manifest").cloned().unwrap_or_else(|| json!({}));
    if !manifest.is_object() {
        return Err("manifest must be an object".to_string());
    }

    let name = params
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| manifest.get("name").and_then(Value::as_str))
        .unwrap_or("Installed Tapp")
        .to_string();
    let description = manifest
        .get("description")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let tapp_id = format!("agent.installed.{}", uuid::Uuid::new_v4().simple());
    let author = json!({"name": "Agent Install", "type": "user_install"});
    if let Some(object) = manifest.as_object_mut() {
        object
            .entry("permissions".to_string())
            .or_insert_with(|| json!([]));
    }
    let now = persist_agent_tapp(ctx, &tapp_id, &name, description, code, manifest, author).await?;

    Ok(json!({
        "success": true,
        "tappId": tapp_id,
        "name": name,
        "frontendAction": {
            "type": "open_window",
            "tappId": tapp_id,
            "timestamp": now.timestamp_millis()
        }
    }))
}

// 报告创建

async fn execute_report_create(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("未命名报告");

    // 读取上游步骤通过 inputFrom/analysisFrom 解析后注入的数据
    let analysis = params
        .get("analysis")
        .or_else(|| params.get("input"))
        .or_else(|| params.get("data"))
        .cloned()
        .unwrap_or(json!({}));

    let format = params
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("markdown");

    let now = Utc::now();
    let content = render_report_content(
        title,
        format,
        &analysis,
        &now.format("%Y-%m-%d %H:%M:%S").to_string(),
        &now.to_rfc3339(),
    );

    let report_id = format_report_id(now.timestamp_millis());

    // 持久化到 tapp_storage
    let report_data = json!({
        "id": report_id,
        "title": title,
        "format": format,
        "content": content,
        "analysis": analysis,
        "createdAt": now.to_rfc3339()
    });

    let new_record = tapp_storage::ActiveModel {
        tapp_id: Set(AGENT_REPORTS_TAPP_ID.to_string()),
        user_id: Set(ctx.user_id),
        key: Set(report_id.clone()),
        value: Set(report_data),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };
    new_record
        .insert(ctx.db)
        .await
        .map_err(|error| persist_resource_error("report", error))?;

    tracing::info!(report_id = %report_id, title = %title, "[ReportCreate] Report persisted");

    Ok(json!({
        "success": true,
        "reportId": report_id,
        "title": title,
        "format": format,
        "content": content,
        "frontendAction": {
            "type": "show_report",
            "params": {
                "reportId": report_id,
                "title": title,
                "format": format,
                "content": content
            },
            "timestamp": now.timestamp_millis()
        }
    }))
}

// 提醒/笔记/书签创建

async fn execute_reminder_create(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or("Missing title")?;
    let datetime = params
        .get("datetime")
        .and_then(|v| v.as_str())
        .ok_or("Missing datetime")?;
    let repeat = reminder_repeat_or_default(params.get("repeat").and_then(|v| v.as_str()));

    let now = Utc::now();
    let reminder_id = format_reminder_id(now.timestamp_millis());

    let reminder_data = json!({
        "id": reminder_id,
        "title": title,
        "datetime": datetime,
        "repeat": repeat,
        "status": "active",
        "createdAt": now.to_rfc3339()
    });

    let new_record = tapp_storage::ActiveModel {
        tapp_id: Set(AGENT_REMINDERS_TAPP_ID.to_string()),
        user_id: Set(ctx.user_id),
        key: Set(reminder_id.clone()),
        value: Set(reminder_data),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };
    new_record
        .insert(ctx.db)
        .await
        .map_err(|error| persist_resource_error("reminder", error))?;

    Ok(json!({
        "success": true,
        "reminderId": reminder_id,
        "title": title,
        "datetime": datetime,
        "repeat": repeat,
        "message": "Reminder created and saved",
        "frontendAction": {
            "type": "show_notification",
            "params": {
                "title": crate::services::agent::response_agent::reminder_created(title),
                "message": crate::services::agent::response_agent::reminder_time(datetime),
                "reminderId": reminder_id
            },
            "timestamp": now.timestamp_millis()
        }
    }))
}

async fn execute_note_create(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let content = extract_note_content(params)?;
    let title = params.get("title").and_then(|v| v.as_str());
    let tags = extract_string_tags(params, "tags");

    let now = Utc::now();
    let note_id = format_note_id(now.timestamp_millis());
    let auto_title = note_auto_title(title, &content, 30);

    let note_data = json!({
        "id": note_id,
        "title": auto_title,
        "content": content,
        "tags": tags,
        "createdAt": now.to_rfc3339(),
        "updatedAt": now.to_rfc3339()
    });

    let new_record = tapp_storage::ActiveModel {
        tapp_id: Set(AGENT_NOTES_TAPP_ID.to_string()),
        user_id: Set(ctx.user_id),
        key: Set(note_id.clone()),
        value: Set(note_data),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };
    new_record
        .insert(ctx.db)
        .await
        .map_err(|error| persist_resource_error("note", error))?;

    Ok(json!({
        "success": true,
        "noteId": note_id,
        "title": auto_title,
        "content": content,
        "tags": tags,
        "createdAt": now.to_rfc3339(),
        "frontendAction": {
            "type": "show_notification",
            "params": {
                "title": crate::services::agent::response_agent::note_saved(&auto_title),
                "noteId": note_id
            },
            "timestamp": now.timestamp_millis()
        }
    }))
}

async fn execute_bookmark_save(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("Missing url")?;
    let title = params.get("title").and_then(|v| v.as_str());
    let description = params.get("description").and_then(|v| v.as_str());
    let tags = extract_string_tags(params, "tags");

    let now = Utc::now();
    let bookmark_id = format_bookmark_id(now.timestamp_millis());

    // 尝试获取网页标题（SSRF：outbound_security 公网 DNS 钉扎、禁止重定向）
    let fetched_title = if title.is_none() {
        match crate::services::outbound_security::build_public_http_client(
            url,
            std::time::Duration::from_secs(10),
            Some("Myriad Agent/1.0 (bookmark title)"),
        )
        .await
        {
            Ok((target_url, client)) => {
                if let Ok(resp) = client.get(target_url).send().await {
                    match crate::services::outbound_security::read_limited_body(resp, 256 * 1024)
                        .await
                    {
                        Ok(bytes) => {
                            let body = String::from_utf8_lossy(&bytes);
                            let body_limited = limit_html_for_title(&body);
                            extract_html_title(&body_limited)
                        }
                        Err(_) => None,
                    }
                } else {
                    None
                }
            }
            Err(e) => {
                tracing::debug!(url = %url, error = %e, "[Bookmark] title fetch blocked/failed");
                None
            }
        }
    } else {
        None
    };

    let final_title = resolve_bookmark_title(title, fetched_title.as_deref());

    let bookmark_data = json!({
        "id": bookmark_id,
        "url": url,
        "title": final_title,
        "description": description,
        "tags": tags,
        "createdAt": now.to_rfc3339()
    });

    let new_record = tapp_storage::ActiveModel {
        tapp_id: Set(AGENT_BOOKMARKS_TAPP_ID.to_string()),
        user_id: Set(ctx.user_id),
        key: Set(bookmark_id.clone()),
        value: Set(bookmark_data),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };
    new_record
        .insert(ctx.db)
        .await
        .map_err(|error| persist_resource_error("bookmark", error))?;

    Ok(json!({
        "success": true,
        "bookmarkId": bookmark_id,
        "url": url,
        "title": final_title,
        "description": description,
        "tags": tags,
        "createdAt": now.to_rfc3339(),
        "frontendAction": {
            "type": "show_notification",
            "params": {
                "title": crate::services::agent::response_agent::bookmark_saved(final_title),
                "bookmarkId": bookmark_id,
                "url": url
            },
            "timestamp": now.timestamp_millis()
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DynamicConfig;

    #[test]
    fn agent_approval_survives_role_policy_changes() {
        let requested = manifest_permission_strings(&json!({
            "permissions": ["storage:read", "ai:generate"]
        }));
        let approved = select_install_approved_permissions(&requested, &[]);
        assert_eq!(approved, requested);
        let mut config = DynamicConfig {
            user_perm_ai_generate: false,
            ..DynamicConfig::default()
        };
        assert_eq!(
            TappPermissionService::filter_permissions_for_role(&config, UserRole::User, &approved)
                .unwrap(),
            vec!["storage:read"]
        );
        config.user_perm_ai_generate = true;
        assert_eq!(
            TappPermissionService::filter_permissions_for_role(&config, UserRole::User, &approved)
                .unwrap(),
            approved
        );
    }

    #[test]
    fn agent_install_unknown_permission_remains_fail_closed() {
        let requested = manifest_permission_strings(&json!({
            "permissions": ["storage:read", "unknown:permission"]
        }));
        let approved = select_install_approved_permissions(&requested, &[]);
        assert!(TappPermissionService::filter_permissions_for_role(
            &DynamicConfig::default(),
            UserRole::Admin,
            &approved
        )
        .is_err());
    }
}
