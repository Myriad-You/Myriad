use super::super::HandlerContext;
use serde_json::{json, Value};
use std::collections::HashMap;

pub(super) async fn execute_config_get(params: &HashMap<String, Value>) -> Result<Value, String> {
    let section = params
        .get("section")
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    let mut config = json!({});
    let needs_dynamic =
        section == "all" || section == "ai" || section == "platforms" || section == "ui";
    let dynamic = if needs_dynamic {
        Some(crate::GLOBAL_DYNAMIC_CONFIG.read().await)
    } else {
        None
    };

    if let Some(dynamic) = dynamic.as_ref() {
        if section == "all" || section == "ai" {
            let resolved = dynamic.resolve_ai_config(crate::config::ModelTier::Standard);
            config["ai"] = json!({
                "enabled": dynamic.text_ai_available(),
                "provider": resolved.provider,
                "model": resolved.model,
            });
        }

        if section == "all" || section == "platforms" {
            let mut platforms = serde_json::Map::new();
            for (name, on) in crate::api::config::platform_configured_flags(dynamic) {
                platforms.insert(name.to_string(), json!(on));
            }
            config["platforms"] = Value::Object(platforms);
        }

        if section == "all" || section == "ui" {
            config["ui"] = crate::api::config::public_ui_config_value(dynamic);
        }
    }

    Ok(json!({
        "section": section,
        "config": config
    }))
}

pub(super) async fn execute_time_info(params: &HashMap<String, Value>) -> Result<Value, String> {
    let timezone = params
        .get("timezone")
        .and_then(|v| v.as_str())
        .unwrap_or("Asia/Shanghai");
    crate::services::agent::data_read_pure::project_time_info(chrono::Utc::now(), timezone)
}

pub(super) async fn execute_auth_status(ctx: &HandlerContext<'_>) -> Result<Value, String> {
    use crate::services::agent::merope::is_logged_in_addressee;
    use crate::services::agent::SYSTEM_USER_ID;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    let user_id = ctx.user_id;
    let (username, is_admin, row_exists) = if user_id == SYSTEM_USER_ID {
        (None, true, false)
    } else if user_id > 0 {
        match ctx
            .db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT username, is_admin FROM users WHERE id = $1 LIMIT 1",
                [user_id.into()],
            ))
            .await
        {
            Ok(Some(row)) => (
                row.try_get::<String>("", "username").ok(),
                row.try_get::<bool>("", "is_admin").unwrap_or(false),
                true,
            ),
            Ok(None) => (None, false, false),
            Err(error) => {
                tracing::warn!(
                    user_id,
                    %error,
                    "[Agent] Failed to load auth.status user; using least privilege"
                );
                (None, false, false)
            }
        }
    } else {
        (None, false, false)
    };

    let is_authenticated = is_logged_in_addressee(user_id) && row_exists;

    let configured = {
        let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        crate::api::config::platform_configured_flags(&dynamic)
    };
    let mut linked_platforms = Vec::new();
    for (name, on) in configured {
        if !on {
            continue;
        }
        let path = crate::services::platform_cache::platform_filtered_cache_path(name)?;
        if tokio::fs::metadata(&path).await.is_ok() {
            linked_platforms.push(name);
        }
    }

    Ok(json!({
        "isAuthenticated": is_authenticated,
        "user": {
            "id": user_id,
            "username": username,
            "isAdmin": is_admin,
        },
        "linkedPlatforms": linked_platforms
    }))
}
