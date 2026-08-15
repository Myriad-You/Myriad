//! Agent API — helpers
use super::*;
use crate::error::HttpError;

// 辅助函数

/// 输入限制常量
pub(crate) const MAX_INPUT_LEN: usize = 2000;
pub(crate) const MAX_HISTORY_ITEMS: usize = 50;

/// 将 API 层的 ProcessContext 转换为 service 层的 RequestContext
pub(crate) fn build_request_context(ctx: ProcessContext) -> RequestContext {
    let conversation_history = ctx.conversation_history.map(|msgs| {
        msgs.into_iter()
            .take(MAX_HISTORY_ITEMS)
            .map(|m| crate::services::agent::types::ConversationMessage {
                role: m.role,
                content: m.content,
                created_at: m.created_at,
            })
            .collect()
    });

    RequestContext {
        current_route: ctx.current_route,
        active_platforms: ctx.active_platforms.unwrap_or_default(),
        preferences: None,
        session_id: ctx.session_id,
        conversation_history,
        custom_data: ctx.custom_data,
        lane_key: None, // 由 API 层在调用处注入
        run_id: None,   // 由 process_stream 在 create_run 后注入
    }
}

/// 将 DataDisplayHint 从 service 层转换为 API 层类型
pub(crate) fn convert_data_display_hint(
    hint: crate::services::agent::DataDisplayHint,
) -> DataDisplayHintApi {
    match hint {
        crate::services::agent::DataDisplayHint::Table { columns, data_path } => {
            DataDisplayHintApi::Table {
                columns: columns
                    .into_iter()
                    .map(|c| ColumnDefApi {
                        field: c.field,
                        title: c.title,
                        width: c.width,
                        sortable: c.sortable,
                    })
                    .collect(),
                data_path,
            }
        }
        crate::services::agent::DataDisplayHint::Chart {
            chart_type,
            x_field,
            y_field,
        } => DataDisplayHintApi::Chart {
            chart_type: format!("{:?}", chart_type).to_lowercase(),
            x_field,
            y_field,
        },
        crate::services::agent::DataDisplayHint::CardList {
            title_field,
            description_field,
            image_field,
        } => DataDisplayHintApi::CardList {
            title_field,
            description_field,
            image_field,
        },
        crate::services::agent::DataDisplayHint::Markdown => DataDisplayHintApi::Markdown,
        crate::services::agent::DataDisplayHint::KeyValue => DataDisplayHintApi::KeyValue,
        crate::services::agent::DataDisplayHint::Timeline {
            time_field,
            content_field,
        } => DataDisplayHintApi::Timeline {
            time_field,
            content_field,
        },
        crate::services::agent::DataDisplayHint::Raw => DataDisplayHintApi::Raw,
    }
}

/// 解析 user_id，返回标准化错误
pub(crate) fn parse_user_id(claims: &Claims) -> Result<i32, HttpError> {
    claims.sub.parse::<i32>().map_err(|_| {
        HttpError::from((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        ))
    })
}

/// 解析 user_id 并校验 Agent 可见性/使用权限
pub(crate) async fn parse_user_id_with_agent_access(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<i32, HttpError> {
    let user_id = parse_user_id(claims)?;
    if let Err(msg) = crate::services::agent::ensure_agent_usage_allowed(db, user_id).await {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": msg, "code": "agent_access_denied" })),
        )));
    }
    Ok(user_id)
}

/// Global Agent management surfaces are restricted to the current administrator.
pub(crate) async fn require_current_admin(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<i32, HttpError> {
    let user_id = parse_user_id(claims)?;
    if !crate::services::agent::user_is_current_admin(db, user_id).await {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Administrator access required", "code": "admin_required" })),
        )));
    }
    Ok(user_id)
}

/// 验证输入长度
pub(crate) fn validate_input(input: &str) -> Result<(), HttpError> {
    if input.len() > MAX_INPUT_LEN {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": crate::services::agent::response_agent::input_too_long(MAX_INPUT_LEN)
            })),
        )));
    }
    if input.trim().is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": crate::services::agent::response_agent::input_empty() })),
        )));
    }
    Ok(())
}
