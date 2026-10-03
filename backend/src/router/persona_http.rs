//! Persona state, run hubs, speech sessions and TAPP interaction callbacks share one owner.
use super::*;
pub(crate) fn build_persona_router(
    app_state: crate::state::AppState,
) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    Router::<crate::state::AppState>::new()
        // Agent AI 任务编排 API
        .nest(
            "/api/agent",
            api::agent::create_agent_routes(app_state.clone()),
        )
        .nest(
            "/api/merope/rig",
            api::merope_rig::create_routes(app_state.clone()),
        )
        // 语音服务 API
        // TTS/ASR/convo；单条 TTS 走 configured_provider，batch 播客固定腾讯
        .nest(
            "/api/speech",
            api::speech::create_speech_routes(app_state.clone()),
        )
        .route(
            "/api/tapp/agent/v2/interactions/stream",
            get(api::tapp_runtime::stream_agent_interactions).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/agent/v2/interactions/{interaction_id}",
            get(api::tapp_runtime::get_agent_interaction).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/agent/v2/interactions/{interaction_id}/accept",
            post(api::tapp_runtime::accept_agent_interaction).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/agent/v2/interactions/{interaction_id}/result",
            post(api::tapp_runtime::submit_agent_interaction_result).route_layer(
                from_fn_with_state(
                    app_state.clone(),
                    middleware::auth::optional_auth_middleware,
                ),
            ),
        )
        .route(
            "/api/tapp/agent/v2/interactions/{interaction_id}/reject",
            post(api::tapp_runtime::reject_agent_interaction).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
        .route(
            "/api/tapp/agent/v2/interactions/{interaction_id}/intents",
            post(api::tapp_runtime::request_agent_intent).route_layer(from_fn_with_state(
                app_state.clone(),
                middleware::auth::optional_auth_middleware,
            )),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn persona_routes_leave_web_as_one_state_domain() {
        let state = crate::state::AppState::new(
            sea_orm::DatabaseConnection::default(),
            AppConfig::default(),
            DynamicConfig::default(),
        );
        let web = super::super::base::build_base_api_router(state.clone())
            .merge(super::super::authenticated::build_authenticated_router(
                state.clone(),
            ))
            .with_state(state.clone());
        let persona = build_persona_router(state.clone()).with_state(state);
        for path in [
            "/api/agent/process/stream",
            "/api/agent/notifications/stream",
            "/api/agent/runs/id/stream",
            "/api/speech/convo/start",
            "/api/speech/convo/chat/completions",
            "/api/merope/rig/active",
            "/api/tapp/agent/v2/interactions/id/result",
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
                "web still owns {path}"
            );
            let status = persona.clone().oneshot(request()).await.unwrap().status();
            // Some nested routers apply authentication to their method fallback.
            assert!(
                matches!(
                    status,
                    StatusCode::METHOD_NOT_ALLOWED | StatusCode::UNAUTHORIZED
                ),
                "persona missing {path}: {status}"
            );
        }
        let response = persona
            .oneshot(
                Request::builder()
                    .uri("/api/agent/capabilities")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
