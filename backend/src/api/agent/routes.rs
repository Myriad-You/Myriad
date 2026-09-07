//! Agent API — routes
use super::*;

// 路由构建

use axum::routing::{delete, get, post, put};
use axum::Router;

/// 创建 Agent API 路由
pub fn create_agent_routes(app_state: crate::state::AppState) -> Router<crate::state::AppState> {
    use crate::middleware;
    use axum::middleware::from_fn_with_state;

    Router::<crate::state::AppState>::new()
        // 健康检查（公开）
        .route("/health", get(health))
        // 队列状态（需要认证）
        .route(
            "/queue/status",
            get(queue_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Heartbeat 任务列表 / 创建（需要认证 + admin）
        .route(
            "/heartbeat",
            get(heartbeat_tasks)
                .post(create_heartbeat)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        // 切换 Heartbeat 任务启用状态（需要认证）
        .route(
            "/heartbeat/{task_id}/toggle",
            post(toggle_heartbeat).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 更新 / 删除 Heartbeat 任务（需要认证）
        .route(
            "/heartbeat/{task_id}",
            put(update_heartbeat)
                .delete(delete_heartbeat)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        // 热重载 MCP 配置（需要认证）
        .route(
            "/mcp/reload",
            post(reload_mcp).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // MCP 服务器状态（需要认证）
        .route(
            "/mcp/status",
            get(mcp_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // MCP 配置读写（admin UI 编辑 mcp_servers.json）
        .route(
            "/mcp/config",
            get(mcp_get_config)
                .put(mcp_put_config)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        .route(
            "/persona",
            get(super::persona::get_persona)
                .put(super::persona::put_persona)
                .delete(super::persona::delete_persona)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        .route(
            "/wardrobe/{outfit_id}/face",
            get(super::persona::get_wardrobe_face).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/persona/signals",
            axum::routing::post(super::persona::report_signals).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/persona/draft",
            axum::routing::post(super::persona::draft_persona).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/persona/import",
            axum::routing::post(super::persona::import_persona).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/persona/name",
            axum::routing::post(super::persona::suggest_name).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/persona/visual-design",
            axum::routing::post(super::persona::suggest_visual_design).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware),
            ),
        )
        .route(
            "/persona/visual-from-portrait",
            axum::routing::post(super::persona::observe_visual_from_portrait).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware),
            ),
        )
        .route(
            "/addressee",
            put(super::persona::put_addressee).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/addressee/music-listening",
            post(super::persona::post_music_listening).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/presence",
            post(super::post_live_presence).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 能力列表（需要认证）
        .route(
            "/capabilities",
            get(list_capabilities).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 处理自然语言请求（需要认证）
        .route(
            "/process",
            post(process).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 流式处理请求（带实时进度更新）
        .route(
            "/process/stream",
            post(process_stream).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Agent run 状态订阅：GET 可由前端在刷新/断线后安全重连
        .route(
            "/runs/{run_id}/stream",
            get(subscribe_run_stream).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 提供澄清（需要认证）
        .route(
            "/runs/{run_id}/performance",
            get(super::playback_direction::read)
                .put(super::playback_direction::observe)
                .delete(super::playback_direction::close)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        // 提供澄清（需要认证）
        .route(
            "/clarify",
            post(clarify).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 确认敏感操作（需要认证）
        .route(
            "/confirm/stream",
            post(confirm_operation_stream).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 执行追踪列表（需要认证）
        .route(
            "/traces",
            get(list_traces).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 任务列表（需要认证）
        .route(
            "/tasks",
            get(list_tasks).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 任务详情（需要认证）
        .route(
            "/tasks/{task_id}",
            get(get_task).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 取消任务（需要认证）
        .route(
            "/tasks/{task_id}/cancel",
            post(cancel_task).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/tasks/{task_id}/frontend-ack",
            post(frontend_step_ack).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 回答任务问题（需要认证）
        .route(
            "/tasks/{task_id}/answer",
            post(answer_task_question).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 回答任务问题 SSE 流式（需要认证）
        .route(
            "/tasks/{task_id}/answer/stream",
            post(answer_task_question_stream).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 中断当前会话并替换为新请求（需要认证）
        .route(
            "/session/interrupt",
            post(interrupt_session).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/session/cancel-chat",
            post(cancel_chat_turn).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 向当前会话注入转向指令（需要认证）
        .route(
            "/session/steer",
            post(steer_session).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 自主 Work 提案：只有用户接受后，前端才把返回的 input 送入 Work。
        .route(
            "/intentions",
            get(list_intentions).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/intentions/{intent_id}/accept",
            post(accept_intention).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/intentions/{intent_id}/dismiss",
            post(dismiss_intention).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/autonomy",
            get(get_autonomy_grant)
                .put(put_autonomy_grant)
                .delete(delete_autonomy_grant)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        // 会话管理路由
        // 创建会话
        .route(
            "/sessions",
            post(create_session).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 列出会话
        .route(
            "/sessions",
            get(list_sessions).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 获取会话消息
        .route(
            "/sessions/{session_id}/messages",
            get(get_session_messages).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 归档会话
        .route(
            "/sessions/{session_id}",
            axum::routing::delete(archive_session).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 更新会话标题
        .route(
            "/sessions/{session_id}",
            axum::routing::patch(update_session).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // AI 生成会话标题
        .route(
            "/sessions/{session_id}/generate-title",
            post(generate_session_title).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 任务预设路由
        // 预设列表（需要认证）
        .route(
            "/presets",
            get(list_presets).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 创建预设（需要认证）
        .route(
            "/presets",
            post(create_preset).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 删除预设（需要认证）
        .route(
            "/presets/{preset_id}",
            axum::routing::delete(delete_preset).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 切换收藏状态（需要认证）
        .route(
            "/presets/{preset_id}/toggle-favorite",
            post(toggle_favorite).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 更新使用时间（需要认证）
        .route(
            "/presets/{preset_id}/use",
            post(use_preset).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 执行预设（直接执行已保存的 recipe，跳过意图分析）
        .route(
            "/presets/{preset_id}/execute",
            post(execute_preset).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 技能/记忆路由
        // 技能列表（需要认证）
        .route(
            "/skills",
            get(list_skills).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 删除技能（需要认证）
        .route(
            "/skills/{skill_id}",
            delete(delete_skill).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 记忆列表（需要认证）
        .route(
            "/memory",
            get(list_memories).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 删除记忆（需要认证）
        .route(
            "/memory/{memory_id}",
            delete(delete_memory)
                .put(update_memory)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        // 通知路由
        // 通知 SSE 流
        .route(
            "/notifications/stream",
            get(notification_stream).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 当前用户通知偏好与事件目录
        .route(
            "/notifications/preferences",
            get(crate::api::notification_preferences::get_notification_preferences)
                .put(crate::api::notification_preferences::update_notification_preferences)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        // 获取历史通知
        .route(
            "/notifications",
            get(list_notifications).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 删除单条通知
        .route(
            "/notifications/{notification_id}",
            delete(delete_notification).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 清空全部通知
        .route(
            "/notifications/clear",
            post(clear_notifications).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
}
