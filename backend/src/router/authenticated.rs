//! Authenticated / DB-backed application routes (AppState).
use super::*;

pub(super) fn build_authenticated_router(
    app_state: crate::state::AppState,
) -> Router<crate::state::AppState> {
        use axum::middleware::from_fn_with_state;
        Router::<crate::state::AppState>::new()
            // 单平台 Insights 生成 -  REQUIRE AUTHENTICATION
            .route(
                "/api/reports/platform",
                post(api::reports::generate_platform_reports)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/reports/generate-all",
                post(api::reports::generate_all_reports)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            // Note: /api/auth/me and /api/auth/logout are now registered above with wrappers
            // Note: /api/config routes are now registered above with wrappers, not here
            // Note: /api/profile/user-info, metadata now registered above with wrappers
            .route("/api/platforms", get(api::platforms::list_platforms))
            .route("/api/profiles", get(api::platforms::get_profiles))
            .route(
                "/api/fetch",
                post(api::platforms::trigger_fetch)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/analysis",
                get(api::analysis::get_analysis)
                    .post(api::analysis::trigger_analysis)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Prompt generation -  REQUIRE AUTHENTICATION
            .route(
                "/api/prompt/generate",
                post(api::prompt::generate_prompt)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // SEO / GEO AI copy assist — admin only (site branding)
            .route(
                "/api/seo/generate-copy",
                post(api::seo_geo::generate_site_seo_copy)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            // Global platform reprocess jobs — admin only (site-level work, not per-user).
            // MYR-015: any authenticated user must not submit/list global reprocess tasks.
            .route(
                "/api/tasks",
                post(api::tasks::submit_task)
                    .get(api::tasks::list_tasks)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/tasks/{task_id}",
                get(api::tasks::get_task_status)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/tasks/platform/{platform}",
                get(api::tasks::get_platform_task)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            // 缓存管理 API — admin only (site-owner platform cache; not per-login-user data)
            .route(
                "/api/cache/status",
                get(api::cache::get_cache_status)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/cache/status/{platform}",
                get(api::cache::get_platform_cache_status)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/cache/previews",
                get(api::cache::get_all_platform_cache_previews)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/cache/preview/{platform}",
                get(api::cache::get_platform_cache_preview)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/cache/{platform}",
                delete(api::cache::clear_platform_cache)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/cache/clear",
                post(api::cache::clear_caches)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/cache/all",
                delete(api::cache::clear_all_caches)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            // Profile fetch/refresh uses site-owner platform credentials — admin only
            // (any logged-in user must not trigger IDOR-style credential use).
            .route(
                "/api/profile/fetch-all",
                post(api::profile::fetch_all_data)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/profile/fetch-platform",
                post(api::profile::fetch_single_platform_data)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/profile/refresh",
                post(api::profile::refresh_platform_data)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/profile/cache",
                delete(api::profile::delete_platform_cache)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            // Raw platform cache dump — admin only (never public; contains full raw JSON)
            .route(
                "/api/profile/metadata",
                get(api::profile::get_raw_metadata)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            // Library data route (公开访问 - 单用户系统)
            .route("/api/library", get(api::profile::get_library_data))
            .route(
                "/api/library/preferences",
                get(api::profile::get_library_source_preferences),
            )
            .route(
                "/api/library/preferences",
                axum::routing::put(api::profile::update_library_source_preferences)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            // Recent activities route (公开访问 - 单用户系统)
            .route("/api/activities", get(api::profile::get_recent_activities))
            // Reports routes (读取端点公开访问，支持未认证用户)
            .route("/api/reports/latest", get(api::reports::get_latest_report))
            .route(
                "/api/reports/list",
                get(api::tapp_runtime::list_reports)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Tapp 应用管理 API
            // 部分公开访问（游客可查看管理员的 Tapp），部分需要认证（在路由内部处理）
            .nest("/api/tapps", api::tapp_store::create_tapp_routes(app_state.clone()))
            // Tapp Playground（管理员 + Pro 模型）
            .nest(
                "/api/tapp-playground",
                api::tapp_playground::create_playground_routes(app_state.clone()),
            )
            // Agent AI 任务编排 API
            // 自然语言任务分解、执行和监控
            .nest("/api/agent", api::agent::create_agent_routes(app_state.clone()))
            // Digital Life 3D
            // Provider operations are admin-only; content-addressed GLB assets
            // remain public so guest home scenes can render them.
            .nest(
                "/api/digital-life/3d",
                api::digital_life_3d::create_routes(app_state.clone()),
            )
            // Brew 阅读 API
            // RSS/Atom 订阅管理、文章获取、阅读状态同步
            .nest("/api/brew", api::brew::create_brew_routes(app_state.clone()))
            // Brewlia AI 增强 API
            // AI 词汇注释、内容摘要等增强阅读功能
            .nest("/api/brewlia", api::brewlia::create_brewlia_routes(app_state.clone()))
            // 语音服务 API
            // 腾讯云 TTS 文本转语音、ASR 语音转文本
            .nest(
                "/api/speech",
                api::speech::create_speech_routes(app_state.clone()),
            )
            // Tapp API
            // Platform reads: public site cache + Runtime Grant (guest widgets OK).
            // Platform writes still require a logged-in user.
            .route(
                "/api/tapp/platform/{platform}/data",
                get(api::tapp_runtime::get_platform_data)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/platform/{platform}/stats",
                get(api::tapp_runtime::get_platform_stats)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/platform/{platform}/distribution/{dimension}",
                get(api::tapp_runtime::get_platform_distribution)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/platform/items",
                post(api::tapp_runtime::add_platform_item)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/platform/items/batch",
                post(api::tapp_runtime::add_platform_items_batch)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Site analytics aggregates for Tapp (guest-safe; Runtime Grant analytics:read)
            .route(
                "/api/tapp/analytics/summary",
                get(api::tapp_runtime::get_tapp_analytics_summary)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/analytics/visitor",
                get(api::tapp_runtime::get_tapp_analytics_visitor)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // AI Task API - 支持权限下放（使用 optional_auth）
            .route(
                "/api/tapp/ai/v2/tasks",
                post(api::tapp_runtime::create_ai_task)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/ai/v2/tasks/{task_id}",
                get(api::tapp_runtime::get_ai_task)
                    .delete(api::tapp_runtime::cancel_ai_task)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/ai/v2/tasks/{task_id}/events",
                get(api::tapp_runtime::stream_ai_task_events)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/ai/v2/usage",
                get(api::tapp_runtime::ai_usage)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // 独立 AI 费用账本（宿主 UI 专用，读取本人逐次调用流水）
            .route(
                "/api/tapp/ai/v2/ledger",
                get(api::tapp_runtime::ai_cost_ledger)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Tapp P0 扩展 API
            // Data Processing: inline transforms support guests; platform/storage
            // inputs and outputs are still denied without their Runtime Grant permissions.
            .route(
                "/api/tapp/data/transform",
                post(api::tapp_runtime::data_transform)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // Cross-Tapp data remains private until a visible one-shot host
            // authorization has produced a consumable Data Access Grant.
            .route(
                "/api/tapp/data-exchange/requests",
                post(api::tapp_runtime::prepare_data_exchange)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/data-exchange/requests/{request_id}/authorize",
                post(api::tapp_runtime::authorize_data_exchange)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/data-exchange/requests/{request_id}",
                delete(api::tapp_runtime::cancel_data_exchange)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/data-exchange/consume",
                post(api::tapp_runtime::consume_data_exchange)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // Context API -  支持权限下放（公开信息）
            .route(
                "/api/tapp/context/app",
                get(api::tapp_runtime::get_context_app)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/context/user",
                get(api::tapp_runtime::get_context_user)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/context/player",
                get(api::tapp_runtime::get_context_player)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/context/navigation",
                get(api::tapp_runtime::get_context_navigation)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/context/system",
                get(api::tapp_runtime::get_context_system)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // Federation feed: guests see public items; users see public + personal items.
            .route(
                "/api/tapp/federation/feed",
                get(api::tapp_runtime::get_federation_feed)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // Tapp P1 扩展 API
            // Report CRUD -  REQUIRE AUTHENTICATION
            .route(
                "/api/tapp/reports",
                post(api::tapp_runtime::create_report)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/report-catalog",
                get(api::tapp_runtime::list_runtime_reports)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/report-catalog/{report_id}",
                get(api::tapp_runtime::get_runtime_report)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/report-catalog/platform/{platform}",
                get(api::tapp_runtime::get_runtime_platform_report)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/reports/tapp/{tapp_id}",
                get(api::tapp_runtime::list_tapp_reports)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/reports/{tapp_id}/{report_id}",
                get(api::tapp_runtime::get_tapp_report)
                    .put(api::tapp_runtime::update_tapp_report)
                    .delete(api::tapp_runtime::delete_tapp_report)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Media Control -  支持权限下放
            .route(
                "/api/tapp/media/control",
                post(api::tapp_runtime::media_control)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/media/status",
                get(api::tapp_runtime::media_status)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // Tapp notifications - unified notification pipeline
            .route(
                "/api/tapp/notifications",
                post(api::tapp_runtime::create_tapp_notification)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Component Registration
            .route(
                "/api/tapp/components/register",
                post(api::tapp_runtime::register_component)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/components/{tapp_id}/{component_type}/{component_id}",
                delete(api::tapp_runtime::unregister_component)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/components/{tapp_id}",
                get(api::tapp_runtime::list_components)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/components/all/{component_type}",
                get(api::tapp_runtime::list_all_components_by_type)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Shortcut Registration
            .route(
                "/api/tapp/shortcuts/register",
                post(api::tapp_runtime::register_shortcut)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/shortcuts/{tapp_id}/{shortcut_id}",
                delete(api::tapp_runtime::unregister_shortcut)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/shortcuts",
                get(api::tapp_runtime::list_shortcuts)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Manifest-scoped Event Broker
            .route(
                "/api/tapp/events/publish",
                post(api::tapp_runtime::publish_event)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/events/stream",
                get(api::tapp_runtime::stream_events)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/stream",
                get(api::tapp_runtime::stream_agent_interactions)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/{interaction_id}",
                get(api::tapp_runtime::get_agent_interaction)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/{interaction_id}/accept",
                post(api::tapp_runtime::accept_agent_interaction)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/{interaction_id}/result",
                post(api::tapp_runtime::submit_agent_interaction_result)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/{interaction_id}/reject",
                post(api::tapp_runtime::reject_agent_interaction)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/{interaction_id}/intents",
                post(api::tapp_runtime::request_agent_intent)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // Metrics & Rate Limit
            .route(
                "/api/tapp/metrics",
                get(api::tapp_runtime::get_tapp_metrics)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/rate-limit/{tapp_id}",
                get(api::tapp_runtime::get_rate_limit_status)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Tapp 定时任务 API
            // Scheduler -  REQUIRE AUTHENTICATION
            .route(
                "/api/tapp/scheduler/tasks",
                get(api::tapp_scheduler::list_tasks)
                    .post(api::tapp_scheduler::register_task)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/{tapp_id}/tasks",
                get(api::tapp_scheduler::list_tapp_tasks)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}",
                get(api::tapp_scheduler::get_task)
                    .delete(api::tapp_scheduler::unregister_task)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}/enable",
                post(api::tapp_scheduler::enable_task)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}/disable",
                post(api::tapp_scheduler::disable_task)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}/trigger",
                post(api::tapp_scheduler::trigger_task)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/ws",
                get(api::tapp_scheduler::scheduler_websocket)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Tapp API 声明系统
            // API Execute - 支持 public 和 protected 两级权限
            .route(
                "/api/tapp/{tapp_id}/api/{api_name}",
                post(api::tapp_runtime::execute_tapp_api)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // API List - 列出 Tapp 可用的 API
            .route(
                "/api/tapp/{tapp_id}/apis",
                get(api::tapp_runtime::list_tapp_apis)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // Context Geo - 公开 API，获取客户端地理位置
            .route(
                "/api/tapp/context/geo",
                get(api::tapp_runtime::get_context_geo)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::optional_auth_middleware)),
            )
            // Image proxy route
            .route("/api/proxy/image", get(api::proxy::proxy_image))
            // Client geo location route
            .route("/api/proxy/client-geo", get(api::proxy::get_client_geo))
            // Hitokoto proxy route
            .route("/api/proxy/hitokoto", get(api::proxy::proxy_hitokoto))
            // Deprecated orphan: no first-party FE caller (reader abandoned "load original").
            // Kept for admin/tools + brew experiments; auth + compute rate-limit required.
            .route(
                "/api/proxy/fetch-content",
                get(api::proxy::fetch_web_content)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Music proxy routes
            .route(
                "/api/proxy/music/netease/playlist/{id}",
                get(api::proxy::proxy_netease_playlist),
            )
            .route(
                "/api/proxy/music/netease/lyrics/{id}",
                get(api::proxy::proxy_netease_lyrics),
            )
            .route(
                "/api/proxy/music/netease/lyrics-verbatim/{id}",
                get(api::proxy::proxy_netease_lyrics_verbatim),
            )
            .route(
                "/api/proxy/music/netease/song/{id}",
                get(api::proxy::proxy_netease_song),
            )
            // Guest-playable: playlist/lyrics/song stay public; audio + play-url
            // stay public too so brew embeds / music player work without login.
            // Abuse control: rate_limit compute-intensive / default buckets.
            .route(
                "/api/proxy/music/netease/audio/{id}",
                get(api::proxy::proxy_netease_audio),
            )
            // 仅解析 HTTPS CDN 播放链（302），音频字节仍直连网易
            .route(
                "/api/proxy/music/netease/play-url/{id}",
                get(api::proxy::proxy_netease_play_url),
            )
            .route(
                "/api/proxy/music/qq/playlist/{id}",
                get(api::proxy::proxy_qq_playlist),
            )
            .route(
                "/api/proxy/music/qq/audio/{id}",
                get(api::proxy::proxy_qq_audio),
            )
            // 仅解析临时播放链（302），音频字节仍直连 QQ CDN（国内 FE geo 分流）
            .route(
                "/api/proxy/music/qq/play-url/{id}",
                get(api::proxy::proxy_qq_play_url),
            )
            .route(
                "/api/proxy/music/qq/lyrics/{id}",
                get(api::proxy::proxy_qq_lyrics),
            )
            .route(
                "/api/proxy/music/kugou/lyrics-verbatim",
                get(api::proxy::proxy_kugou_lyrics_verbatim),
            )
            // Bilibili debug/live-fetch routes — admin only (not public cache)
            .route(
                "/api/bilibili/user",
                get(api::bilibili::get_bilibili_user).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/bilibili/user/{uid}",
                get(api::bilibili::get_bilibili_user_info).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/bilibili/favorites/{uid}",
                get(api::bilibili::get_bilibili_favorites).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/bilibili/bangumi/{uid}",
                get(api::bilibili::get_bilibili_bangumi).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/bilibili/bangumi/all/{uid}",
                get(api::bilibili::get_all_bilibili_bangumi).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            // Bangumi debug routes — admin only; tokens from server config (not query)
            .route(
                "/api/bangumi/user",
                get(api::bangumi::get_bangumi_user).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/bangumi/user/{username}",
                get(api::bangumi::get_bangumi_user_info).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/bangumi/me",
                get(api::bangumi::get_bangumi_me).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/bangumi/collections/{username}",
                get(api::bangumi::get_bangumi_collections).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            // YouTube Data API v3 — admin diagnostic (key smoke-test / curl).
            // Site UI uses config key + reports fetchers; no FE caller yet (kept on purpose).
            .route(
                "/api/youtube/channel",
                get(api::youtube::get_youtube_channel).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/youtube/bundle",
                get(api::youtube::get_youtube_bundle).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            // Steam presence widget (server config only; public-ish for home card)
            .route("/api/steam/presence", get(api::steam::get_steam_presence))
            // Steam debug routes — admin only; API key from server config (never query)
            .route(
                "/api/steam/user",
                get(api::steam::get_steam_user).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/steam/user/info",
                get(api::steam::get_steam_user_info).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/steam/games",
                get(api::steam::get_steam_games).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            // 游戏公开状态小组件（UID / Gamertag / Online ID，无用户 Cookie）
            .route(
                "/api/game/presence",
                get(api::game_presence::get_game_presence),
            )
            .route(
                "/api/game/presence/capabilities",
                get(api::game_presence::get_game_presence_capabilities),
            )
            .route(
                "/api/steam/wishlist/{steam_id}",
                get(api::steam::get_steam_wishlist).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            .route(
                "/api/steam/stats",
                get(api::steam::get_steam_stats).route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
            )
            // Public Steam Store appdetails (no API key). Used by brew embedProcessor
            // without credentials — must stay guest-reachable. Debug routes above
            // remain admin (they use server steam_api_key).
            .route(
                "/api/steam/game/{app_id}",
                get(api::steam::get_steam_game_details),
            )
            // X (Twitter) — 直连调试接口会带 bearer query，必须登录；正式同步走配置 + profile fetch
            .route(
                "/api/x/user",
                get(api::x::get_x_user).route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/x/user/info",
                get(api::x::get_x_user_info)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // 分享到 X：仅生成 Intent 链接（不代发帖、不 OAuth）
            .route(
                "/api/x/share/status",
                get(api::x::share_status).route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/x/share",
                post(api::x::share_to_x).route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Discord — 调试接口带 access_token query，必须登录；正式同步走配置 + profile fetch
            .route(
                "/api/discord/status",
                get(api::discord::discord_status)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/discord/me",
                get(api::discord::get_discord_me)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/discord/profile",
                get(api::discord::get_discord_profile)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            // Discord 数据平台一键授权（start 需 admin cookie；callback 公开 + state CSRF）
            .route(
                "/api/platforms/discord/oauth/start",
                get(api::discord::oauth_start)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/platforms/discord/oauth/callback",
                get(api::discord::oauth_callback),
            )
            // MyAnimeList — 调试接口：username 必填，client_id 可选；正式同步走配置 + profile fetch
            .route(
                "/api/mal/user",
                get(api::mal::get_mal_user).route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/mal/user/{username}",
                get(api::mal::get_mal_user_info)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/mal/anime/{username}",
                get(api::mal::get_mal_anime_list)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )
            .route(
                "/api/mal/manga/{username}",
                get(api::mal::get_mal_manga_list)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware)),
            )

            // Updater admin proxy (must be under AppState so admin_middleware can State-extract DB)
            .route(
                "/api/admin/updater/status",
                get(api::updater_admin::status)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/available",
                get(api::updater_admin::available)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/jobs",
                get(api::updater_admin::jobs)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/jobs/{id}",
                get(api::updater_admin::job)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/snapshots",
                get(api::updater_admin::snapshots)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/snapshots/{id}",
                axum::routing::delete(api::updater_admin::delete_snapshot)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/commits",
                get(api::updater_admin::commits)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/builds",
                get(api::updater_admin::builds)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/releases",
                get(api::updater_admin::releases)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/compare",
                get(api::updater_admin::compare)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/update",
                post(api::updater_admin::trigger_update)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            // Public name must match frontend `updaterApi.setPrefs` → POST /prefs
            // (handler still proxies to the updater service's own `/prefs`).
            .route(
                "/api/admin/updater/prefs",
                post(api::updater_admin::set_prefs)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/rollback",
                post(api::updater_admin::rollback)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/diagnostics",
                get(api::updater_admin::diagnostics)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/rescue/exit-maintenance",
                post(api::updater_admin::exit_maintenance)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/rescue/forget-current",
                post(api::updater_admin::forget_current)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/rescue/continue",
                post(api::updater_admin::rescue_continue)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/self-update",
                post(api::updater_admin::self_update)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/proxy-update",
                post(api::updater_admin::proxy_update)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )
            .route(
                "/api/admin/updater/self-update/last",
                get(api::updater_admin::self_update_last)
                    .route_layer(from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware)),
            )

}

#[cfg(test)]
mod security_route_wiring_tests {
    fn router_src() -> &'static str {
        include_str!("authenticated.rs")
    }

    fn route_has_middleware(src: &str, route_path: &str, middleware: &str) -> bool {
        let needle = format!("\"{route_path}\"");
        let Some(start) = src.find(&needle) else {
            return false;
        };
        let after = &src[start + needle.len()..];
        let end = after.find(".route(").unwrap_or_else(|| after.len().min(800));
        after[..end].contains(middleware)
    }

    #[test]
    fn site_owner_credential_routes_require_admin_middleware() {
        let src = router_src();
        for path in [
            "/api/profile/fetch-all",
            "/api/profile/fetch-platform",
            "/api/profile/refresh",
            "/api/profile/metadata",
            "/api/cache/previews",
            "/api/cache/preview/{platform}",
        ] {
            assert!(
                route_has_middleware(src, path, "admin_middleware"),
                "{path} must be behind admin_middleware"
            );
        }
    }

    #[test]
    fn global_reprocess_task_routes_require_admin_middleware() {
        // MYR-015: site-level reprocess must not be open to every authenticated user.
        let src = router_src();
        for path in [
            "/api/tasks",
            "/api/tasks/{task_id}",
            "/api/tasks/platform/{platform}",
        ] {
            assert!(
                route_has_middleware(src, path, "admin_middleware"),
                "{path} must be behind admin_middleware"
            );
        }
    }

    #[test]
    fn steam_bangumi_debug_routes_require_admin_middleware() {
        let src = router_src();
        for path in [
            "/api/steam/user",
            "/api/steam/user/info",
            "/api/steam/games",
            "/api/steam/stats",
            "/api/bangumi/user",
            "/api/bangumi/me",
            "/api/bangumi/collections/{username}",
        ] {
            assert!(
                route_has_middleware(src, path, "admin_middleware"),
                "{path} must be behind admin_middleware"
            );
        }
    }

    #[test]
    fn guest_music_play_and_steam_game_are_public() {
        let src = router_src();
        // Guest brew/music: no auth/admin layer on play-url / audio / steam store details.
        assert!(
            !route_has_middleware(src, "/api/proxy/music/netease/play-url/{id}", "auth_middleware")
                && !route_has_middleware(
                    src,
                    "/api/proxy/music/netease/play-url/{id}",
                    "admin_middleware"
                ),
            "play-url must stay guest-public"
        );
        assert!(
            !route_has_middleware(src, "/api/proxy/music/netease/audio/{id}", "auth_middleware")
                && !route_has_middleware(
                    src,
                    "/api/proxy/music/netease/audio/{id}",
                    "admin_middleware"
                ),
            "netease audio must stay guest-public"
        );
        assert!(
            !route_has_middleware(src, "/api/proxy/music/qq/audio/{id}", "auth_middleware")
                && !route_has_middleware(src, "/api/proxy/music/qq/audio/{id}", "admin_middleware"),
            "qq audio must stay guest-public"
        );
        assert!(
            !route_has_middleware(src, "/api/proxy/music/qq/play-url/{id}", "auth_middleware")
                && !route_has_middleware(
                    src,
                    "/api/proxy/music/qq/play-url/{id}",
                    "admin_middleware"
                ),
            "qq play-url must stay guest-public"
        );
        assert!(
            !route_has_middleware(src, "/api/steam/game/{app_id}", "admin_middleware")
                && !route_has_middleware(src, "/api/steam/game/{app_id}", "auth_middleware"),
            "steam store game details must stay guest-public for embeds"
        );
        // Open egress still auth-gated.
        assert!(route_has_middleware(src, "/api/proxy/fetch-content", "auth_middleware"));
    }

    #[test]
    fn updater_prefs_public_path_matches_frontend_and_is_admin() {
        let src = router_src();
        // Canonical public path (frontend updaterApi.setPrefs → POST /prefs).
        assert!(
            route_has_middleware(src, "/api/admin/updater/prefs", "admin_middleware"),
            "prefs must be registered under admin_middleware"
        );
        // Do not re-register the broken /defaults alias as the prefs endpoint.
        assert!(
            !src.contains("\"/api/admin/updater/defaults\""),
            "defaults must not be the public prefs route name"
        );
    }
}
