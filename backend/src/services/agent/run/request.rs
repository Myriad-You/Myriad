//! What a run is asked with: the message's limits, its context as the agent
//! reads it, and how a data display hint is shaped for the response.

use axum::{Json, http::StatusCode};
use myriad_error::AppError;

use crate::error::HttpError;
use crate::services::agent::RequestContext;
use crate::services::agent::ai_process_pure::USER_TEXT_MAX_CHARS;

use super::*;

/// 输入限制常量（Unicode 标量，不是字节）
pub(crate) const MAX_INPUT_LEN: usize = USER_TEXT_MAX_CHARS;
pub(crate) const MAX_HISTORY_ITEMS: usize = 50;

/// 将 API 层的 ProcessContext 转换为 service 层的 RequestContext
pub(crate) fn build_request_context(mut ctx: ProcessContext) -> RequestContext {
    let images = crate::services::agent::chat_attachments::take_images(ctx.custom_data.as_mut());
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
        // Only a channel group run sets this, after building the context.
        venue: None,
        images,
        // Only a channel private chat sets this, after building the context.
        channel_chat: None,
        chime: None,
        speaker: None,
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
