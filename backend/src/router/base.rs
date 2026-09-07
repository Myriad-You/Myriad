//! Base API routes (setup, system, public config, federation surface).
use super::*;

/// Guard the open setup window before body extraction, database extraction, or
/// Argon2. Handlers then match `MYRIAD_SETUP_SECRET` from header or JSON so a
/// body-only client still works on every setup write.
async fn require_installation_capability(req: Request, next: Next) -> Response {
    if let Err(error) = crate::api::setup_bootstrap::require_setup_window() {
        return crate::error::app_error_response(error);
    }
    next.run(req).await
}

/// Config-mode only: no DB / no `AppState`. Setup + public auth bootstrap surface.
///
/// Handlers that need a live DB use `extract::Db` and get **503** (not 404) when
/// the process has no connection. OAuth list/login stay registered so the SPA
/// does not treat missing routes as hard 404 during setup.
pub(super) fn build_config_mode_router() -> Router {
    Router::new()
        .route("/health", get(api::health))
        .route("/api/setup/config", get(api::setup::get_setup_config))
        // Registered without AppState: extract::Db → 503 until DB is wired.
        .route("/api/setup/status", get(api::setup::check_setup_status))
        .route(
            "/api/setup/database-config",
            post(api::setup::save_database_config)
                .route_layer(from_fn(require_installation_capability)),
        )
        .route(
            "/api/setup/init-database",
            post(api::setup::init_database).route_layer(from_fn(require_installation_capability)),
        )
        .route(
            "/api/setup/create-admin",
            post(api::auth_local::create_admin)
                .route_layer(from_fn(require_installation_capability)),
        )
        // system status stays public for operators during setup
        .route("/api/system/status", get(api::system::system_status))
        // OAuth discovery + login redirect (no AppState); avoids 404→login broken in setup.
        .route("/api/auth/oauth/providers", get(api::oauth::list_providers))
        .route(
            "/api/auth/oauth/{slug}/login",
            get(api::oauth::provider_login),
        )
        .route(
            "/api/auth/oauth/{slug}/callback",
            get(api::oauth::provider_callback),
        )
        .route("/api/auth/login", post(api::auth_local::local_login))
        .route("/api/auth/me", get(api::auth::get_current_user))
        .route("/api/auth/logout", post(api::auth::logout))
}

