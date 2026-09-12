//! Agent API — helpers
use super::*;
use crate::error::HttpError;
use crate::services::agent::ai_process_pure::USER_TEXT_MAX_CHARS;

// 辅助函数

/// 输入限制常量（Unicode 标量，不是字节）
pub(crate) const MAX_INPUT_LEN: usize = USER_TEXT_MAX_CHARS;
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
        interaction_mode: ctx.mode.unwrap_or_default(),
        current_route: ctx.current_route,
        active_platforms: ctx.active_platforms.unwrap_or_default(),
        preferences: None,
        session_id: ctx.session_id,
        conversation_history,
        custom_data: ctx.custom_data,
        lane_key: None, // 由 API 层在调用处注入
        run_id: None,   // 由 `start_process_run`（及 Chat 同等路径）在 `create_run` 后注入
        source_intent_id: ctx.intention_id,
        autonomy_permission_cap: ctx.autonomy_permission_cap,
        rig_state: ctx
            .rig_state
            .as_ref()
            .and_then(myriad_merope::sanitize_rig_state),
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
            Json(AppError::public_json("Invalid user")),
        ))
    })
}

/// Existing pairings must remain revocable after Agent access is withdrawn.
pub(crate) fn parse_pairing_user_id(claims: &Claims) -> Result<i32, HttpError> {
    let id = parse_user_id(claims)?;
    if id <= 0 {
        return Err(HttpError::from((
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Login required")),
        )));
    }
    Ok(id)
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
    if input.chars().count() > MAX_INPUT_LEN {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                crate::services::agent::response_agent::input_too_long(MAX_INPUT_LEN),
            )),
        )));
    }
    if input.trim().is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                crate::services::agent::response_agent::input_empty(),
            )),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod agent_entry_gate_tests {
    /// 取 `fn <name>(` 之后的一段源码，够覆盖签名和开头几行。
    fn head_of(source: &str, name: &str) -> String {
        let at = source
            .find(&format!("fn {name}("))
            .unwrap_or_else(|| panic!("{name} not found"));
        source[at..].chars().take(700).collect()
    }

    /// 能把一轮 Work 提交进 Agent 的 HTTP 入口，必须走带可见性判定的解析。
    ///
    /// `interrupt_session` 提交 `context: None` 即 Work；只解析 user_id 拦不住。
    #[test]
    fn every_work_entry_checks_module_visibility() {
        let cases: [(&str, &str, &str); 6] = [
            ("process.rs", include_str!("process.rs"), "process"),
            (
                "process.rs",
                include_str!("process.rs"),
                "start_process_run",
            ),
            ("process.rs", include_str!("process.rs"), "clarify"),
            ("presets.rs", include_str!("presets.rs"), "execute_preset"),
            (
                "heartbeat_mcp.rs",
                include_str!("heartbeat_mcp.rs"),
                "interrupt_session",
            ),
            (
                "intentions.rs",
                include_str!("intentions.rs"),
                "accept_intention",
            ),
        ];
        for (file, source, name) in cases {
            let head = head_of(source, name);
            assert!(
                head.contains("parse_user_id_with_agent_access"),
                "{file}::{name} 是 Work 入口，却没有过模块可见性这道门"
            );
        }
    }

    /// 能力过滤是按人给的，不是按档给的：非管理员照常进 Work，
    /// 每一步能不能落地由 `get_user_permissions` 说了算。
    #[test]
    fn work_entries_do_not_gate_on_role() {
        for (file, source) in [
            ("process.rs", include_str!("process.rs")),
            ("presets.rs", include_str!("presets.rs")),
            ("intentions.rs", include_str!("intentions.rs")),
            ("heartbeat_mcp.rs", include_str!("heartbeat_mcp.rs")),
        ] {
            assert!(
                !source.contains("interaction_mode_allowed"),
                "{file} 不该按身份分运行时路径；能力过滤在 get_user_permissions"
            );
        }
    }
}
use myriad_error::AppError;
