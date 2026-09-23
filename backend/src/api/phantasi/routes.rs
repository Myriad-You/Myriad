//! Phantasi HTTP route table: sources / list / detail / state / comments / RSSHub.
use axum::{
    Router,
    http::header,
    routing::{delete, get, post, put},
};
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::services::data_paths::paths;

use super::{
    applications, comments, feeds_list, feeds_opml, feeds_sources, note_docs, notes, reading_item,
    reading_mark, reading_stats, reading_sync, reading_sync_ws,
};

/// 创建 Phantasi API 路由
pub fn create_phantasi_routes(app_state: crate::state::AppState) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    // 读凭据的 JSON API：无凭据当游客，带了就必须有效（与各 handler 原先一致）。
    // 有效时注入 Claims；当前管理员另带本请求的核验标记，handler 不再自行验签查库。
    let api = Router::<crate::state::AppState>::new()
        // 订阅源管理
        .route(
            "/sources",
            get(feeds_sources::list_sources).post(feeds_sources::add_source),
        )
        .route(
            "/sources/{id}",
            put(feeds_sources::update_source).delete(feeds_sources::delete_source),
        )
        .route("/sources/{id}/refresh", post(feeds_sources::refresh_source))
        .route("/sources/discover", post(feeds_sources::discover_source))
        // OPML 导入导出
        .route("/import-opml", post(feeds_opml::import_opml))
        .route("/export-opml", get(feeds_opml::export_opml))
        // 分类管理
        .route(
            "/categories",
            get(feeds_sources::list_categories).post(feeds_sources::create_category),
        )
        .route(
            "/categories/{id}",
            put(feeds_sources::update_category).delete(feeds_sources::delete_category),
        )
        .route("/topics", get(feeds_list::list_subscription_topics))
        .route("/topics/cards", put(feeds_list::put_feed_topic_cards))
        // 笔记（站长自写内容；写路径一律管理员）
        .route(
            "/notes/rss",
            get(super::notes_rss::get_notes_rss_settings)
                .put(super::notes_rss::put_notes_rss_settings),
        )
        .route(
            "/notes/editor-preference",
            get(super::note_editor::get_preference).put(super::note_editor::put_preference),
        )
        .route(
            "/notes/docs/{id}/history",
            get(super::note_editor::list_history),
        )
        .route(
            "/notes/docs/{id}/history/{version}",
            get(super::note_editor::get_history_entry),
        )
        .route(
            "/notes/docs/{id}/history/{version}/restore",
            post(super::note_editor::restore_history),
        )
        .route("/notes", post(notes::create_note))
        .route("/notes/preview", post(notes::preview_note))
        .route(
            "/notes/docs",
            get(note_docs::list_note_docs).post(note_docs::create_note_doc),
        )
        .route(
            "/notes/author-candidates",
            get(note_docs::list_note_author_candidates),
        )
        .route(
            "/notes/docs/for-item/{item_id}",
            get(note_docs::get_note_doc_for_item),
        )
        .route(
            "/notes/docs/{id}/authors",
            get(note_docs::list_note_doc_authors).post(note_docs::add_note_doc_author),
        )
        .route(
            "/notes/docs/{id}/authors/{user_id}",
            delete(note_docs::remove_note_doc_author),
        )
        .route(
            "/notes/docs/{id}",
            get(note_docs::get_note_doc)
                .put(note_docs::update_note_doc)
                .delete(note_docs::delete_note_doc),
        )
        .route(
            "/notes/docs/{id}/topic",
            put(note_docs::update_note_doc_topic),
        )
        .route(
            "/notes/docs/{id}/publish",
            post(note_docs::publish_note_doc),
        )
        .route(
            "/notes/docs/{id}/schedule",
            post(note_docs::schedule_note_doc),
        )
        .route(
            "/notes/docs/{id}/unschedule",
            post(note_docs::unschedule_note_doc),
        )
        .route(
            "/notes/{id}",
            put(notes::update_note).delete(notes::delete_note),
        )
        // 文章获取
        .route("/items", get(feeds_list::list_items))
        .route("/items/{id}", get(reading_item::get_item))
        .route("/items/{id}/topic", put(feeds_list::update_item_topic))
        .route(
            "/items/{id}/suggest-topic",
            post(feeds_list::suggest_item_topic),
        )
        // 阅读状态
        .route("/items/{id}/read", post(reading_mark::mark_read))
        .route("/items/{id}/unread", post(reading_mark::mark_unread))
        .route("/items/{id}/star", post(reading_mark::star_item))
        .route("/items/{id}/unstar", post(reading_mark::unstar_item))
        .route("/mark-all-read", post(reading_mark::mark_all_read))
        // 用户评论（批注）
        .route(
            "/items/{id}/comments",
            get(comments::list_comments).post(comments::create_comment),
        )
        .route("/comments", get(comments::list_admin_comments))
        .route(
            "/applications",
            get(applications::list_applications).post(applications::create_application),
        )
        .route(
            "/applications/{id}/approve",
            post(applications::approve_application),
        )
        .route(
            "/applications/{id}/reject",
            post(applications::reject_application),
        )
        .route(
            "/applications/{id}",
            delete(applications::delete_application),
        )
        .route(
            "/comments/{id}",
            put(comments::update_comment).delete(comments::delete_comment),
        )
        .route(
            "/comments/{id}/replies",
            get(comments::list_comment_replies),
        )
        // 离线同步
        .route("/sync-states", post(reading_sync::sync_states))
        // 统计信息
        .route("/stats", get(reading_stats::get_stats))
        // RSSHub 实例管理
        .route(
            "/rsshub/instances",
            get(super::rsshub::list_rsshub_instances).post(super::rsshub::add_rsshub_instance),
        )
        .route(
            "/rsshub/instances/{id}",
            put(super::rsshub::update_rsshub_instance)
                .delete(super::rsshub::delete_rsshub_instance),
        )
        .route(
            "/rsshub/instances/{id}/health-check",
            post(super::rsshub::health_check_rsshub_instance),
        )
        .route(
            "/rsshub/instances/{id}/reset",
            post(super::rsshub::reset_rsshub_instance),
        )
        .route(
            "/rsshub/health-check-all",
            post(super::rsshub::health_check_all_rsshub_instances),
        )
        .route_layer(from_fn_with_state(
            app_state.clone(),
            crate::middleware::auth::optional_current_admin_auth_middleware,
        ));

    // 不读凭据的路由（RSS、静态图标、图片缓存）和自带 auth_middleware 的 WebSocket
    // 不挂可选认证：过期 cookie 不能让订阅器或图片请求 401。
    Router::<crate::state::AppState>::new()
        .merge(api)
        .route("/notes.xml", get(super::notes_rss::notes_rss))
        .route(
            "/notes/docs/{id}/ws",
            get(note_docs::note_doc_websocket).route_layer(from_fn_with_state(
                app_state.clone(),
                crate::middleware::auth::auth_middleware,
            )),
        )
        // WebSocket（通知）
        .route(
            "/ws",
            get(reading_sync_ws::phantasi_websocket).route_layer(from_fn_with_state(
                app_state.clone(),
                crate::middleware::auth::auth_middleware,
            )),
        )
        // 图标静态文件：Cache-Control max-age=86400；本层无 CompressionLayer
        .nest_service(
            "/icons",
            tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("public, max-age=86400, immutable"),
                ))
                .service(ServeDir::new(&paths().phantasi_icons)),
        )
        // 图片缓存：已迁 alias 走持久媒体服务；未命中才读 cache。
        .merge(crate::api::media_public::image_cache_routes())
        // Tapp 运行时携带 Grant 头时做服务端归因与权限强制；宿主 UI 请求不受影响
        .route_layer(from_fn_with_state(
            app_state.clone(),
            crate::api::tapp_runtime::phantasi_host_attribution,
        ))
}

/// Leftover `/api/brew/image-cache/*` URLs from pre-rename persona assets.
pub fn legacy_brew_image_cache_routes() -> Router<crate::state::AppState> {
    crate::api::media_public::brew_image_cache_routes()
}

#[cfg(test)]
mod tests {
    #[test]
    fn assemble_by_read_path() {
        let routes = include_str!("routes.rs");
        assert!(routes.contains("reading_item::get_item"));
        assert!(routes.contains("feeds_list::list_items"));
        assert!(routes.contains("feeds_opml::export_opml"));
        assert!(routes.contains("reading_mark::mark_all_read"));
        assert!(routes.contains("reading_stats::get_stats"));
        assert!(routes.contains("comments::list_comments"));
        assert!(routes.contains("super::rsshub::list_rsshub_instances"));
        assert!(routes.contains("legacy_brew_image_cache_routes"));
        assert!(include_str!("../../router/authenticated.rs").contains("/api/brew"));
        assert!(include_str!("reading_item.rs").contains("pub(crate) async fn get_item"));
        assert!(!include_str!("reading_sync_ws.rs").contains("pub(crate) async fn get_item"));
        let comments = include_str!("comments.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or("");
        assert!(!comments.contains("list_rsshub_instances"));
    }
}