pub(super) fn build_base_api_router(
    app_state: crate::state::AppState,
) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    let api_router = Router::<crate::state::AppState>::new()
        .route("/health", get(api::health))
        // Setup routes (always available when DB/AppState is wired)
        .route("/api/setup/config", get(api::setup::get_setup_config))
        .route("/api/setup/status", get(api::setup::check_setup_status))
        .route(
            "/api/setup/database-config",
            post(api::setup::save_database_config)
                .route_layer(from_fn(require_installation_capability)),
        )
        .route(
            "/api/setup/init-database",
            post(api::setup::init_database).route_layer(from_fn(require_installation_capability)),
        )
        .route(
            "/api/setup/create-admin",
            post(api::auth_local::create_admin)
                .route_layer(from_fn(require_installation_capability)),
        )
        // System management routes
        // P2: system/status 暴露了一些系统信息，但为了监控保持公开（考虑移除敏感字段）
        .route("/api/system/status", get(api::system::system_status))
        .route(
            "/api/system/reload-config",
            post(api::system::reload_config)
                // P1 修复：配置重载应该只有 admin 可以触发
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        // 系统监控指标端点（内存、任务、连接等）-  需要管理员权限
        .route(
            "/api/metrics",
            get(api::metrics::get_metrics).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // 站点访客统计：collect/pageview 公开写入；summary 仅管理员
        .route("/api/analytics/collect", post(api::analytics::collect))
        .route(
            "/api/analytics/pageview",
            post(api::analytics::record_pageview),
        )
        // 公开访客卡片：只有站点总量 + 7 日趋势 + 调用者自己的到达序号，
        // 页面 / 来源 / 国家 / 停留等细分仍然只走下面的 admin summary
        .route(
            "/api/analytics/visitor",
            get(api::analytics::get_visitor_card),
        )
        .route(
            "/api/analytics/summary",
            get(api::analytics::get_summary).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // AI usage monitor (admin): day / user / model breakdown from cost ledger
        .route(
            "/api/analytics/ai-usage",
            get(api::analytics::get_ai_usage_summary).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/analytics/export",
            get(api::analytics::export_analytics).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/analytics/import",
            post(api::analytics::import_analytics).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/diagnostics",
            get(api::diagnostics::runtime_diagnostics).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // Authentication routes (use wrapper for dynamic DB access)
        .route("/api/auth/login", post(api::auth_local::local_login))
        .route("/api/auth/me", get(api::auth::get_current_user))
        .route(
            "/api/auth/logout",
            post(api::auth::logout), // 不需要认证中间件
        )
        // OAuth routes stay registered even if DB is temporarily unavailable,
        // keeping login/setup surfaces on 503 responses instead of 404s.
        .route("/api/auth/oauth/providers", get(api::oauth::list_providers))
        .route(
            "/api/auth/oauth/{slug}/login",
            get(api::oauth::provider_login),
        )
        .route(
            "/api/auth/oauth/{slug}/callback",
            get(api::oauth::provider_callback),
        )
        .route(
            "/api/auth/oauth/{slug}/link",
            get(api::oauth::provider_link).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/auth/oauth/{slug}/unlink/{identity_id}",
            axum::routing::delete(api::oauth::provider_unlink).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/auth/identities",
            get(api::oauth::list_my_identities).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/auth/identities/{identity_id}/primary",
            post(api::oauth::set_primary_identity).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 画像源：账号 / OAuth 身份 /（站长）平台画像，选定后全站出口同步
        .route(
            "/api/users/me/avatar-sources",
            get(api::avatar_source::list_my_avatar_sources).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/users/me/avatar-source",
            axum::routing::put(api::avatar_source::set_my_avatar_source).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware),
            ),
        )
        // 名称/简介文案来源（与画像源独立；auto / account / platform / identity）
        .route(
            "/api/users/me/profile-text-sources",
            get(api::profile_text_source::list_my_profile_text_sources).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware),
            ),
        )
        .route(
            "/api/users/me/profile-text-source",
            axum::routing::put(api::profile_text_source::set_my_profile_text_source).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware),
            ),
        )
        .route(
            "/api/auth/change-password",
            post(api::auth_local::change_password).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        // 设置页用户管理（列表/创建/详情/更新/解绑 identity）— 仅管理员
        .route(
            "/api/admin/users",
            get(api::admin_users::list_users)
                .post(api::auth_local::admin_create_user)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/admin/users/{id}",
            get(api::admin_users::get_user)
                .patch(api::admin_users::update_user)
                .delete(api::admin_users::delete_user)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/admin/users/{id}/tapps/{tapp_id}",
            axum::routing::delete(api::admin_users::uninstall_user_tapp).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware),
            ),
        )
        .route(
            "/api/admin/users/{id}/identities/{identity_id}",
            axum::routing::delete(api::admin_users::unlink_identity).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware),
            ),
        )
        .route(
            "/api/admin/users/{id}/avatar-sources",
            get(api::avatar_source::list_user_avatar_sources).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/admin/users/{id}/avatar-source",
            axum::routing::put(api::avatar_source::set_user_avatar_source).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware),
            ),
        )
        .route(
            "/api/admin/users/{id}/profile-text-sources",
            get(api::profile_text_source::list_user_profile_text_sources).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware),
            ),
        )
        .route(
            "/api/admin/users/{id}/profile-text-source",
            axum::routing::put(api::profile_text_source::set_user_profile_text_source).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware),
            ),
        )
        // Site public domain (BASE_URL / FRONTEND_URL / CORS) — not federation Move
        .route(
            "/api/admin/site/domain",
            post(change_site_domain).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // ActivityPub domain Move (emit Move to followers for every local user)
        .route(
            "/api/admin/federation/domain-move",
            post(api::federation::admin_federation_domain_move).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // PR #4: 公开注册（开关受 allow_local_registration 控制） + 后补密码 + 本地登录开关
        .route("/api/auth/register", post(api::auth_local::register))
        .route(
            "/api/auth/me/set-password",
            post(api::auth_local::set_password).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/auth/me/local-login",
            axum::routing::patch(api::auth_local::toggle_local_login).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::auth_middleware),
            ),
        )
        // Configuration routes (use wrapper for dynamic DB access)
        .route(
            "/api/config",
            get(api::config::get_config).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route(
            "/api/config",
            post(update_config).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/config/settings-backup",
            get(export_settings)
                .post(restore_settings)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/config/settings-backup/preview",
            post(preview_settings_restore).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/config/dashboard",
            post(api::config::update_dashboard_config).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/config/control-panel",
            post(api::config::update_control_panel_config).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        .route(
            "/api/config/tapp-window-schemes",
            post(api::config::update_tapp_window_schemes).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )), // 登录用户可保存
        )
        .route(
            "/api/config/module-visibility",
            get(api::config::get_module_visibility_preferences),
        )
        .route(
            "/api/config/module-visibility",
            put(api::config::update_module_visibility_preferences).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // 一言配置读取公开；全局写入仅管理员
        .route(
            "/api/config/hitokoto",
            get(api::config::get_hitokoto_config),
        )
        .route(
            "/api/config/hitokoto",
            axum::routing::put(api::config::update_hitokoto_config).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware),
            ),
        )
        // 报告过期设置：读取公开（读取路径需要）；写入仅管理员
        .route(
            "/api/config/report-settings",
            get(api::config::get_report_settings),
        )
        .route(
            "/api/config/report-settings",
            axum::routing::put(api::config::update_report_settings).route_layer(
                from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware),
            ),
        )
        // 权限配置 API
        .route("/api/config/permissions", get(api::config::get_permissions)) // 公开端点：获取当前用户权限
        .route(
            "/api/config/permissions",
            post(api::config::update_permissions).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )), // 仅管理员
        )
        // PR #6: OAuth providers + 本地注册开关（仅管理员可读写）
        .route(
            "/api/config/oauth-providers",
            get(api::config::get_oauth_providers)
                .put(api::config::update_oauth_providers)
                .route_layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::admin_middleware,
                )),
        )
        .route(
            "/api/config/test",
            post(api::config::test_platform).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::auth_middleware,
            )),
        )
        .route("/api/config/metadata", get(api::config::get_site_metadata)) // 公开端点：网站元数据
        .route("/api/config/public", get(api::config::get_public_config)) // 公开端点：平台公开信息（用于社交链接）
        .route("/api/config/ui", get(api::config::get_public_ui_config)) // 公开端点：UI 运行时（壁纸/动效/音乐/站点展示）
        // SEO：sitemap / robots / 公开 Tapp·Brew 摘要与爬虫 HTML 壳
        .route("/sitemap.xml", get(api::seo::sitemap_xml))
        .route("/api/seo/sitemap.xml", get(api::seo::sitemap_xml))
        .route("/robots.txt", get(api::seo::robots_txt))
        .route("/llms.txt", get(api::seo::llms_txt))
        .route("/api/seo/llms.txt", get(api::seo::llms_txt))
        .route("/api/seo/tapp/{tapp_id}", get(api::seo::tapp_seo_summary))
        .route("/", get(api::seo::home_seo_html))
        .route("/tapp", get(api::seo::tapp_list_seo_html))
        .route("/tapp/run/{tapp_id}", get(api::seo::tapp_run_seo_html))
        .route(
            "/api/seo/brew/{item_id}",
            get(api::seo::brew_item_seo_summary),
        )
        .route("/brew", get(api::seo::brew_list_seo_html))
        .route("/brew/item/{item_id}", get(api::seo::brew_item_seo_html))
        .route("/library", get(api::seo::library_seo_html))
        .route("/reports", get(api::seo::reports_seo_html))
        // CSRF Token 获取端点
        .route("/api/csrf-token", get(middleware::csrf::get_csrf_token))
        // AI推荐API -  公开端点：图标推荐服务
        .route(
            "/api/ai/recommend-icon",
            post(api::ai_recommend::recommend_icon),
        )
        // Profile routes (use wrapper for dynamic DB access) - ALWAYS REGISTERED
        .route("/api/profile/user-info", get(api::profile::get_user_info))
        .route("/api/profile/batch", get(api::profile::get_batch_user_info)); // 批量 API

    // /api/profile/metadata (raw cache dump) is admin-only — registered on authenticated router.

    api_router
        .route(
            "/api/profile/metadata/status/{platform}",
            get(api::profile::get_platform_metadata_status).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::admin_middleware,
            )),
        )
        // Federation (MFP) 公开端点
        // Layer 1: 发现（无需认证）
        .route(
            "/.well-known/webfinger",
            get(federation::discovery::webfinger),
        )
        .route(
            "/.well-known/nodeinfo",
            get(federation::discovery::nodeinfo_wellknown),
        )
        .route("/nodeinfo/2.1", get(federation::discovery::nodeinfo))
        // Public room directory card (join-by-id / federated public join)
        .route(
            "/api/federation/public/rooms/{room_id}",
            get(api::federation::federation_get_public_room),
        )
        .route(
            "/api/federation/public/limits",
            get(federation::limits::public_limits),
        )
        // Layer 2: Actor + Outbox + Collections（无需认证，AP 标准端点）
        .route("/users/{username}", get(federation::actor::get_actor))
        .route(
            "/users/{username}/avatar",
            get(federation::actor::get_avatar),
        )
        .route(
            "/api/home/widget-fonts/{file}",
            get(api::widget_fonts::get_widget_font),
        )
        .nest_service(
            "/api/federation/avatar-cache",
            tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("public, max-age=604800, immutable"),
                ))
                .service(ServeDir::new(&services::data_paths::paths().cache_images)),
        )
        // Public federation media (Note attachments Image/Video) — URLs embedded in AP.
        // Intentionally unauthenticated GET so remote instances can fetch media during
        // federation. Must stay outside session/auth middleware (see delivery.rs docs).
        .nest_service(
            "/media/federation",
            tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("public, max-age=604800"),
                ))
                .service(ServeDir::new(federation::content::federation_media_root())),
        )
        .route(
            "/users/{username}/outbox",
            get(federation::outbox::get_outbox),
        )
        // 让 generate_activity_id 产出的 id 真正可解引用（与 Outbox 同一可见性投影）
        .route("/activities/{id}", get(federation::outbox::get_activity))
        // Public objects embedded in federation Create activities.  Each
        // handler applies the same fail-closed published-content projection
        // as the Outbox; private/unpublished MFP rows must remain 404.
        .route("/notes/{id}", get(federation::outbox::get_note))
        .route("/reports/{id}", get(federation::outbox::get_report))
        .route(
            "/brew/articles/{id}",
            get(federation::outbox::get_brew_article),
        )
        .route("/tapps/{id}", get(federation::outbox::get_tapp))
        .route("/library/{id}", get(federation::outbox::get_library))
        .route(
            "/users/{username}/followers",
            get(federation::actor::get_followers),
        )
        .route(
            "/users/{username}/following",
            get(federation::actor::get_following),
        )
        // Layer 2: Inbox（远程实例投递，通过 HTTP Signature 验证）
        // Live body limit follows memory profile (default 8 MiB / saver 4 MiB).
        // Concurrent buffering is also gated by inbox inflight budget (429).
        .merge(
            Router::<crate::state::AppState>::new()
                .route(
                    "/users/{username}/inbox",
                    post(federation::inbox::post_inbox),
                )
                .route("/inbox", post(federation::inbox::post_shared_inbox))
                // 远端可达表面。verify_preparse_gate 会在解析 JSON 之前先校验
                // 签名头 / Date / Digest，攻击者要触发解析必须先算出正确的
                // SHA-256 —— 这是这个上限敢放宽的前提。
                .layer(axum::middleware::from_fn(
                    federation::limits::live_inbox_body_limit,
                )),
        )
        // Federation API（需认证）
        // Tapp 宿主归因与认证在 api::federation::router() 内按 Router 级统一挂载。
        .merge(api::federation::router(app_state.clone()))
}

#[cfg(test)]
mod config_mode_route_tests {
    /// Source contract: config-mode must keep setup/status + oauth providers registered
    /// so missing DB yields 503 from handlers/extractors, not SPA 404.
    #[test]
    fn config_mode_router_registers_setup_status_and_oauth_providers() {
        let src = include_str!("base.rs");
        // Inside build_config_mode_router body (before build_base_api_router).
        let start = src
            .find("fn build_config_mode_router")
            .expect("config mode fn");
        let end = src.find("fn build_base_api_router").expect("base api fn");
        let body = &src[start..end];
        assert!(
            body.contains("\"/api/setup/status\""),
            "config-mode must register /api/setup/status"
        );
        assert!(
            body.contains("\"/api/auth/oauth/providers\""),
            "config-mode must register oauth providers list"
        );
        assert!(
            body.contains("\"/api/auth/oauth/{slug}/login\""),
            "config-mode must register oauth login"
        );
    }
}
