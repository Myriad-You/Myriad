//! The federation HTTP domain, constructed only by its process (or development all).
use super::*;

pub(crate) fn build_federation_router(
    app_state: crate::state::AppState,
) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    Router::<crate::state::AppState>::new()
        // Federation feed: guests see public items; users see public + personal items.
        .route(
            "/api/tapp/federation/feed",
            get(api::tapp_runtime::get_federation_feed).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        // Room-peer feed: public posts from every instance sharing a joined group chat.
        .route(
            "/api/tapp/federation/rooms-feed",
            get(api::tapp_runtime::get_federation_rooms_feed).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
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
        // Federation (MFP) 公开端点
        // 发现（无需认证）
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
        // Actor + Outbox + Collections（无需认证，AP 标准端点）
        .route("/users/{username}", get(federation::actor::get_actor))
        .route(
            "/users/{username}/avatar",
            get(federation::actor::get_avatar),
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
        // federation. Must stay outside session/auth middleware.
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
        // Inbox（远程实例投递，通过 HTTP Signature 验证）
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
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn federation_routes_are_absent_from_web_and_keep_worker_auth() {
        let state = crate::state::AppState::new(
            sea_orm::DatabaseConnection::default(),
            AppConfig::default(),
            crate::config::DynamicConfig::default(),
        );
        let web = super::super::base::build_base_api_router(state.clone())
            .merge(super::super::authenticated::build_authenticated_router(
                state.clone(),
            ))
            .with_state(state.clone());
        let federation = build_federation_router(state.clone()).with_state(state);
        for path in [
            "/inbox",
            "/users/alice",
            "/notes/example",
            "/api/tapp/federation/feed",
            "/api/federation/public/limits",
            "/api/admin/federation/domain-move",
        ] {
            let request = || {
                Request::builder()
                    .method("OPTIONS")
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .unwrap()
            };
            assert_eq!(
                web.clone().oneshot(request()).await.unwrap().status(),
                StatusCode::NOT_FOUND,
                "web owns {path}"
            );
            assert_eq!(
                federation
                    .clone()
                    .oneshot(request())
                    .await
                    .unwrap()
                    .status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "worker missing {path}"
            );
        }
        let response = federation
            .oneshot(
                Request::builder()
                    .uri("/api/federation/delivery")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
