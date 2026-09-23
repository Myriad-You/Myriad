//! Authenticated / DB-backed application routes (AppState).
use super::*;

pub(super) fn build_authenticated_router(
    app_state: crate::state::AppState,
) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    Router::<crate::state::AppState>::new()
        // 单平台报告生成 — admin_middleware
        .route(
            "/api/reports/platform",
            post(api::reports::generate_platform_reports).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/reports/generate-all",
            post(api::reports::generate_all_reports).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route("/api/platforms", get(api::platforms::list_platforms))
        .route(
            "/api/github/repo",
            get(api::github_stars::get_repo).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Site-funded prompt generation is a management operation.
        .route(
            "/api/prompt/generate",
            post(api::prompt::generate_prompt).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/media/migration",
            get(api::media::media_migration_status)
                .post(api::media::advance_media_migration)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        // Home free-layout stickers — admin AI generation + local upload
        .route(
            "/api/home/stickers/generate",
            post(api::home_stickers::generate_home_sticker)
                .layer(axum::extract::DefaultBodyLimit::max(14 * 1024 * 1024))
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/home/stickers/upload",
            post(api::home_stickers::upload_home_sticker)
                .layer(axum::extract::DefaultBodyLimit::max(14 * 1024 * 1024))
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/media",
            get(api::media::list_media)
                .post(api::media::upload_media)
                .layer(axum::extract::DefaultBodyLimit::max(24 * 1024 * 1024))
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/media/{id}/edit-preview",
            post(api::media_edit::preview_edit)
                .layer(axum::extract::DefaultBodyLimit::max(16 * 1024))
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/media/{id}/edits",
            post(api::media_edit::save_edit)
                .layer(axum::extract::DefaultBodyLimit::max(14 * 1024 * 1024))
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/media/{id}/content",
            get(api::media::serve_media_content)
                .head(api::media::serve_media_content)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        .route(
            "/api/media/{id}/publication",
            post(api::media::publish_media)
                .delete(api::media::unpublish_media)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/media/{id}",
            axum::routing::delete(api::media::delete_media).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/home/widget-fonts",
            post(api::widget_fonts::upload_widget_font)
                .layer(axum::extract::DefaultBodyLimit::max(4 * 1024 * 1024))
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        // SEO / GEO AI copy assist — admin only (site branding)
        .route(
            "/api/seo/generate-copy",
            post(api::seo_geo::generate_site_seo_copy).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/seo/apply-copy",
            post(api::seo_review::apply_site_seo_copy).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // Global platform reprocess jobs — admin only (site-level work, not per-user).
        // any authenticated user must not submit/list global reprocess tasks.
        .route(
            "/api/tasks",
            post(api::tasks::submit_task)
                .get(api::tasks::list_tasks)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/tasks/{task_id}",
            get(api::tasks::get_task_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/tasks/platform/{platform}",
            get(api::tasks::get_platform_task).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // 缓存管理 API — admin only (site-owner platform cache; not per-login-user data)
        .route(
            "/api/cache/status",
            get(api::cache::get_cache_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/cache/status/{platform}",
            get(api::cache::get_platform_cache_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/cache/previews",
            get(api::cache::get_all_platform_cache_previews).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/cache/preview/{platform}",
            get(api::cache::get_platform_cache_preview).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/cache/{platform}",
            delete(api::cache::clear_platform_cache).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/cache/clear",
            post(api::cache::clear_caches).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/cache/all",
            delete(api::cache::clear_all_caches).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // Profile fetch/refresh uses site-owner platform credentials — admin only
        // (any logged-in user must not trigger IDOR-style credential use).
        .route(
            "/api/profile/fetch-all",
            post(api::profile::fetch_all_data).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/profile/fetch-platform",
            post(api::profile::fetch_single_platform_data).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/profile/refresh",
            post(api::profile::refresh_platform_data).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/profile/cache",
            delete(api::profile::delete_platform_cache).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // Raw platform cache dump — admin only (never public; contains full raw JSON)
        .route(
            "/api/profile/metadata",
            get(api::profile::get_raw_metadata).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // Library — 公开读站长资料库
        .route("/api/library", get(api::profile::get_library_data))
        .route(
            "/api/library/preferences",
            get(api::profile::get_library_source_preferences),
        )
        .route(
            "/api/library/preferences",
            axum::routing::put(api::profile::update_library_source_preferences).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware),
            ),
        )
        // Recent activities — 公开读站长动态
        .route("/api/activities", get(api::profile::get_recent_activities))
        // GET /api/reports/latest 公开；GET /api/reports/list 须登录
        .route("/api/reports/latest", get(api::reports::get_latest_report))
        .route(
            "/api/reports/list",
            get(api::tapp_runtime::list_reports).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Tapp 应用管理 API
        // 公开目录/资源 optional_current_auth；安装/启停/写路径须登录（create_tapp_routes）
        .nest(
            "/api/tapps",
            api::tapp_store::create_tapp_routes(app_state.clone()),
        )
        // Tapp Playground（管理员 + Pro 模型）
        .nest(
            "/api/tapp-playground",
            api::tapp_playground::create_playground_routes(app_state.clone()),
        )
        // Tripo 3D
        // Provider operations are admin-only; content-addressed GLB assets
        // remain public so guest home scenes can render them.
        .nest(
            "/api/model3d",
            api::model3d::create_routes(app_state.clone()),
        )
        // Phantasi 阅读 API
        // RSS/Atom 订阅管理、文章获取、阅读状态同步
        .nest(
            "/api/phantasi",
            api::phantasi::create_phantasi_routes(app_state.clone()),
        )
        .nest("/api/brew", api::phantasi::legacy_brew_image_cache_routes())
        // Phantasiai AI 增强 API
        // 注释 / 播客脚本 / 风格标签
        .nest(
            "/api/phantasiai",
            api::phantasiai::create_phantasiai_routes(app_state.clone()),
        )
        // Tapp API
        // Platform reads: public site cache + Runtime Grant (guest widgets OK).
        // Platform writes still require a logged-in user.
        .route(
            "/api/tapp/platform/{platform}/data",
            get(api::tapp_runtime::get_platform_data).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/platform/{platform}/stats",
            get(api::tapp_runtime::get_platform_stats).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/platform/{platform}/distribution/{dimension}",
            get(api::tapp_runtime::get_platform_distribution).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/platform/items",
            post(api::tapp_runtime::add_platform_item).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/platform/items/batch",
            post(api::tapp_runtime::add_platform_items_batch).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Site analytics aggregates for Tapp (guest-safe; Runtime Grant analytics:read)
        .route(
            "/api/tapp/analytics/summary",
            get(api::tapp_runtime::get_tapp_analytics_summary).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/analytics/visitor",
            get(api::tapp_runtime::get_tapp_analytics_visitor).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // AI Task API — optional_auth；elevated AI 能力走授予（可下放）
        .route(
            "/api/tapp/ai/v2/tasks",
            post(api::tapp_runtime::create_ai_task).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/ai/v2/tasks/{task_id}",
            get(api::tapp_runtime::get_ai_task)
                .delete(api::tapp_runtime::cancel_ai_task)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::optional_auth_middleware,
                )),
        )
        .route(
            "/api/tapp/ai/v2/tasks/{task_id}/events",
            get(api::tapp_runtime::stream_ai_task_events).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/ai/v2/usage",
            get(api::tapp_runtime::ai_usage).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // TAPP Tripo 3D — optional_auth + 授予 3d:generate
        .route(
            "/api/tapp/3d/status",
            get(api::tapp_runtime::model3d_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/3d/files",
            post(api::tapp_runtime::upload_model3d_file)
                .route_layer(axum::extract::DefaultBodyLimit::max(24 * 1024 * 1024))
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::optional_auth_middleware,
                )),
        )
        .route(
            "/api/tapp/3d/tasks",
            post(api::tapp_runtime::create_model3d_task).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/3d/tasks/{task_id}",
            get(api::tapp_runtime::get_model3d_task).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/3d/tasks/{task_id}/await",
            post(api::tapp_runtime::await_model3d_task).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // 独立 AI 费用账本（宿主 UI 专用，读取本人逐次调用流水）
        .route(
            "/api/tapp/ai/v2/ledger",
            get(api::tapp_runtime::ai_cost_ledger).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Tapp 扩展 API
        // Data Processing: inline transforms support guests; platform/storage
        // inputs and outputs are still denied without their Runtime Grant permissions.
        .route(
            "/api/tapp/data/transform",
            post(api::tapp_runtime::data_transform).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // Cross-Tapp data remains private until a visible one-shot host
        // authorization has produced a consumable Data Access Grant.
        .route(
            "/api/tapp/data-exchange/requests",
            post(api::tapp_runtime::prepare_data_exchange).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/data-exchange/requests/{request_id}/authorize",
            post(api::tapp_runtime::authorize_data_exchange).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/data-exchange/requests/{request_id}",
            delete(api::tapp_runtime::cancel_data_exchange).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/data-exchange/consume",
            post(api::tapp_runtime::consume_data_exchange).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // Context API — optional_auth；不校验授予
        .route(
            "/api/tapp/context/app",
            get(api::tapp_runtime::get_context_app).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/context/user",
            get(api::tapp_runtime::get_context_user).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/context/player",
            get(api::tapp_runtime::get_context_player).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/context/navigation",
            get(api::tapp_runtime::get_context_navigation).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/context/system",
            get(api::tapp_runtime::get_context_system).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // Tapp 报告 catalog + CRUD — 须登录
        .route(
            "/api/tapp/reports",
            post(api::tapp_runtime::create_report).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/report-catalog",
            get(api::tapp_runtime::list_runtime_reports).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/report-catalog/{report_id}",
            get(api::tapp_runtime::get_runtime_report).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/report-catalog/platform/{platform}",
            get(api::tapp_runtime::get_runtime_platform_report).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/reports/tapp/{tapp_id}",
            get(api::tapp_runtime::list_tapp_reports).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/reports/{tapp_id}/{report_id}",
            get(api::tapp_runtime::get_tapp_report)
                .put(api::tapp_runtime::update_tapp_report)
                .delete(api::tapp_runtime::delete_tapp_report)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        // Media Control — optional_auth；`media:control` 为 basic 授予，不是下放
        .route(
            "/api/tapp/media/control",
            post(api::tapp_runtime::media_control).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/media/status",
            get(api::tapp_runtime::media_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // Tapp notifications - unified notification pipeline
        .route(
            "/api/tapp/notifications",
            post(api::tapp_runtime::create_tapp_notification).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Component Registration
        .route(
            "/api/tapp/components/register",
            post(api::tapp_runtime::register_component).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/components/{tapp_id}/{component_type}/{component_id}",
            delete(api::tapp_runtime::unregister_component).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/components/{tapp_id}",
            get(api::tapp_runtime::list_components).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/components/all/{component_type}",
            get(api::tapp_runtime::list_all_components_by_type).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Shortcut Registration
        .route(
            "/api/tapp/shortcuts/register",
            post(api::tapp_runtime::register_shortcut).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/shortcuts/{tapp_id}/{shortcut_id}",
            delete(api::tapp_runtime::unregister_shortcut).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/shortcuts",
            get(api::tapp_runtime::list_shortcuts).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Manifest-scoped Event Broker
        .route(
            "/api/tapp/events/publish",
            post(api::tapp_runtime::publish_event).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/events/stream",
            get(api::tapp_runtime::stream_events).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // Metrics & Rate Limit
        .route(
            "/api/tapp/metrics",
            get(api::tapp_runtime::get_tapp_metrics).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/rate-limit/{tapp_id}",
            get(api::tapp_runtime::get_rate_limit_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Tapp 定时任务 API
        // Scheduler -  REQUIRE AUTHENTICATION
        .route(
            "/api/tapp/scheduler/tasks",
            get(api::tapp_scheduler::list_tasks)
                .post(api::tapp_scheduler::register_task)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        .route(
            "/api/tapp/scheduler/{tapp_id}/tasks",
            get(api::tapp_scheduler::list_tapp_tasks).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}",
            get(api::tapp_scheduler::get_task)
                .delete(api::tapp_scheduler::unregister_task)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::auth_middleware,
                )),
        )
        .route(
            "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}/enable",
            post(api::tapp_scheduler::enable_task).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}/disable",
            post(api::tapp_scheduler::disable_task).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}/trigger",
            post(api::tapp_scheduler::trigger_task).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/tapp/scheduler/ws",
            get(api::tapp_scheduler::scheduler_websocket).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Tapp API 声明系统
        // 声明 API 执行 — optional_auth；audience public/protected/manager；http 另需授予 network:fetch
        .route(
            "/api/tapp/{tapp_id}/api/{api_name}",
            post(api::tapp_runtime::execute_tapp_api).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // API List - 列出 Tapp 可用的 API
        .route(
            "/api/tapp/{tapp_id}/apis",
            get(api::tapp_runtime::list_tapp_apis).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // Inbound declared routes for other programs. No Runtime Grant, no
        // guest cookie, and no unauthenticated catalog (that would advertise
        // which public installs expose signed endpoints).
        .route(
            "/tapi/{tapp_id}/{route}",
            get(api::tapp_runtime::execute_inbound_route)
                .post(api::tapp_runtime::execute_inbound_route)
                .layer(axum::extract::DefaultBodyLimit::max(
                    myriad_tapp_contract::contract_rules::ROUTE_MAX_BODY_BYTES,
                )),
        )
        // Context Geo - 公开 API，获取客户端地理位置
        .route(
            "/api/tapp/context/geo",
            get(api::tapp_runtime::get_context_geo).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // Image proxy route
        .route("/api/proxy/image", get(api::proxy::proxy_image))
        // Client geo location route
        .route("/api/proxy/client-geo", get(api::proxy::get_client_geo))
        // Hitokoto proxy route
        .route("/api/proxy/hitokoto", get(api::proxy::proxy_hitokoto))
        // auth_middleware + compute rate-limit。
        .route(
            "/api/proxy/fetch-content",
            get(api::proxy::fetch_web_content).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
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
        // stay public too so phantasi embeds / music player work without login.
        .route(
            "/api/proxy/music/netease/audio/{id}",
            get(api::proxy::proxy_netease_audio),
        )
        // 解析播放链（默认 302，可 json），音频字节不经本机
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
        // 解析临时播放链（默认 302，可 json），音频字节直连 QQ CDN
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
        // Public Steam Store appdetails (no API key). Used by phantasi embedProcessor
        // without credentials — must stay guest-reachable. Debug routes above
        // remain admin (they use server steam_api_key).
        .route(
            "/api/steam/game/{app_id}",
            get(api::steam::get_steam_game_details),
        )
        // X 调试读 — 须登录；bearer 仅服务端配置，query 拒绝
        .route(
            "/api/x/user",
            get(api::x::get_x_user).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/x/user/info",
            get(api::x::get_x_user_info).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 分享到 X：仅生成 Intent 链接（不代发帖、不 OAuth）
        .route(
            "/api/x/share/status",
            get(api::x::share_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/x/share",
            post(api::x::share_to_x).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Discord 调试读 — 须登录；token 仅服务端 OAuth 落库，query 拒绝
        .route(
            "/api/discord/status",
            get(api::discord::discord_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/discord/me",
            get(api::discord::get_discord_me).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/discord/profile",
            get(api::discord::get_discord_profile).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Discord 数据平台 OAuth：start 须登录，handler 再验管理员；callback 公开 + state CSRF
        .route(
            "/api/platforms/discord/oauth/start",
            get(api::discord::oauth_start).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/platforms/discord/oauth/callback",
            get(api::discord::oauth_callback),
        )
        // MyAnimeList — 调试接口：username 必填，client_id 可选；正式同步走配置 + profile fetch
        .route(
            "/api/mal/user",
            get(api::mal::get_mal_user).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/mal/user/{username}",
            get(api::mal::get_mal_user_info).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/mal/anime/{username}",
            get(api::mal::get_mal_anime_list).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/mal/manga/{username}",
            get(api::mal::get_mal_manga_list).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // Updater admin proxy (must be under AppState so admin_middleware can State-extract DB)
        .route(
            "/api/admin/updater/status",
            get(api::updater_admin::status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/available",
            get(api::updater_admin::available).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/jobs",
            get(api::updater_admin::jobs).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/jobs/{id}",
            get(api::updater_admin::job).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/snapshots",
            get(api::updater_admin::snapshots).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/snapshots/{id}",
            axum::routing::delete(api::updater_admin::delete_snapshot).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware),
            ),
        )
        .route(
            "/api/admin/updater/commits",
            get(api::updater_admin::commits).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/builds",
            get(api::updater_admin::builds).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/releases",
            get(api::updater_admin::releases).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/compare",
            get(api::updater_admin::compare).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/update",
            post(api::updater_admin::trigger_update).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // Public name must match frontend `updaterApi.setPrefs` → POST /prefs
        // (handler still proxies to the updater service's own `/prefs`).
        .route(
            "/api/admin/updater/prefs",
            post(api::updater_admin::set_prefs).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/last-failed/dismiss",
            post(api::updater_admin::dismiss_last_failed).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/self-update/last/dismiss",
            post(api::updater_admin::dismiss_self_update_last).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/rollback",
            post(api::updater_admin::rollback).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/diagnostics",
            get(api::updater_admin::diagnostics).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/process-logs",
            get(api::updater_admin::process_logs).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/rescue/exit-maintenance",
            post(api::updater_admin::exit_maintenance).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/rescue/forget-current",
            post(api::updater_admin::forget_current).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/rescue/continue",
            post(api::updater_admin::rescue_continue).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/self-update",
            post(api::updater_admin::self_update).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/updater/self-update/last",
            get(api::updater_admin::self_update_last).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
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
        let end = after
            .find(".route(")
            .unwrap_or_else(|| after.len().min(800));
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
        // site-level reprocess must not be open to every authenticated user.
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
        // Guest phantasi/music: no auth/admin layer on play-url / audio / steam store details.
        assert!(
            !route_has_middleware(
                src,
                "/api/proxy/music/netease/play-url/{id}",
                "auth_middleware"
            ) && !route_has_middleware(
                src,
                "/api/proxy/music/netease/play-url/{id}",
                "admin_middleware"
            ),
            "play-url must stay guest-public"
        );
        assert!(
            !route_has_middleware(
                src,
                "/api/proxy/music/netease/audio/{id}",
                "auth_middleware"
            ) && !route_has_middleware(
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
        assert!(route_has_middleware(
            src,
            "/api/proxy/fetch-content",
            "auth_middleware"
        ));
    }

    #[test]
    fn updater_prefs_public_path_matches_frontend_and_is_admin() {
        let src = router_src();
        // Canonical public path (frontend updaterApi.setPrefs → POST /prefs).
        assert!(
            route_has_middleware(src, "/api/admin/updater/prefs", "admin_middleware"),
            "prefs must be registered under admin_middleware"
        );
        // 不得把 prefs 再注册成 /api/admin/updater/defaults
        assert!(
            !src.contains("\"/api/admin/updater/defaults\""),
            "defaults must not be the public prefs route name"
        );
        assert!(
            route_has_middleware(
                src,
                "/api/admin/updater/last-failed/dismiss",
                "admin_middleware"
            ),
            "last-failed dismiss must be registered under admin_middleware"
        );
        assert!(
            route_has_middleware(
                src,
                "/api/admin/updater/self-update/last/dismiss",
                "admin_middleware"
            ),
            "self-update last dismiss must be registered under admin_middleware"
        );
        assert!(
            route_has_middleware(src, "/api/media", "admin_middleware"),
            "media catalog must be admin only"
        );
        assert!(
            route_has_middleware(src, "/api/media/{id}", "admin_middleware"),
            "media delete must be admin only"
        );
        assert!(
            route_has_middleware(src, "/api/media/{id}/content", "auth_middleware"),
            "private media content must be authenticated"
        );
        assert!(
            route_has_middleware(src, "/api/media/{id}/publication", "admin_middleware"),
            "media publication must be admin only"
        );
    }

    #[test]
    fn process_log_export_is_admin_only() {
        assert!(route_has_middleware(
            router_src(),
            "/api/admin/updater/process-logs",
            "admin_middleware"
        ));
    }
}
