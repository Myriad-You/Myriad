use super::super::HandlerContext;
use serde_json::{json, Value};
use std::collections::HashMap;

pub(super) async fn execute_tapp_widget(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let tapp_id = params
        .get("tappId")
        .and_then(Value::as_str)
        .ok_or("Missing tappId parameter")?;
    let mut page_params = params.clone();
    page_params.insert("level".to_string(), json!("widgets"));
    let page = super::super::ui_control::execute_tapp_page_content(&page_params, ctx).await?;
    let widgets = page
        .get("content")
        .and_then(|content| content.get("widgets"))
        .cloned()
        .unwrap_or_else(|| json!([]));

    Ok(json!({
        "tappId": tapp_id,
        "total": widgets.as_array().map(Vec::len).unwrap_or(0),
        "widgets": widgets
    }))
}

/// 带 tappId 的探权：标记需重新授权时授予层为空。
///
/// 不得把批准列里剩下的可解析名字报成 `granted: true`——只有授予权限决定行为，
/// 标记表示授权前提已消失。
pub(super) fn install_permission_is_granted(needs_reauthorization: bool, listed: bool) -> bool {
    !needs_reauthorization && listed
}

/// 权限检查：当前会话角色的授予权限；带 tappId 时再与该安装的批准权限求交。
/// 该安装 `needs_reauthorization` 时一律 `granted: false`。
pub(super) async fn execute_permission_check(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    use crate::services::permission_service::{
        role_from_user_id, TappPermission, TappPermissionService,
    };

    let permission = params
        .get("permission")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tapp_id = params
        .get("tappId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let is_admin = crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await;
    let role = role_from_user_id(ctx.user_id, is_admin);

    let Some(parsed) = TappPermission::from_str(permission) else {
        return Ok(json!({
            "permission": permission,
            "granted": false,
            "role": role.as_str(),
            "tappId": tapp_id,
        }));
    };

    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    let granted = if let Some(tapp_id) = tapp_id {
        match crate::services::tapp_ownership::resolve_accessible_tapp(ctx.db, ctx.user_id, tapp_id)
            .await
        {
            Ok(tapp) => {
                let approved =
                    crate::services::tapp_declared_api::installed_permissions_from_tapp(&tapp);
                let listed = match TappPermissionService::filter_permissions_for_role(
                    &config, role, &approved,
                ) {
                    Ok(granted_list) => granted_list.iter().any(|p| p == parsed.as_str()),
                    Err(_) => false,
                };
                install_permission_is_granted(tapp.needs_reauthorization, listed)
            }
            Err(_) => false,
        }
    } else {
        TappPermissionService::check(&config, role, parsed)
    };

    Ok(json!({
        "permission": permission,
        "granted": granted,
        "role": role.as_str(),
        "tappId": tapp_id,
    }))
}
